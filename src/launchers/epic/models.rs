use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::storage::Account;

/// Standard Epic Games Launcher client credentials used across Legendary / Heroic.
pub const EPIC_CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
pub const EPIC_CLIENT_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";
pub const EPIC_BASIC_AUTH: &str = "Basic MzRhMDJjZjhmNDQxNGUyOWIxNTkyMTg3NmRhMzZmOWE6ZGFhZmJjY2M3Mzc3NDUwMzlkZmZlNTNkOTRmYzc2Y2Y=";
pub const EPIC_USER_AGENT: &str = "UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit";
pub const EPIC_AUTH_URL: &str = "https://www.epicgames.com/id/api/redirect?clientId=34a02cf8f4414e29b15921876da36f9a&responseType=code";

/// Standard Fortnite PC client credentials used to obtain tokens with `fortnite:profile:commands ALL` scope.
pub const FORTNITE_CLIENT_ID: &str = "ec684b8c687f479fadea3cb2ad83f5c6";
pub const FORTNITE_CLIENT_SECRET: &str = "e1f31c211f28413186262d37a13fc84d";
pub const FORTNITE_BASIC_AUTH: &str = "Basic ZWM2ODRiOGM2ODdmNDc5ZmFkZWEzY2IyYWQ4M2Y1YzY6ZTFmMzFjMjExZjI4NDEzMTg2MjYyZDM3YTEzZmM4NGQ=";
pub const FORTNITE_USER_AGENT: &str = "Fortnite/++Fortnite+Release-25.00-CL-25717004 Windows/10.0.19041.1.256.64bit";

/// Response from Epic OAuth2 token endpoint (`/account/api/oauth/token`).
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub account_id: String,
    pub client_id: String,
    #[serde(rename = "displayName", alias = "display_name")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_expires: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal_client: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_service: Option<String>,
}

impl std::fmt::Debug for OAuthTokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthTokenResponse")
            .field("access_token", &"[REDACTED]")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("account_id", &self.account_id)
            .field("client_id", &self.client_id)
            .field("display_name", &self.display_name)
            .field("device_id", &self.device_id)
            .finish()
    }
}

/// Metadata recorded when a DeviceAuth credential is created.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct DeviceAuthCreated {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(rename = "ipAddress", alias = "ip_address", default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    #[serde(rename = "dateTime", alias = "date_time", default, skip_serializing_if = "Option::is_none")]
    pub date_time: Option<String>,
}

/// Permanent device credentials generated via `POST /account/api/public/account/{accountId}/deviceAuth`.
/// Saved securely in the OS Keyring under key `swch:epic:<accountId>`.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DeviceAuth {
    #[serde(rename = "accountId", alias = "account_id")]
    pub account_id: String,
    #[serde(rename = "deviceId", alias = "device_id")]
    pub device_id: String,
    pub secret: String,
    #[serde(rename = "refreshToken", alias = "refresh_token", default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(rename = "userAgent", alias = "user_agent", default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<DeviceAuthCreated>,
}

impl DeviceAuth {
    pub fn new(account_id: impl Into<String>, device_id: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            account_id: account_id.into(),
            device_id: device_id.into(),
            secret: secret.into(),
            refresh_token: None,
            user_agent: None,
            created: None,
        }
    }

    pub fn created_at(&self) -> Option<&str> {
        self.created.as_ref().and_then(|c| c.date_time.as_deref())
    }
}

impl std::fmt::Debug for DeviceAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceAuth")
            .field("account_id", &self.account_id)
            .field("device_id", &self.device_id)
            .field("secret", &"[REDACTED]")
            .field("user_agent", &self.user_agent)
            .field("created", &self.created)
            .finish()
    }
}

/// Single-use exchange code valid for 300 seconds from `GET /account/api/oauth/exchange`.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExchangeCodeResponse {
    pub code: String,
    #[serde(rename = "creatingClientId", alias = "creating_client_id")]
    pub creating_client_id: String,
    #[serde(rename = "expiresInSeconds", alias = "expires_in_seconds")]
    pub expires_in_seconds: u32,
}

impl std::fmt::Debug for ExchangeCodeResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExchangeCodeResponse")
            .field("code", &"[REDACTED]")
            .field("creating_client_id", &self.creating_client_id)
            .field("expires_in_seconds", &self.expires_in_seconds)
            .finish()
    }
}

