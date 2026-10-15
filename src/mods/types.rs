use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path};

pub const API_VERSION: &str = "1.0.0";
pub const MAX_PACKAGE: u64 = 50 * 1024 * 1024;
pub const MAX_EXPANDED: u64 = 200 * 1024 * 1024;
pub const MAX_MESSAGE: usize = 1024 * 1024;
pub type Result<T> = std::result::Result<T, String>;

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
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: String,
    pub platforms: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub games: Vec<GameTarget>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub ui: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
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
        if self.backend.is_none() && self.ui.is_none() && self.theme.is_none() {
            return Err("Mod has no entry point".into());
        }
        for entry in [&self.backend, &self.ui, &self.theme].into_iter().flatten() {
            safe_relative(entry)?;
        }
        Ok(())
    }
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
    if games.len() > 100 {
        return Err("At most 100 game bindings are supported".into());
    }
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
    #[serde(default)]
    pub mods: Vec<CatalogEntry>,
    #[serde(default)]
    pub blocked: Vec<String>,
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
