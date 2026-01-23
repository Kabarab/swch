package models

type AccountStat struct {
	AccountID   string `json:"accountId"`
	DisplayName string `json:"displayName"`
	Username    string `json:"username"`
	PlaytimeMin int    `json:"playtimeMin"`
	LastPlayed  int64  `json:"lastPlayed"`
}

type LibraryGame struct {
	ID                  string        `json:"id"`
	Name                string        `json:"name"`
	Platform            string        `json:"platform"`
	IconURL             string        `json:"iconUrl"`
	ExePath             string        `json:"exePath"`
	AvailableOnAccounts []AccountStat `json:"availableOn"`
}
type Account struct {
	ID          string `json:"id"`
	DisplayName string `json:"displayName"`
	Username    string `json:"username"`
	Platform    string `json:"platform"`
	AvatarURL   string `json:"avatarUrl"`
	OwnedGames  []Game `json:"ownedGames"`
	Comment     string `json:"comment"`
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
