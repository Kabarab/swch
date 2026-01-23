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
	ctx   context.Context
	steam *scanner.SteamScanner
}

func NewApp() *App {
	return &App{
		steam: scanner.NewSteamScanner(),
	}
}

func (a *App) Startup(ctx context.Context) {
	a.ctx = ctx
}

// ... (методы GetLibrary, GetLaunchers, SelectExe, AddCustomGame оставляем без изменений) ...

func (a *App) GetLibrary() []models.LibraryGame {
	var library []models.LibraryGame
	library = append(library, a.steam.GetGames()...)
	library = append(library, scanner.ScanEpicGames()...)
	library = append(library, scanner.LoadCustomGames()...)
	sort.Slice(library, func(i, j int) bool { return library[i].Name < library[j].Name })
	return library
}

func (a *App) GetLaunchers() []models.LauncherGroup {
	var groups []models.LauncherGroup
	steamAccs := a.steam.GetAccounts()
	if len(steamAccs) > 0 {
		groups = append(groups, models.LauncherGroup{Name: "Steam", Platform: "Steam", Accounts: steamAccs})
	}
	epicAccs := scanner.ScanEpicAccounts()
	if len(epicAccs) > 0 {
		groups = append(groups, models.LauncherGroup{Name: "Epic Games", Platform: "Epic", Accounts: epicAccs})
	}
	return groups
}

func (a *App) SelectExe() string {
	path, err := runtime.OpenFileDialog(a.ctx, runtime.OpenDialogOptions{
		Title: "Select Game Executable",
		Filters: []runtime.FileFilter{{DisplayName: "Executables (*.exe)", Pattern: "*.exe"}},
	})
	if err != nil { return "" }
	return path
}

func (a *App) AddCustomGame(name string, exePath string) string {
	if name == "" || exePath == "" { return "Error: empty fields" }
	newGame := models.LibraryGame{
		ID: fmt.Sprintf("custom_%d", time.Now().Unix()),
		Name: name, Platform: "Custom", ExePath: exePath, IconURL: "",
	}
	err := scanner.SaveCustomGame(newGame)
	if err != nil { return err.Error() }
	return "Success"
}

// SwitchToAccount
func (a *App) SwitchToAccount(accountName string, platform string) string {
	if platform == "Steam" {
		if accountName == "UNKNOWN" { return "Error: Unknown Login" }

		sys.KillSteam()

		// Патчим файл
		_ = a.steam.SetUserActive(accountName)

		// Чистим реестр и ставим юзера
		if err := sys.SetSteamUser(accountName); err != nil {
			return "Registry Error: " + err.Error()
		}

		steamDir, err := sys.GetSteamPath()
		if err != nil { return "Steam path not found" }
		exePath := filepath.Join(steamDir, "steam.exe")

		sys.StartGame(exePath)
		return "Switched to " + accountName
	}
	return "Platform not supported"
}

// LaunchGame
func (a *App) LaunchGame(accountName string, gameID string, platform string, exePath string) string {
	if platform == "Steam" {
		if accountName == "UNKNOWN" { return "Error: Unknown Login" }

		fmt.Println("Closing Steam...")
		sys.KillSteam()
		
		// 1. Патчим VDF
		_ = a.steam.SetUserActive(accountName)

		// 2. Реестр (с очисткой ActiveProcess!)
		if accountName != "" {
			fmt.Printf("Switching to: %s\n", accountName)
			if err := sys.SetSteamUser(accountName); err != nil {
				return "Error: " + err.Error()
			}
		}
		
		fmt.Println("Launching...")
		
		steamDir, _ := sys.GetSteamPath()
		steamExe := filepath.Join(steamDir, "steam.exe")
		
		// Запуск через аргумент -applaunch гарантирует, что Steam сначала прогрузится, а потом запустит игру
		sys.StartGameWithArgs(steamExe, "-applaunch", gameID)
		
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