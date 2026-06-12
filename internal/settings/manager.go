package settings

import (
	"encoding/json"
	"os"
	"sync"
)

type AccountSettings struct {
	Comment     string            `json:"comment"`
	AvatarPath  string            `json:"avatarPath"`
	Hidden      bool              `json:"hidden"`
	GameNotes   map[string]string `json:"gameNotes"`
	HiddenGames map[string]bool   `json:"hiddenGames"`
}

type GameSettings struct {
	Pinned bool `json:"pinned"` // Избранное (Favorite в терминах Playnite)
	Hidden bool `json:"hidden,omitempty"`
	// Пользовательские категории игры (Playnite-style categories)
	Categories []string `json:"categories,omitempty"`
	// Статус прохождения: notPlayed, planToPlay, playing, played, beaten, completed, onHold, abandoned
	CompletionStatus string `json:"completionStatus,omitempty"`
	// Оценка пользователя от 1 до 10 (0 = не задана)
	UserScore int `json:"userScore,omitempty"`
	// Статистика игрового времени
	PlaytimeSeconds int64 `json:"playtimeSeconds,omitempty"`
	PlayCount       int   `json:"playCount,omitempty"`
	LastPlayed      int64 `json:"lastPlayed,omitempty"` // unix-время последнего запуска
}

type Manager struct {
	AccountsFile string
	GamesFile    string
	Accounts     map[string]AccountSettings
	Games        map[string]GameSettings
	mu           sync.RWMutex
}

func NewManager() *Manager {
	// Use UserConfigDir/swch preferably, but for compatibility use current logic or "accounts_settings.json" in CWD?
	// app.go used "accounts_settings.json" in CWD. I should stick to CWD or move to ConfigDir.
	// For robustness, moving to ConfigDir is better, but might break existing data if not migrated.
	// I'll check if existing files exist in CWD. If so, use them.
	// app.go used const settingsFile = "accounts_settings.json"

	m := &Manager{
		AccountsFile: "accounts_settings.json",
		GamesFile:    "games_settings.json",
		Accounts:     make(map[string]AccountSettings),
		Games:        make(map[string]GameSettings),
	}
	m.Load()
	return m
}

func (m *Manager) Load() {
	m.mu.Lock()
	defer m.mu.Unlock()

	if data, err := os.ReadFile(m.AccountsFile); err == nil {
		json.Unmarshal(data, &m.Accounts)
	}
	if data, err := os.ReadFile(m.GamesFile); err == nil {
		json.Unmarshal(data, &m.Games)
	}
}

func (m *Manager) Save() {
	m.mu.RLock()
	defer m.mu.RUnlock()

	data, _ := json.MarshalIndent(m.Accounts, "", "  ")
	os.WriteFile(m.AccountsFile, data, 0644)

	gData, _ := json.MarshalIndent(m.Games, "", "  ")
	os.WriteFile(m.GamesFile, gData, 0644)
}

func (m *Manager) GetAccountSettings(key string) AccountSettings {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.Accounts[key]
}

func (m *Manager) SetAccountSettings(key string, settings AccountSettings) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.Accounts[key] = settings
	// Save is usually called after? Or auto-save?
	// app.go called saveSettings() locally.
	// We can auto-save for simplicity or expose Save.
}

func (m *Manager) GetGameSettings(id string) GameSettings {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.Games[id]
}

func (m *Manager) SetGameSettings(id string, settings GameSettings) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.Games[id] = settings
}

// UpdateGameSettings атомарно читает, изменяет и сохраняет настройки игры.
// Используется из нескольких горутин (например, трекером игрового времени).
func (m *Manager) UpdateGameSettings(id string, fn func(*GameSettings)) {
	m.mu.Lock()
	defer m.mu.Unlock()
	s := m.Games[id]
	fn(&s)
	m.Games[id] = s
}

// AllGameSettings возвращает копию всех настроек игр.
func (m *Manager) AllGameSettings() map[string]GameSettings {
	m.mu.RLock()
	defer m.mu.RUnlock()
	out := make(map[string]GameSettings, len(m.Games))
	for k, v := range m.Games {
		out[k] = v
	}
	return out
}
