package app

import (
	"context"
	"sort"
	"swch/internal/models"
	"swch/internal/scanner"
	"swch/internal/sys"
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

// GetLibrary возвращает ВСЕ игры со всех лаунчеров
func (a *App) GetLibrary() []models.LibraryGame {
	var library []models.LibraryGame

	// 1. Steam
	library = append(library, a.steam.GetGames()...)

	// 2. Epic Games
	library = append(library, scanner.ScanEpicGames()...)

	// 3. Riot / Torrent (заглушки для примера структуры)
	// library = append(library, scanner.ScanRiotGames()...)

	// Сортировка по имени
	sort.Slice(library, func(i, j int) bool {
		return library[i].Name < library[j].Name
	})

	return library
}

// GetLaunchers возвращает аккаунты, сгруппированные по лаунчерам
func (a *App) GetLaunchers() []models.LauncherGroup {
	var groups []models.LauncherGroup

	// Steam Group
	steamAccs := a.steam.GetAccounts()
	if len(steamAccs) > 0 {
		groups = append(groups, models.LauncherGroup{
			Name:     "Steam",
			Platform: "Steam",
			Accounts: steamAccs,
		})
	}

	// Epic Group
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

func (a *App) LaunchGame(accountName string, gameID string, platform string) string {
	if platform == "Steam" {
		sys.KillSteam()
		// Если аккаунт не указан (например для Epic), просто запускаем
		if accountName != "" {
			err := sys.SetSteamUser(accountName)
			if err != nil { return "Error registry" }
		}
		sys.StartGame("steam://run/" + gameID)
		return "Launched on Steam"
	}
	
	if platform == "Epic" {
		// Запуск Epic игры: com.epicgames.launcher://apps/{AppID}?action=launch&silent=true
		sys.StartGame("com.epicgames.launcher://apps/" + gameID + "?action=launch&silent=true")
		return "Launched on Epic"
	}

	return "Platform not supported"
}