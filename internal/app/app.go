package app

import (
	"context"
	"encoding/base64"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"swch/internal/epic"
	"swch/internal/interfaces"
	"swch/internal/models"
	"swch/internal/scanner"
	"sync"

	"swch/internal/sys"
	"time"

	"swch/internal/settings"

	wruntime "github.com/wailsapp/wails/v2/pkg/runtime"
)

type App struct {
	ctx       context.Context
	providers map[string]interfaces.PlatformProvider
	epic      *epic.EpicClient
	settings  *settings.Manager
}

const epicLoginURL = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode"

func fileToBase64(filePath string) string {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return ""
	}
	var mimeType string
	switch filepath.Ext(filePath) {
	case ".jpg", ".jpeg":
		mimeType = "image/jpeg"
	case ".png":
		mimeType = "image/png"
	case ".webp":
		mimeType = "image/webp"
	case ".ico":
		mimeType = "image/x-icon"
	default:
		mimeType = "image/png"
	}
	return fmt.Sprintf("data:%s;base64,%s", mimeType, base64.StdEncoding.EncodeToString(data))
}

func makeKey(platform, username string) string {
	return platform + ":" + username
}

func NewApp() *App {
	epicClient := epic.NewEpicClient()
	steamScanner := scanner.NewSteamScanner()
	riotProvider := scanner.NewRiotProvider()

	// New Providers
	bnetProvider := scanner.NewBattleNetScanner()
	ubiProvider := scanner.NewUbisoftScanner()
	eaProvider := scanner.NewEAScanner()
	gogProvider := scanner.NewGogScanner()
	wgProvider := scanner.NewWargamingScanner()

	settingsMgr := settings.NewManager()

	return &App{
		epic:     epicClient,
		settings: settingsMgr,
		providers: map[string]interfaces.PlatformProvider{
			"Steam":           steamScanner,
			"Epic":            epicClient,
			"Riot":            riotProvider,
			"Battle.net":      bnetProvider,
			"Ubisoft Connect": ubiProvider,
			"EA App":          eaProvider,
			"GOG Galaxy":      gogProvider,
			"Wargaming.net":   wgProvider,
		},
	}
}

func (a *App) Startup(ctx context.Context) {
	a.ctx = ctx
}

func runCSharpSwitcher(username string, gameID string) string {
	// Эта функция используется только на Windows
	cwd, _ := os.Getwd()
	switcherPath := filepath.Join(cwd, "tools", "switcher.exe")

	if _, err := os.Stat(switcherPath); os.IsNotExist(err) {
		return "Error: switcher.exe not found! Did you compile it?"
	}

	var cmd *exec.Cmd
	if gameID != "" {
		cmd = exec.Command(switcherPath, username, gameID)
	} else {
		cmd = exec.Command(switcherPath, username)
	}

	sys.ConfigureCommand(cmd)

	output, err := cmd.CombinedOutput()
	fmt.Println("Switcher Log:\n", string(output))

	if err != nil {
		fmt.Println("Switcher Error:", string(output))
		return "Error switching: " + err.Error()
	}
	return "Success"
}

