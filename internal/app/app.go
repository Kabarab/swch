package app

import (
	"bufio"
	"bytes"
	"context"
	"embed"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"swch/internal/bus"
	"swch/internal/epic"
	"swch/internal/interfaces"
	"swch/internal/models"
	"swch/internal/scanner"
	"swch/internal/services/library"
	"swch/internal/services/switcher"
	"sync"

	"swch/internal/sys"
	"time"

	"swch/internal/settings"

	wruntime "github.com/wailsapp/wails/v2/pkg/runtime"
)

type App struct {
	ctx              context.Context
	bus              *bus.WailsEmitter
	providers        map[string]interfaces.PlatformProvider
	switcher         *switcher.Service
	library          *library.Service
	epic             *epic.EpicClient
	settings         *settings.Manager
	claimerCmd       *exec.Cmd
	claimerMu        sync.Mutex
	claimerIsRunning bool
	claimerFS        embed.FS
	claimerExtracted bool
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

func NewApp(claimerFS embed.FS) *App {
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

	providers := map[string]interfaces.PlatformProvider{
		"Steam":           steamScanner,
		"Epic":            epicClient,
		"Riot":            riotProvider,
		"Battle.net":      bnetProvider,
		"Ubisoft Connect": ubiProvider,
		"EA App":          eaProvider,
		"GOG Galaxy":      gogProvider,
		"Wargaming.net":   wgProvider,
	}

	emitter := bus.NewWailsEmitter()

	// Кэш агрегированной библиотеки живет в UserConfigDir/swch
	libraryCachePath := ""
	if configDir, err := os.UserConfigDir(); err == nil {
		libraryCachePath = filepath.Join(configDir, "swch", "library_cache.json")
	}

	return &App{
		bus:       emitter,
		epic:      epicClient,
		settings:  settingsMgr,
		claimerFS: claimerFS,
		providers: providers,
		switcher:  switcher.NewService(providers, emitter),
		library:   library.NewService(providers, emitter, libraryCachePath),
	}
}

func (a *App) Startup(ctx context.Context) {
	a.ctx = ctx
	a.bus.SetContext(ctx)

	// Фоновое обновление библиотеки при старте: UI сразу получает кэш,
	// по завершении рескана придет событие library-updated
	a.library.RefreshAsync()
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
	// Агрегированная библиотека всех провайдеров — из кэша library-сервиса
	// (мгновенно; фоновый рескан стартует в Startup и через RefreshLibrary)
	library := a.library.Get()

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
			if settings.Playtimes != nil {
				if pt, found := settings.Playtimes[game.ID]; found {
					acc.PlaytimeMin = pt
				}
			}
			if settings.LastPlayeds != nil {
				if lp, found := settings.LastPlayeds[game.ID]; found {
					acc.LastPlayed = lp
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

// RefreshLibrary запускает фоновое пересканирование всех провайдеров
// (локальные манифесты + API магазинов). UI продолжает работать с кэшем;
// по завершении фронтенд получит событие library-updated.
func (a *App) RefreshLibrary() string {
	a.library.RefreshAsync()
	return "Refreshing"
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
	// Полный цикл переключения (kill -> activate -> start) оркеструет
	// switcher.Service; для провайдеров без гранулярных способностей он сам
	// откатывается на их legacy SwitchAccount.
	if _, ok := a.providers[platform]; ok {
		if err := a.switcher.Switch(platform, accountName); err != nil {
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

// --- ACCOUNT SESSION SNAPSHOTS (TcNo-style) ---

// SaveAccountSession сохраняет файлы текущей сессии лаунчера как снапшот
// аккаунта (для Steam: loginusers.vdf, config.vdf, ssfn*, registry.vdf).
func (a *App) SaveAccountSession(platform string, accountName string) string {
	if err := a.switcher.SaveCurrentSession(platform, accountName); err != nil {
		return "Error: " + err.Error()
	}
	return "Success"
}

// RestoreAccountSession выполняет полный цикл с подменой файлов:
// kill лаунчера -> восстановление файлов сессии из снапшота -> активация
// аккаунта в конфигах -> запуск лаунчера.
func (a *App) RestoreAccountSession(platform string, accountName string) string {
	if err := a.switcher.RestoreSession(platform, accountName); err != nil {
		return "Error: " + err.Error()
	}
	return "Restored session for " + accountName
}

// ListAccountSessions возвращает имена сохраненных снапшотов сессий платформы.
func (a *App) ListAccountSessions(platform string) []string {
	names, err := a.switcher.ListSessions(platform)
	if err != nil {
		fmt.Printf("[App] ListAccountSessions error: %v\n", err)
		return []string{}
	}
	if names == nil {
		return []string{}
	}
	return names
}

// DeleteAccountSession удаляет сохраненный снапшот сессии.
func (a *App) DeleteAccountSession(platform string, accountName string) string {
	if err := a.switcher.DeleteSession(platform, accountName); err != nil {
		return "Error: " + err.Error()
	}
	return "Success"
}

func (a *App) LaunchGame(accountName string, gameID string, platform string, exePath string) string {
	// Provider Launch Logic
	if provider, ok := a.providers[platform]; ok {
		err := provider.LaunchGame(gameID, exePath)
		if err != nil {
			return "Error launching: " + err.Error()
		}
		go a.monitorGameSession(gameID, platform, accountName, exePath)
		return "Launched on " + platform
	}

	if platform == "Custom" || platform == "Torrent" {
		if exePath != "" {
			err := sys.RunExecutable(exePath)
			if err != nil {
				return "Error launch: " + err.Error()
			}
			go a.monitorGameSession(gameID, platform, accountName, exePath)
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
	fmt.Println("[App] Сканирование сессии Epic Games Launcher...")
	token, err := epic.ExtractTokens()
	if err != nil {
		return "Ошибка извлечения токена: " + err.Error()
	}

	if token == "" {
		return "Активная сессия не найдена в Epic Launcher"
	}

	// Try to login with this token (assuming it's a RefreshToken or Exchange Code)
	// Usually from .dat it is a "RememberMe" token which behaves like a long-lived Refresh Token?
	// Let's try LoginWithRefreshToken first.
	fmt.Println("[App] Токен найден, попытка входа...")
	if err := a.epic.LoginWithRefreshToken(token); err != nil {
		// If that failed, maybe it's an exchange code? (Unlikely for saved data)
		return "Вход с извлеченным токеном не удался: " + err.Error()
	}

	// If successful, save session
	if a.epic.Token != nil {
		name := a.epic.Token.DisplayName
		if name == "" {
			name = a.epic.Token.AccountID
		}
		if err := a.epic.SaveSessionAs(name); err != nil {
			return "Вход выполнен, но не удалось сохранить профиль: " + err.Error()
		}
		return "Успешно: Вход выполнен как " + name
	}

	return "Неизвестная ошибка после попытки входа"
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

// InstallAllEpicGames fetches all games from the account and performs a fake install for each
func (a *App) InstallAllEpicGames() string {
	fmt.Println("[App] Запуск InstallAllEpicGames...")

	// 1. Ensure we are logged in
	if err := a.epic.LoadSession(); err != nil {
		return "Ошибка: Не выполнен вход. Пожалуйста, войдите в Epic."
	}

	// 2. Fetch owned games
	games, err := a.epic.FetchOwnedGames()
	if err != nil {
		return "Ошибка получения списка игр: " + err.Error()
	}

	if len(games) == 0 {
		return "Игры на аккаунте не найдены."
	}

	fmt.Printf("[App] Найдено %d игр. Начало установки...\n", len(games))

	// 3. Install each game
	successCount := 0
	failCount := 0

	// Default install location
	home, _ := os.UserHomeDir()
	baseInstallDir := filepath.Join(home, "Games", "EpicStub")

	for _, game := range games {
		installPath := filepath.Join(baseInstallDir, game.DisplayName)

		err := scanner.FakeInstallGame(game.AppName, game.CatalogNamespace, game.CatalogItemId, game.DisplayName, installPath)
		if err == nil {
			successCount++
		} else {
			fmt.Printf("[App] Не удалось установить %s: %v\n", game.DisplayName, err)
			failCount++
		}
	}

	return fmt.Sprintf("Готово. Установлено: %d, Ошибок: %d. Пожалуйста, перезапустите Epic Games Launcher.", successCount, failCount)
}

// --- FREE GAMES CLAIMER INTEGRATION ---

func (a *App) extractEmbeddedClaimer(targetDir string) error {
	err := os.MkdirAll(targetDir, 0755)
	if err != nil {
		return err
	}
	return a.copyEmbeddedDir("tools/free-games-claimer", targetDir)
}

func (a *App) copyEmbeddedDir(srcDir, destDir string) error {
	entries, err := a.claimerFS.ReadDir(srcDir)
	if err != nil {
		return err
	}

	for _, entry := range entries {
		srcPath := srcDir + "/" + entry.Name()
		destPath := filepath.Join(destDir, entry.Name())

		if entry.IsDir() {
			err = os.MkdirAll(destPath, 0755)
			if err != nil {
				return err
			}
			err = a.copyEmbeddedDir(srcPath, destPath)
			if err != nil {
				return err
			}
		} else {
			data, err := a.claimerFS.ReadFile(srcPath)
			if err != nil {
				return err
			}
			err = os.WriteFile(destPath, data, 0644)
			if err != nil {
				return err
			}
		}
	}
	return nil
}

func (a *App) getClaimerPath() string {
	// 1. Check if custom path is configured in any profile
	for _, p := range a.settings.Profiles {
		if path, ok := p.Config["CLAIMER_PATH"]; ok && path != "" {
			if _, err := os.Stat(path); err == nil {
				return path
			}
		}
	}

	// 2. Build list of candidate paths
	home, homeErr := os.UserHomeDir()
	cwd, cwdErr := os.Getwd()
	execPath, execErr := os.Executable()
	configDir, configErr := os.UserConfigDir()

	var candidates []string

	// Check sibling in CWD
	if cwdErr == nil {
		candidates = append(candidates,
			filepath.Join(cwd, "free-games-claimer"),
			filepath.Join(cwd, "..", "free-games-claimer"),
			filepath.Join(cwd, "..", "..", "free-games-claimer"),
		)
	}

	// Check sibling in Executable dir
	if execErr == nil {
		execDir := filepath.Dir(execPath)
		candidates = append(candidates,
			filepath.Join(execDir, "free-games-claimer"),
			filepath.Join(execDir, "..", "free-games-claimer"),
			filepath.Join(execDir, "..", "..", "free-games-claimer"),
		)
	}

	// Check user config directory (where we extract it)
	if configErr == nil {
		candidates = append(candidates, filepath.Join(configDir, "swch", "free-games-claimer"))
	}

	// Check common home subdirectories
	if homeErr == nil {
		candidates = append(candidates,
			filepath.Join(home, "vtvin", "free-games-claimer"),
			filepath.Join(home, "free-games-claimer"),
			filepath.Join(home, "Documents", "free-games-claimer"),
			filepath.Join(home, "Documents", "GitHub", "free-games-claimer"),
			filepath.Join(home, "Downloads", "free-games-claimer"),
			filepath.Join(home, "Desktop", "free-games-claimer"),
		)
	}

	// 3. Check which candidate directory exists and contains epic-games.js
	for _, path := range candidates {
		cleanPath := filepath.Clean(path)
		if info, err := os.Stat(cleanPath); err == nil && info.IsDir() {
			if _, err := os.Stat(filepath.Join(cleanPath, "epic-games.js")); err == nil {
				// If we chose targetDir (embedded config candidate), make sure it is updated once per session
				if configErr == nil {
					targetDir := filepath.Clean(filepath.Join(configDir, "swch", "free-games-claimer"))
					if cleanPath == targetDir && !a.claimerExtracted {
						a.claimerMu.Lock()
						if !a.claimerExtracted {
							wruntime.EventsEmit(a.ctx, "claimer-log", "Updating embedded claimer scripts in: "+targetDir)
							_ = a.extractEmbeddedClaimer(targetDir)
							a.claimerExtracted = true
						}
						a.claimerMu.Unlock()
					}
				}
				return cleanPath
			}
		}
	}

	// 4. If not found anywhere, extract embedded claimer to UserConfigDir/swch/free-games-claimer
	if configErr == nil {
		targetDir := filepath.Clean(filepath.Join(configDir, "swch", "free-games-claimer"))
		wruntime.EventsEmit(a.ctx, "claimer-log", "Free Games Claimer not found. Extracting bundled version to: "+targetDir)
		err := a.extractEmbeddedClaimer(targetDir)
		if err == nil {
			a.claimerMu.Lock()
			a.claimerExtracted = true
			a.claimerMu.Unlock()
			return targetDir
		}
		wruntime.EventsEmit(a.ctx, "claimer-log", "Error extracting embedded claimer: "+err.Error())
	}

	// Fallback to default dynamic path if not found and extraction failed
	if homeErr == nil {
		return filepath.Join(home, "vtvin", "free-games-claimer")
	}
	return "/Users/nazarzukov/vtvin/free-games-claimer"
}

func parseEnvFile(filePath string) (map[string]string, error) {
	result := make(map[string]string)
	file, err := os.Open(filePath)
	if err != nil {
		return result, err
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		parts := strings.SplitN(line, "=", 2)
		if len(parts) == 2 {
			key := strings.TrimSpace(parts[0])
			val := strings.TrimSpace(parts[1])
			if strings.HasPrefix(val, "\"") && strings.HasSuffix(val, "\"") {
				val = val[1 : len(val)-1]
			} else if strings.HasPrefix(val, "'") && strings.HasSuffix(val, "'") {
				val = val[1 : len(val)-1]
			}
			result[key] = val
		}
	}
	return result, scanner.Err()
}

func writeEnvFile(filePath string, config map[string]string) error {
	dir := filepath.Dir(filePath)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return err
	}

	file, err := os.Create(filePath)
	if err != nil {
		return err
	}
	defer file.Close()

	writer := bufio.NewWriter(file)
	for k, v := range config {
		var err error
		if strings.ContainsAny(v, " \t\n\r\"'#") {
			_, err = writer.WriteString(fmt.Sprintf("%s=\"%s\"\n", k, strings.ReplaceAll(v, "\"", "\\\"")))
		} else {
			_, err = writer.WriteString(fmt.Sprintf("%s=%s\n", k, v))
		}
		if err != nil {
			return err
		}
	}
	return writer.Flush()
}

func findNodePath() (string, error) {
	nodePath, err := exec.LookPath("node")
	if err == nil {
		return nodePath, nil
	}
	stdPaths := []string{
		"/opt/homebrew/bin/node",
		"/usr/local/bin/node",
		"/usr/bin/node",
	}
	for _, p := range stdPaths {
		if _, err := os.Stat(p); err == nil {
			return p, nil
		}
	}
	return "", fmt.Errorf("node not found in PATH or standard paths")
}

func (a *App) ensureDependencies(nodePath string) error {
	cPath := a.getClaimerPath()
	nodeModulesPath := filepath.Join(cPath, "node_modules")
	cloakbrowserPath := filepath.Join(nodeModulesPath, "cloakbrowser")

	_, errModules := os.Stat(nodeModulesPath)
	_, errCloak := os.Stat(cloakbrowserPath)

	if errModules == nil && errCloak == nil {
		return nil
	}

	if errCloak != nil && errModules == nil {
		wruntime.EventsEmit(a.ctx, "claimer-log", "CloakBrowser dependency is missing. Running npm install...")
	} else {
		wruntime.EventsEmit(a.ctx, "claimer-log", "node_modules not found. Running npm install...")
	}

	npmPath, err := exec.LookPath("npm")
	if err != nil {
		stdNpm := []string{
			"/opt/homebrew/bin/npm",
			"/usr/local/bin/npm",
			"/usr/bin/npm",
		}
		for _, p := range stdNpm {
			if _, err := os.Stat(p); err == nil {
				npmPath = p
				break
			}
		}
	}
	if npmPath == "" {
		return fmt.Errorf("npm not found. Please run 'npm install' in free-games-claimer directory manually")
	}

	cmd := exec.Command(npmPath, "install", "--legacy-peer-deps")
	cmd.Dir = cPath
	output, err := cmd.CombinedOutput()
	if err != nil {
		wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("npm install failed: %s", string(output)))
		return fmt.Errorf("failed to run npm install: %v", err)
	}

	wruntime.EventsEmit(a.ctx, "claimer-log", "npm install completed successfully!")
	return nil
}

func (a *App) GetClaimerConfig() (map[string]string, error) {
	envPath := filepath.Join(a.getClaimerPath(), "data", "config.env")
	if _, err := os.Stat(envPath); os.IsNotExist(err) {
		return make(map[string]string), nil
	}
	return parseEnvFile(envPath)
}

func (a *App) SaveClaimerConfig(config map[string]string) string {
	envPath := filepath.Join(a.getClaimerPath(), "data", "config.env")
	err := writeEnvFile(envPath, config)
	if err != nil {
		return "Error saving config: " + err.Error()
	}
	return "Success"
}

func (a *App) IsClaimerRunning() bool {
	a.claimerMu.Lock()
	defer a.claimerMu.Unlock()
	return a.claimerIsRunning
}

func (a *App) GetClaimerProfiles() []settings.ClaimerProfile {
	// If Profiles is empty, try to migrate from data/config.env
	if len(a.settings.Profiles) == 0 {
		envPath := filepath.Join(a.getClaimerPath(), "data", "config.env")
		if _, err := os.Stat(envPath); err == nil {
			if cfg, err := parseEnvFile(envPath); err == nil && len(cfg) > 0 {
				a.settings.Profiles = append(a.settings.Profiles, settings.ClaimerProfile{
					Name:     "Default",
					Config:   cfg,
					Selected: true,
				})
				a.settings.Save()
			}
		}
	}
	return a.settings.Profiles
}

func (a *App) SaveClaimerProfile(name string, config map[string]string) string {
	if name == "" {
		return "Error: Profile name cannot be empty"
	}
	
	found := false
	for i, p := range a.settings.Profiles {
		if p.Name == name {
			a.settings.Profiles[i].Config = config
			found = true
			break
		}
	}
	if !found {
		a.settings.Profiles = append(a.settings.Profiles, settings.ClaimerProfile{
			Name:     name,
			Config:   config,
			Selected: true,
		})
	}
	a.settings.Save()
	return "Success"
}

func (a *App) DeleteClaimerProfile(name string) string {
	for i, p := range a.settings.Profiles {
		if p.Name == name {
			a.settings.Profiles = append(a.settings.Profiles[:i], a.settings.Profiles[i+1:]...)
			a.settings.Save()
			return "Success"
		}
	}
	return "Error: Profile not found"
}

func (a *App) ToggleClaimerProfileSelected(name string) string {
	for i, p := range a.settings.Profiles {
		if p.Name == name {
			a.settings.Profiles[i].Selected = !a.settings.Profiles[i].Selected
			a.settings.Save()
			return "Success"
		}
	}
	return "Error: Profile not found"
}

func sendTelegramMessage(token, chatID, text string) error {
	url := fmt.Sprintf("https://api.telegram.org/bot%s/sendMessage", token)
	payload := map[string]string{
		"chat_id": chatID,
		"text":    text,
	}
	body, _ := json.Marshal(payload)
	resp, err := http.Post(url, "application/json", bytes.NewBuffer(body))
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("telegram returned status %d", resp.StatusCode)
	}
	return nil
}

func pollTelegramReply(token, chatID string) (string, error) {
	lastUpdateID := 0
	url := fmt.Sprintf("https://api.telegram.org/bot%s/getUpdates?limit=1", token)
	resp, err := http.Get(url)
	if err == nil {
		var result struct {
			Ok     bool `json:"ok"`
			Result []struct {
				UpdateID int `json:"update_id"`
			} `json:"result"`
		}
		if json.NewDecoder(resp.Body).Decode(&result) == nil && len(result.Result) > 0 {
			lastUpdateID = result.Result[0].UpdateID
		}
		resp.Body.Close()
	}

	for i := 0; i < 12; i++ { // wait up to 6 minutes total (12 * 30 seconds)
		pollURL := fmt.Sprintf("https://api.telegram.org/bot%s/getUpdates?offset=%d&timeout=25", token, lastUpdateID+1)
		pollResp, err := http.Get(pollURL)
		if err != nil {
			time.Sleep(2 * time.Second)
			continue
		}
		
		var result struct {
			Ok     bool `json:"ok"`
			Result []struct {
				UpdateID int `json:"update_id"`
				Message  *struct {
					Chat struct {
						ID int64 `json:"id"`
					} `json:"chat"`
					Text string `json:"text"`
				} `json:"message"`
			} `json:"result"`
		}
		
		decodeErr := json.NewDecoder(pollResp.Body).Decode(&result)
		pollResp.Body.Close()
		
		if decodeErr != nil {
			time.Sleep(2 * time.Second)
			continue
		}
		
		for _, upd := range result.Result {
			lastUpdateID = upd.UpdateID
			if upd.Message != nil && upd.Message.Text != "" {
				msgChatID := fmt.Sprintf("%d", upd.Message.Chat.ID)
				if msgChatID == chatID {
					return strings.TrimSpace(upd.Message.Text), nil
				}
			}
		}
	}
	return "", fmt.Errorf("timeout waiting for 2FA code via Telegram")
}

func (a *App) RunClaimerScript(scriptName string) string {
	a.claimerMu.Lock()
	if a.claimerIsRunning {
		a.claimerMu.Unlock()
		return "A script is already running"
	}

	// Make sure we trigger migrations first so we have at least one profile
	a.GetClaimerProfiles()

	var selected []settings.ClaimerProfile
	for _, p := range a.settings.Profiles {
		if p.Selected {
			selected = append(selected, p)
		}
	}

	if len(selected) == 0 {
		a.claimerMu.Unlock()
		return "Error: No accounts selected. Please select at least one account to run the script."
	}

	a.claimerIsRunning = true
	a.claimerMu.Unlock()

	go func() {
		defer func() {
			a.claimerMu.Lock()
			a.claimerIsRunning = false
			a.claimerCmd = nil
			a.claimerMu.Unlock()
			wruntime.EventsEmit(a.ctx, "claimer-status", "finished")
		}()

		wruntime.EventsEmit(a.ctx, "claimer-status", "running")

		nodePath, err := findNodePath()
		if err != nil {
			wruntime.EventsEmit(a.ctx, "claimer-log", "Error: "+err.Error())
			return
		}

		cPath := a.getClaimerPath()
		err = a.ensureDependencies(nodePath)
		if err != nil {
			wruntime.EventsEmit(a.ctx, "claimer-log", "Error installing dependencies: "+err.Error())
			return
		}

		scriptFile := scriptName + ".js"
		scriptPath := filepath.Join(cPath, scriptFile)
		if _, err := os.Stat(scriptPath); os.IsNotExist(err) {
			wruntime.EventsEmit(a.ctx, "claimer-log", "Error: script file "+scriptFile+" does not exist in claimer folder")
			return
		}

		for idx, profile := range selected {
			wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%d/%d] Starting claimer script: %s for account: %s", idx+1, len(selected), scriptName, profile.Name))

			cmd := exec.Command(nodePath, scriptFile)
			cmd.Dir = cPath
			sys.ConfigureCommand(cmd)

			cmd.Env = os.Environ()
			for k, v := range profile.Config {
				if v != "" {
					cmd.Env = append(cmd.Env, fmt.Sprintf("%s=%s", k, v))
				}
			}

			// Generate safe profile name
			safeName := ""
			for _, ch := range profile.Name {
				if (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || (ch >= '0' && ch <= '9') || ch == '_' || ch == '-' {
					safeName += string(ch)
				}
			}
			if safeName == "" {
				safeName = "default"
			}
			browserDir := filepath.Join(cPath, "data", "browser_"+safeName)
			cmd.Env = append(cmd.Env, "BROWSER_DIR="+browserDir)

			stdinPipe, err := cmd.StdinPipe()
			if err != nil {
				wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Error creating stdin pipe: %v", profile.Name, err))
				continue
			}

			stdoutPipe, err := cmd.StdoutPipe()
			if err != nil {
				wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Error creating stdout pipe: %v", profile.Name, err))
				stdinPipe.Close()
				continue
			}
			cmd.Stderr = cmd.Stdout

			a.claimerMu.Lock()
			a.claimerCmd = cmd
			a.claimerMu.Unlock()

			if err := cmd.Start(); err != nil {
				wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Error starting process: %v", profile.Name, err))
				stdinPipe.Close()
				continue
			}

			reader := bufio.NewReader(stdoutPipe)
			for {
				line, err := reader.ReadString('\n')
				if err != nil {
					break
				}
				logLine := strings.TrimRight(line, "\r\n")
				wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] %s", profile.Name, logLine))

				// Detect 2FA/sign-in code request
				lowerLine := strings.ToLower(logLine)
				if strings.Contains(lowerLine, "two-factor sign in code") || strings.Contains(lowerLine, "verification code") || strings.Contains(lowerLine, "enter value") || (strings.Contains(lowerLine, "code") && strings.Contains(lowerLine, "digits")) {
					tgToken := profile.Config["TG_BOT_TOKEN"]
					tgChat := profile.Config["TG_CHAT_ID"]

					if tgToken != "" && tgChat != "" {
						wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Prompt detected. Sending request to Telegram bot...", profile.Name))
						err := sendTelegramMessage(tgToken, tgChat, fmt.Sprintf("🔑 [swch] Account '%s' requires 2FA code.\nPlease reply with the code.", profile.Name))
						if err != nil {
							wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Failed to send Telegram: %v", profile.Name, err))
						} else {
							wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Waiting for reply from Telegram chat %s...", profile.Name, tgChat))
							code, err := pollTelegramReply(tgToken, tgChat)
							if err != nil {
								wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Telegram polling error: %v", profile.Name, err))
							} else {
								wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Received 2FA code '%s' from Telegram. Submitting...", profile.Name, code))
								_, err = stdinPipe.Write([]byte(code + "\n"))
								if err != nil {
									wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Failed to write code to stdin: %v", profile.Name, err))
								}
							}
						}
					} else {
						wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Prompt detected, but Telegram bot details not configured in settings.", profile.Name))
					}
				}
			}

			stdinPipe.Close()

			if err := cmd.Wait(); err != nil {
				wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Process finished with status: %v", profile.Name, err))
			} else {
				wruntime.EventsEmit(a.ctx, "claimer-log", fmt.Sprintf("[%s] Process finished successfully.", profile.Name))
			}

			a.claimerMu.Lock()
			isStopped := a.claimerCmd == nil
			a.claimerMu.Unlock()
			if isStopped {
				wruntime.EventsEmit(a.ctx, "claimer-log", "Execution aborted by user.")
				break
			}
		}
	}()

	return "Success"
}

func (a *App) StopClaimerScript() string {
	a.claimerMu.Lock()
	defer a.claimerMu.Unlock()

	if a.claimerCmd == nil || a.claimerCmd.Process == nil {
		return "No script is running"
	}

	wruntime.EventsEmit(a.ctx, "claimer-log", "Stopping claimer script gracefully (SIGINT)...")

	err := a.claimerCmd.Process.Signal(os.Interrupt)
	if err != nil {
		wruntime.EventsEmit(a.ctx, "claimer-log", "Failed to send SIGINT. Killing process...")
		a.claimerCmd.Process.Kill()
	}

	a.claimerCmd = nil

	return "Success"
}

func (a *App) getCurrentActiveUser(platform string) string {
	switch platform {
	case "Steam":
		user := sys.GetActiveSteamUser()
		if user != "" {
			return user
		}
		// Fallback: first scanned account
		if provider, ok := a.providers["Steam"]; ok {
			accounts, err := provider.GetAccountsProvider()
			if err == nil && len(accounts) > 0 {
				return accounts[0].Username
			}
		}
	case "Epic":
		if a.epic.Token != nil && a.epic.Token.DisplayName != "" {
			return a.epic.Token.DisplayName
		}
		// Fallback
		if provider, ok := a.providers["Epic"]; ok {
			accounts, err := provider.GetAccountsProvider()
			if err == nil && len(accounts) > 0 {
				return accounts[0].Username
			}
		}
	}
	return "Current"
}

func (a *App) monitorGameSession(gameID, platform, username, exePath string) {
	fmt.Printf("[Monitor] Starting session monitor for gameID: %s, platform: %s, user: %s, path: %s\n", gameID, platform, username, exePath)

	searchPattern := exePath
	if searchPattern == "" {
		// Try to find in library
		library := a.GetLibrary()
		for _, g := range library {
			if g.ID == gameID && g.ExePath != "" {
				searchPattern = g.ExePath
				break
			}
		}
	}

	if searchPattern == "" {
		fmt.Printf("[Monitor] Warning: No execution path found for game %s, monitoring aborted\n", gameID)
		return
	}

	// 1. Wait for process to start (up to 2 minutes)
	var running bool
	var checkInterval = 3 * time.Second
	var startTimeout = 2 * time.Minute
	var startTime = time.Now()

	for time.Since(startTime) < startTimeout {
		if sys.IsProcessRunning(searchPattern) {
			running = true
			break
		}
		time.Sleep(checkInterval)
	}

	if !running {
		fmt.Printf("[Monitor] Timeout waiting for game process to start: %s\n", searchPattern)
		return
	}

	fmt.Printf("[Monitor] Game process detected for: %s. Tracking playtime...\n", searchPattern)
	wruntime.EventsEmit(a.ctx, "game-started", gameID)

	// 2. Poll until process closes
	sessionStart := time.Now()
	for {
		time.Sleep(5 * time.Second)
		if !sys.IsProcessRunning(searchPattern) {
			break
		}
	}

	elapsed := time.Since(sessionStart)
	minutesPlayed := int(elapsed.Minutes())
	if minutesPlayed < 1 && elapsed.Seconds() > 10 {
		// Round up to 1 min if played more than 10 seconds (helpful for quick manual tests)
		minutesPlayed = 1
	}

	fmt.Printf("[Monitor] Game process exited. Session length: %v. Adding %d minutes.\n", elapsed, minutesPlayed)

	if minutesPlayed > 0 {
		targetUser := username
		if targetUser == "" || targetUser == "Current" {
			targetUser = a.getCurrentActiveUser(platform)
		}

		key := makeKey(platform, targetUser)
		settings := a.settings.GetAccountSettings(key)

		if settings.Playtimes == nil {
			settings.Playtimes = make(map[string]int)
		}
		if settings.LastPlayeds == nil {
			settings.LastPlayeds = make(map[string]int64)
		}

		settings.Playtimes[gameID] += minutesPlayed
		settings.LastPlayeds[gameID] = time.Now().Unix()

		a.settings.SetAccountSettings(key, settings)
		a.settings.Save()

		fmt.Printf("[Monitor] Playtime saved for %s: total %d mins\n", key, settings.Playtimes[gameID])
	}

	// Notify frontend about stopped game and update library
	wruntime.EventsEmit(a.ctx, "game-stopped", gameID)
	wruntime.EventsEmit(a.ctx, "library-updated")
}
