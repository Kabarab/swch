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

type WargamingScanner struct {
	SwapConfig *FileSwapConfig
}

func NewWargamingScanner() interfaces.PlatformProvider {
	programData := GetWindowsEnv("ProgramData")
	appData := GetWindowsEnv("APPDATA")

	if programData == "" {
		programData = "/tmp/mock/ProgramData"
	}
	if appData == "" {
		appData = "/tmp/mock/AppData"
	}

	config := &FileSwapConfig{
		ID:           "wargaming",
		ProcessNames: []string{"wgc.exe"},
		Directories: []string{
			filepath.Join(appData, "Wargaming.net", "GameCenter"),
			// Key auth data is likely in preferences.xml in ProgramData or AppData
			// Official docs say ProgramData/Wargaming.net/GameCenter
		},
		Files: []string{
			filepath.Join(programData, "Wargaming.net", "GameCenter", "preferences.xml"),
		},
	}

	return &WargamingScanner{
		SwapConfig: config,
	}
}

func (w *WargamingScanner) GetName() string {
	return "Wargaming.net"
}

func (w *WargamingScanner) GetGamesProvider() ([]models.LibraryGame, error) {
	var games []models.LibraryGame

	// Common Install: C:\Games
	// Wargaming usually installs to C:\Games\World_of_Tanks etc.
	commonPath := "C:\\Games"

	if runtime.GOOS != "windows" {
		return games, nil
	}

	entries, err := os.ReadDir(commonPath)
	if err != nil {
		return games, nil
	}

	for _, entry := range entries {
		if entry.IsDir() {
			// Check if likely a WG game
			// e.g. World_of_Tanks_EU
			name := entry.Name()
			if name == "World_of_Tanks_EU" || name == "World_of_Warships" || name == "World_of_Tanks_Blitz" {
				games = append(games, models.LibraryGame{
					ID:          "wg_" + name,
					Name:        name,
					Platform:    "Wargaming",
					IsInstalled: true,
					ExePath:     filepath.Join(commonPath, name, "wgc_api.exe"), // Often has launcher inside or we assume WGC manages it
				})
			}
		}
	}
	return games, nil
}

func (w *WargamingScanner) GetAccountsProvider() ([]models.Account, error) {
	storageBase, err := w.SwapConfig.GetStoragePath()
	if err != nil {
		return nil, err
	}

	entries, err := os.ReadDir(storageBase)
	if err != nil {
		return []models.Account{}, nil
	}

	var accounts []models.Account
	for _, entry := range entries {
		if entry.IsDir() && entry.Name() != "DIRS" && entry.Name() != "FILES" {
			accounts = append(accounts, models.Account{
				ID:       "wg_" + entry.Name(),
				Username: entry.Name(),
				Platform: "Wargaming",
			})
		}
	}
	return accounts, nil
}

func (w *WargamingScanner) SwitchAccount(accountName string) error {
	if runtime.GOOS != "windows" {
		return fmt.Errorf("switching not supported on %s", runtime.GOOS)
	}

	err := w.SwapConfig.LoadState(accountName)
	if err != nil {
		return err
	}

	exePath := "C:\\Program Files (x86)\\Wargaming.net\\GameCenter\\wgc.exe" // Or where installed
	return sys.RunExecutable(exePath)
}

func (w *WargamingScanner) LaunchGame(gameID string, exePath string) error {
	if exePath != "" {
		return sys.RunExecutable(exePath)
	}
	return fmt.Errorf("exe path required for Wargaming game launch")
}