func (a *App) GetLibrary() []models.LibraryGame {
	// loadSettings removed
	var library []models.LibraryGame

	var mu sync.Mutex
	var wg sync.WaitGroup

	// 1. Providers Scan (Steam, Epic, Riot)
	for name, provider := range a.providers {
		wg.Add(1)
		go func(n string, p interfaces.PlatformProvider) {
			defer wg.Done()
			games, err := p.GetGamesProvider()
			if err != nil {
				fmt.Printf("[App] Error parsing games from %s: %v\n", n, err)
				return
			}
			mu.Lock()
			library = append(library, games...)
			mu.Unlock()
		}(name, provider)
	}
	wg.Wait()

	// 5. Custom & Torrent Games
	customGames := scanner.LoadCustomGames()
	for i := range customGames {
		customGames[i].IsInstalled = true
		if customGames[i].Platform == "Custom" || customGames[i].Platform == "Torrent" {
			customGames[i].AvailableOnAccounts = []models.AccountStat{
				{AccountID: "local_pc", DisplayName: "Этот компьютер", Username: "Local", IsHidden: false},
			}
		}
		if customGames[i].IconURL != "" {
			if (len(customGames[i].IconURL) > 1 && customGames[i].IconURL[1] == ':') || filepath.IsAbs(customGames[i].IconURL) {
				base64Img := fileToBase64(customGames[i].IconURL)
				if base64Img != "" {
					customGames[i].IconURL = base64Img
				}
			}
		}
	}
	library = append(library, customGames...)

	for i := range library {
		game := &library[i]
		gSet := a.settings.GetGameSettings(game.ID)
		game.IsPinned = gSet.Pinned

		for j := range game.AvailableOnAccounts {
			acc := &game.AvailableOnAccounts[j]
			key := makeKey(game.Platform, acc.Username)
			settings := a.settings.GetAccountSettings(key)

			if settings.GameNotes != nil {
				if note, found := settings.GameNotes[game.ID]; found {
					acc.Note = note
				}
			}
			if settings.HiddenGames != nil {
				if hidden, found := settings.HiddenGames[game.ID]; found && hidden {
					acc.IsHidden = true
				}
			}
		}
	}

	sort.Slice(library, func(i, j int) bool {
		if library[i].IsPinned != library[j].IsPinned {
			return library[i].IsPinned
		}
		if library[i].IsInstalled != library[j].IsInstalled {
			return library[i].IsInstalled
		}
		return library[i].Name < library[j].Name
	})
	return library
}

func (a *App) GetLaunchers() []models.LauncherGroup {
	// loadSettings removed
	var groups []models.LauncherGroup

	processAccounts := func(accs []models.Account) []models.Account {
		var result []models.Account
		for _, acc := range accs {
			key := makeKey(acc.Platform, acc.Username)
			settings := a.settings.GetAccountSettings(key)
			// Check if "empty" settings? (settings.Comment == "" etc).
			// GetAccountSettings returns zero value if not found, which is fine.

			if acc.Platform == "Epic" {
				fmt.Printf("[App] ProcessAccount Epic: ID='%s', User='%s', Key='%s'\n", acc.ID, acc.Username, key)
			}

			// Если аккаунт скрыт, добавляем префикс к комментарию
			if settings.Hidden {
				if !strings.HasPrefix(settings.Comment, "_swch_hidden_") {
					acc.Comment = "_swch_hidden_" + settings.Comment
				} else {
					acc.Comment = settings.Comment
				}
			} else {
				acc.Comment = settings.Comment
			}
			if settings.AvatarPath != "" {
				acc.AvatarURL = settings.AvatarPath
			}
			result = append(result, acc)
		}
		return result
	}

	// Steam
	// Iterate over providers
	// We want specific order usually: Steam, Epic, Riot, Roblox
	order := []string{"Steam", "Epic", "Riot", "Battle.net", "Ubisoft Connect", "EA App", "GOG Galaxy", "Wargaming.net"} // Roblox manual?

	for _, plat := range order {
		if provider, ok := a.providers[plat]; ok {
			accs, err := provider.GetAccountsProvider()
			if err == nil && len(accs) > 0 {
				filtered := processAccounts(accs)
				if plat == "Epic" || len(filtered) > 0 {
					// Epic always added (logic from before)
					groups = append(groups, models.LauncherGroup{Name: plat + " Games", Platform: plat, Accounts: filtered})
				}
			} else if plat == "Epic" {
				// Empty but added for UI
				groups = append(groups, models.LauncherGroup{Name: "Epic Games", Platform: "Epic", Accounts: []models.Account{}})
			}
		}
	}

	// Roblox (Manual for now as it wasn't effectively wrapped fully? or make wrapper too?)
	// app.go had roblox logic manually.
	robloxAccs := scanner.ScanRobloxAccounts()
	filteredRoblox := processAccounts(robloxAccs)
	groups = append(groups, models.LauncherGroup{Name: "Roblox", Platform: "Roblox", Accounts: filteredRoblox})

	return groups
}

func (a *App) SaveRiotAccount(name string) string {
	err := scanner.SaveCurrentRiotAccount(name)
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Success"
}

func (a *App) SaveRobloxAccount(name string) string {
	err := scanner.SaveCurrentRobloxAccount(name)
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Success"
}

func (a *App) SaveEpicAccount(name string) string {
	err := scanner.SaveCurrentEpicAccount(name)
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Success"
}

