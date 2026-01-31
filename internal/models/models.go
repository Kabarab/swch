// internal/models/models.go
package models

// EpicGame представляет игру из Legendary
type EpicGame struct {
	AppName     string   `json:"app_name"`
	AppTitle    string   `json:"app_title"`
	Version     string   `json:"version"`
	IsInstalled bool     `json:"is_installed"`
	InstallPath string   `json:"install_path"`
	Metadata    EpicMeta `json:"metadata"`
}

type EpicMeta struct {
	KeyImages []EpicImage `json:"key_images"`
}

type EpicImage struct {
	Type   string `json:"type"`
	URL    string `json:"url"`
	Height int    `json:"height"`
	Width  int    `json:"width"`
}

// Упрощенная структура для отправки на фронтенд (если нужно)
type GameUI struct {
	ID        string `json:"id"`
	Title     string `json:"title"`
	Image     string `json:"image"`
	Installed bool   `json:"installed"`
	Source    string `json:"source"` // "epic", "steam" и т.д.
}
type AccountStat struct {
	AccountID   string `json:"accountId"`
	DisplayName string `json:"displayName"`
	Username    string `json:"username"`
	PlaytimeMin int    `json:"playtimeMin"`
	LastPlayed  int64  `json:"lastPlayed"`
	Note        string `json:"note"`
	// Новое поле: скрыт ли аккаунт для этой игры
	IsHidden bool `json:"isHidden"`
}

type LibraryGame struct {
	ID                  string        `json:"id"`
	Name                string        `json:"name"`
	Platform            string        `json:"platform"`
	IconURL             string        `json:"iconUrl"`
	ExePath             string        `json:"exePath"`
	AvailableOnAccounts []AccountStat `json:"availableOn"`
	IsInstalled         bool          `json:"isInstalled"`
	IsPinned bool `json:"isPinned"`
	IsMacSupported      bool          `json:"isMacSupported"`
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

type Settings struct {
	Accounts []Account `json:"accounts"`
}

