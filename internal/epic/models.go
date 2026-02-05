package epic

import (
	"time"
)

const (
	// API Endpoints
	AuthHost            = "https://account-public-service-prod03.ol.epicgames.com"
	EntitlementsHost    = "https://ecommerceapp-public.ak.epicgames.com"
	CatalogHost         = "https://catalog-public-service-prod06.ol.epicgames.com"
	LauncherServiceHost = "https://launcher-public-service-prod06.ol.epicgames.com"

	// Auth URLs
	OAuthTokenURL    = AuthHost + "/account/api/oauth/token"
	OAuthExchangeURL = AuthHost + "/account/api/oauth/exchange"
	OAuthVerifyURL   = AuthHost + "/account/api/oauth/verify"

	// Library URLs
	EntitlementsURL = EntitlementsHost + "/ecommerceapp/api/public/v1/entitlements"

	// Default Client ID for Epic Games Launcher
	// This one is often used for exchange code flow
	LauncherAppClientID = "34a02cf8f4414e29b15921876da36f9a"
	LauncherAppSecret   = "1A697711ED0D47C68DA6031A68A78619"
)

type TokenResponse struct {
	AccessToken      string    `json:"access_token"`
	ExpiresIn        int       `json:"expires_in"`
	ExpiresAt        time.Time `json:"expires_at"`
	TokenType        string    `json:"token_type"`
	RefreshToken     string    `json:"refresh_token"`
	RefreshExpires   int       `json:"refresh_expires"`
	RefreshExpiresAt time.Time `json:"refresh_expires_at"`
	AccountID        string    `json:"account_id"`
	ClientID         string    `json:"client_id"`
	InternalClient   bool      `json:"internal_client"`
	Scope            []string  `json:"scope"`
	DisplayName      string    `json:"displayName"`
}

type Entitlement struct {
	ID              string `json:"id"`
	EntitlementName string `json:"entitlementName"`
	Namespace       string `json:"namespace"`
	CatalogItemId   string `json:"catalogItemId"`
	AccountId       string `json:"accountId"`
	IdentityId      string `json:"identityId"`
	EntitlementType string `json:"entitlementType"`
	GrantDate       string `json:"grantDate"`
	Consumed        bool   `json:"consumed"`
}

type CatalogItem struct {
	ID               string                 `json:"id"`
	Title            string                 `json:"title"`
	Description      string                 `json:"description"`
	KeyImages        []KeyImage             `json:"keyImages"`
	Namespace        string                 `json:"namespace"`
	Categories       []Category             `json:"categories"`
	ReleaseInfo      []ReleaseInfo          `json:"releaseInfo"`
	CustomAttributes map[string]interface{} `json:"customAttributes"`

	// Extended fields for internal use
	AppID       string `json:"app_id,omitempty"` // Extract from ReleaseInfo
	IsInstalled bool   `json:"is_installed"`
	InstallPath string `json:"install_path"`
}

type KeyImage struct {
	Type   string `json:"type"`
	URL    string `json:"url"`
	MD5    string `json:"md5"`
	Width  int    `json:"width"`
	Height int    `json:"height"`
	Size   int    `json:"size"`
}

type Category struct {
	Path string `json:"path"`
}

type ReleaseInfo struct {
	ID       string   `json:"id"`
	AppID    string   `json:"appId"`
	Platform []string `json:"platform"`
}

type ExchangeCodeResponse struct {
	Code             string `json:"code"`
	CreatingClientId string `json:"creatingClientId"`
	ExpiresInSeconds int    `json:"expiresInSeconds"`
}
