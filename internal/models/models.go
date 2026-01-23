package models

// AccountStat - статистика игры на конкретном аккаунте
type AccountStat struct {
	AccountID   string `json:"accountId"`
	DisplayName string `json:"displayName"` // Ник (для отображения)
	Username    string `json:"username"`    // Логин (для переключения) <--- НОВОЕ ПОЛЕ
	PlaytimeMin int    `json:"playtimeMin"`
	LastPlayed  int64  `json:"lastPlayed"`
}

// LibraryGame - игра, установленная на ПК
type LibraryGame struct {
	ID                  string        `json:"id"`
	Name                string        `json:"name"`
	Platform            string        `json:"platform"`
	IconURL             string        `json:"iconUrl"`
	ExePath             string        `json:"exePath"`
	AvailableOnAccounts []AccountStat `json:"availableOn"`
}

// Account - сущность аккаунта для меню "Аккаунты"
type Account struct {
	ID          string `json:"id"`
	DisplayName string `json:"displayName"`
	Username    string `json:"username"`
	Platform    string `json:"platform"`
	AvatarURL   string `json:"avatarUrl"`
	OwnedGames  []Game `json:"ownedGames"`
}

type Game struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	Platform string `json:"platform"`
	ImageURL string `json:"imageUrl"`
}

type LauncherGroup struct {
	Name     string    `json:"name"`
	Platform string    `json:"platform"`
	Accounts []Account `json:"accounts"`
}