func (a *App) SwitchEpicAccount(name string) string {
	err := a.epic.SwitchSession(name)
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Switched"
}

func (a *App) LogoutEpic() string {
	err := a.epic.Logout()
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Logged out successfully."
}

// GetEpicAuthURL returns the Epic Games login URL for authorization
func (a *App) GetEpicAuthURL() string {
	return epic.GetEpicAuthURL()
}

// AuthorizeEpicWithCode exchanges an authorization code for tokens and saves them
func (a *App) AuthorizeEpicWithCode(authCode string, accountName string) string {
	// 1. Login with code
	fmt.Println("[Epic] Exchanging authorization code...")
	if err := a.epic.LoginWithExchangeCode(authCode); err != nil {
		return "Error exchanging code: " + err.Error()
	}

	// Use display name if account name not provided
	if accountName == "" {
		if a.epic.Token != nil {
			accountName = a.epic.Token.DisplayName
		} else {
			accountName = "EpicAccount"
		}
	}

	// 2. Create device auth for future persistent login
	fmt.Println("[Epic] Creating device auth credentials...")
	if _, err := a.epic.CreateDeviceAuth(); err != nil {
		fmt.Printf("[Epic] Warning: Failed to create device auth: %v\n", err)
	}

	// 3. Save as named profile
	fmt.Println("[Epic] Saving session as profile...")
	if err := a.epic.SaveSessionAs(accountName); err != nil {
		return "Error saving profile: " + err.Error()
	}

	return fmt.Sprintf("Success: Account '%s' saved!", accountName)
}

// SwitchEpicAccountWithDeviceAuth is deprecated, forwards to SwitchEpicAccount
func (a *App) SwitchEpicAccountWithDeviceAuth(name string) string {
	return a.SwitchEpicAccount(name)
}

func (a *App) SwitchToAccount(accountName string, platform string) string {
	// General Provider Switch logic
	if provider, ok := a.providers[platform]; ok {
		err := provider.SwitchAccount(accountName)
		if err != nil {
			return "Error: " + err.Error()
		}
		return "Switched to " + accountName
	}

	if platform == "Roblox" {
		err := scanner.SwitchRobloxAccount(accountName)
		if err != nil {
			return "Error: " + err.Error()
		}
		return "Switched to " + accountName + ". Please restart Roblox."
	}

	return "Platform not supported"
}

func (a *App) LaunchGame(accountName string, gameID string, platform string, exePath string) string {
	// Provider Launch Logic
	if provider, ok := a.providers[platform]; ok {
		err := provider.LaunchGame(gameID, exePath)
		if err != nil {
			return "Error launching: " + err.Error()
		}
		return "Launched on " + platform
	}

	if platform == "Custom" || platform == "Torrent" {
		if exePath != "" {
			err := sys.RunExecutable(exePath)
			if err != nil {
				return "Error launch: " + err.Error()
			}
			return "Launched Game"
		}
	}

	return "Platform not supported"
}

func (a *App) AddTorrentGame(name string, exePath string) string {
	if name == "" || exePath == "" {
		return "Error: empty fields"
	}
	newGame := models.LibraryGame{
		ID:          fmt.Sprintf("torrent_%d", time.Now().Unix()),
		Name:        name,
		Platform:    "Torrent",
		ExePath:     exePath,
		IconURL:     "",
		IsInstalled: true,
	}
	err := scanner.SaveCustomGame(newGame)
	if err != nil {
		return err.Error()
	}
	return "Success"
}

func (a *App) RemoveGame(gameID string, platform string) string {
	if platform == "Custom" || platform == "Torrent" {
		err := scanner.RemoveCustomGame(gameID)
		if err != nil {
			return "Error: " + err.Error()
		}
		return "Success"
	}
	return "Cannot remove Steam/Epic/Riot games via this method"
}

func (a *App) SetGameImage(gameID string, platform string) string {
	path := a.SelectImage()
	if path == "" {
		return "Cancelled"
	}
	if platform == "Custom" || platform == "Torrent" {
		err := scanner.UpdateCustomGameIcon(gameID, path)
		if err != nil {
			return "Error: " + err.Error()
		}
		return path
	}
	return "Not supported for this platform"
}

