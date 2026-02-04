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
	"swch/internal/legendary"
	"swch/internal/models"
	"swch/internal/scanner"
	"swch/internal/sys"
	"time"

	wruntime "github.com/wailsapp/wails/v2/pkg/runtime"
)

type App struct {
	ctx   context.Context
	steam *scanner.SteamScanner
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

	// 2. Epic Games (Official)
	epicGames := scanner.ScanEpicGames()
	library = append(library, epicGames...)

	// 3. Legendary (Epic CLI)
	legendaryGames := legendary.ScanLegendaryGames()
	library = append(library, legendaryGames...)

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

	// Epic (Official)
	epicAccs := scanner.ScanEpicAccounts()
	filteredEpic := processAccounts(epicAccs)
	groups = append(groups, models.LauncherGroup{Name: "Epic Games", Platform: "Epic", Accounts: filteredEpic})

	// Legendary
	legendaryAccs := legendary.ScanLegendaryAccounts()
	filteredLegendary := processAccounts(legendaryAccs)
	// Добавляем группу Legendary всегда, чтобы интерфейс знал о поддержке
	groups = append(groups, models.LauncherGroup{Name: "Legendary", Platform: "Legendary", Accounts: filteredLegendary})

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

// --- Legendary Функции ---

func (a *App) LoginLegendaryAccount() string {
	err := legendary.LaunchLegendaryAuth()
	if err != nil {
		return "Error launching terminal: " + err.Error()
	}
	return "Terminal launched. Please complete login there."
}

func (a *App) SaveLegendaryAccount(name string) string {
	err := legendary.SaveCurrentLegendaryAccount(name)
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Success"
}

func (a *App) SwitchLegendaryAccount(name string) string {
	err := legendary.SwitchLegendaryAccount(name)
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Switched Legendary to " + name
}

// -------------------------

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
	err := scanner.SwitchEpicAccount(name)
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Switched"
}

func (a *App) LogoutEpic() string {
	err := scanner.LogoutEpic()
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Logged out successfully. You can now login to Epic with a new account."
}

// GetEpicAuthURL returns the Epic Games login URL for authorization
func (a *App) GetEpicAuthURL() string {
	return scanner.GetEpicAuthURL()
}

// AuthorizeEpicWithCode exchanges an authorization code for device auth credentials
// and saves them to the specified account
func (a *App) AuthorizeEpicWithCode(authCode string, accountName string) string {
	// 1. Exchange auth code for access token
	fmt.Println("[Epic] Exchanging authorization code for access token...")
	tokenResp, err := scanner.ExchangeAuthCodeForToken(authCode)
	if err != nil {
		fmt.Printf("[Epic] Error exchanging code: %v\n", err)
		return "Error exchanging code: " + err.Error()
	}

	fmt.Printf("[Epic] Got access token for account: %s (%s)\n", tokenResp.DisplayName, tokenResp.AccountID)

	// Use display name if account name not provided
	if accountName == "" {
		accountName = tokenResp.DisplayName
	}

	// 2. Create device auth credentials
	fmt.Println("[Epic] Creating device auth credentials...")
	deviceAuth, err := scanner.CreateDeviceAuth(tokenResp.AccessToken, tokenResp.AccountID)
	if err != nil {
		return "Error creating device auth: " + err.Error()
	}

	fmt.Printf("[Epic] Device auth created: DeviceID=%s\n", deviceAuth.DeviceID)

	// 3. Save device auth to account directory
	configDir, err := os.UserConfigDir()
	if err != nil {
		fmt.Printf("[Epic] Error getting config dir: %v\n", err)
		return "Error getting config dir: " + err.Error()
	}

	accountDir := filepath.Join(configDir, "swch", "epic_accounts", accountName)

	fmt.Printf("[Epic] Saving to account dir: %s\n", accountDir)

	if err := os.MkdirAll(accountDir, 0755); err != nil {
		fmt.Printf("[Epic] Error creating dir: %v\n", err)
		return "Error creating account directory: " + err.Error()
	}

	if err := scanner.SaveDeviceAuth(accountDir, deviceAuth); err != nil {
		return "Error saving device auth: " + err.Error()
	}

	// 4. Save meta.json with account name and display name
	metaPath := filepath.Join(accountDir, "meta.json")

	type MetaData struct {
		Name        string `json:"name"`
		DisplayName string `json:"displayName"`
		AccountID   string `json:"accountId"`
	}

	meta := MetaData{
		Name:        accountName,
		DisplayName: tokenResp.DisplayName,
		AccountID:   tokenResp.AccountID,
	}

	metaBytes, err := json.MarshalIndent(meta, "", "  ")
	if err != nil {
		return "Error creating meta file: " + err.Error()
	}

	if err := os.WriteFile(metaPath, metaBytes, 0644); err != nil {
		return "Error saving meta file: " + err.Error()
	}

	return fmt.Sprintf("Success: Account '%s' saved with Device Auth!", accountName)
}