/// Representation of an Epic account exposed to callers and API responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpicAccount {
    pub account_id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub is_active: bool,
    pub last_used: Option<String>,
}

impl From<Account> for EpicAccount {
    fn from(acc: Account) -> Self {
        let display_name = acc
            .display_name
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| acc.username.clone());
        Self {
            account_id: acc.id,
            display_name,
            email: acc.email,
            is_active: acc.is_active,
            last_used: acc.last_used,
        }
    }
}

impl From<&Account> for EpicAccount {
    fn from(acc: &Account) -> Self {
        let display_name = acc
            .display_name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&acc.username)
            .to_string();
        Self {
            account_id: acc.id.clone(),
            display_name,
            email: acc.email.clone(),
            is_active: acc.is_active,
            last_used: acc.last_used.clone(),
        }
    }
}

/// Epic API error payload structure returned with 4xx/5xx HTTP responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpicApiErrorResponse {
    #[serde(rename = "errorCode", alias = "error_code")]
    pub error_code: Option<String>,
    #[serde(rename = "errorMessage", alias = "error_message")]
    pub error_message: Option<String>,
    #[serde(rename = "numericErrorCode", alias = "numeric_error_code")]
    pub numeric_error_code: Option<i32>,
    #[serde(rename = "originatingService", alias = "originating_service")]
    pub originating_service: Option<String>,
    pub intent: Option<String>,
    #[serde(rename = "messageVars", alias = "message_vars", default)]
    pub message_vars: Vec<String>,
}

/// Entitlement record representing cloud-owned game or DLC from Epic Games backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entitlement {
    pub id: String,
    #[serde(rename = "entitlementName", alias = "entitlement_name")]
    pub entitlement_name: String,
    pub namespace: String,
    #[serde(rename = "catalogItemId", alias = "catalog_item_id")]
    pub catalog_item_id: String,
    #[serde(rename = "accountId", alias = "account_id")]
    pub account_id: String,
    #[serde(rename = "identityId", alias = "identity_id", default)]
    pub identity_id: Option<String>,
    #[serde(rename = "entitlementType", alias = "entitlement_type")]
    pub entitlement_type: String,
    #[serde(rename = "grantDate", alias = "grant_date", default)]
    pub grant_date: Option<String>,
    #[serde(default)]
    pub consumable: Option<bool>,
    pub status: String,
    pub active: bool,
    #[serde(rename = "useCount", alias = "use_count", default)]
    pub use_count: Option<i64>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
}

impl Entitlement {
    /// Returns true if this entitlement represents a playable base game executable.
    pub fn is_executable(&self) -> bool {
        self.entitlement_type.eq_ignore_ascii_case("EXECUTABLE")
            || self.entitlement_type.eq_ignore_ascii_case("AUDIENCE")
    }

    /// Returns true if this entitlement represents an add-on or DLC.
    pub fn is_dlc(&self) -> bool {
        !self.is_executable()
    }
}

/// Catalog item record for metadata, banners, categories, and DLC linkages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "keyImages", alias = "key_images", default)]
    pub key_images: Option<Vec<KeyImage>>,
    #[serde(default)]
    pub categories: Option<Vec<CategoryPath>>,
    pub namespace: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(rename = "creationDate", alias = "creation_date", default)]
    pub creation_date: Option<String>,
    #[serde(rename = "lastModifiedDate", alias = "last_modified_date", default)]
    pub last_modified_date: Option<String>,
    #[serde(rename = "customAttributes", alias = "custom_attributes", default)]
    pub custom_attributes: Option<HashMap<String, CustomAttribute>>,
    #[serde(rename = "mainGameItem", alias = "main_game_item", default)]
    pub main_game_item: Option<MainGameRef>,
}

impl CatalogItem {
    /// Returns true if this item is an add-on / DLC referencing a base game.
    pub fn is_dlc(&self) -> bool {
        self.main_game_item.is_some()
    }

    /// Returns true if this item is a standalone base game.
    pub fn is_base_game(&self) -> bool {
        self.main_game_item.is_none()
    }

