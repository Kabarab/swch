package app

import (
	"context"
	"fmt"
	"path/filepath"
	"sort"
	"swch/internal/models"
	"swch/internal/scanner"
	"swch/internal/sys"
	"time"

	"github.com/wailsapp/wails/v2/pkg/runtime"
)

type App struct {
	ctx     context.Context
	steam   *scanner.SteamScanner
}

func NewApp() *App {
	return &App{
		steam: scanner.NewSteamScanner(),
	}
}

func (a *App) Startup(ctx context.Context) {
	a.ctx = ctx
}

func (a *App) GetLibrary() []models.LibraryGame {
	var library []models.LibraryGame

	// 1. Игры Steam
	library = append(library, a.steam.GetGames()...)

	// 2. Игры Epic
	library = append(library, scanner.ScanEpicGames()...)

	// 3. Свои игры (Torrent и т.д.)
	library = append(library, scanner.LoadCustomGames()...)

	sort.Slice(library, func(i, j int) bool {
		return library[i].Name < library[j].Name
	})

	return library
}

func (a *App) GetLaunchers() []models.LauncherGroup {
	var groups []models.LauncherGroup

	steamAccs := a.steam.GetAccounts()
	if len(steamAccs) > 0 {
		groups = append(groups, models.LauncherGroup{
			Name: "Steam", Platform: "Steam", Accounts: steamAccs,
		})
	}

	epicAccs := scanner.ScanEpicAccounts()
	if len(epicAccs) > 0 {
		groups = append(groups, models.LauncherGroup{
			Name: "Epic Games", Platform: "Epic", Accounts: epicAccs,
		})
	}

	return groups
}

// SelectExe открывает системное окно выбора файла
func (a *App) SelectExe() string {
	path, err := runtime.OpenFileDialog(a.ctx, runtime.OpenDialogOptions{
		Title: "Select Game Executable",
		Filters: []runtime.FileFilter{
			{DisplayName: "Executables (*.exe)", Pattern: "*.exe"},
		},
	})
	if err != nil { return "" }
	return path
}

// AddCustomGame сохраняет выбранную игру
func (a *App) AddCustomGame(name string, exePath string) string {
	if name == "" || exePath == "" { return "Error: empty fields" }
	
	newGame := models.LibraryGame{
		ID:       fmt.Sprintf("custom_%d", time.Now().Unix()), // Генерируем уникальный ID
		Name:     name,
		Platform: "Custom",
		ExePath:  exePath,
		IconURL:  "", // Иконку можно добавить позже
	}
	
	err := scanner.SaveCustomGame(newGame)
	if err != nil { return err.Error() }
	return "Success"
}

func (a *App) SwitchToAccount(accountName string, platform string) string {
	if platform == "Steam" {
		sys.KillSteam() // Убиваем процессы
		
		if accountName != "" {
			// Меняем реестр
			err := sys.SetSteamUser(accountName)
			if err != nil {
				return "Error: " + err.Error()
			}
		}

		// Получаем путь к Steam.exe
		steamDir, err := sys.GetSteamPath()
		if err != nil {
			return "Steam path not found"
		}
		
		exePath := filepath.Join(steamDir, "steam.exe")
		
		// Запускаем клиент
		sys.StartGame(exePath)
		return "Switched to " + accountName
	}
	
	return "Platform not supported for direct switching"
}


func (a *App) LaunchGame(accountName string, gameID string, platform string, exePath string) string {
	if platform == "Steam" {
		if accountName == "UNKNOWN" {
			return "Error: Login not found. Please login manually once."
		}

		fmt.Println("Closing Steam...")
		sys.KillSteam()
		
		if accountName != "" {
			fmt.Printf("Switching registry to: %s\n", accountName)
			err := sys.SetSteamUser(accountName)
			if err != nil {
				return "Registry Error: " + err.Error()
			}
		}
		
		fmt.Println("Launching...")
		sys.StartGame("steam://run/" + gameID)
		return "Launched on Steam"
	}


func (a *App) LaunchGame(accountName string, gameID string, platform string, exePath string) string {
	if platform == "Steam" {
		fmt.Println("Closing Steam...")
		sys.KillSteam() // Теперь ждет 2 сек
		
		if accountName != "" {
			fmt.Printf("Switching to user: %s\n", accountName)
			// accountName должен быть логином (username)
			err := sys.SetSteamUser(accountName)
			if err != nil {
				return "Registry Error: " + err.Error()
			}
		}
		
		fmt.Println("Launching Steam Game...")
		sys.StartGame("steam://run/" + gameID)
		return "Launched on Steam"
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