use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path};

pub const API_VERSION: &str = "1.1.0";
pub type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, Default)]
pub struct PackageLimits {
    pub zip_bytes: Option<u64>,
    pub expanded_bytes: Option<u64>,
    pub files: Option<u64>,
}
impl PackageLimits {
    pub fn from_env() -> Result<Self> {
        fn read(key: &str) -> Result<Option<u64>> {
            match std::env::var(key) {
                Err(std::env::VarError::NotPresent) => Ok(None),
                Ok(value) if value.trim().is_empty() => Ok(None),
                Ok(value) => value.trim().parse::<u64>().map(|n| (n > 0).then_some(n))
                    .map_err(|_| format!("{key} must be a nonnegative integer")),
                Err(e) => Err(e.to_string()),
            }
        }
        Ok(Self { zip_bytes: read("SWCH_MOD_MAX_ZIP_BYTES")?, expanded_bytes: read("SWCH_MOD_MAX_EXPANDED_BYTES")?, files: read("SWCH_MOD_MAX_FILES")? })
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"-_.".contains(&c))
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && safe_relative(id).is_ok()
}

pub fn safe_relative(value: &str) -> Result<&Path> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains(['\\', ':', '\0'])
        || path.is_absolute()
        || path
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
        || value.split('/').any(|p| {
            p.is_empty()
                || p == "."
                || p == ".."
                || p.ends_with(['.', ' '])
                || p.chars().any(|c| c.is_control() || "<>\"|?*".contains(c))
                || matches!(
                    p.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
    {
        return Err("Invalid package path".into());
    }
    Ok(path)
}

pub fn target() -> &'static str {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "windows-x64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-arm64"
    } else {
        "unsupported"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: String,
    pub platforms: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub games: Vec<GameTarget>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub ui: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_tools: Vec<NativeTool>,
}

impl Manifest {
    pub fn validate(&self, platform: Option<&str>) -> Result<()> {
        if !valid_id(&self.id)
            || self.name.trim().is_empty()
            || self.name.len() > 120
            || self.description.len() > 2000
        {
            return Err("Invalid mod identity".into());
        }
        semver::Version::parse(&self.version).map_err(|e| e.to_string())?;
        let normalized = self.api.replace(" <", ", <");
        let req =
            semver::VersionReq::parse(&normalized.replace(",,", ",")).map_err(|e| e.to_string())?;
        if !req.matches(&semver::Version::parse(API_VERSION).unwrap()) {
            return Err("Unsupported SDK API version".into());
        }
        if !self.native_tools.is_empty() && req.matches(&semver::Version::new(1,0,0)) {
            return Err("Native tools require a minimum SDK API of 1.1.0".into());
        }
        if self.platforms.is_empty()
            || self
                .platforms
                .iter()
                .any(|p| !matches!(p.as_str(), "windows-x64" | "macos-arm64"))
        {
            return Err("Invalid platforms".into());
        }
        if platform.is_some_and(|p| !self.platforms.iter().any(|x| x == p)) {
            return Err("Mod does not support this platform".into());
        }
        validate_game_targets(&self.games)?;
        let mut tools = std::collections::HashSet::new();
        for tool in &self.native_tools {
            if !valid_id(&tool.id) || !tools.insert((&tool.id, &tool.platform))
                || !matches!(tool.platform.as_str(), "windows-x64" | "macos-arm64")
                || tool.version.trim().is_empty() || safe_relative(&tool.version)?.components().count() != 1
                || tool.license.trim().is_empty() || tool.files.is_empty() {
                return Err("Invalid native tool metadata".into());
            }
            let mut files = std::collections::HashSet::new();
            for file in &tool.files {
                safe_relative(&file.path)?;
                if !files.insert(file.path.to_ascii_lowercase()) || file.path.to_ascii_lowercase().ends_with(".node")
                    || file.sha256.len() != 64 || !file.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return Err("Invalid native tool file".into());
                }
            }
        }
        if let Some(cover) = &self.cover_url {
            let valid = cover.len() <= 2048
                && !cover.chars().any(char::is_whitespace)
                && !cover.chars().any(char::is_control)
                && reqwest::Url::parse(cover).ok().is_some_and(|url| {
                    url.scheme() == "https"
                        && url.host_str().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                });
            if !valid {
                return Err("Mod cover_url must be an HTTPS URL without credentials (at most 2048 bytes)".into());
            }
        }
        if self.backend.is_none() && self.ui.is_none() && self.theme.is_none() {
            return Err("Mod has no entry point".into());
        }
        for entry in [&self.backend, &self.ui, &self.theme].into_iter().flatten() {
            safe_relative(entry)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeTool {
    pub id: String,
    pub platform: String,
    pub version: String,
    pub license: String,
    pub files: Vec<NativeToolFile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeToolFile {
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub executable: bool,
}

/// A stable game/application identity, scoped to its launcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameTarget {
    pub launcher: String,
    pub app_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

pub fn validate_game_targets(games: &[GameTarget]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for game in games {
        if game.launcher.is_empty()
            || game.launcher.len() > 256
            || !game.launcher.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"-_.:".contains(&c))
            || game.app_id.trim().is_empty()
            || game.app_id.len() > 256
            || game.app_id != game.app_id.trim()
            || game.app_id.chars().any(char::is_control)
            || game.name.as_ref().is_some_and(|n| n.trim().is_empty() || n.len() > 256)
            || !seen.insert((&game.launcher, &game.app_id))
        {
            return Err("Invalid or duplicate game binding".into());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Contributions {
    #[serde(default)]
    pub job_types: Vec<String>,
    #[serde(default)]
    pub launchers: Vec<Platform>,
    #[serde(default)]
    pub tabs: Vec<Contribution>,
    #[serde(default)]
    pub actions: Vec<Contribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub mod_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Catalog {
    #[serde(default)]
    pub schema: u32,
    #[serde(default, deserialize_with="deserialize_catalog_entries")]
    pub mods: Vec<CatalogEntry>,
    #[serde(default)]
    pub blocked: Vec<String>,
}

// Publishers keep legacy manifest fields separate from extensions so older strict
// clients can still read the catalog. New clients merge before compatibility checks.
fn deserialize_catalog_entries<'de,D:serde::Deserializer<'de>>(deserializer:D)->std::result::Result<Vec<CatalogEntry>,D::Error> {
    let entries=Vec::<Value>::deserialize(deserializer)?;
    entries.into_iter().map(|mut entry| {
        if let Some(extensions)=entry.get("manifest_extensions").and_then(Value::as_object).cloned() {
            let manifest=entry.get_mut("manifest").and_then(Value::as_object_mut).ok_or_else(||serde::de::Error::custom("Missing catalog manifest"))?;
            for (key,value) in extensions { if !manifest.contains_key(&key) {manifest.insert(key,value);} }
        }
        serde_json::from_value(entry).map_err(serde::de::Error::custom)
    }).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub manifest: Manifest,
    pub author: String,
    pub repository: String,
    pub repository_id: u64,
    pub owner_id: u64,
    pub release: String,
    pub download_url: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads_updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SavedState {
    #[serde(default)]
    pub games: Option<Vec<GameTarget>>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub development: Option<String>,
    #[serde(default)]
    pub source: Option<CatalogEntry>,
}

#[derive(Debug, Serialize)]
pub struct ModInfo {
    pub manifest: Manifest,
    pub games: Vec<GameTarget>,
    pub enabled: bool,
    pub development: bool,
    pub status: String,
    pub error: Option<String>,
    pub contributions: Contributions,
    pub source: Option<CatalogEntry>,
}

pub fn theme_tokens(value: &Value) -> Result<()> {
    let map = value.as_object().ok_or("Theme must be an object")?;
    for (key, value) in map {
        let v = value.as_str().ok_or("Theme values must be strings")?;
        let valid = if key == "radius" {
            v.strip_suffix("px")
                .and_then(|x| x.parse::<u8>().ok())
                .is_some_and(|n| n <= 32)
        } else {
            matches!(
                key.as_str(),
                "bg" | "panel" | "panel-hover" | "line" | "text" | "muted" | "accent"
            ) && v.len() == 7
                && v.starts_with('#')
                && v[1..].bytes().all(|b| b.is_ascii_hexdigit())
        };
        if !valid {
            return Err(format!("Unsupported theme token: {key}"));
        }
    }
    Ok(())
}
