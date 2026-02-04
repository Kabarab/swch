package app

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"swch/internal/epic"
	"swch/internal/models"
	"swch/internal/scanner"

	"swch/internal/sys"
	"time"

	wruntime "github.com/wailsapp/wails/v2/pkg/runtime"
)

type App struct {
	ctx   context.Context
	steam *scanner.SteamScanner
	epic  *epic.EpicClient
}

type AccountSettings struct {
	Comment     string            `json:"comment"`
	AvatarPath  string            `json:"avatarPath"`
	Hidden      bool              `json:"hidden"`
	GameNotes   map[string]string `json:"gameNotes"`
	HiddenGames map[string]bool   `json:"hiddenGames"`
}

type GameSettings struct {
	Pinned bool `json:"pinned"`
}

var accountSettingsMap = make(map[string]AccountSettings)
var gameSettingsMap = make(map[string]GameSettings)

const settingsFile = "accounts_settings.json"
const gameSettingsFile = "games_settings.json"

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

func loadSettings() {
	data, err := os.ReadFile(settingsFile)
	if err == nil {
		json.Unmarshal(data, &accountSettingsMap)
	}
	gData, gErr := os.ReadFile(gameSettingsFile)
	if gErr == nil {
		json.Unmarshal(gData, &gameSettingsMap)
	}
}

func saveSettings() {
	data, _ := json.MarshalIndent(accountSettingsMap, "", "  ")
	os.WriteFile(settingsFile, data, 0644)
}

func saveGameSettings() {
	data, _ := json.MarshalIndent(gameSettingsMap, "", "  ")
	os.WriteFile(gameSettingsFile, data, 0644)
}

func makeKey(platform, username string) string {
	return platform + ":" + username
}

