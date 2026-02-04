package epic

import (
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

func (c *EpicClient) LoginWithExchangeCode(code string) error {
	data := url.Values{}
	
	// Determine if it's likely an exchange code or already a SID
	// If it contains "sid", we might need a different flow, but usually, 
	// for native client we use "exchange_code" or "authorization_code".
	
	// Try to parse as JSON first just in case
	var jsonMap map[string]interface{}
	if err := json.Unmarshal([]byte(code), &jsonMap); err == nil {
		if sid, ok := jsonMap["sid"].(string); ok {
			code = sid
		} else if ac, ok := jsonMap["authorizationCode"].(string); ok {
			code = ac
		}
	}

	data.Set("grant_type", "exchange_code")
	data.Set("exchange_code", code)
	data.Set("token_type", "eg1")

	return c.requestToken(data)
}

func (c *EpicClient) LoginWithRefreshToken(refreshToken string) error {
	data := url.Values{}
	data.Set("grant_type", "refresh_token")
	data.Set("refresh_token", refreshToken)
	data.Set("token_type", "eg1")

	return c.requestToken(data)
}

func (c *EpicClient) requestToken(data url.Values) error {
	req, err := http.NewRequest("POST", OAuthTokenURL, strings.NewReader(data.Encode()))
	if err != nil {
		return err
	}

	auth := base64.StdEncoding.EncodeToString([]byte(LauncherAppClientID + ":" + LauncherAppSecret))
	req.Header.Set("Authorization", "Basic "+auth)
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

	c.Token = &token
	return c.SaveSession()
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

func (c *EpicClient) LoadSession() error {
	path := filepath.Join(c.ConfigDir, "session.json")
	if _, err := os.Stat(path); os.IsNotExist(err) {
		return fmt.Errorf("session not found")
	}

	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	var token TokenResponse
	if err := json.Unmarshal(data, &token); err != nil {
		return err
	}

	// Check if token needs refresh (heuristic: 5 minutes before expiry)
	if time.Now().After(token.ExpiresAt.Add(-5 * time.Minute)) {
		return c.LoginWithRefreshToken(token.RefreshToken)
	}

	c.Token = &token
	return nil
}

func (c *EpicClient) GetExchangeCode() (string, error) {
	if c.Token == nil {
		return "", fmt.Errorf("not authenticated")
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
		body, _ := io.ReadAll(resp.Body)
		return "", fmt.Errorf("exchange code request failed: %s - %s", resp.Status, string(body))
	}

	var res ExchangeCodeResponse
	if err := json.NewDecoder(resp.Body).Decode(&res); err != nil {
		return "", err
	}

	return res.Code, nil
}
