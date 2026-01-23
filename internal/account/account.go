package account

// BasicStatsStatValueAndIcon имитирует вложенную структуру из C# (если она нужна)
type BasicStatsStatValueAndIcon struct {
	Value string
	Icon  string
}

// Account - основная структура аккаунта
type Account struct {
	Platform    string
	ImagePath   string
	DisplayName string
	AccountId   string // Уникальный ID (SteamID64, и т.д.)
	Login       string // Логин для авто-входа (Line0/Line2 в оригинале)
	
	// В Go map соответствует Dictionary. 
	// C#: Dictionary<string, Dictionary<string, BasicStats.StatValueAndIcon>>
	UserStats map[string]map[string]BasicStatsStatValueAndIcon
}

// NewAccount создает новый экземпляр
func NewAccount(platform, login, displayName string) *Account {
	return &Account{
		Platform:    platform,
		Login:       login,
		DisplayName: displayName,
		UserStats:   make(map[string]map[string]BasicStatsStatValueAndIcon),
	}
}