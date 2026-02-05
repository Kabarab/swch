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

// GetGamesProvider implements PlatformProvider
func (c *EpicClient) GetGamesProvider() ([]models.LibraryGame, error) {
	// 1. Verify session (optional, but good practice)
	if err := c.LoadSession(); err != nil {
		fmt.Println("[Epic] Нет активной сессии Epic:", err)
		// We might still want to return installed games via manifest if no session?
		// app.go logic had a fallback.
		// For now, let's try to get full library, which internally does entitlements + manifests.
		// If auth fails, GetFullLibrary fails.
	}

	var library []models.LibraryGame
	var libraryMap = make(map[string]bool) // To avoid duplicates if we merge sources

	// 1. Try to fetch full library from API (requires active session)
	// We do this first because it gives better metadata (images, titles)
	apiItems, err := c.GetFullLibrary()
	if err != nil {
		fmt.Println("[Epic] Note: Failed to fetch API library (not logged in?), falling back to local manifests:", err)
	} else {
		for _, item := range apiItems {
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

			userName := "Unknown"
			if c.Token != nil {
				userName = c.Token.DisplayName
			}

			libGame := models.LibraryGame{
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
			library = append(library, libGame)
			libraryMap[gameID] = true
		}
	}

	// 2. Always scan local manifests for installed games
	// This ensures we show games even if offline or not logged in via API
	localManifests := scanner.ScanEpicGames()
	for _, m := range localManifests {
		// If we already have this game from API, we might want to update it (e.g. valid ExePath)
		// or just skip. API usually has better data, but local manifest has correct install path.

		// For now, let's append if not exists, or update path if exists?
		// Simple approach: Add if not in map.
		// Note: IDs might differ. Manifest uses AppName. API uses ItemId/AppId.
		// c.GetFullLibrary tries to match, so ideally they match.

		if !libraryMap[m.ID] {
			library = append(library, m)
		}
	}

	return library, nil
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
