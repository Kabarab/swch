package epic

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
)

func (c *EpicClient) GetEntitlements() ([]Entitlement, error) {
	if c.Token == nil {
		return nil, fmt.Errorf("not authenticated")
	}

	u, _ := url.Parse(EntitlementsURL)
	// We might need to handle pagination if the user has many games
	// but for now let's assume one page is enough.

	req, err := http.NewRequest("GET", u.String(), nil)
	if err != nil {
		return nil, err
	}

	req.Header.Set("Authorization", "bearer "+c.Token.AccessToken)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("entitlements request failed: %s - %s", resp.Status, string(body))
	}

	var entitlements []Entitlement
	if err := json.NewDecoder(resp.Body).Decode(&entitlements); err != nil {
		return nil, err
	}

	return entitlements, nil
}

func (c *EpicClient) GetCatalogItems(namespace string, itemIds []string) ([]CatalogItem, error) {
	if c.Token == nil {
		return nil, fmt.Errorf("not authenticated")
	}

	if len(itemIds) == 0 {
		return nil, nil
	}

	// Catalog Service URL: https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace/{namespace}/bulk/items
	endpoint := fmt.Sprintf("%s/catalog/api/shared/namespace/%s/bulk/items", CatalogHost, namespace)

	params := url.Values{}
	for _, id := range itemIds {
		params.Add("id", id)
	}

	u, _ := url.Parse(endpoint)
	u.RawQuery = params.Encode()

	req, err := http.NewRequest("GET", u.String(), nil)
	if err != nil {
		return nil, err
	}

	req.Header.Set("Authorization", "bearer "+c.Token.AccessToken)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("catalog request failed: %s - %s", resp.Status, string(body))
	}

	var items map[string]CatalogItem
	if err := json.NewDecoder(resp.Body).Decode(&items); err != nil {
		return nil, err
	}

	var result []CatalogItem
	for _, item := range items {
		result = append(result, item)
	}

	return result, nil
}

// GetFullLibrary fetches all games associated with the account
// It fetches entitlements, enriches them with catalog data (images, titles),
// and checks local installation status via manifests.
func (c *EpicClient) GetFullLibrary() ([]CatalogItem, error) {
	// 1. Fetch Entitlements (What the user owns)
	entitlements, err := c.GetEntitlements()
	if err != nil {
		return nil, fmt.Errorf("failed to get entitlements: %w", err)
	}

	// 2. Scan Local Manifests (What is installed)
	installedGames, _ := ScanInstalledGames()

	// 3. Prepare for Catalog Fetching
	// Group items by namespace to batch requests
	byNamespace := make(map[string][]string)

	// Map to track which entitlements we are processing to avoid duplicates
	// Key: CatalogItemID, Value: Entitlement
	entitlementMap := make(map[string]Entitlement)

	for _, e := range entitlements {
		// Filter relevant entitlements (Games, DLCs, Software)
		// Usually EXECUTABLE is what we want for games.
		// Sometimes 'AUDIENCE' or 'ENTITLEMENT' types exist too.
		if e.EntitlementType == "EXECUTABLE" && e.CatalogItemId != "" {
			byNamespace[e.Namespace] = append(byNamespace[e.Namespace], e.CatalogItemId)
			entitlementMap[e.CatalogItemId] = e
		}
	}

	var allItems []CatalogItem

	// 4. Batch Fetch Catalog Items
	for ns, ids := range byNamespace {
		// Epic API might limit batch size, but usually 50-100 is fine.
		// We'll send them in chunks if needed, but for now simple implementation.
		items, err := c.GetCatalogItems(ns, ids)
		if err != nil {
			fmt.Printf("Warning: failed to fetch catalog items for namespace %s: %v\n", ns, err)
			continue
		}
		allItems = append(allItems, items...)
	}

	// 5. Merge Data
	for i := range allItems {
		item := &allItems[i]

		// Check invalid images
		// Sometimes Epic returns broken URLs or empty lists

		// Check installation status
		// We match by AppID (which is usually the ArtifactId or ItemId depending on context)
		// The CatalogItem ID usually matches the .item file AppName for the main game.
		// However, sometimes it is more complex.
		// Manifest.AppName matches the ArtifactID.

		// Let's try to match by ID first
		if installed, ok := installedGames[item.ID]; ok {
			item.IsInstalled = true
			item.InstallPath = installed.InstallPath
			item.AppID = installed.AppID // Ensure we have the correct launch ID
		}

		// If not found by ID, sometimes the entitlement ID differs from the Artifact ID.
		// But for now, direct ID match is the standard way.
	}

	// Also include installed games that might not be in the entitlement list
	// (e.g. Free games claimed long ago or on a different account but still installed)
	// This is optional but good UX. For now, we only show what the USER owns on THIS account.
	// Implementing "Installed but not owned" requires cross-referencing.

	// We might want to filter out DLCs if purely showing "Library".
	// Usually checking if the category contains "games" helps.
	var gamesOnly []CatalogItem
	for _, item := range allItems {
		isGame := false
		for _, cat := range item.Categories {
			if strings.Contains(cat.Path, "games") || strings.Contains(cat.Path, "applications") {
				isGame = true
				break
			}
		}
		// If no category logic is strict, we include everything that is an EXECUTABLE.
		if isGame {
			gamesOnly = append(gamesOnly, item)
		}
	}

	return gamesOnly, nil
}
