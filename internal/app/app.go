package app

import (
	"context"
	"swch/internal/models"
	"swch/internal/scanner"
	"swch/internal/sys"
)

type App struct {
	ctx     context.Context
	scanner *scanner.SteamScanner
}

func NewApp() *App {
	return &App{
		scanner: scanner.NewSteamScanner(),
	}
}

func (a *App) Startup(ctx context.Context) {
	a.ctx = ctx
}

// GetAccounts возвращает список аккаунтов (вызывается из JS)
func (a *App) GetAccounts() []models.Account {
	return a.scanner.Scan()
}

// LaunchGame переключает аккаунт и запускает игру (вызывается из JS)
func (a *App) LaunchGame(accountName string, gameID string) string {
	// 1. Закрываем Steam
	sys.KillSteam()
	
	// 2. Меняем пользователя в реестре
	// Примечание: В идеале нужно передавать AccountName (логин), а не DisplayName.
	// Если DisplayName совпадает с логином - сработает.
	err := sys.SetSteamUser(accountName)
	if err != nil {
		return "Error: " + err.Error()
	}

	// 3. Запускаем игру
	sys.StartGame(gameID)
	
	return "Success"
}