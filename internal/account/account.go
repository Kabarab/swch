package account

type BasicStatsStatValueAndIcon struct {
	Value string
	Icon  string
}

type Account struct {
	Platform    string
	ImagePath   string
	DisplayName string
	AccountId   string
	Login       string
	UserStats   map[string]map[string]BasicStatsStatValueAndIcon
}

func NewAccount(platform, login, displayName string) *Account {
	return &Account{
		Platform:    platform,
		Login:       login,
		DisplayName: displayName,
		UserStats:   make(map[string]map[string]BasicStatsStatValueAndIcon),
	}
}