    /// Returns true if this item represents Unreal Engine editor content or assets.
    pub fn is_engine_asset(&self) -> bool {
        self.namespace == "ue"
            || self
                .categories
                .as_ref()
                .map_or(false, |cats| cats.iter().any(|c| c.path == "assets"))
    }

    /// Retrieves the portrait box art URL (typically 1200x1600).
    pub fn tall_box_art(&self) -> Option<&str> {
        self.key_images
            .as_ref()
            .and_then(|images| images.iter().find(|img| img.image_type == "DieselGameBoxTall"))
            .map(|img| img.url.as_str())
    }

    /// Retrieves the landscape banner URL (typically 2560x1440).
    pub fn landscape_box_art(&self) -> Option<&str> {
        self.key_images
            .as_ref()
            .and_then(|images| images.iter().find(|img| img.image_type == "DieselGameBox"))
            .map(|img| img.url.as_str())
    }

    /// Reads custom attribute string value (e.g. "CanRunOffline", "FolderName").
    pub fn get_custom_attribute(&self, key: &str) -> Option<&str> {
        self.custom_attributes
            .as_ref()
            .and_then(|attrs| attrs.get(key))
            .map(|attr| attr.value.as_str())
    }
}

/// Reference to parent base game item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MainGameRef {
    pub id: String,
}

/// Promotional image asset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyImage {
    #[serde(rename = "type", alias = "image_type")]
    pub image_type: String,
    pub url: String,
    #[serde(default)]
    pub md5: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

/// Category taxonomy node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CategoryPath {
    pub path: String,
}

/// Custom key-value metadata attached to catalog items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomAttribute {
    #[serde(rename = "type", alias = "attr_type", default)]
    pub attr_type: Option<String>,
    pub value: String,
}

/// Summary produced after synchronizing cloud entitlements and local manifests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncSummary {
    pub total_entitlements: usize,
    pub base_games_count: usize,
    pub dlc_count: usize,
    pub installed_count: usize,
}

/// Comprehensive error type for Epic Games integration.
#[derive(Error, Debug)]
pub enum EpicError {
    #[error("HTTP request failed: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("Epic API error ({status}): {message} (code: {code:?}, numeric: {numeric:?})")]
    Api {
        status: reqwest::StatusCode,
        code: Option<String>,
        message: String,
        numeric: Option<i32>,
    },

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Invalid authorization code format: {0}")]
    InvalidAuthCode(String),

    #[error("JSON serialization/deserialization failed: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Credential store error: {0}")]
    Keyring(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("DeviceAuth credential not found for account: {0}")]
    DeviceAuthNotFound(String),

    #[error("No active Epic account")]
    NoActiveAccount,

    #[error("Database error: {0}")]
    Database(String),

    #[error("URL parse error: {0}")]
    UrlParse(String),

    #[error("I/O error: {0}")]
    Io(String),
}

impl From<crate::storage::DbError> for EpicError {
    fn from(err: crate::storage::DbError) -> Self {
        EpicError::Database(err.to_string())
    }
}

impl From<crate::parsers::ManifestError> for EpicError {
    fn from(err: crate::parsers::ManifestError) -> Self {
        match err {
            crate::parsers::ManifestError::Io(e) => EpicError::Io(e.to_string()),
            crate::parsers::ManifestError::Json(e) => EpicError::Serialization(e),
            crate::parsers::ManifestError::Invalid(msg) => EpicError::Database(msg),
        }
    }
}

impl From<std::io::Error> for EpicError {
    fn from(err: std::io::Error) -> Self {
        EpicError::Io(err.to_string())
    }
}

impl From<crate::storage::AtomicError> for EpicError {
    fn from(err: crate::storage::AtomicError) -> Self {
        EpicError::Io(err.to_string())
    }
}

impl From<crate::sys::process::ProcessError> for EpicError {
    fn from(err: crate::sys::process::ProcessError) -> Self {
        EpicError::Io(err.to_string())
    }
}

impl EpicError {
    pub fn is_invalid_credentials(&self) -> bool {
        match self {
            EpicError::Api { code, .. } => {
                code.as_deref() == Some("errors.com.epicgames.account.invalid_account_credentials")
            }
            _ => false,
        }
    }

