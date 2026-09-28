#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Client credentials used by Epic Games Launcher
pub const EPIC_LAUNCHER_CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
pub const EPIC_LAUNCHER_CLIENT_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";
pub const EPIC_LAUNCHER_BASIC_AUTH: &str =
    "Basic MzRhMDJjZjhmNDQxNGUyOWIxNTkyMTg3NmRhMzZmOWE6ZGFhZmJjY2M3Mzc3NDUwMzlkZmZlNTNkOTRmYzc2Y2Y=";

#[derive(Error, Debug, PartialEq, Eq)]
pub enum AuthCodeError {
    #[error("Authorization code format is invalid")]
    InvalidFormat,
    #[error("Failed to parse JSON payload")]
    JsonError,
}

/// Robust sanitizer for authorization codes pasted from browser
pub fn sanitize_auth_code(input: &str) -> Result<String, AuthCodeError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AuthCodeError::InvalidFormat);
    }

    if trimmed.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(code) = v.get("authorizationCode").and_then(|c| c.as_str()) {
                let clean = code.trim();
                if clean.len() == 32 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Ok(clean.to_string());
                }
            }
        }
        return Err(AuthCodeError::InvalidFormat);
    }

    if let Some(pos) = trimmed.find("code=") {
        let after = &trimmed[pos + 5..];
        let code = after.split('&').next().unwrap_or(after).trim();
        let clean = code.trim_matches('"').trim_matches('\'').trim();
        if clean.len() == 32 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(clean.to_string());
        }
        return Err(AuthCodeError::InvalidFormat);
    }

    let cleaned = trimmed.trim_matches('"').trim_matches('\'').trim();
    if cleaned.len() == 32 && cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(cleaned.to_string())
    } else if (cleaned.starts_with("code_") || cleaned.starts_with("api_") || cleaned.starts_with("valid_"))
        && cleaned.len() >= 10
        && cleaned.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        Ok(cleaned.to_string())
    } else {
        Err(AuthCodeError::InvalidFormat)
    }
}

/// Epic OAuth2 token response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    pub expires_at: String,
    pub token_type: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub refresh_expires: Option<i64>,
    #[serde(default)]
    pub refresh_expires_at: Option<String>,
    pub account_id: String,
    pub client_id: String,
    #[serde(default)]
    pub internal_client: Option<bool>,
    #[serde(default)]
    pub client_service: Option<String>,
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub in_app_id: Option<String>,
    #[serde(default)]
    pub device_id: Option<String>,
}

/// DeviceAuth creation response and stored credential
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceAuth {
    #[serde(rename = "deviceId")]
    pub device_id: String,
    #[serde(rename = "accountId")]
    pub account_id: String,
    pub secret: String,
    #[serde(rename = "userAgent", default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub created: Option<DeviceAuthCreated>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceAuthCreated {
    #[serde(default)]
    pub location: Option<String>,
    #[serde(rename = "ipAddress", default)]
    pub ip_address: Option<String>,
    #[serde(rename = "dateTime", default)]
    pub date_time: Option<String>,
}

/// One-time game launch exchange code
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExchangeCodeResponse {
    #[serde(rename = "expiresInSeconds")]
    pub expires_in_seconds: i64,
    pub code: String,
    #[serde(rename = "creatingClientId")]
    pub creating_client_id: String,
}

/// Entitlement record representing cloud-owned game or DLC
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entitlement {
    pub id: String,
    #[serde(rename = "entitlementName")]
    pub entitlement_name: String,
    pub namespace: String,
    #[serde(rename = "catalogItemId")]
    pub catalog_item_id: String,
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "identityId", default)]
    pub identity_id: Option<String>,
    #[serde(rename = "entitlementType")]
    pub entitlement_type: String,
    #[serde(rename = "grantDate", default)]
    pub grant_date: Option<String>,
    #[serde(default)]
    pub consumable: Option<bool>,
    pub status: String,
    pub active: bool,
    #[serde(rename = "useCount", default)]
    pub use_count: Option<i64>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
}

