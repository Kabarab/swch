package app

import (
	"context"
	"encoding/json" // <--- ДОБАВЛЕН ИМПОРТ
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

// Структура для хранения настроек в JSON файле
type AccountSettings struct {
	Comment    string `json:"comment"`
	AvatarPath string `json:"avatarPath"`
	Hidden     bool   `json:"hidden"`
}

var accountSettingsMap = make(map[string]AccountSettings)

const settingsFile = "accounts_settings.json"

func loadSettings() {
	data, err := os.ReadFile(settingsFile)
	if err == nil {
		json.Unmarshal(data, &accountSettingsMap)
	}
}

func saveSettings() {
	data, _ := json.MarshalIndent(accountSettingsMap, "", "  ")
	os.WriteFile(settingsFile, data, 0644)
}

func makeKey(platform, username string) string {
	return platform + ":" + username
}

// --- Методы для вызова из JS ---

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

func (a *App) SelectImage() string {
	path, err := runtime.OpenFileDialog(a.ctx, runtime.OpenDialogOptions{
		Title: "Select Avatar",
		Filters: []runtime.FileFilter{
			{DisplayName: "Images", Pattern: "*.png;*.jpg;*.jpeg;*.ico"},
		},
	})
	if err != nil {
		return ""
	}
	return path
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

func (a *App) GetLibrary() []models.LibraryGame {
	var library []models.LibraryGame
	library = append(library, a.steam.GetGames()...)
	library = append(library, scanner.ScanEpicGames()...)
	library = append(library, scanner.LoadCustomGames()...)
	sort.Slice(library, func(i, j int) bool { return library[i].Name < library[j].Name })
	return library
}

// ВАЖНО: Обновленный метод GetLaunchers
func (a *App) GetLaunchers() []models.LauncherGroup {
	loadSettings() // Загружаем настройки перед чтением аккаунтов

	var groups []models.LauncherGroup

	// Функция-хелпер для фильтрации и обогащения данных
	processAccounts := func(accs []models.Account) []models.Account {
		var result []models.Account
		for _, acc := range accs {
			key := makeKey(acc.Platform, acc.Username)
			settings, exists := accountSettingsMap[key]

			// Если аккаунт скрыт - пропускаем его
			if exists && settings.Hidden {
				continue
			}

			// Если есть настройки, применяем их
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
	if len(epicAccs) > 0 {
		filtered := processAccounts(epicAccs)
		if len(filtered) > 0 {
			groups = append(groups, models.LauncherGroup{Name: "Epic Games", Platform: "Epic", Accounts: filtered})
		}
	}

	return groups
}

func (a *App) SelectExe() string {
	path, err := runtime.OpenFileDialog(a.ctx, runtime.OpenDialogOptions{
		Title:   "Select Game Executable",
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
		ID:       fmt.Sprintf("custom_%d", time.Now().Unix()),
		Name:     name,
		Platform: "Custom",
		ExePath:  exePath,
		IconURL:  "",
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
	return "Platform not supported"
}

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
		sys.StartGame("com.epicgames.launcher://apps/" + gameID + "?action=launch&silent=true")
		return "Launched on Epic"
	}

	if platform == "Custom" {
		if exePath != "" {
			sys.StartGame(exePath)
			return "Launched Custom Game"
		}
	}

	return "Platform not supported"
}