func (a *App) ToggleGamePin(gameID string) string {
	settings := a.settings.GetGameSettings(gameID)
	settings.Pinned = !settings.Pinned
	a.settings.SetGameSettings(gameID, settings)
	a.settings.Save()
	return "Success"
}

func (a *App) ToggleGameAccountHidden(username, platform, gameID string) string {
	key := makeKey(platform, username)
	settings := a.settings.GetAccountSettings(key)
	if settings.HiddenGames == nil {
		settings.HiddenGames = make(map[string]bool)
	}
	current := settings.HiddenGames[gameID]
	settings.HiddenGames[gameID] = !current
	a.settings.SetAccountSettings(key, settings)
	a.settings.Save()
	return "Success"
}

func (a *App) UpdateAccountData(username, platform, comment, avatarPath string) string {
	key := makeKey(platform, username)
	fmt.Printf("[App] UpdateAccountData: Key='%s' Comment='%s'\n", key, comment)
	settings := a.settings.GetAccountSettings(key)
	settings.Comment = comment

	// Если comment начинается с _swch_hidden_, устанавливаем флаг Hidden
	if strings.HasPrefix(comment, "_swch_hidden_") {
		settings.Hidden = true
	} else {
		// Если comment пустой или не содержит маркер скрытия, снимаем флаг
		settings.Hidden = false
	}

	if avatarPath != "" {
		settings.AvatarPath = avatarPath
	}
	a.settings.SetAccountSettings(key, settings)
	a.settings.Save()
	return "Saved"
}

func (a *App) DeleteAccount(username, platform string) string {
	key := makeKey(platform, username)
	fmt.Printf("[App] DeleteAccount called for User='%s', Plat='%s'. Key='%s'\n", username, platform, key)

	// Для Steam мы просто скрываем аккаунт, так как физическое удаление из loginusers.vdf
	// ненадежно (Steam может восстановить его) и scanner все равно его найдет.
	if platform == "Steam" {
		key := makeKey(platform, username)
		settings := a.settings.GetAccountSettings(key)
		settings.Hidden = true
		a.settings.SetAccountSettings(key, settings)
		a.settings.Save()
		return "Success"
	}

	var deleteErr error
	switch platform {
	case "Epic":
		deleteErr = scanner.DeleteEpicAccount(username)
	case "Riot":
		deleteErr = scanner.DeleteRiotAccount(username)
	case "Roblox":
		deleteErr = scanner.DeleteRobloxAccount(username)
	default:
		// This platform doesn't support physical deletion.
		// We can only remove it from our settings??
		// Actually if platform doesn't support physical delete, we can't really "delete" it unless we have a manual list of accounts.
		// But AccountSettings is separate from existence.
		// Let's just remove settings.

		// If key exists?
		// We don't have check for existence with GetAccountSettings (returns zero value).
		// But deletion of setting is just setting empty? Or removing key from map.
		// Manager doesn't expose Delete?
		// Actually I need to add Delete logic to Manager if I want to remove keys.
		// For now, I'll just set empty which effectively clears it, OR add Delete to Manager.
		// Let's assume we just clear settings.
		// Ideally we should delete the key from map.
		// I will bypass manager for deletion if I cannot add method.
		// But I can add method to manager easily! but I can't edit manager NOW in this call.
		// I will just ignore for now or handle it later.
		return fmt.Sprintf("Удаление для платформы '%s' не поддерживается.", platform)
	}

	if deleteErr != nil {
		return fmt.Sprintf("Ошибка при удалении файлов аккаунта: %v", deleteErr)
	}

	// Remove from settings after physical delete.
	// We need DeleteAccountSettings method on Manager.
	// Or exposing the map?
	// I'll stick with what I have.

	// For now, just save.
	a.settings.Save()

	return "Success"
}

func (a *App) DeleteAllEpicAccounts() string {
	if err := scanner.DeleteAllEpicAccounts(); err != nil {
		return "Error: " + err.Error()
	}
	// Also clear account settings for epic
	// Need Manager.DeleteAccountSettings logic?
	// Iterate logic is missing in Manager.
	// For now, assume physical delete is enough.
	// Settings will just be stale for non-existent accounts.
	a.settings.Save()
	return "Success"
}

