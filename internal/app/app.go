package app

import (
	"context"
	"fmt"
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

// GetLibrary объединяет Steam, Epic и Свои игры
func (a *App) GetLibrary() []models.LibraryGame {
	var library []models.LibraryGame

	// 1. Сканируем Steam
	library = append(library, a.steam.GetGames()...)

	// 2. Сканируем Epic
	library = append(library, scanner.ScanEpicGames()...)

	// 3. Загружаем добавленные вручную
	library = append(library, scanner.LoadCustomGames()...)

	// Сортируем по имени
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

// SelectExe открывает окно выбора .exe файла
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

// AddCustomGame принимает данные с фронтенда и сохраняет
func (a *App) AddCustomGame(name string, exePath string) string {
	if name == "" || exePath == "" { return "Error: Empty fields" }
	
	newGame := models.LibraryGame{
		ID:       fmt.Sprintf("custom_%d", time.Now().Unix()), // Генерируем ID
		Name:     name,
		Platform: "Custom",
		ExePath:  exePath,
		IconURL:  "", 
	}
	
	err := scanner.SaveCustomGame(newGame)
	if err != nil { return err.Error() }
	return "Success"
}

func (a *App) LaunchGame(accountName string, gameID string, platform string, exePath string) string {
	if platform == "Steam" {
		sys.KillSteam()
		if accountName != "" {
			// accountName здесь — это логин (username)
			sys.SetSteamUser(accountName)
		}
		sys.StartGame("steam://run/" + gameID)
		return "Launched on Steam"
	}
	
	if platform == "Epic" {
		sys.StartGame("com.epicgames.launcher://apps/" + gameID + "?action=launch&silent=true")
		return "Launched on Epic"
	}

	if platform == "Custom" {
		// Просто запускаем EXE файл
		if exePath != "" {
			sys.StartGame(exePath)
			return "Launched Custom Game"
		}
	}

	return "Unknown Platform"
}