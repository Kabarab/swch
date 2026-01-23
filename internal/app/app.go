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

func (a *App) GetLibrary() []models.LibraryGame {
	var library []models.LibraryGame

	// 1. Steam
	library = append(library, a.steam.GetGames()...)

	// 2. Epic Games
	library = append(library, scanner.ScanEpicGames()...)

	// 3. Custom Games (Добавленные вручную)
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
			Name:     "Steam",
			Platform: "Steam",
			Accounts: steamAccs,
		})
	}

	epicAccs := scanner.ScanEpicAccounts()
	if len(epicAccs) > 0 {
		groups = append(groups, models.LauncherGroup{
			Name:     "Epic Games",
			Platform: "Epic",
			Accounts: epicAccs,
		})
	}

	return groups
}

// SelectExe открывает диалог выбора файла
func (a *App) SelectExe() string {
	path, err := runtime.OpenFileDialog(a.ctx, runtime.OpenDialogOptions{
		Title: "Select Game Executable",
		Filters: []runtime.FileFilter{
			{DisplayName: "Executables", Pattern: "*.exe"},
		},
	})
	if err != nil { return "" }
	return path
}

// AddCustomGame сохраняет игру
func (a *App) AddCustomGame(name string, exePath string) string {
	if name == "" || exePath == "" { return "Invalid data" }
	
	newGame := models.LibraryGame{
		ID:       fmt.Sprintf("custom_%d", time.Now().Unix()),
		Name:     name,
		Platform: "Custom",
		ExePath:  exePath,
		IconURL:  "", // Можно добавить выбор иконки позже
	}
	
	err := scanner.SaveCustomGame(newGame)
	if err != nil { return err.Error() }
	return "Success"
}

func (a *App) LaunchGame(accountName string, gameID string, platform string, exePath string) string {
	if platform == "Steam" {
		sys.KillSteam()
		if accountName != "" {
			// Здесь accountName должен быть логином (username)
			err := sys.SetSteamUser(accountName)
			if err != nil { return "Error registry" }
		}
		sys.StartGame("steam://run/" + gameID)
		return "Launched on Steam"
	}
	
	if platform == "Epic" {
		sys.StartGame("com.epicgames.launcher://apps/" + gameID + "?action=launch&silent=true")
		return "Launched on Epic"
	}

	if platform == "Custom" {
		if exePath != "" {
			// Запускаем EXE напрямую
			sys.StartGame(exePath)
			return "Launched Custom Game"
		}
	}

	return "Platform not supported"
}