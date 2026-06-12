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
	Playtimes   map[string]int    `json:"playtimes"`
	LastPlayeds map[string]int64  `json:"lastPlayeds"`
}

type GameSettings struct {
	Pinned bool `json:"pinned"`
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