    pub fn is_auth_code_expired(&self) -> bool {
        match self {
            EpicError::Api { code, .. } => {
                code.as_deref() == Some("errors.com.epicgames.account.oauth.authorization_code_not_found")
            }
            _ => false,
        }
    }
}

/// Robust sanitizer for authorization codes entered by users.
/// Supports raw 32-hex codes, quoted codes, JSON payloads, and redirect URLs with `#code=` or `?code=`.
pub fn sanitize_auth_code(input: &str) -> Result<String, EpicError> {
    // Strip invisible zero-width formatting characters commonly introduced via copy-paste:
    // U+200B (ZWSP), U+200C (ZWNJ), U+200D (ZWJ), U+FEFF (BOM / ZWNBSP), U+2060 (Word Joiner)
    let cleaned_input: std::borrow::Cow<str> = if input.contains(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}', '\u{2060}']) {
        input
            .chars()
            .filter(|c| !matches!(c, '\u{200B}'..='\u{200D}' | '\u{FEFF}' | '\u{2060}'))
            .collect::<String>()
            .into()
    } else {
        input.into()
    };
    let trimmed = cleaned_input.trim();
    if trimmed.is_empty() {
        return Err(EpicError::InvalidAuthCode("Authorization code is empty".to_string()));
    }

    // Case 1: JSON payload: { "authorizationCode": "..." }
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(code) = val.get("authorizationCode").and_then(|c| c.as_str()) {
                let code = code.trim();
                if is_valid_hex_32(code) {
                    return Ok(code.to_lowercase());
                }
            }
        }
    }

    // Case 2: URL containing code= parameter or fragment (#code=... or ?code=...)
    if let Some(pos) = trimmed.find("code=") {
        let remainder = &trimmed[pos + 5..];
        let code = remainder
            .split(&['&', '#', ' ', '"', '\'', '}'][..])
            .next()
            .unwrap_or(remainder)
            .trim();
        if is_valid_hex_32(code) {
            return Ok(code.to_lowercase());
        }
    }

    // Case 3: Raw code, possibly quoted
    let cleaned = trimmed
        .trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == ' ')
        .trim();

    if is_valid_hex_32(cleaned) {
        Ok(cleaned.to_lowercase())
    } else {
        let display_snippet = if cleaned.chars().nth(16).is_some() {
            format!("{}...", cleaned.chars().take(16).collect::<String>())
        } else {
            cleaned.to_string()
        };

        Err(EpicError::InvalidAuthCode(format!(
            "Expected 32-character hex code, got '{}'",
            display_snippet
        )))
    }
}

fn is_valid_hex_32(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Connected external platform account (e.g. PSN, Xbox, Nintendo, Steam, Twitch).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ExternalAuthItem {
    #[serde(rename = "accountId", alias = "account_id", default)]
    pub account_id: Option<String>,
    #[serde(rename = "type", alias = "auth_type", default)]
    pub auth_type: String,
    #[serde(rename = "externalDisplayName", alias = "external_display_name", default)]
    pub external_display_name: Option<String>,
    #[serde(rename = "dateAdded", alias = "date_added", default)]
    pub date_added: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ExternalAuthWire {
    List(Vec<ExternalAuthItem>),
    Map(HashMap<String, ExternalAuthWireEntry>),
}

#[derive(Debug, Deserialize)]
struct ExternalAuthWireEntry {
    #[serde(rename = "accountId", alias = "account_id", default)]
    pub account_id: Option<String>,
    #[serde(rename = "type", alias = "auth_type", default)]
    pub auth_type: Option<String>,
    #[serde(rename = "externalDisplayName", alias = "external_display_name", default)]
    pub external_display_name: Option<String>,
    #[serde(rename = "dateAdded", alias = "date_added", default)]
    pub date_added: Option<String>,
}

fn deserialize_external_auths<'de, D>(deserializer: D) -> Result<Vec<ExternalAuthItem>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<ExternalAuthWire>::deserialize(deserializer)?;
    match opt {
        Some(ExternalAuthWire::List(list)) => Ok(list),
        Some(ExternalAuthWire::Map(map)) => {
            let mut items = Vec::with_capacity(map.len());
            for (key, entry) in map {
                items.push(ExternalAuthItem {
                    account_id: entry.account_id,
                    auth_type: entry.auth_type.unwrap_or(key),
                    external_display_name: entry.external_display_name,
                    date_added: entry.date_added,
                });
            }
            items.sort_by(|a, b| a.auth_type.cmp(&b.auth_type));
            Ok(items)
        }
        None => Ok(Vec::new()),
    }
}

/// Detailed account info retrieved from `GET /account/api/public/account/{accountId}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EpicAccountDetails {
    pub id: String,
    #[serde(rename = "displayName", alias = "display_name", default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(rename = "emailVerified", alias = "email_verified", default)]
    pub email_verified: bool,
    #[serde(rename = "tfaEnabled", alias = "tfa_enabled", default)]
    pub tfa_enabled: bool,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(rename = "lastLogin", alias = "last_login", default)]
    pub last_login: Option<String>,
    #[serde(rename = "created", default)]
    pub created: Option<String>,
    #[serde(rename = "canUpdateDisplayName", alias = "can_update_display_name", default)]
    pub can_update_display_name: Option<bool>,
    #[serde(rename = "numberOfDisplayNameChanges", alias = "number_of_display_name_changes", default)]
    pub number_of_display_name_changes: Option<u32>,
    #[serde(rename = "externalAuths", alias = "external_auths", default, deserialize_with = "deserialize_external_auths")]
    pub external_auths: Vec<ExternalAuthItem>,
}

