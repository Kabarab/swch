package scanner

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
)

// Epic OAuth constants
const (
	// launcherAppClient2 - public Epic credentials used by legendary and other tools
	EpicClientID     = "34a02cf8f4414e29b15921876da36f9a"
	EpicClientSecret = "daafbccc737745039dffe53d94fc76cf"

	// API endpoints
	EpicTokenURL      = "https://account-public-service-prod.ol.epicgames.com/account/api/oauth/token"
	EpicDeviceAuthURL = "https://account-public-service-prod.ol.epicgames.com/account/api/public/account/%s/deviceAuth"
	EpicExchangeURL   = "https://account-public-service-prod.ol.epicgames.com/account/api/oauth/exchange"

	// Auth login URL - user will be redirected here
	EpicAuthLoginURL = "https://www.epicgames.com/id/api/redirect?clientId=%s&responseType=code"
)

// DeviceAuth stores the device authentication credentials
// These credentials don't expire until the user changes their password
type DeviceAuth struct {
	AccountID string `json:"accountId"`
	DeviceID  string `json:"deviceId"`
	Secret    string `json:"secret"`
}

// TokenResponse represents the OAuth token response from Epic
type TokenResponse struct {
	AccessToken  string `json:"access_token"`
	RefreshToken string `json:"refresh_token"`
	ExpiresIn    int    `json:"expires_in"`
	TokenType    string `json:"token_type"`
	AccountID    string `json:"account_id"`
	DisplayName  string `json:"displayName"`
	ErrorCode    string `json:"errorCode,omitempty"`
	ErrorMessage string `json:"errorMessage,omitempty"`
}

// ExchangeCodeResponse represents the exchange code response
type ExchangeCodeResponse struct {
	ExpiresInSeconds int    `json:"expiresInSeconds"`
	Code             string `json:"code"`
	CreatingClientID string `json:"creatingClientId"`
}

// getAuthHeader returns the Basic auth header for Epic API
func getEpicAuthHeader() string {
	credentials := EpicClientID + ":" + EpicClientSecret
	return "Basic " + base64.StdEncoding.EncodeToString([]byte(credentials))
}

// ExchangeAuthCodeForToken exchanges an authorization code for access token
func ExchangeAuthCodeForToken(authCode string) (*TokenResponse, error) {
	data := url.Values{}
	data.Set("grant_type", "authorization_code")
	data.Set("code", authCode)

	req, err := http.NewRequest("POST", EpicTokenURL, strings.NewReader(data.Encode()))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Authorization", getEpicAuthHeader())

	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)

	var tokenResp TokenResponse
	if err := json.Unmarshal(body, &tokenResp); err != nil {
		return nil, fmt.Errorf("failed to parse response: %w", err)
	}

	if tokenResp.ErrorCode != "" {
		return nil, fmt.Errorf("Epic API error: %s - %s", tokenResp.ErrorCode, tokenResp.ErrorMessage)
	}

	return &tokenResp, nil
}

// CreateDeviceAuth creates device auth credentials using an access token
// These credentials can be used to get new access tokens without user interaction
func CreateDeviceAuth(accessToken, accountID string) (*DeviceAuth, error) {
	apiURL := fmt.Sprintf(EpicDeviceAuthURL, accountID)
	fmt.Printf("[Epic:Auth] Creating Device Auth for %s. URL: %s\n", accountID, apiURL)

	req, err := http.NewRequest("POST", apiURL, bytes.NewBuffer([]byte{}))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Content-Type", "application/json")

	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	fmt.Printf("[Epic:Auth] Device Auth Response (%d): %s\n", resp.StatusCode, string(body))

	if resp.StatusCode != 200 {
		return nil, fmt.Errorf("failed to create device auth (status %d): %s", resp.StatusCode, string(body))
	}

	var result struct {
		DeviceID  string `json:"deviceId"`
		AccountID string `json:"accountId"`
		Secret    string `json:"secret"`
	}

	if err := json.Unmarshal(body, &result); err != nil {
		return nil, fmt.Errorf("failed to parse device auth response: %w", err)
	}

	return &DeviceAuth{
		AccountID: result.AccountID,
		DeviceID:  result.DeviceID,
		Secret:    result.Secret,
	}, nil
}

// LoginWithDeviceAuth gets an access token using device auth credentials
func LoginWithDeviceAuth(auth *DeviceAuth) (*TokenResponse, error) {
	data := url.Values{}
	data.Set("grant_type", "device_auth")
	data.Set("account_id", auth.AccountID)
	data.Set("device_id", auth.DeviceID)
	data.Set("secret", auth.Secret)

	req, err := http.NewRequest("POST", EpicTokenURL, strings.NewReader(data.Encode()))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Authorization", getEpicAuthHeader())

	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)

	var tokenResp TokenResponse
	if err := json.Unmarshal(body, &tokenResp); err != nil {
		return nil, fmt.Errorf("failed to parse response: %w", err)
	}

	if tokenResp.ErrorCode != "" {
		return nil, fmt.Errorf("Epic API error: %s - %s", tokenResp.ErrorCode, tokenResp.ErrorMessage)
	}

	return &tokenResp, nil
}

// GetExchangeCode gets an exchange code that can be used to launch Epic Launcher
func GetExchangeCode(accessToken string) (string, error) {
	req, err := http.NewRequest("GET", EpicExchangeURL, nil)
	if err != nil {
		return "", fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Authorization", "Bearer "+accessToken)

	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		return "", fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)

	if resp.StatusCode != 200 {
		return "", fmt.Errorf("failed to get exchange code (status %d): %s", resp.StatusCode, string(body))
	}

	var result ExchangeCodeResponse
	if err := json.Unmarshal(body, &result); err != nil {
		return "", fmt.Errorf("failed to parse exchange code response: %w", err)
	}

	return result.Code, nil
}

// SaveDeviceAuth saves device auth credentials to the account directory
func SaveDeviceAuth(accountDir string, auth *DeviceAuth) error {
	authPath := filepath.Join(accountDir, "device_auth.json")

	data, err := json.MarshalIndent(auth, "", "  ")
	if err != nil {
		return fmt.Errorf("failed to marshal device auth: %w", err)
	}

	if err := os.WriteFile(authPath, data, 0600); err != nil {
		return fmt.Errorf("failed to save device auth: %w", err)
	}

	fmt.Printf("[Epic] Saved device auth to %s\n", authPath)
	return nil
}

// LoadDeviceAuth loads device auth credentials from the account directory
func LoadDeviceAuth(accountDir string) (*DeviceAuth, error) {
	authPath := filepath.Join(accountDir, "device_auth.json")

	data, err := os.ReadFile(authPath)
	if err != nil {
		return nil, fmt.Errorf("failed to read device auth: %w", err)
	}

	var auth DeviceAuth
	if err := json.Unmarshal(data, &auth); err != nil {
		return nil, fmt.Errorf("failed to parse device auth: %w", err)
	}

	return &auth, nil
}

// GetEpicAuthURL returns the URL for Epic login
func GetEpicAuthURL() string {
	return fmt.Sprintf(EpicAuthLoginURL, EpicClientID)
}
