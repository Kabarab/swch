package epic

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"
)

type EpicClient struct {
	httpClient *http.Client
	Token      *TokenResponse
	ConfigDir  string
}

func NewEpicClient() *EpicClient {
	configDir, _ := os.UserConfigDir()
	epicDir := filepath.Join(configDir, "swch", "epic_native")
	_ = os.MkdirAll(epicDir, 0755)

	return &EpicClient{
		httpClient: &http.Client{
			Timeout: 30 * time.Second,
		},
		ConfigDir: epicDir,
	}
}

// GetEpicAuthURL returns the URL to start the authorization flow
func GetEpicAuthURL() string {
	// Standard Epic Auth URL
	return fmt.Sprintf("https://www.epicgames.com/id/api/redirect?clientId=%s&responseType=code", LauncherAppClientID)
}

// DeviceAuth stores the device authentication credentials
type DeviceAuth struct {
	AccountID string `json:"accountId"`
	DeviceID  string `json:"deviceId"`
	Secret    string `json:"secret"`
}

// getAuthHeader returns the Basic auth header for Epic API
func (c *EpicClient) getAuthHeader() string {
	credentials := LauncherAppClientID + ":" + LauncherAppSecret
	// Debug log to verify correct credentials are loaded (showing first 4 chars)
	fmt.Printf("[EpicAuth] Using ClientID: %s... Secret: %s...\n",
		LauncherAppClientID[:4], LauncherAppSecret[:4])
	return "Basic " + base64.StdEncoding.EncodeToString([]byte(credentials))
}

func (c *EpicClient) LoginWithExchangeCode(code string) error {
	data := url.Values{}

	// Handle potential JSON input for code (some flows return JSON)
	var jsonMap map[string]interface{}
	if err := json.Unmarshal([]byte(code), &jsonMap); err == nil {
		if sid, ok := jsonMap["sid"].(string); ok {
			code = sid
		} else if ac, ok := jsonMap["authorizationCode"].(string); ok {
			code = ac
		} else if redirectURL, ok := jsonMap["redirectUrl"].(string); ok {
			// Epic sometimes returns the code inside a redirectUrl field
			if parsedURL, err := url.Parse(redirectURL); err == nil {
				if codeParam := parsedURL.Query().Get("code"); codeParam != "" {
					code = codeParam
				}
			}
		}
	}

	data.Set("grant_type", "authorization_code")
	data.Set("code", code)
	// data.Set("token_type", "eg1") // Removing this as it might cause invalid_client with some headers

	return c.requestToken(data)
}

func (c *EpicClient) LoginWithRefreshToken(refreshToken string) error {
	data := url.Values{}
	data.Set("grant_type", "refresh_token")
	data.Set("refresh_token", refreshToken)
	data.Set("token_type", "eg1")

	return c.requestToken(data)
}

func (c *EpicClient) LoginWithDeviceAuth(deviceAuth *DeviceAuth) error {
	data := url.Values{}
	data.Set("grant_type", "device_auth")
	data.Set("account_id", deviceAuth.AccountID)
	data.Set("device_id", deviceAuth.DeviceID)
	data.Set("secret", deviceAuth.Secret)
	data.Set("token_type", "eg1")

	return c.requestToken(data)
}

func (c *EpicClient) requestToken(data url.Values) error {
	req, err := http.NewRequest("POST", OAuthTokenURL, strings.NewReader(data.Encode()))
	if err != nil {
		return err
	}

	req.Header.Set("Authorization", c.getAuthHeader())
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("token request failed: %s - %s", resp.Status, string(body))
	}

	var token TokenResponse
	if err := json.NewDecoder(resp.Body).Decode(&token); err != nil {
		return err
	}

	// Calculate explicit expiry times
	now := time.Now()
	token.ExpiresAt = now.Add(time.Duration(token.ExpiresIn) * time.Second)
	if token.RefreshExpires > 0 {
		token.RefreshExpiresAt = now.Add(time.Duration(token.RefreshExpires) * time.Second)
	}

	c.Token = &token
	return c.SaveSession()
}

