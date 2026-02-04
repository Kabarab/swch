package epic

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
)

// GetFullLibrary fetches all entitlements and attempts to resolve them to library items
func (c *EpicClient) GetFullLibrary() ([]CatalogItem, error) {
	if c.Token == nil {
		if err := c.LoadSession(); err != nil {
			return nil, fmt.Errorf("not authenticated: %w", err)
		}
	}

	entitlements, err := c.fetchEntitlements()
	if err != nil {
		return nil, err
	}

	// For a full implementation, we'd need to query the Catalog API to get details (Images, Titles) for each namespace/ID.
	// However, for now, we can try to map what we have or just return the raw entitlements as items if they have enough info.
	// Epic's Entitlement response doesn't have Title/Images. We NEED to query Catalog.

	// Group by namespace to batch requests? Or just query catalog items.
	// Using a bulk query if possible.
	// For simplicity in this step, let's just return what we can, but likely we need a Catalog query.
	// Let's implement a basic Catalog lookup.

	var items []CatalogItem
	// Limit to 50 for now or handle pagination/bulk
	// In a real app we'd need to be robust.
	// Map entitlement to CatalogItem
	for _, ent := range entitlements {
		// Only interest in games (entitlementType=EXECUTABLE? or similar, or just all)
		// Epic return "AUDIENCE", "EXECUTABLE" etc.
		// For now, let's just assume we want everything and filter later?
		// Actually, let's simple fetch details for valid IDs.
		// CatalogItemId is key.
		if ent.CatalogItemId != "" {
			item, err := c.fetchCatalogItem(ent.Namespace, ent.CatalogItemId)
			if err == nil {
				// Determine if installed?
				// Logic for "IsInstalled" would require checking local manifests (not scope of this specific function, handled in App)
				items = append(items, *item)
			}
		}
	}

	return items, nil
}

func (c *EpicClient) fetchEntitlements() ([]Entitlement, error) {
	u, _ := url.Parse(EntitlementsURL)
	q := u.Query()
	q.Set("accountId", c.Token.AccountID)
	// count? start?
	q.Set("count", "5000") // Fetch a lot
	u.RawQuery = q.Encode()

	req, err := http.NewRequest("GET", u.String(), nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Authorization", "Bearer "+c.Token.AccessToken)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		b, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("entitlements request failed: %s - %s", resp.Status, string(b))
	}

	var result []Entitlement
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, err
	}
	return result, nil
}

// fetchCatalogItem is a helper. In reality you'd want bulk fetching.
func (c *EpicClient) fetchCatalogItem(namespace, catalogItemId string) (*CatalogItem, error) {
	// https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace/{namespace}/catalogItem/{id}
	apiURL := fmt.Sprintf("%s/catalog/api/shared/namespace/%s/catalogItem/%s", CatalogHost, namespace, catalogItemId)

	req, err := http.NewRequest("GET", apiURL, nil)
	if err != nil {
		return nil, err
	}
	// Catalog is public usually? Or needs auth?
	req.Header.Set("Authorization", "Bearer "+c.Token.AccessToken)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("catalog item error")
	}

	var item CatalogItem
	if err := json.NewDecoder(resp.Body).Decode(&item); err != nil {
		return nil, err
	}
	return &item, nil
}