// SwitchEpicAccountWithDeviceAuth switches to an Epic account using Device Auth
func (a *App) SwitchEpicAccountWithDeviceAuth(name string) string {
	home, _ := os.UserHomeDir()
	accountDir := filepath.Join(home, "Library", "Application Support", "swch", "epic_accounts", name)
	if runtime.GOOS == "windows" {
		accountDir = filepath.Join(home, "AppData", "Local", "swch", "epic_accounts", name)
	}

	// 1. Load device auth
	deviceAuth, err := scanner.LoadDeviceAuth(accountDir)
	if err != nil {
		// Fallback to old method if no device auth
		fmt.Printf("[Epic] No device auth found, using legacy switch: %v\n", err)
		return a.SwitchEpicAccount(name)
	}

	// 2. Get new access token using device auth
	fmt.Println("[Epic] Logging in with device auth...")
	tokenResp, err := scanner.LoginWithDeviceAuth(deviceAuth)
	if err != nil {
		return "Error logging in with device auth: " + err.Error()
	}

	fmt.Printf("[Epic] Got access token for: %s\n", tokenResp.DisplayName)

	// 3. Get exchange code
	fmt.Println("[Epic] Getting exchange code...")
	exchangeCode, err := scanner.GetExchangeCode(tokenResp.AccessToken)
	if err != nil {
		return "Error getting exchange code: " + err.Error()
	}

	fmt.Printf("[Epic] Got exchange code: %s...\n", exchangeCode[:8])

	// 4. Kill existing Epic launcher
	sys.KillEpic()
	time.Sleep(500 * time.Millisecond)

	// 5. Launch Epic with exchange code
	fmt.Println("[Epic] Launching Epic Games Launcher with exchange code...")
	if runtime.GOOS == "darwin" {
		cmd := exec.Command("open", "-a", "Epic Games Launcher", "--args",
			"-AUTH_TYPE=exchangecode",
			"-AUTH_LOGIN=unused",
			"-AUTH_PASSWORD="+exchangeCode)
		if err := cmd.Start(); err != nil {
			return "Error launching Epic: " + err.Error()
		}
	} else {
		// Windows
		epicPath := filepath.Join(os.Getenv("PROGRAMFILES(X86)"), "Epic Games", "Launcher", "Portal", "Binaries", "Win64", "EpicGamesLauncher.exe")
		cmd := exec.Command(epicPath,
			"-AUTH_TYPE=exchangecode",
			"-AUTH_LOGIN=unused",
			"-AUTH_PASSWORD="+exchangeCode)
		if err := cmd.Start(); err != nil {
			return "Error launching Epic: " + err.Error()
		}
	}

	return "Switched to " + name
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
		// Use Device Auth method - it will fallback to legacy if no device auth exists
		result := a.SwitchEpicAccountWithDeviceAuth(accountName)
		return result
	}

	if platform == "Legendary" {
		err := legendary.SwitchLegendaryAccount(accountName)
		if err != nil {
			return "Error: " + err.Error()
		}
		// Legendary не требует перезапуска процессов, так как это CLI
		return "Switched to " + accountName
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
		if accountName != "" && accountName != "Main Profile" {
			err := scanner.SwitchEpicAccount(accountName)
			if err != nil {
				return "Error switching epic: " + err.Error()
			}
		}
		sys.StartGame("com.epicgames.launcher://apps/" + gameID + "?action=launch&silent=true")
		return "Launched on Epic"
	}

	if platform == "Legendary" {
		// Для Legendary проверяем, нужно ли сменить конфиг перед запуском
		if accountName != "" && accountName != "Active Account" {
			err := legendary.SwitchLegendaryAccount(accountName)
			if err != nil {
				return "Error switching legendary: " + err.Error()
			}
		}

		// Запуск игры через legendary launch
		err := legendary.LaunchGame(gameID)
		if err != nil {
			return "Error launching: " + err.Error()
		}
		return "Launched via Legendary"
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
	case "Legendary":
		deleteErr = legendary.DeleteLegendaryAccount(username)
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

// EpicLogin выполняет вход в Epic Games через SID
func (a *App) LegendaryLogin(sid string) string {
	err := legendary.Auth(sid)
	if err != nil {
		// Возвращаем текст ошибки на фронтенд
		return err.Error()
	}
	return "Success"
}

// LoginLegendaryAuto открывает встроенное окно браузера для автоматического входа
func (a *App) LoginLegendaryAuto() string {
	return "Автоматический вход временно недоступен. Пожалуйста, используйте ручной метод или вход через браузер."
}

func (a *App) GetLegendaryGames() []models.GameUI {
	games, err := legendary.ListGames()
	if err != nil {
		// Log error
		return []models.GameUI{}
	}

	var result []models.GameUI
	for _, g := range games {
		// Ищем подходящую картинку
		imgURL := ""
		for _, img := range g.Metadata.KeyImages {
			if img.Type == "DieselGameBox" || img.Type == "Thumbnail" {
				imgURL = img.URL
				break
			}
		}

		result = append(result, models.GameUI{
			ID:        g.AppName,
			Title:     g.AppTitle,
			Installed: g.IsInstalled,
			Image:     imgURL,
			Source:    "epic",
		})
	}
	return result
}

// EpicCheckStatus проверяет, залогинен ли пользователь
func (a *App) LegendaryCheckStatus() bool {
	return legendary.Status()
}

// GetEpicLibrary возвращает список игр с картинками
func (a *App) GetLegendaryLibrary() []models.GameUI {
	games, err := legendary.ListGames()
	if err != nil {
		// Логируем ошибку, возвращаем пустой список
		return []models.GameUI{}
	}

	var uiGames []models.GameUI
	for _, g := range games {
		img := ""
		// Ищем подходящую картинку (обычно Thumbnail или BoxArt)
		for _, image := range g.Metadata.KeyImages {
			if image.Type == "Thumbnail" || image.Type == "DieselGameBox" {
				img = image.URL
				break
			}
		}

		uiGames = append(uiGames, models.GameUI{
			ID:        g.AppName,
			Title:     g.AppTitle,
			Installed: g.IsInstalled,
			Image:     img,
		})
	}
	return uiGames
}
func (a *App) LegendaryInstallGame(appName string) string {
	err := legendary.InstallGame(appName, "")
	if err != nil {
		return err.Error()
	}
	return "Installation Started"
}

// EpicLaunchGame запускает игру
func (a *App) LegendaryLaunchGame(appName string) string {
	err := legendary.LaunchGame(appName)
	if err != nil {
		return err.Error()
	}
	return "Launched"
}

// EpicLogout выходит из аккаунта
func (a *App) LegendaryLogout() {
	legendary.Logout()
}

// LaunchEpicGame запускает игру по её AppName
func (a *App) LaunchLegendaryGame(id string) {
	legendary.LaunchGame(id)
}

func (a *App) OpenLegendaryLoginPage() {
	url := epicLoginURL
	wruntime.BrowserOpenURL(a.ctx, url)
}

// OpenEpicAuthPage opens the Epic Games auth URL in the default browser
func (a *App) OpenEpicAuthPage() {
	url := scanner.GetEpicAuthURL()
	wruntime.BrowserOpenURL(a.ctx, url)
}

func (a *App) GetClipboardText() string {
	text, err := wruntime.ClipboardGetText(a.ctx)
	if err != nil {
		return ""
	}
	return text
}

func (a *App) LegendaryLoginWithBrowser() string {
	err := legendary.LoginWithBrowser()
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Success"
}
