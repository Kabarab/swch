package epic

import (
	"fmt"
	"swch/internal/models"
)

// GetName implements PlatformProvider
func (c *EpicClient) GetName() string {
	return "Epic"
}

// GetGamesProvider implements PlatformProvider
func (c *EpicClient) GetGamesProvider() ([]models.LibraryGame, error) {
	// 1. Verify session (optional, but good practice)
	if err := c.LoadSession(); err != nil {
		fmt.Println("[Epic] No active Epic session:", err)
		// We might still want to return installed games via manifest if no session?
		// app.go logic had a fallback.
		// For now, let's try to get full library, which internally does entitlements + manifests.
		// If auth fails, GetFullLibrary fails.
	}

	items, err := c.GetFullLibrary()
	if err != nil {
		fmt.Println("[Epic] Failed to fetch Epic library:", err)
		// Fallback: only installed games from manifests
		localManifests, _ := ScanInstalledGames()
		var library []models.LibraryGame
		for _, m := range localManifests {
			library = append(library, models.LibraryGame{
				ID:          m.AppID,
				Name:        m.Name,
				Platform:    "Epic",
				IconURL:     "https://upload.wikimedia.org/wikipedia/commons/3/31/Epic_Games_logo.svg",
				ExePath:     m.InstallPath,
				IsInstalled: true,
				// Cannot determine owner without auth, so leave empty or generic
				AvailableOnAccounts: []models.AccountStat{},
			})
		}
		return library, nil
	}

	var library []models.LibraryGame
	for _, item := range items {
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

		library = append(library, models.LibraryGame{
			ID:          gameID,
			Name:        item.Title,
			Platform:    "Epic",
			IconURL:     imgURL,
			ExePath:     item.InstallPath,
			IsInstalled: item.IsInstalled,
			AvailableOnAccounts: []models.AccountStat{
				{
					AccountID:   "epic_native",
					DisplayName: "Epic Account", // Todo: Use AccountID from token?
					Username:    userName,
				},
			},
		})
	}
	return library, nil
}

// GetAccountsProvider implements PlatformProvider
func (c *EpicClient) GetAccountsProvider() ([]models.Account, error) {
	sessions, err := c.ListSessions()
	if err != nil {
		return nil, err
	}

	var accounts []models.Account
	for _, s := range sessions {
		accounts = append(accounts, models.Account{
			ID:          "epic_" + s.Name, // Unique ID for UI
			DisplayName: s.DisplayName,
			Username:    s.Name, // Using profile name as username identifier
			Platform:    "Epic",
		})
	}

	// Add current verified session if not in list
	// (Logic similar to app.go could be added here if needed, but ListSessions usually covers saved profiles)

	return accounts, nil
}

// SwitchAccount implements PlatformProvider
func (c *EpicClient) SwitchAccount(accountName string) error {
	fmt.Printf("[Epic] Request to switch account to: %s\n", accountName)
	// If it's already active, maybe check? But SwitchSession handles loading from profile.
	return c.SwitchSession(accountName)
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
