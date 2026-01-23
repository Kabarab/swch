package models

// AccountStat - статистика игры на конкретном аккаунте
type AccountStat struct {
	AccountID   string `json:"accountId"`
	DisplayName string `json:"displayName"`
	PlaytimeMin int    `json:"playtimeMin"`
	LastPlayed  int64  `json:"lastPlayed"`
}

// LibraryGame - игра, установленная на ПК
type LibraryGame struct {
	ID                  string        `json:"id"`           // Уникальный ID (AppID)
	Name                string        `json:"name"`         // Название
	Platform            string        `json:"platform"`     // Steam, Epic, etc.
	IconURL             string        `json:"iconUrl"`      // Картинка
	ExePath             string        `json:"exePath"`      // Путь к запуску
	AvailableOnAccounts []AccountStat `json:"availableOn"`  // Список аккаунтов, где она есть
}

// Account - сущность аккаунта для меню "Аккаунты"
type Account struct {
	ID          string `json:"id"`
	DisplayName string `json:"displayName"`
	Username    string `json:"username"`
	Platform    string `json:"platform"`
	AvatarURL   string `json:"avatarUrl"`
}

// Launcher - группа аккаунтов
type LauncherGroup struct {
	Name     string    `json:"name"`
	Platform string    `json:"platform"`
	Accounts []Account `json:"accounts"`
}