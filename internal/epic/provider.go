package epic

import (
	"fmt"
	"swch/internal/models"
	"swch/internal/scanner"
)

// GetName implements PlatformProvider
func (c *EpicClient) GetName() string {
	return "Epic"
}

// ScanInstalledGames implements interfaces.LocalLibraryScanner:
// установленные игры из локальных манифестов EGS (Data/Manifests/*.item).
// Работает без сессии и без сети.
func (c *EpicClient) ScanInstalledGames() ([]models.LibraryGame, error) {
	return scanner.ScanEpicGames(), nil
}

// GetRemoteLibrary implements interfaces.RemoteLibraryProvider: купленные игры
// через Epic API (entitlements + catalog). Токены берутся из сохраненной
// сессии модуля аккаунтов (LoadSession).
func (c *EpicClient) GetRemoteLibrary() ([]models.LibraryGame, error) {
	if err := c.LoadSession(); err != nil {
		return nil, fmt.Errorf("нет активной сессии Epic: %w", err)
	}

	apiItems, err := c.GetFullLibrary()
	if err != nil {
		return nil, fmt.Errorf("ошибка получения библиотеки Epic: %w", err)
	}

	userName := "Unknown"
	if c.Token != nil {
		userName = c.Token.DisplayName
	}

	var games []models.LibraryGame
	for _, item := range apiItems {
		games = append(games, apiItemToLibraryGame(item, userName))
	}
	return games, nil
}

// apiItemToLibraryGame приводит CatalogItem из Epic API к единому формату.
func apiItemToLibraryGame(item CatalogItem, userName string) models.LibraryGame {
	// Find icon
	imgURL := "https://upload.wikimedia.org/wikipedia/commons/3/31/Epic_Games_logo.svg"
	for _, img := range item.KeyImages {
		if img.Type == "Thumbnail" || img.Type == "DieselGameBox" {
			imgURL = img.URL
			break
		}
	}

	// Use AppID if available (from manifest match), otherwise ID (Catalog ID)
	gameID := item.ID
	if item.AppID != "" {
		gameID = item.AppID
	}

	return models.LibraryGame{
		ID:          gameID,
		Name:        item.Title,
		Platform:    "Epic",
		IconURL:     imgURL,
		ExePath:     item.InstallPath,
		IsInstalled: item.IsInstalled,
		AvailableOnAccounts: []models.AccountStat{
			{
				AccountID:   "epic_api", // Generic ID for API source
				DisplayName: "Epic Account",
				Username:    userName,
			},
		},
	}
}

// GetGamesProvider implements PlatformProvider (legacy-формат "все одним
// списком"). services/library использует гранулярные методы напрямую.
func (c *EpicClient) GetGamesProvider() ([]models.LibraryGame, error) {
	remote, err := c.GetRemoteLibrary()
	if err != nil {
		fmt.Println("[Epic] Note: Failed to fetch API library (not logged in?), falling back to local manifests:", err)
	}

	// Локальные манифесты добавляются всегда: игры видны даже оффлайн.
	// API дает лучшие метаданные, манифест — корректный путь установки;
	// MergeLibraries объединяет их по ID.
	installed, _ := c.ScanInstalledGames()
	return models.MergeLibraries(remote, installed), nil
}

// GetAccountsProvider implements PlatformProvider
func (c *EpicClient) GetAccountsProvider() ([]models.Account, error) {
	// Use scanner to get native accounts
	sessions := scanner.ScanEpicAccounts()
	// No error check needed as it returns empty slice on error

	var accounts []models.Account
	for _, s := range sessions {
		// scanner returns models.Account, so we just use them
		// But valid platform is "Epic"
		s.Platform = "Epic"
		accounts = append(accounts, s)
	}

	// Add current verified session if not in list
	// (Logic similar to app.go could be added here if needed, but ListSessions usually covers saved profiles)

	return accounts, nil
}

// SwitchAccount implements PlatformProvider
func (c *EpicClient) SwitchAccount(accountName string) error {
	fmt.Printf("[Epic] Request to switch account to: %s\n", accountName)
	// If it's already active, maybe check? But SwitchSession handles loading from profile.
	return scanner.SwitchEpicAccount(accountName)
}

// LaunchGame implements PlatformProvider
func (c *EpicClient) LaunchGame(gameID string, exePath string) error {
	// If exePath is empty, try to find it via manifest (ScanInstalledGames)
	// GetFullLibrary already fills ExePath but LaunchGame might be called with empty path if cached data is stale?
	// For now, assume exePath is provided or we try to re-scan.

	targetPath := exePath
	if targetPath == "" {
		installed, _ := ScanInstalledGames()
		if game, ok := installed[gameID]; ok {
			targetPath = game.InstallPath
		}
	}

	cmd, err := c.GetLaunchCommand(gameID, targetPath)
	if err != nil {
		return fmt.Errorf("error preparing launch: %w", err)
	}

	if err := c.RunExecutable(cmd); err != nil {
		return fmt.Errorf("error launching: %w", err)
	}
	return nil
}
