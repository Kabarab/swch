package epic

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
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

// GetFullLibrary fetches entitlements and then hydrates them with catalog data
func (c *EpicClient) GetFullLibrary() ([]CatalogItem, error) {
	entitlements, err := c.GetEntitlements()
	if err != nil {
		return nil, err
	}

	// Group by namespace for bulk catalog requests
	byNamespace := make(map[string][]string)
	for _, e := range entitlements {
		if e.EntitlementType == "EXECUTABLE" {
			byNamespace[e.Namespace] = append(byNamespace[e.Namespace], e.CatalogItemId)
		}
	}

	var allItems []CatalogItem
	for ns, ids := range byNamespace {
		items, err := c.GetCatalogItems(ns, ids)
		if err != nil {
			fmt.Printf("Warning: failed to fetch catalog items for namespace %s: %v\n", ns, err)
			continue
		}
		allItems = append(allItems, items...)
	}

	return allItems, nil
}