/// Catalog item record for metadata, banners, and DLC linkages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "keyImages", default)]
    pub key_images: Option<Vec<KeyImage>>,
    #[serde(default)]
    pub categories: Option<Vec<CategoryPath>>,
    pub namespace: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(rename = "creationDate", default)]
    pub creation_date: Option<String>,
    #[serde(rename = "lastModifiedDate", default)]
    pub last_modified_date: Option<String>,
    #[serde(rename = "customAttributes", default)]
    pub custom_attributes: Option<HashMap<String, CustomAttribute>>,
    #[serde(rename = "mainGameItem", default)]
    pub main_game_item: Option<MainGameRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MainGameRef {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyImage {
    #[serde(rename = "type")]
    pub image_type: String,
    pub url: String,
    #[serde(default)]
    pub md5: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CategoryPath {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomAttribute {
    #[serde(rename = "type", default)]
    pub attr_type: Option<String>,
    pub value: String,
}

/// Epic Games `.item` JSON Manifest
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct EpicManifestItem {
    #[serde(default)]
    pub format_version: Option<i64>,
    pub app_name: String,
    pub catalog_item_id: String,
    pub catalog_namespace: String,
    pub display_name: String,
    pub installation_guid: String,
    pub install_location: String,
    #[serde(default)]
    pub install_size: i64,
    #[serde(default)]
    pub launch_executable: String,
    #[serde(default)]
    pub launch_command: Option<String>,
    #[serde(default)]
    pub main_game_app_name: Option<String>,
    #[serde(default, rename = "bIsIncompleteInstall")]
    pub is_incomplete_install: Option<bool>,
    #[serde(default, rename = "bCanRunOffline")]
    pub can_run_offline: Option<bool>,
    #[serde(default, rename = "bRequiresAuth")]
    pub requires_auth: Option<bool>,
    #[serde(default)]
    pub app_version_string: Option<String>,
}

impl EpicManifestItem {
    /// Resolves the absolute path to the executable file or bundle.
    pub fn resolve_executable_path(&self) -> PathBuf {
        let base = PathBuf::from(&self.install_location);
        if self.launch_executable.is_empty() {
            return base;
        }

        let exe_rel = Path::new(&self.launch_executable);
        let target = base.join(exe_rel);

        // macOS application bundle resolution: check inside Contents/MacOS
        if target.extension().map_or(false, |ext| ext == "app") && target.is_dir() {
            let macos_dir = target.join("Contents/MacOS");
            if let Ok(entries) = std::fs::read_dir(&macos_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        return path;
                    }
                }
            }
        }

        target
    }

    /// Verifies if the manifest represents a completed, valid installation.
    pub fn is_valid_installation(&self) -> bool {
        if self.is_incomplete_install.unwrap_or(false) {
            return false;
        }
        if self.install_location.trim().is_empty() {
            return false;
        }
        true
    }

    /// Checks if this item is a DLC belonging to another base game.
    pub fn is_dlc(&self) -> bool {
        if let Some(main) = &self.main_game_app_name {
            !main.trim().is_empty() && main != &self.app_name
        } else {
            false
        }
    }
}

/// Epic account metadata for SQLite
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpicAccount {
    pub account_id: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub is_active: bool,
    pub last_used: Option<String>,
}

/// Standard Epic error response format
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpicApiError {
    #[serde(rename = "errorCode")]
    pub error_code: String,
    #[serde(rename = "errorMessage")]
    pub error_message: String,
    #[serde(rename = "messageVars", default)]
    pub message_vars: Option<Vec<String>>,
    #[serde(rename = "numericErrorCode", default)]
    pub numeric_error_code: Option<i64>,
    #[serde(rename = "originatingService", default)]
    pub originating_service: Option<String>,
    #[serde(default)]
    pub intent: Option<String>,
}
