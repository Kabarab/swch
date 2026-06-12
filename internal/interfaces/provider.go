package interfaces

import "swch/internal/models"

type PlatformProvider interface {
	GetName() string
	GetGamesProvider() ([]models.LibraryGame, error)
	GetAccountsProvider() ([]models.Account, error)
	SwitchAccount(accountName string) error
	LaunchGame(gameID string, exePath string) error
}

// --- Опциональные способности провайдеров ---
// Проверяются через type assertion (interface upgrade), поэтому существующие
// провайдеры не обязаны реализовывать всё сразу: switcher.Service использует
// гранулярные шаги, если провайдер их умеет, и откатывается на SwitchAccount,
// если нет.

// ProcessController — управление процессом лаунчера платформы.
type ProcessController interface {
	KillLauncher() error
	StartLauncher() error
}

// AccountActivator активирует аккаунт в конфигах лаунчера, НЕ трогая процесс:
// kill/start берет на себя switcher.Service (концепция TcNo).
type AccountActivator interface {
	ActivateAccount(accountName string) error
}

// SessionSnapshotter — снапшоты файлов сессии лаунчера (бэкап/рестор как в TcNo).
// Имя аккаунта используется как имя каталога снапшота; валидацию имени
// выполняет switcher.Service.
type SessionSnapshotter interface {
	SaveSession(accountName string) error
	RestoreSession(accountName string) error
	ListSessions() ([]string, error)
	DeleteSession(accountName string) error
}

// LocalLibraryScanner — поиск установленных игр по локальным папкам и
// манифестам лаунчера (steamapps/appmanifest_*.acf, EGS Data/Manifests/*.item).
// Работает без сессии и без сети.
type LocalLibraryScanner interface {
	ScanInstalledGames() ([]models.LibraryGame, error)
}

// RemoteLibraryProvider — получение купленных игр через API магазина.
// Токены берутся из сохраненных сессий модуля аккаунтов; результат включает
// и неустановленные игры (IsInstalled=false).
type RemoteLibraryProvider interface {
	GetRemoteLibrary() ([]models.LibraryGame, error)
}