func NewApp() *App {
	return &App{
		steam: scanner.NewSteamScanner(),
		epic:  epic.NewEpicClient(),
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
	loadSettings()
	var library []models.LibraryGame

	// 1. Steam
	steamGames := a.steam.GetGames()
	library = append(library, steamGames...)

	// 2. Epic Games - Native Client
	// Verify we have a session
	if err := a.epic.LoadSession(); err != nil {
		fmt.Println("[App] No active Epic session:", err)
	}

	nativeEpicGames, err := a.epic.GetFullLibrary()
	if err == nil {
		for _, item := range nativeEpicGames {
			// Find icon
			imgURL := "https://upload.wikimedia.org/wikipedia/commons/3/31/Epic_Games_logo.svg"
			for _, img := range item.KeyImages {
				if img.Type == "Thumbnail" || img.Type == "DieselGameBox" {
					imgURL = img.URL
					break
				}
			}

			// Add to library
			library = append(library, models.LibraryGame{
				ID:          item.ID, // Use Catalog ID (usually ArtifactID)
				Name:        item.Title,
				Platform:    "Epic", // Unified Platform
				IconURL:     imgURL,
				ExePath:     item.InstallPath, // Filled by GetFullLibrary if installed
				IsInstalled: item.IsInstalled,
				AvailableOnAccounts: []models.AccountStat{
					{
						AccountID:   "epic_native",
						DisplayName: "Epic Account",
						Username:    a.epic.Token.DisplayName,
					},
				},
			})
		}
	} else {
		fmt.Println("[App] Failed to fetch Epic library:", err)
		// Fallback: only installed games from manifests if API fails
		localManifests, _ := epic.ScanInstalledGames()
		for _, m := range localManifests {
			library = append(library, models.LibraryGame{
				ID:          m.AppID,
				Name:        m.Name,
				Platform:    "Epic",
				IconURL:     "https://upload.wikimedia.org/wikipedia/commons/3/31/Epic_Games_logo.svg",
				ExePath:     m.InstallPath,
				IsInstalled: true,
			})
		}
	}

	// 4. Riot Games
	riotGames := scanner.ScanRiotGames()
	library = append(library, riotGames...)

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
		if gSet, ok := gameSettingsMap[game.ID]; ok {
			game.IsPinned = gSet.Pinned
		}
		for j := range game.AvailableOnAccounts {
			acc := &game.AvailableOnAccounts[j]
			key := makeKey(game.Platform, acc.Username)
			if settings, ok := accountSettingsMap[key]; ok {
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
	loadSettings()
	var groups []models.LauncherGroup

	processAccounts := func(accs []models.Account) []models.Account {
		var result []models.Account
		for _, acc := range accs {
			key := makeKey(acc.Platform, acc.Username)
			settings, exists := accountSettingsMap[key]
			if acc.Platform == "Epic" {
				fmt.Printf("[App] ProcessAccount Epic: ID='%s', User='%s', Key='%s', Exists=%v\n", acc.ID, acc.Username, key, exists)
			}

			if exists {
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
			}
			result = append(result, acc)
		}
		return result
	}

	// Steam
	steamAccs := a.steam.GetAccounts()
	if len(steamAccs) > 0 {
		filtered := processAccounts(steamAccs)
		if len(filtered) > 0 {
			groups = append(groups, models.LauncherGroup{Name: "Steam", Platform: "Steam", Accounts: filtered})
		}
	}

	// Epic Games (Native)
	epicSessions, _ := a.epic.ListSessions()
	var epicAccs []models.Account
	for _, s := range epicSessions {
		epicAccs = append(epicAccs, models.Account{
			ID:          "epic_" + s.Name,
			DisplayName: s.DisplayName,
			Username:    s.Name,
			Platform:    "Epic",
		})
	}
	// Add current verified session if not in list (optional, but good for UX)
	if a.epic.Token != nil {
		found := false
		for _, acc := range epicAccs {
			if acc.Username == "active" { // Simplified check
				found = true
			}
		}
		if !found {
			// We could add the active one, but usually ListSessions covers saved ones.
		}
	}

	processAccounts(epicAccs)
	if len(epicAccs) > 0 {
		groups = append(groups, models.LauncherGroup{Name: "Epic Games", Platform: "Epic", Accounts: epicAccs})
	}

	// Riot
	riotAccs := scanner.ScanRiotAccounts()
	filteredRiot := processAccounts(riotAccs)
	groups = append(groups, models.LauncherGroup{Name: "Riot Games", Platform: "Riot", Accounts: filteredRiot})

	// Roblox
	robloxAccs := scanner.ScanRobloxAccounts()
	filteredRoblox := processAccounts(robloxAccs)
	groups = append(groups, models.LauncherGroup{Name: "Roblox", Platform: "Roblox", Accounts: filteredRoblox})

	return groups
}

// --- Legendary Функции Удалены ---

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
	err := a.epic.SaveSessionAs(name)
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
	if platform == "Steam" {
		if accountName == "UNKNOWN" {
			return "Error: Login not found."
		}

		// ЛОГИКА ДЛЯ MACOS
		if runtime.GOOS == "darwin" {
			fmt.Println("[App] Switching Steam account on macOS...")

			// 1. Убиваем Steam и ждем гарантии закрытия
			sys.KillSteam()

			// Небольшая пауза для системы, чтобы освободить дескрипторы файлов
			time.Sleep(1 * time.Second)

			// 2. Сначала правим registry.vdf (это главное для автологина)
			if err := sys.SetSteamUser(accountName); err != nil {
				fmt.Println("[App] Error setting registry user:", err)
			}

			// 3. Затем правим loginusers.vdf (список аккаунтов)
			if err := a.steam.SetUserActive(accountName); err != nil {
				return "Error updating VDF: " + err.Error()
			}

			fmt.Println("[App] Configs updated. Launching Steam...")

			// 4. Запускаем Steam
			sys.StartGame("steam://open/main")
			return "Switched to " + accountName
		}

		// ЛОГИКА ДЛЯ WINDOWS
		res := runCSharpSwitcher(accountName, "")
		if res == "Success" {
			return "Switched to " + accountName
		}
		return res
	}

	if platform == "Epic" {
		fmt.Printf("[App] Request to switch Epic account to: %s\n", accountName)
		err := a.epic.SwitchSession(accountName)
		if err != nil {
			return "Error: " + err.Error()
		}
		return "Switched to " + accountName
	}

	if platform == "Legendary" {
		return "Legendary is deprecated. Please use Epic Games platform."
	}

	if platform == "Riot" {
		err := scanner.SwitchRiotAccount(accountName)
		if err != nil {
			return "Error: " + err.Error()
		}
		return "Switched to " + accountName + ". Please restart Riot Client."
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
	if platform == "Steam" {
		if accountName == "UNKNOWN" {
			return "Error: Login not found."
		}

		if runtime.GOOS == "darwin" {
			sys.KillSteam()
			time.Sleep(1 * time.Second)

			sys.SetSteamUser(accountName)
			a.steam.SetUserActive(accountName)

			sys.StartGame("steam://run/" + gameID)
			return "Launched on Steam"
		}

		res := runCSharpSwitcher(accountName, gameID)
		if res == "Success" {
			return "Launched on Steam"
		}
		return res
	}

	if platform == "Epic" {
		// Switch to account if needed (optional, assuming user is already switched)
		if accountName != "" && accountName != "Main Profile" {
			if a.epic.Token == nil || a.epic.Token.DisplayName != accountName {
				_ = a.epic.SwitchSession(accountName)
			}
		}

		// Find installed path from library (re-scan or cache)
		// For now we assume exePath is passed correctly or we scan.
		// If exePath is empty, we try to find it.
		targetPath := exePath
		if targetPath == "" {
			// Try to find via manifest
			installed, _ := epic.ScanInstalledGames()
			if game, ok := installed[gameID]; ok {
				targetPath = game.InstallPath
			}
		}

		cmd, err := a.epic.GetLaunchCommand(gameID, targetPath)
		if err != nil {
			return "Error preparing launch: " + err.Error()
		}

		err = a.epic.RunExecutable(cmd)
		if err != nil {
			return "Error launching: " + err.Error()
		}
		return "Launched on Epic"
	}

	if platform == "Riot" {
		if accountName != "" {
			err := scanner.SwitchRiotAccount(accountName)
			if err != nil {
				return "Error switching riot: " + err.Error()
			}
		}

		if runtime.GOOS == "darwin" {
			riotPath := "/Applications/Riot Games/Riot Client.app"
			if _, err := os.Stat(riotPath); os.IsNotExist(err) {
				return "Error: Riot Client.app not found in Applications"
			}
			err := sys.StartGameWithArgs(riotPath, "--launch-product="+gameID, "--launch-patchline=live")
			if err != nil {
				return "Error launching: " + err.Error()
			}
			return "Launched on Riot"
		}

		riotClientPath := "C:\\Riot Games\\Riot Client\\RiotClientServices.exe"
		if _, err := os.Stat(riotClientPath); os.IsNotExist(err) {
			return "Error: RiotClientServices.exe not found at default location"
		}

		err := sys.StartGameWithArgs(riotClientPath, "--launch-product="+gameID, "--launch-patchline=live")
		if err != nil {
			return "Error launching: " + err.Error()
		}
		return "Launched on Riot"
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
	loadSettings()
	settings := gameSettingsMap[gameID]
	settings.Pinned = !settings.Pinned
	gameSettingsMap[gameID] = settings
	saveGameSettings()
	return "Success"
}

func (a *App) ToggleGameAccountHidden(username, platform, gameID string) string {
	loadSettings()
	key := makeKey(platform, username)
	settings := accountSettingsMap[key]
	if settings.HiddenGames == nil {
		settings.HiddenGames = make(map[string]bool)
	}
	current := settings.HiddenGames[gameID]
	settings.HiddenGames[gameID] = !current
	accountSettingsMap[key] = settings
	saveSettings()
	return "Success"
}

func (a *App) UpdateAccountData(username, platform, comment, avatarPath string) string {
	loadSettings()
	key := makeKey(platform, username)
	fmt.Printf("[App] UpdateAccountData: Structuring key '%s' for User='%s', Plat='%s', Comment='%s'\n", key, username, platform, comment)
	settings := accountSettingsMap[key]
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
	accountSettingsMap[key] = settings
	saveSettings()
	return "Saved"
}

func (a *App) DeleteAccount(username, platform string) string {
	loadSettings()
	key := makeKey(platform, username)
	fmt.Printf("[App] DeleteAccount called for User='%s', Plat='%s'. Key='%s'\n", username, platform, key)

	// Для Steam мы просто скрываем аккаунт, так как физическое удаление из loginusers.vdf
	// ненадежно (Steam может восстановить его) и scanner все равно его найдет.
	if platform == "Steam" {
		settings := accountSettingsMap[key]
		settings.Hidden = true
		accountSettingsMap[key] = settings
		saveSettings()
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
		// We can only remove it from our settings.
		if _, ok := accountSettingsMap[key]; ok {
			delete(accountSettingsMap, key)
			saveSettings()
			return "Success"
		}
		// If it's not in settings either, then there's nothing to do.
		return fmt.Sprintf("Удаление для платформы '%s' не поддерживается, и аккаунт не найден.", platform)
	}

	if deleteErr != nil {
		return fmt.Sprintf("Ошибка при удалении файлов аккаунта: %v", deleteErr)
	}

	// Also remove from settings if physical deletion was successful.
	delete(accountSettingsMap, key)
	saveSettings()

	return "Success"
}

func (a *App) DeleteAllEpicAccounts() string {
	if err := scanner.DeleteAllEpicAccounts(); err != nil {
		return "Error: " + err.Error()
	}
	loadSettings()
	for k := range accountSettingsMap {
		if strings.HasPrefix(k, "Epic:") {
			delete(accountSettingsMap, k)
		}
	}
	saveSettings()
	return "Success"
}

func (a *App) UpdateGameNote(username, platform, gameID, note string) string {
	loadSettings()
	key := makeKey(platform, username)
	settings := accountSettingsMap[key]
	if settings.GameNotes == nil {
		settings.GameNotes = make(map[string]string)
	}
	settings.GameNotes[gameID] = note
	accountSettingsMap[key] = settings
	saveSettings()
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
