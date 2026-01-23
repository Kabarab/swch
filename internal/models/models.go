package models

type Game struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	Platform string `json:"platform"`
	ImageURL string `json:"imageUrl"`
}

type Account struct {
	ID          string `json:"id"`
	DisplayName string `json:"displayName"` // Steam Persona Name (Никнейм)
	Username    string `json:"username"`    // Steam Account Name (Логин)
	AvatarURL   string `json:"avatarUrl"`
	Platform    string `json:"platform"`
	OwnedGames  []Game `json:"ownedGames"`
}