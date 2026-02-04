package scanner

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"swch/internal/interfaces"
	"swch/internal/models"
	"swch/internal/sys"
)

type BattleNetScanner struct {
	SwapConfig *FileSwapConfig
}

func NewBattleNetScanner() interfaces.PlatformProvider {
	// Construct paths based on OS (Windows primarily)
	appData := GetWindowsEnv("APPDATA")
	localAppData := GetWindowsEnv("LOCALAPPDATA")

	// Fallback for non-windows (Mac Dev) - empty paths will just be ignored or fail gracefully
	if appData == "" {
		appData = "/tmp/mock/AppData"
	}
	if localAppData == "" {
		localAppData = "/tmp/mock/LocalAppData"
	}

	config := &FileSwapConfig{
		ID:           "battlenet",
		ProcessNames: []string{"Battle.net.exe", "Agent.exe"},
		Directories: []string{
			filepath.Join(localAppData, "Battle.net"),
			filepath.Join(localAppData, "Blizzard Entertainment"),
			filepath.Join(appData, "Battle.net"),
		},
		Files: []string{
			// Key config file
			filepath.Join(appData, "Battle.net", "Battle.net.config"),
		},
	}

	return &BattleNetScanner{
		SwapConfig: config,
	}
}

func (b *BattleNetScanner) GetName() string {
	return "Battle.net"
}

// BattleNetConfig struct to parse games (simplified)
type BattleNetConfig struct {
	Client struct {
		Install struct {
			DefaultInstallPath string `json:"DefaultInstallPath"`
		} `json:"Install"`
	} `json:"Client"`
	// Games map might be complex, simplified approach:
}

func (b *BattleNetScanner) GetGamesProvider() ([]models.LibraryGame, error) {
	var games []models.LibraryGame

	// 1. Try to read Battle.net.config to find install paths
	// Path: %APPDATA%/Battle.net/Battle.net.config
	appData := GetWindowsEnv("APPDATA")
	if appData == "" {
		// Mock for Mac
		return games, nil
	}

	// Basic parsing or just hardcoded scan for now as struct is complex
	// Scanning common folders is more robust if config format changes.
	commonPaths := []string{
		"C:\\Program Files (x86)\\Battle.net",
		"C:\\Program Files\\Battle.net",
		"C:\\Program Files (x86)\\World of Warcraft",
		"C:\\Program Files (x86)\\Call of Duty",
		"C:\\Program Files (x86)\\Overwatch",
		"C:\\Program Files (x86)\\Diablo IV",
		"C:\\Program Files (x86)\\Hearthstone",
		"C:\\Program Files (x86)\\StarCraft II",
	}

	// Helper to check and add
	checkAndAdd := func(path, name, id string) {
		if _, err := os.Stat(path); err == nil {
			games = append(games, models.LibraryGame{
				ID:          "bnet_" + id,
				Name:        name,
				Platform:    "Battle.net",
				IsInstalled: true,
				ExePath:     filepath.Join(path, name+".exe"), // Guessing executable
				// IconURL: ...
			})
		}
	}

	// This is very basic. In reality we should parse .product.db or .build.info
	// But those are binary/protobuf often.
	// For MVP, we scan common dirs.
	for _, p := range commonPaths {
		name := filepath.Base(p)
		checkAndAdd(p, name, name)
	}

	return games, nil
}

func (b *BattleNetScanner) GetAccountsProvider() ([]models.Account, error) {
	// We can list folders in our swch data storage
	storageBase, err := b.SwapConfig.GetStoragePath()
	if err != nil {
		return nil, err
	}

	entries, err := os.ReadDir(storageBase)
	if err != nil {
		// If dir doesn't exist, no accounts saved yet
		return []models.Account{}, nil
	}

	var accounts []models.Account
	for _, e := range entries {
		if e.IsDir() && e.Name() != "DIRS" && e.Name() != "FILES" { // Ignore internal dirs if any (implementation keeps DIRS/FILES inside account folder)
			accounts = append(accounts, models.Account{
				ID:       "bnet_" + e.Name(),
				Username: e.Name(),
				Platform: "Battle.net",
			})
		}
	}
	return accounts, nil
}

func (b *BattleNetScanner) SwitchAccount(accountName string) error {
	// 1. Save current "Anonymous" or "Last" state potentially?
	// For now, user must manually save "Current" as "Name" before switching.
	// We just load the target.

	if runtime.GOOS != "windows" {
		return fmt.Errorf("switching not supported on %s", runtime.GOOS)
	}

	err := b.SwapConfig.LoadState(accountName)
	if err != nil {
		return err
	}

	// Restart Battle.net
	// We killed it in LoadState. Now we launch it.
	// Path? We need to find the executable.
	exePath := "C:\\Program Files (x86)\\Battle.net\\Battle.net.exe"
	if _, err := os.Stat(exePath); os.IsNotExist(err) {
		// Try another common path
		exePath = "C:\\Program Files\\Battle.net\\Battle.net.exe"
	}

	return sys.RunExecutable(exePath)
}

func (b *BattleNetScanner) LaunchGame(gameID string, exePath string) error {
	// Battle.net games usually launched via "battlenet://<ProductCode>"
	// parsing product code from ID?
	// Or just running ExePath if provided.
	if exePath != "" {
		return sys.RunExecutable(exePath)
	}
	return fmt.Errorf("exe path required for Battle.net game launch")
}
