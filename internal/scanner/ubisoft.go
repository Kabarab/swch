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

type UbisoftScanner struct {
	SwapConfig *FileSwapConfig
}

func NewUbisoftScanner() interfaces.PlatformProvider {
	localAppData := GetWindowsEnv("LOCALAPPDATA")
	// Mock for Mac
	if localAppData == "" {
		localAppData = "/tmp/mock/LocalAppData"
	}

	config := &FileSwapConfig{
		ID:           "ubisoft",
		ProcessNames: []string{"upc.exe", "Uplay.exe", "UbisoftGameLauncher.exe"},
		Directories: []string{
			filepath.Join(localAppData, "Ubisoft Game Launcher"),
		},
		Files: []string{
			// Key session files
			filepath.Join(localAppData, "Ubisoft Game Launcher", "users.dat"),
			filepath.Join(localAppData, "Ubisoft Game Launcher", "settings.yml"),
		},
	}

	return &UbisoftScanner{
		SwapConfig: config,
	}
}

func (u *UbisoftScanner) GetName() string {
	return "Ubisoft Connect"
}

func (u *UbisoftScanner) GetGamesProvider() ([]models.LibraryGame, error) {
	var games []models.LibraryGame

	// Scan common install folder
	// default: C:\Program Files (x86)\Ubisoft\Ubisoft Game Launcher\games
	commonPath := "C:\\Program Files (x86)\\Ubisoft\\Ubisoft Game Launcher\\games"

	if runtime.GOOS != "windows" {
		return games, nil
	}

	entries, err := os.ReadDir(commonPath)
	if err != nil {
		return games, nil // Ignore error if default path doesn't exist
	}

	for _, e := range entries {
		if e.IsDir() {
			games = append(games, models.LibraryGame{
				ID:          "ubi_" + e.Name(),
				Name:        e.Name(),
				Platform:    "Ubisoft",
				IsInstalled: true,
				ExePath:     filepath.Join(commonPath, e.Name(), e.Name()+".exe"), // Guessing
			})
		}
	}

	return games, nil
}

func (u *UbisoftScanner) GetAccountsProvider() ([]models.Account, error) {
	storageBase, err := u.SwapConfig.GetStoragePath()
	if err != nil {
		return nil, err
	}

	entries, err := os.ReadDir(storageBase)
	if err != nil {
		return []models.Account{}, nil
	}

	var accounts []models.Account
	for _, e := range entries {
		if e.IsDir() && e.Name() != "DIRS" && e.Name() != "FILES" {
			accounts = append(accounts, models.Account{
				ID:       "ubi_" + e.Name(),
				Username: e.Name(),
				Platform: "Ubisoft",
			})
		}
	}
	return accounts, nil
}

func (u *UbisoftScanner) SwitchAccount(accountName string) error {
	if runtime.GOOS != "windows" {
		return fmt.Errorf("switching not supported on %s", runtime.GOOS)
	}

	err := u.SwapConfig.LoadState(accountName)
	if err != nil {
		return err
	}

	exePath := "C:\\Program Files (x86)\\Ubisoft\\Ubisoft Game Launcher\\UbisoftGameLauncher.exe"
	return sys.RunExecutable(exePath)
}

func (u *UbisoftScanner) LaunchGame(gameID string, exePath string) error {
	// Ubisoft games: uplay://launch/{gameId}
	// Need map of Name -> AppID
	// For now, launch EXE directly if provided
	if exePath != "" {
		return sys.RunExecutable(exePath)
	}
	return fmt.Errorf("exe path required for Ubisoft game launch")
}
