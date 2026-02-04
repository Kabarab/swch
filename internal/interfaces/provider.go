package interfaces

import "swch/internal/models"

type PlatformProvider interface {
	GetName() string
	GetGamesProvider() ([]models.LibraryGame, error)
	GetAccountsProvider() ([]models.Account, error)
	SwitchAccount(accountName string) error
	LaunchGame(gameID string, exePath string) error
}