func (c *EpicClient) CreateDeviceAuth() (*DeviceAuth, error) {
	if c.Token == nil {
		return nil, fmt.Errorf("not authenticated")
	}

	apiURL := fmt.Sprintf(AuthHost+"/account/api/public/account/%s/deviceAuth", c.Token.AccountID)
	req, err := http.NewRequest("POST", apiURL, bytes.NewBuffer([]byte{}))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Authorization", "Bearer "+c.Token.AccessToken)
	req.Header.Set("Content-Type", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("device auth creation failed: %s - %s", resp.Status, string(body))
	}

	var result struct {
		DeviceID  string `json:"deviceId"`
		AccountID string `json:"accountId"`
		Secret    string `json:"secret"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, err
	}

	deviceAuth := &DeviceAuth{
		AccountID: result.AccountID,
		DeviceID:  result.DeviceID,
		Secret:    result.Secret,
	}

	// Automatically save device auth credential locally for future use
	_ = c.SaveDeviceAuth(deviceAuth)

	return deviceAuth, nil
}

func (c *EpicClient) SaveSession() error {
	if c.Token == nil {
		return nil
	}
	data, err := json.MarshalIndent(c.Token, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(c.ConfigDir, "session.json"), data, 0644)
}

func (c *EpicClient) SaveDeviceAuth(auth *DeviceAuth) error {
	data, err := json.MarshalIndent(auth, "", "  ")
	if err != nil {
		return err
	}
	// We save it generally, but in a real multi-account scenario we might want to key it by AccountID
	return os.WriteFile(filepath.Join(c.ConfigDir, "device_auth.json"), data, 0600)
}

func (c *EpicClient) LoadDeviceAuth() (*DeviceAuth, error) {
	path := filepath.Join(c.ConfigDir, "device_auth.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	var auth DeviceAuth
	if err := json.Unmarshal(data, &auth); err != nil {
		return nil, err
	}
	return &auth, nil
}

func (c *EpicClient) LoadSession() error {
	// 1. Try to load existing session
	path := filepath.Join(c.ConfigDir, "session.json")
	if _, err := os.Stat(path); err == nil {
		data, err := os.ReadFile(path)
		if err == nil {
			var token TokenResponse
			if err := json.Unmarshal(data, &token); err == nil {
				// Check expiry
				if time.Now().After(token.ExpiresAt.Add(-5 * time.Minute)) {
					// Need refresh
					// Only try if we have a refresh token
					if token.RefreshToken != "" {
						err := c.LoginWithRefreshToken(token.RefreshToken)
						if err == nil {
							return nil // Success with refresh
						}
						// If refresh failed, fall through to Device Auth
						fmt.Println("Refresh failed, trying Device Auth...", err)
					}
				} else {
					c.Token = &token
					return nil // Valid session
				}
			}
		}
	}

	// 2. If no valid session, try Device Auth
	deviceAuth, err := c.LoadDeviceAuth()
	if err == nil {
		return c.LoginWithDeviceAuth(deviceAuth)
	}

	// 3. If no device auth, try to import native session (Windows only for now)
	if err := c.TryImportNativeSession(); err == nil {
		fmt.Println("Successfully imported native session!")
		return nil
	}

	return fmt.Errorf("no valid session or device auth found, and native import failed")
}

func (c *EpicClient) GetExchangeCode() (string, error) {
	if c.Token == nil {
		// Try to ensure we are authenticated
		if err := c.LoadSession(); err != nil {
			return "", fmt.Errorf("not authenticated and failed to load session: %w", err)
		}
	}

	req, err := http.NewRequest("GET", OAuthExchangeURL, nil)
	if err != nil {
		return "", err
	}

	req.Header.Set("Authorization", "bearer "+c.Token.AccessToken)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		// If 401, maybe token expired just now?
		body, _ := io.ReadAll(resp.Body)
		return "", fmt.Errorf("exchange code request failed: %s - %s", resp.Status, string(body))
	}

	var res ExchangeCodeResponse
	if err := json.NewDecoder(resp.Body).Decode(&res); err != nil {
		return "", err
	}

	return res.Code, nil
}

// AccountData represents metadata for a saved account
type AccountData struct {
	ID          string `json:"id"`
	DisplayName string `json:"displayName"`
	Name        string `json:"name"` // Local alias
}

// SaveSessionAs saves the current session as a named profile
func (c *EpicClient) SaveSessionAs(name string) error {
	if c.Token == nil {
		return fmt.Errorf("no active session to save")
	}

	// Create directory for the profile
	profileDir := filepath.Join(c.ConfigDir, "profiles", name)
	if err := os.MkdirAll(profileDir, 0755); err != nil {
		return err
	}

	// Save session
	data, err := json.MarshalIndent(c.Token, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(profileDir, "session.json"), data, 0644); err != nil {
		return err
	}

	// Save metadata
	meta := AccountData{
		ID:          c.Token.AccountID,
		DisplayName: c.Token.DisplayName,
		Name:        name,
	}
	metaData, _ := json.MarshalIndent(meta, "", "  ")
	return os.WriteFile(filepath.Join(profileDir, "meta.json"), metaData, 0644)
}

// SwitchSession switches the active session to the named profile
func (c *EpicClient) SwitchSession(name string) error {
	profileDir := filepath.Join(c.ConfigDir, "profiles", name)
	sessionPath := filepath.Join(profileDir, "session.json")

	if _, err := os.Stat(sessionPath); os.IsNotExist(err) {
		return fmt.Errorf("profile '%s' not found", name)
	}

	// Read the session from profile
	data, err := os.ReadFile(sessionPath)
	if err != nil {
		return err
	}

	// Verify it's valid
	var token TokenResponse
	if err := json.Unmarshal(data, &token); err != nil {
		return err
	}

	// Set as current token
	c.Token = &token

	// Save as the *active* session (session.json in root config dir)
	return c.SaveSession()
}

// ListSessions lists all saved profiles
func (c *EpicClient) ListSessions() ([]AccountData, error) {
	profilesDir := filepath.Join(c.ConfigDir, "profiles")
	entries, err := os.ReadDir(profilesDir)
	if err != nil {
		// If dir doesn't exist, just return empty
		return nil, nil
	}

	var accounts []AccountData
	for _, e := range entries {
		if e.IsDir() {
			metaPath := filepath.Join(profilesDir, e.Name(), "meta.json")
			if data, err := os.ReadFile(metaPath); err == nil {
				var meta AccountData
				if err := json.Unmarshal(data, &meta); err == nil {
					accounts = append(accounts, meta)
				}
			}
		}
	}
	return accounts, nil
}

// Logout clears the current session
func (c *EpicClient) Logout() error {
	c.Token = nil
	// Remove session.json
	path := filepath.Join(c.ConfigDir, "session.json")
	if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
		return err
	}
	return nil
}
