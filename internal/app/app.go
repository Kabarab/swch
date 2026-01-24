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

// AccountSettings хранит пользовательские настройки для аккаунта
type AccountSettings struct {
	Comment    string            `json:"comment"`
	AvatarPath string            `json:"avatarPath"`
	Hidden     bool              `json:"hidden"`
	GameNotes  map[string]string `json:"gameNotes"`
	// HiddenGames: GameID -> true (скрыть аккаунт из списка запуска конкретной игры)
	HiddenGames map[string]bool `json:"hiddenGames"`
}

var accountSettingsMap = make(map[string]AccountSettings)

const settingsFile = "accounts_settings.json"

// loadSettings загружает настройки из файла
func loadSettings() {
	data, err := os.ReadFile(settingsFile)
	if err == nil {
		json.Unmarshal(data, &accountSettingsMap)
	}
}

// saveSettings сохраняет настройки в файл
func saveSettings() {
	data, _ := json.MarshalIndent(accountSettingsMap, "", "  ")
	os.WriteFile(settingsFile, data, 0644)
}

// makeKey создает уникальный ключ для аккаунта (Платформа:Логин)
func makeKey(platform, username string) string {
	return platform + ":" + username
}

// NewApp создает новый экземпляр приложения
func NewApp() *App {
	return &App{
		steam: scanner.NewSteamScanner(),
	}
}

// Startup вызывается при инициализации приложения
func (a *App) Startup(ctx context.Context) {
	a.ctx = ctx
}

// --- Внутренние методы ---

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

// --- Методы, вызываемые из JS (Frontend) ---

// DeleteAccountFromGame скрывает аккаунт только для указанной игры
func (a *App) DeleteAccountFromGame(username, platform, gameID string) string {
	loadSettings()
	key := makeKey(platform, username)
	settings := accountSettingsMap[key]

	if settings.HiddenGames == nil {
		settings.HiddenGames = make(map[string]bool)
	}
	settings.HiddenGames[gameID] = true
	accountSettingsMap[key] = settings

	saveSettings()
	return "Removed"
}

// GetLibrary возвращает список всех игр с примененными заметками и фильтрацией аккаунтов
func (a *App) GetLibrary() []models.LibraryGame {
	loadSettings() // Загружаем актуальные настройки

	var library []models.LibraryGame

	// 1. Steam Games
	steamGames := a.steam.GetGames()
	for i := range steamGames {
		steamGames[i].IsInstalled = true
	}
	library = append(library, steamGames...)

	// 2. Epic Games
	epicGames := scanner.ScanEpicGames()
	for i := range epicGames {
		epicGames[i].IsInstalled = true
	}
	library = append(library, epicGames...)

	// 3. Custom Games
	customGames := scanner.LoadCustomGames()
	for i := range customGames {
		customGames[i].IsInstalled = true
	}
	library = append(library, customGames...)

	// Проходимся по библиотеке: фильтруем аккаунты и добавляем заметки
	for i := range library {
		game := &library[i]

		var visibleAccounts []models.AccountStat

		for _, acc := range game.AvailableOnAccounts {
			key := makeKey(game.Platform, acc.Username)

			if settings, ok := accountSettingsMap[key]; ok {
				// А) Глобальное скрытие (удален из вкладки Аккаунты)
				if settings.Hidden {
					continue
				}
				// Б) Скрытие для конкретной игры
				if settings.HiddenGames != nil && settings.HiddenGames[game.ID] {
					continue
				}

				// В) Применение заметки
				if settings.GameNotes != nil {
					if note, found := settings.GameNotes[game.ID]; found {
						acc.Note = note
					}
				}
			}
			// Если аккаунт прошел проверки, добавляем его в список видимых
			visibleAccounts = append(visibleAccounts, acc)
		}
		// Заменяем исходный список на отфильтрованный
		game.AvailableOnAccounts = visibleAccounts
	}

	sort.Slice(library, func(i, j int) bool { return library[i].Name < library[j].Name })
	return library
}

// GetLaunchers возвращает список аккаунтов для вкладки "Accounts"
func (a *App) GetLaunchers() []models.LauncherGroup {
	loadSettings()

	var groups []models.LauncherGroup

	processAccounts := func(accs []models.Account) []models.Account {
		var result []models.Account
		for _, acc := range accs {
			key := makeKey(acc.Platform, acc.Username)
			settings, exists := accountSettingsMap[key]

			// Пропускаем глобально скрытые аккаунты
			if exists && settings.Hidden {
				continue
			}

			// Применяем настройки (комментарий, аватарка)
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

// UpdateAccountData обновляет глобальные данные аккаунта (комментарий, аватарка)
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

// DeleteAccount помечает аккаунт как скрытый глобально
func (a *App) DeleteAccount(username, platform string) string {
	loadSettings()
	key := makeKey(platform, username)

	settings := accountSettingsMap[key]
	settings.Hidden = true
	accountSettingsMap[key] = settings

	saveSettings()
	return "Account removed from list"
}

// UpdateGameNote сохраняет заметку для конкретной связки Аккаунт+Игра
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

// SelectImage открывает диалог выбора картинки
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

// SelectExe открывает диалог выбора исполняемого файла
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

// AddCustomGame добавляет пользовательскую игру
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

// SwitchToAccount переключает аккаунт (только Steam)
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

// LaunchGame запускает игру (с переключением аккаунта для Steam)
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
