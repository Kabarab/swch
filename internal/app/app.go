package app

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"swch/internal/models"
	"swch/internal/scanner"
	"swch/internal/sys"
	"syscall"
	"time"

	"github.com/wailsapp/wails/v2/pkg/runtime"
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

	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}

	output, err := cmd.CombinedOutput()
	fmt.Println("Switcher Log:\n", string(output))

	if err != nil {
		fmt.Println("Switcher Error:", string(output))
		return "Error switching: " + err.Error()
	}

	return "Success"
}

// GetLibrary собирает все игры. Добавлена логика для Torrent игр.
func (a *App) GetLibrary() []models.LibraryGame {
	loadSettings()

	var library []models.LibraryGame

	// 1. Steam
	steamGames := a.steam.GetGames()
	library = append(library, steamGames...)

	// 2. Epic Games
	epicGames := scanner.ScanEpicGames()
	library = append(library, epicGames...)

	// 3. Custom & Torrent Games
	customGames := scanner.LoadCustomGames()
	for i := range customGames {
		customGames[i].IsInstalled = true

		// ВАЖНО: Добавляем фейковый аккаунт для запуска без лаунчеров
		if customGames[i].Platform == "Custom" || customGames[i].Platform == "Torrent" {
			customGames[i].AvailableOnAccounts = []models.AccountStat{
				{
					AccountID:   "local_pc",
					DisplayName: "Этот компьютер",
					Username:    "Local",
					IsHidden:    false,
				},
			}
		}
	}
	library = append(library, customGames...)

	// Применяем настройки
	for i := range library {
		game := &library[i]

		// Закрепление
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

			if exists && settings.Hidden {
				continue
			}
			if exists {
				acc.Comment = settings.Comment
				if settings.AvatarPath != "" {
					acc.AvatarURL = settings.AvatarPath
				}
			}
			result = append(result, acc)
		}
		return result
	}

	steamAccs := a.steam.GetAccounts()
	if len(steamAccs) > 0 {
		filtered := processAccounts(steamAccs)
		if len(filtered) > 0 {
			groups = append(groups, models.LauncherGroup{Name: "Steam", Platform: "Steam", Accounts: filtered})
		}
	}

	epicAccs := scanner.ScanEpicAccounts()
	filtered := processAccounts(epicAccs)
	groups = append(groups, models.LauncherGroup{Name: "Epic Games", Platform: "Epic", Accounts: filtered})

	return groups
}

// === НОВЫЕ ФУНКЦИИ ===

// AddTorrentGame добавляет игру с платформой "Torrent"
func (a *App) AddTorrentGame(name string, exePath string) string {
	if name == "" || exePath == "" {
		return "Error: empty fields"
	}
	newGame := models.LibraryGame{
		ID:          fmt.Sprintf("torrent_%d", time.Now().Unix()),
		Name:        name,
		Platform:    "Torrent", // Специальная платформа
		ExePath:     exePath,
		IconURL:     "", // Картинку можно установить позже
		IsInstalled: true,
	}
	err := scanner.SaveCustomGame(newGame)
	if err != nil {
		return err.Error()
	}
	return "Success"
}

// RemoveGame удаляет игру (только для Custom и Torrent)
func (a *App) RemoveGame(gameID string, platform string) string {
	if platform == "Custom" || platform == "Torrent" {
		err := scanner.RemoveCustomGame(gameID)
		if err != nil {
			return "Error: " + err.Error()
		}
		return "Success"
	}
	return "Cannot remove Steam/Epic games via this method"
}

// SetGameImage открывает диалог выбора картинки и устанавливает её для игры
func (a *App) SetGameImage(gameID string, platform string) string {
	// 1. Открываем диалог выбора файла
	path := a.SelectImage()
	if path == "" {
		return "Cancelled"
	}

	// 2. Сохраняем путь к картинке
	if platform == "Custom" || platform == "Torrent" {
		err := scanner.UpdateCustomGameIcon(gameID, path)
		if err != nil {
			return "Error: " + err.Error()
		}
		return path // Возвращаем путь, чтобы фронтенд мог обновить картинку сразу
	}

	return "Not supported for this platform"
}

// =====================

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

func (a *App) SaveEpicAccount(name string) string {
	err := scanner.SaveCurrentEpicAccount(name)
	if err != nil {
		return "Error: " + err.Error()
	}
	return "Success"
}

func (a *App) UpdateAccountData(username, platform, comment, avatarPath string) string {
	loadSettings()
	key := makeKey(platform, username)
	settings := accountSettingsMap[key]
	settings.Comment = comment
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
	settings := accountSettingsMap[key]
	settings.Hidden = true
	accountSettingsMap[key] = settings
	saveSettings()
	return "Account removed from list"
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
	path, err := runtime.OpenFileDialog(a.ctx, runtime.OpenDialogOptions{
		Title: "Select Image",
		Filters: []runtime.FileFilter{
			{DisplayName: "Images", Pattern: "*.png;*.jpg;*.jpeg;*.ico;*.webp"},
		},
	})
	if err != nil {
		return ""
	}
	return path
}

func (a *App) SelectExe() string {
	path, err := runtime.OpenFileDialog(a.ctx, runtime.OpenDialogOptions{
		Title:   "Select Executable",
		Filters: []runtime.FileFilter{{DisplayName: "Executables (*.exe)", Pattern: "*.exe"}},
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

func (a *App) SwitchToAccount(accountName string, platform string) string {
	if platform == "Steam" {
		if accountName == "UNKNOWN" {
			return "Error: Login not found."
		}
		res := runCSharpSwitcher(accountName, "")
		if res == "Success" {
			return "Switched to " + accountName
		}
		return res
	}
	if platform == "Epic" {
		err := scanner.SwitchEpicAccount(accountName)
		if err != nil {
			return "Error: " + err.Error()
		}
		return "Switched to " + accountName + ". Please restart Epic Launcher."
	}
	return "Platform not supported"
}

// LaunchGame обновлен для поддержки Torrent/Custom
func (a *App) LaunchGame(accountName string, gameID string, platform string, exePath string) string {
	if platform == "Steam" {
		if accountName == "UNKNOWN" {
			return "Error: Login not found."
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

	// Запуск Torrent и Custom игр напрямую
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
