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

type GogScanner struct {
	SwapConfig *FileSwapConfig
}

func NewGogScanner() interfaces.PlatformProvider {
	programData := GetWindowsEnv("ProgramData")
	appData := GetWindowsEnv("APPDATA")
	localAppData := GetWindowsEnv("LOCALAPPDATA")

	if programData == "" {
		programData = "/tmp/mock/ProgramData"
	}
	if appData == "" {
		appData = "/tmp/mock/AppData"
	}
	if localAppData == "" {
		localAppData = "/tmp/mock/LocalAppData"
	}

	config := &FileSwapConfig{
		ID:           "gog",
		ProcessNames: []string{"GalaxyClient.exe", "GalaxyClientService.exe", "GalaxyCommunication.exe"},
		Directories: []string{
			filepath.Join(programData, "GOG.com", "Galaxy", "storage"),
			filepath.Join(localAppData, "GOG.com", "Galaxy", "Configuration"),
		},
		Files: []string{
			// Add specific config files if directories aren't enough
		},
	}

	return &GogScanner{
		SwapConfig: config,
	}
}

func (g *GogScanner) GetName() string {
	return "GOG Galaxy"
}

func (g *GogScanner) GetGamesProvider() ([]models.LibraryGame, error) {
	var games []models.LibraryGame

	// Default Install Path
	commonPath := "C:\\Program Files (x86)\\GOG Galaxy\\Games"

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
				ID:          "gog_" + entry.Name(),
				Name:        entry.Name(),
				Platform:    "GOG Galaxy",
				IsInstalled: true,
				ExePath:     filepath.Join(commonPath, entry.Name(), "Launch "+entry.Name()+".lnk"), // GOG usually creates LNKs or we guess EXE
				// Real EXE guessing is hard without manifest.
			})
		}
	}
	return games, nil
}

func (g *GogScanner) GetAccountsProvider() ([]models.Account, error) {
	storageBase, err := g.SwapConfig.GetStoragePath()
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
				ID:       "gog_" + entry.Name(),
				Username: entry.Name(),
				Platform: "GOG Galaxy",
			})
		}
	}
	return accounts, nil
}

func (g *GogScanner) SwitchAccount(accountName string) error {
	if runtime.GOOS != "windows" {
		return fmt.Errorf("switching not supported on %s", runtime.GOOS)
	}

	err := g.SwapConfig.LoadState(accountName)
	if err != nil {
		return err
	}

	exePath := "C:\\Program Files (x86)\\GOG Galaxy\\GalaxyClient.exe"
	return sys.RunExecutable(exePath)
}

func (g *GogScanner) LaunchGame(gameID string, exePath string) error {
	// GOG usually launches via direct EXE or "goggalaxy://openGameView/{gameId}"
	if exePath != "" {
		return sys.RunExecutable(exePath)
	}
	return fmt.Errorf("exe path required for GOG game launch")
}