/// Raw response from Fortnite MCP QueryProfile endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FortniteMcpResponse {
    #[serde(rename = "profileRevision", alias = "profile_revision", default)]
    pub profile_revision: Option<i64>,
    #[serde(rename = "profileId", alias = "profile_id", default)]
    pub profile_id: Option<String>,
    #[serde(rename = "profileChanges", alias = "profile_changes", default)]
    pub profile_changes: Vec<FortniteMcpProfileChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FortniteMcpProfileChange {
    #[serde(rename = "changeType", alias = "change_type", default)]
    pub change_type: Option<String>,
    #[serde(default)]
    pub profile: Option<FortniteProfileData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FortniteProfileData {
    #[serde(rename = "profileId", alias = "profile_id", default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub items: HashMap<String, FortniteRawItem>,
    #[serde(default)]
    pub stats: Option<FortniteRawStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FortniteRawItem {
    #[serde(rename = "templateId", alias = "template_id", default)]
    pub template_id: String,
    #[serde(default)]
    pub quantity: Option<i64>,
    #[serde(default)]
    pub attributes: Option<FortniteRawItemAttributes>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FortniteRawItemAttributes {
    #[serde(default)]
    pub favorite: Option<bool>,
    #[serde(default)]
    pub level: Option<i64>,
    #[serde(rename = "creation_time", alias = "creationTime", alias = "creationDate", default)]
    pub creation_time: Option<String>,
    #[serde(default)]
    pub variants: Option<Vec<FortniteRawVariant>>,
    #[serde(default)]
    pub platform: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FortniteRawVariant {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub owned: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FortniteRawStats {
    #[serde(default)]
    pub attributes: Option<FortniteRawStatsAttributes>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FortniteRawStatsAttributes {
    #[serde(default, alias = "seasonLevel")]
    pub level: Option<i64>,
    #[serde(rename = "account_level", alias = "accountLevel", default)]
    pub account_level: Option<i64>,
    #[serde(rename = "book_level", alias = "bookLevel", default)]
    pub book_level: Option<i64>,
    #[serde(rename = "book_purchased", alias = "bookPurchased", default)]
    pub book_purchased: Option<bool>,
    #[serde(rename = "book_xp", alias = "bookXp", default)]
    pub book_xp: Option<i64>,
    #[serde(rename = "lifetime_wins", alias = "lifetimeWins", default)]
    pub lifetime_wins: Option<i64>,
    #[serde(rename = "season_num", alias = "seasonNum", default)]
    pub season_num: Option<i64>,
    #[serde(rename = "past_seasons", alias = "pastSeasons", default)]
    pub past_seasons: Option<Vec<FortniteRawPastSeason>>,
    #[serde(rename = "last_match_end_datetime", alias = "lastMatchEndDatetime", alias = "last_match_time", default)]
    pub last_match_end_datetime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FortniteRawPastSeason {
    #[serde(rename = "seasonNumber", alias = "season_number", default)]
    pub season_number: i64,
    #[serde(rename = "seasonLevel", alias = "season_level", default)]
    pub season_level: Option<i64>,
    #[serde(rename = "bookLevel", alias = "book_level", default)]
    pub book_level: Option<i64>,
    #[serde(rename = "bookPurchased", alias = "book_purchased", default)]
    pub book_purchased: Option<bool>,
    #[serde(rename = "numWins", alias = "num_wins", default)]
    pub num_wins: Option<i64>,
}

/// Category of Fortnite cosmetic item.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FortniteCosmeticType {
    Outfit,
    Backpack,
    Pickaxe,
    Glider,
    Emote,
    Wrap,
    Music,
    Contrail,
    LoadingScreen,
    Other,
}

impl FortniteCosmeticType {
    pub fn display_name(&self) -> &'static str {
        match self {
            FortniteCosmeticType::Outfit => "Экипировка",
            FortniteCosmeticType::Backpack => "Рюкзак",
            FortniteCosmeticType::Pickaxe => "Кирка",
            FortniteCosmeticType::Glider => "Дельтаплан",
            FortniteCosmeticType::Emote => "Эмоция",
            FortniteCosmeticType::Wrap => "Обёртка",
            FortniteCosmeticType::Music => "Музыка",
            FortniteCosmeticType::Contrail => "След падения",
            FortniteCosmeticType::LoadingScreen => "Экран загрузки",
            FortniteCosmeticType::Other => "Предмет",
        }
    }
}

/// Human-readable Fortnite cosmetic item representation with rarity, icon and style variants.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FortniteCosmeticItem {
    pub id: String,
    pub template_id: String,
    pub name: String,
    pub cosmetic_type: FortniteCosmeticType,
    pub rarity: String,
    pub description: Option<String>,
    pub image_url: String,
    pub is_favorite: bool,
    pub is_exclusive: bool,
    pub variants_count: usize,
    pub variant_styles: Vec<String>,
    pub date_acquired: Option<String>,
}

/// Complete account and Fortnite inventory overview matching LZT Market's checker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FortniteAccountSummary {
    pub account_id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub tfa_enabled: bool,
    pub country: Option<String>,
    pub can_update_display_name: Option<bool>,
    pub last_login: Option<String>,
    pub external_auths: Vec<ExternalAuthItem>,

    // Fortnite specifics
    pub has_fortnite: bool,
    pub total_vbucks: i64,
    pub vbucks_purchased: i64,
    pub vbucks_earned: i64,
    pub season_level: i64,
    pub account_level: i64,
    pub battle_pass_level: i64,
    pub battle_pass_purchased: bool,
    pub lifetime_wins: i64,
    pub has_save_the_world: bool,
    pub is_stw_founder: bool,
    pub last_match_end_datetime: Option<String>,
    pub account_created: Option<String>,
    pub days_inactive: Option<i64>,

    // Locker counts
    pub total_cosmetics: usize,
    pub outfits_count: usize,
    pub backpacks_count: usize,
    pub pickaxes_count: usize,
    pub gliders_count: usize,
    pub emotes_count: usize,
    pub wraps_count: usize,
    pub exclusive_count: usize,

    // Detailed lists
    pub cosmetics: Vec<FortniteCosmeticItem>,
    pub past_seasons: Vec<FortniteRawPastSeason>,
    pub updated_at: i64,
}

/// Resolves cosmetic metadata (name, type, rarity, exclusivity) from a raw Epic template ID.
pub fn resolve_cosmetic_metadata(template_id: &str) -> (String, FortniteCosmeticType, String, bool) {
    let parts: Vec<&str> = template_id.splitn(2, ':').collect();
    let (prefix, raw_id) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        ("", template_id)
    };

    let prefix_lower = prefix.to_lowercase();
    let cosmetic_type = match prefix_lower.as_str() {
        "athenacharacter" => FortniteCosmeticType::Outfit,
        "athenabackpack" => FortniteCosmeticType::Backpack,
        "athenapickaxe" => FortniteCosmeticType::Pickaxe,
        "athenaglider" => FortniteCosmeticType::Glider,
        "athenadance" => FortniteCosmeticType::Emote,
        "athenaitemwrap" => FortniteCosmeticType::Wrap,
        "athenamusicpack" => FortniteCosmeticType::Music,
        "athenaskydivecontrail" => FortniteCosmeticType::Contrail,
        "athenaloadingscreen" => FortniteCosmeticType::LoadingScreen,
        _ => FortniteCosmeticType::Other,
    };

    let id_lower = raw_id.to_lowercase();

    // Curated database of iconic, rare, and exclusive cosmetics
    let known: Option<(&str, &str, bool)> = match id_lower.as_str() {
        // Legendary & OG Outfits
        "cid_028_athena_commando_f" => Some(("Renegade Raider", "rare", true)),
        "cid_017_athena_commando_m" => Some(("Aerial Assault Trooper", "rare", true)),
        "cid_035_athena_commando_m_medieval" => Some(("Black Knight", "legendary", true)),
        "cid_039_athena_commando_f_disco" => Some(("Sparkle Specialist", "epic", true)),
        "cid_033_athena_commando_f_medieval" => Some(("Royale Knight", "rare", true)),
        "cid_032_athena_commando_m_medieval" => Some(("Blue Squire", "rare", true)),
        "cid_084_athena_commando_m_space" => Some(("The Reaper", "legendary", true)),
        "cid_083_athena_commando_f_tactical" => Some(("Elite Agent", "epic", true)),
        "cid_089_athena_commando_m_retrogrey" => Some(("Havoc", "epic", true)),
        "cid_088_athena_commando_f_retroharbor" => Some(("Sub Commander", "epic", true)),
        "cid_119_athena_commando_f_camo" => Some(("Trailblazer", "epic", true)),
        "cid_162_athena_commando_m_tacticalbadass" => Some(("Ragnarok", "legendary", true)),
        "cid_175_athena_commando_m_celestial" => Some(("Galaxy", "epic", true)),
        "cid_221_athena_commando_f_space" | "cid_eon" => Some(("Eon", "legendary", true)),
        "cid_228_athena_commando_m_tacticalwoodland" | "cid_helix" => Some(("Double Helix", "epic", true)),
        "cid_268_athena_commando_m_streetops" => Some(("Reflex", "rare", true)),
        "cid_371_athena_commando_m_speedyred" => Some(("IKONIK", "epic", true)),
        "cid_479_athena_commando_f_glow" => Some(("GLOW", "epic", true)),
        "cid_313_athena_commando_m_kpopfashion" => Some(("Wonder", "epic", true)),
        "cid_434_athena_commando_f_stealthhonor" => Some(("Honor Guard", "epic", true)),
        "cid_342_athena_commando_m_streetops" => Some(("Stealth Reflex", "rare", true)),
        "cid_478_athena_commando_m_spiderknightxbox" | "cid_roguespiderknight" => Some(("Rogue Spider Knight", "legendary", true)),
        "cid_516_athena_commando_m_ashenwrap" => Some(("Dark Vertex", "legendary", true)),
        "cid_138_athena_commando_m_ps4" => Some(("Royale Bomber", "epic", true)),
        "cid_444_athena_commando_f_blueheadband" => Some(("Neo Versa", "epic", true)),
        "cid_788_athena_commando_f_wildcat" | "cid_wildcat" => Some(("Wildcat", "epic", true)),
        "cid_703_athena_commando_m_cyclone" => Some(("Travis Scott", "icon", true)),
        "cid_704_athena_commando_m_cyclonespace" => Some(("Astro Jack", "icon", true)),
        "cid_113_athena_commando_m_founder" => Some(("Warpaint", "legendary", true)),
        "cid_114_athena_commando_f_founder" => Some(("Rose Team Leader", "legendary", true)),
        "cid_030_athena_commando_m_halloween" => Some(("Skull Trooper", "epic", false)),
        "cid_029_athena_commando_f_halloween" => Some(("Ghoul Trooper", "epic", false)),
        "cid_116_athena_commando_m_carbide" => Some(("Carbide", "legendary", false)),
        "cid_115_athena_commando_m_omega" => Some(("Omega", "legendary", true)),
        "cid_242_athena_commando_f_bullseye" => Some(("Bullseye", "uncommon", false)),
        // Rare Pickaxes
        "pickaxe_id_015_raider" => Some(("Raider's Revenge", "epic", true)),
        "pickaxe_id_013_sub" => Some(("AC/DC", "epic", true)),
        "pickaxe_id_080_harvester" => Some(("The Reaper (Scythe)", "rare", false)),
        "pickaxe_id_029_assault" => Some(("Axecalibur", "rare", true)),
        "pickaxe_id_318_festiveharvester" => Some(("Merry Mint Axe", "epic", true)),
        "pickaxe_id_355_poisonharvester" => Some(("Harley Hitter", "dc", false)),
        "pickaxe_id_019_candy" => Some(("Candy Axe", "epic", false)),
        "pickaxe_id_leviathanaxe" => Some(("Leviathan Axe", "gaminglegends", false)),
        "pickaxe_id_starwand" => Some(("Star Wand", "rare", false)),
        "pickaxe_lockjaw" => Some(("Crowbar", "rare", false)),
        // Rare Emotes
        "eid_floss" => Some(("Floss", "rare", true)),
        "eid_takethel" => Some(("Take The L", "rare", true)),
        "eid_worm" => Some(("The Worm", "rare", true)),
        "eid_fresh" => Some(("Fresh", "epic", true)),
        "eid_tidy" => Some(("Tidy", "rare", true)),
        "eid_scenario" => Some(("Scenario", "rare", true)),
        "eid_rambunctious" => Some(("Rambunctious", "rare", true)),
        // Gliders
        "glider_id_001" => Some(("Mako", "uncommon", true)),
        "glider_id_002_medieval" => Some(("Sir Glider the Brave", "uncommon", true)),
        "glider_id_003_disco" => Some(("Get Down!", "epic", true)),
        "glider_id_014_space" => Some(("High Octane", "epic", true)),
        "umbrella_founder" => Some(("Founder's Umbrella", "common", true)),
        "glider_founder" => Some(("Founder's Glider", "common", true)),
        _ => None,
    };

    if let Some((name, rarity, exclusive)) = known {
        return (name.to_string(), cosmetic_type, rarity.to_string(), exclusive);
    }

    // Dynamic heuristic parser for unknown items:
    // Strip technical prefixes and numbers, convert snake_case to Title Case
    let mut cleaned = id_lower.as_str();
    for prefix in [
        "cid_athena_commando_m_",
        "cid_athena_commando_f_",
        "cid_athena_commando_",
        "pickaxe_id_",
        "glider_id_",
        "bid_athena_commando_m_",
        "bid_athena_commando_f_",
        "bid_",
        "eid_",
        "wrap_",
    ] {
        if let Some(rest) = cleaned.strip_prefix(prefix) {
            cleaned = rest;
            break;
        }
    }

    // Strip pattern like "cid_123_" or "cid_"
    if let Some(rest) = cleaned.strip_prefix("cid_") {
        cleaned = rest;
    }
    if let Some(pos) = cleaned.find('_') {
        if cleaned[..pos].chars().all(|c| c.is_ascii_digit()) {
            cleaned = &cleaned[pos + 1..];
        }
    }
    for sub in ["athena_commando_m_", "athena_commando_f_", "athena_commando_"] {
        if let Some(rest) = cleaned.strip_prefix(sub) {
            cleaned = rest;
            break;
        }
    }

    let words: Vec<String> = cleaned
        .split(|c: char| c == '_' || c == '-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect();

    let name = if words.is_empty() {
        raw_id.to_string()
    } else {
        words.join(" ")
    };

    let default_rarity = match cosmetic_type {
        FortniteCosmeticType::Outfit => "epic",
        FortniteCosmeticType::Pickaxe => "rare",
        FortniteCosmeticType::Backpack => "rare",
        FortniteCosmeticType::Glider => "rare",
        FortniteCosmeticType::Emote => "rare",
        FortniteCosmeticType::Wrap => "uncommon",
        FortniteCosmeticType::Music => "uncommon",
        FortniteCosmeticType::Contrail => "uncommon",
        FortniteCosmeticType::LoadingScreen => "uncommon",
        FortniteCosmeticType::Other => "common",
    };

    (name, cosmetic_type, default_rarity.to_string(), false)
}
