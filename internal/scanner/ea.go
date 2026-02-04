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

type EAScanner struct {
	SwapConfig *FileSwapConfig
}

func NewEAScanner() interfaces.PlatformProvider {
	appData := GetWindowsEnv("APPDATA")
	localAppData := GetWindowsEnv("LOCALAPPDATA")
	if appData == "" {
		appData = "/tmp/mock/AppData"
	}
	if localAppData == "" {
		localAppData = "/tmp/mock/LocalAppData"
	}

	config := &FileSwapConfig{
		ID:           "ea",
		ProcessNames: []string{"EADesktop.exe", "EABackgroundService.exe", "Link2EA.exe"},
		Directories: []string{
			filepath.Join(localAppData, "Electronic Arts", "EA Desktop"),
			filepath.Join(appData, "Electronic Arts", "EA Desktop"),
		},
		Files: []string{},
	}

	return &EAScanner{
		SwapConfig: config,
	}
}

func (e *EAScanner) GetName() string {
	return "EA App"
}

func (e *EAScanner) GetGamesProvider() ([]models.LibraryGame, error) {
	var games []models.LibraryGame

	// Scan common install folder
	// default: C:\Program Files\EA Games
	commonPath := "C:\\Program Files\\EA Games"

	if runtime.GOOS != "windows" {
		return games, nil
	}

	entries, err := os.ReadDir(commonPath)
	if err != nil {
		return games, nil
	}

	for _, entry := range entries {
		if entry.IsDir() {
			games = append(games, models.LibraryGame{
				ID:          "ea_" + entry.Name(),
				Name:        entry.Name(),
				Platform:    "EA App",
				IsInstalled: true,
				ExePath:     filepath.Join(commonPath, entry.Name(), entry.Name()+".exe"), // Guessing
			})
		}
	}

	return games, nil
}

func (e *EAScanner) GetAccountsProvider() ([]models.Account, error) {
	storageBase, err := e.SwapConfig.GetStoragePath()
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
				ID:       "ea_" + entry.Name(),
				Username: entry.Name(),
				Platform: "EA App",
			})
		}
	}
	return accounts, nil
}

func (e *EAScanner) SwitchAccount(accountName string) error {
	if runtime.GOOS != "windows" {
		return fmt.Errorf("switching not supported on %s", runtime.GOOS)
	}

	err := e.SwapConfig.LoadState(accountName)
	if err != nil {
		return err
	}

	exePath := "C:\\Program Files\\Electronic Arts\\EA Desktop\\EA Desktop\\EADesktop.exe"
	return sys.RunExecutable(exePath)
}

func (e *EAScanner) LaunchGame(gameID string, exePath string) error {
	// EA games launch via link2ea://launchgame/...
	if exePath != "" {
		return sys.RunExecutable(exePath)
	}
	return fmt.Errorf("exe path required for EA game launch")
}
