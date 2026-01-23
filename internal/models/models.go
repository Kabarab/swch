package models

type Game struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	Platform string `json:"platform"`
	ImageURL string `json:"imageUrl"`
}

type Account struct {
	ID          string `json:"id"`
	DisplayName string `json:"displayName"`
	AvatarURL   string `json:"avatarUrl"`
	Platform    string `json:"platform"`
	OwnedGames  []Game `json:"ownedGames"`
}