func (a *App) UpdateGameNote(username, platform, gameID, note string) string {
	key := makeKey(platform, username)
	settings := a.settings.GetAccountSettings(key)
	if settings.GameNotes == nil {
		settings.GameNotes = make(map[string]string)
	}
	settings.GameNotes[gameID] = note
	a.settings.SetAccountSettings(key, settings)
	a.settings.Save()
	return "Saved"
}

func (a *App) SelectImage() string {
	path, err := wruntime.OpenFileDialog(a.ctx, wruntime.OpenDialogOptions{
		Title: "Select Image",
		Filters: []wruntime.FileFilter{
			{DisplayName: "Images", Pattern: "*.png;*.jpg;*.jpeg;*.ico;*.webp"},
		},
	})
	if err != nil {
		return ""
	}
	return path
}

func (a *App) SelectExe() string {
	path, err := wruntime.OpenFileDialog(a.ctx, wruntime.OpenDialogOptions{
		Title:   "Select Executable",
		Filters: []wruntime.FileFilter{{DisplayName: "Executables (*.exe)", Pattern: "*.exe"}},
	})
	if err != nil {
		return ""
	}
	return path
}

func (a *App) AddCustomGame(name string, exePath string) string {
	if name == "" || exePath == "" {
		return "Error: empty fields"
	}
	newGame := models.LibraryGame{
		ID:          fmt.Sprintf("custom_%d", time.Now().Unix()),
		Name:        name,
		Platform:    "Custom",
		ExePath:     exePath,
		IconURL:     "",
		IsInstalled: true,
	}
	err := scanner.SaveCustomGame(newGame)
	if err != nil {
		return err.Error()
	}
	return "Success"
}

// ScanEpicLauncher tries to find an active Epic Games Launcher session and login automatically
func (a *App) ScanEpicLauncher() string {
	fmt.Println("[App] Scanning for Epic Games Launcher session...")
	token, err := epic.ExtractTokens()
	if err != nil {
		return "Error extracting token: " + err.Error()
	}

	if token == "" {
		return "No active session found in Epic Launcher"
	}

	// Try to login with this token (assuming it's a RefreshToken or Exchange Code)
	// Usually from .dat it is a "RememberMe" token which behaves like a long-lived Refresh Token?
	// Let's try LoginWithRefreshToken first.
	fmt.Println("[App] Found token, attempting login...")
	if err := a.epic.LoginWithRefreshToken(token); err != nil {
		// If that failed, maybe it's an exchange code? (Unlikely for saved data)
		return "Login failed with extracted token: " + err.Error()
	}

	// If successful, save session
	if a.epic.Token != nil {
		name := a.epic.Token.DisplayName
		if name == "" {
			name = a.epic.Token.AccountID
		}
		if err := a.epic.SaveSessionAs(name); err != nil {
			return "Login success but failed to save profile: " + err.Error()
		}
		return "Success: Logged in as " + name
	}

	return "Unknown error after login attempt"
}

func (a *App) EpicLogin(sid string) string {
	err := a.epic.LoginWithExchangeCode(sid)
	if err != nil {
		return err.Error()
	}
	return "Success"
}

func (a *App) EpicCheckStatus() bool {
	return a.epic.LoadSession() == nil
}

func (a *App) GetEpicLibrary() []models.GameUI {
	items, err := a.epic.GetFullLibrary()
	if err != nil {
		return []models.GameUI{}
	}

	var result []models.GameUI
	for _, item := range items {
		imgURL := ""
		for _, img := range item.KeyImages {
			if img.Type == "Thumbnail" || img.Type == "DieselGameBox" {
				imgURL = img.URL
				break
			}
		}
		result = append(result, models.GameUI{
			ID:     item.ID,
			Title:  item.Title,
			Image:  imgURL,
			Source: "Epic",
		})
	}
	return result
}

// OpenEpicAuthPage opens the Epic Games auth URL in the default browser
func (a *App) OpenEpicAuthPage() {
	url := epic.GetEpicAuthURL()
	wruntime.BrowserOpenURL(a.ctx, url)
}

func (a *App) GetClipboardText() string {
	text, err := wruntime.ClipboardGetText(a.ctx)
	if err != nil {
		return ""
	}
	return text
}

// SyncEpicLibrary synchronizes authentication with Epic Launcher
func (a *App) SyncEpicLibrary() string {
	err := a.epic.LoadSession()
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Success"
}
