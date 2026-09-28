//! Epic Games `.item` JSON Manifest Parser & Path Resolver
//!
//! Parses local `.item` installation manifests written by Epic Games Launcher,
//! resolves cross-platform installation directories, and resolves macOS `.app`
//! bundle Mach-O executables.

use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error types encountered during manifest parsing or directory scanning.
#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("I/O error reading manifest: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Manifest file has invalid structure: {0}")]
    Invalid(String),
}

/// Epic Games `.item` JSON Manifest structure.
///
/// Contains installation, executable, and ownership metadata for a local game or DLC.
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

    #[serde(default)]
    pub mandatory_app_folder_name: Option<String>,

    #[serde(default, rename = "bIsIncompleteInstall", alias = "IsIncompleteInstall", alias = "is_incomplete_install")]
    pub is_incomplete_install: Option<bool>,

    #[serde(default, rename = "bCanRunOffline", alias = "CanRunOffline", alias = "can_run_offline")]
    pub can_run_offline: Option<bool>,

    #[serde(default, rename = "bRequiresAuth", alias = "RequiresAuth", alias = "requires_auth")]
    pub requires_auth: Option<bool>,

    #[serde(default)]
    pub app_version_string: Option<String>,
}

impl EpicManifestItem {
    /// Resolves the absolute path to the executable file or bundle.
    ///
    /// On macOS:
    /// - If `InstallLocation` + `LaunchExecutable` points to an existing `.app` bundle directory,
    ///   resolves the executable inside `Contents/MacOS/` (via `Info.plist` or stem match).
    /// - If the `.app` bundle does not exist on disk (e.g. test environment), returns the `.app` path.
    ///
    /// On Windows/Linux:
    /// - Joins `InstallLocation` with `LaunchExecutable`.
    pub fn resolve_executable_path(&self) -> PathBuf {
        let base = PathBuf::from(&self.install_location);
        if self.launch_executable.trim().is_empty() {
            if base.extension().map_or(false, |ext| ext == "app") && base.is_dir() {
                if let Some(exe) = find_bundle_executable(&base) {
                    return exe;
                }
            }
            return base;
        }

        let exe_rel = Path::new(&self.launch_executable);
        let target = base.join(exe_rel);

        // macOS application bundle resolution: check inside Contents/MacOS
        if target.extension().map_or(false, |ext| ext == "app") && target.is_dir() {
            if let Some(exe) = find_bundle_executable(&target) {
                return exe;
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

    /// Checks if this item is an add-on or DLC belonging to another base game.
    pub fn is_dlc(&self) -> bool {
        if let Some(main) = &self.main_game_app_name {
            !main.trim().is_empty() && main != &self.app_name
        } else {
            false
        }
    }
}

/// Helper to resolve the binary inside a macOS `.app` bundle.
fn find_bundle_executable(bundle: &Path) -> Option<PathBuf> {
    let contents_macos = bundle.join("Contents/MacOS");
    if !contents_macos.is_dir() {
        return None;
    }

    // 1. Try reading CFBundleExecutable from Contents/Info.plist
    let info_plist = bundle.join("Contents/Info.plist");
    if let Ok(plist_content) = fs::read_to_string(&info_plist) {
        if let Some(exe_name) = parse_cf_bundle_executable(&plist_content) {
            let candidate = contents_macos.join(&exe_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // 2. Try matching the bundle file stem (e.g., "Civ6.app" -> "Civ6")
    if let Some(stem) = bundle.file_stem().and_then(|s| s.to_str()) {
        let candidate = contents_macos.join(stem);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // 3. Fallback: inspect directory entries for the first regular file
    if let Ok(entries) = fs::read_dir(&contents_macos) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        if let Some(first) = files.into_iter().next() {
            return Some(first);
        }
    }

    None
}

/// Extracts CFBundleExecutable string from Info.plist XML content.
fn parse_cf_bundle_executable(plist: &str) -> Option<String> {
    let key = "<key>CFBundleExecutable</key>";
    if let Some(pos) = plist.find(key) {
        let after = &plist[pos + key.len()..];
        if let Some(str_start) = after.find("<string>") {
            let after_str = &after[str_start + 8..];
            if let Some(str_end) = after_str.find("</string>") {
                let name = after_str[..str_end].trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

/// Parses a single `.item` JSON manifest from a string.
pub fn parse_manifest_from_str(content: &str) -> Result<EpicManifestItem, ManifestError> {
    let trimmed = content.trim().trim_start_matches('\u{FEFF}');
    if trimmed.is_empty() {
        return Err(ManifestError::Invalid("Manifest content is empty".to_string()));
    }

    let item: EpicManifestItem = serde_json::from_str(trimmed)?;
    if item.app_name.trim().is_empty() {
        return Err(ManifestError::Invalid("AppName is empty".to_string()));
    }

    Ok(item)
}

/// Parses an `.item` file from disk.
pub fn parse_manifest_file<P: AsRef<Path>>(path: P) -> Result<EpicManifestItem, ManifestError> {
    let p = path.as_ref();
    let content = fs::read_to_string(p).map_err(|e| {
        tracing::error!("Failed to read manifest file {}: {}", p.display(), e);
        ManifestError::Io(e)
    })?;
    parse_manifest_from_str(&content)
}

/// Scans a directory for all `.item` files and parses them.
///
/// Skips corrupt files with warnings rather than failing the entire directory scan.
/// Missing directories return an empty `Vec`.
pub fn scan_manifest_directory<P: AsRef<Path>>(dir: P) -> Result<Vec<EpicManifestItem>, ManifestError> {
    let path = dir.as_ref();
    if !path.is_dir() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    let entries = fs::read_dir(path)?;

    for entry in entries.flatten() {
        let file_path = entry.path();
        if file_path.is_file() {
            if let Some(ext) = file_path.extension() {
                if ext.eq_ignore_ascii_case("item") {
                    match parse_manifest_file(&file_path) {
                        Ok(item) => items.push(item),
                        Err(e) => {
                            tracing::warn!("Failed to parse manifest {}: {}", file_path.display(), e);
                        }
                    }
                }
            }
        }
    }

    Ok(items)
}

/// Platform-specific candidate directories without env overrides (useful for testing).
pub fn resolve_platform_manifest_dirs(os: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match os {
        "windows" => {
            if let Ok(program_data) = std::env::var("ProgramData") {
                dirs.push(PathBuf::from(program_data).join(r"Epic\EpicGamesLauncher\Data\Manifests"));
            } else {
                dirs.push(PathBuf::from(r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests"));
            }
        }
        "macos" => {
            dirs.push(PathBuf::from("/Users/Shared/Epic Games/EpicGamesLauncher/Data/Manifests"));
            if let Some(home) = dirs::home_dir() {
                dirs.push(home.join("Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests"));
            }
        }
        "linux" => {
            if let Some(home) = dirs::home_dir() {
                dirs.push(home.join(".local/share/bottles/bottles/Epic-Games-Store/drive_c/ProgramData/Epic/EpicGamesLauncher/Data/Manifests"));
                dirs.push(home.join(".wine/drive_c/ProgramData/Epic/EpicGamesLauncher/Data/Manifests"));
                dirs.push(home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/tools/epic/Data/Manifests"));
            }
        }
        _ => {}
    }
    dirs
}

/// Resolves standard Epic Games manifest paths for current platform.
///
/// If `SWCH_EPIC_MANIFESTS_DIR` environment variable is set, it is prepended.
pub fn resolve_default_manifest_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(override_dir) = std::env::var("SWCH_EPIC_MANIFESTS_DIR") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            dirs.push(PathBuf::from(trimmed));
        }
    }

    #[cfg(target_os = "windows")]
    {
        dirs.extend(resolve_platform_manifest_dirs("windows"));
    }

    #[cfg(target_os = "macos")]
    {
        dirs.extend(resolve_platform_manifest_dirs("macos"));
    }

    #[cfg(target_os = "linux")]
    {
        dirs.extend(resolve_platform_manifest_dirs("linux"));
    }

    dirs
}

/// Convenience helper to scan all resolved default manifest directories.
pub fn scan_all_default_manifests() -> Vec<EpicManifestItem> {
    let mut all = Vec::new();
    for dir in resolve_default_manifest_dirs() {
        if let Ok(items) = scan_manifest_directory(dir) {
            all.extend(items);
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_valid_manifest() {
        let json = r#"{
            "FormatVersion": 0,
            "AppName": "Sugar",
            "CatalogItemId": "4b6c317ad5654dafa94fbf2a11b64e10",
            "CatalogNamespace": "0a2d5924765643a39e05465c04da7d50",
            "DisplayName": "Cyberpunk 2077",
            "InstallationGuid": "{GUID-123}",
            "InstallLocation": "C:\\Games\\Cyberpunk 2077",
            "InstallSize": 75161927680,
            "LaunchExecutable": "bin\\x64\\Cyberpunk2077.exe",
            "bIsIncompleteInstall": false,
            "bCanRunOffline": true,
            "bRequiresAuth": true
        }"#;

        let item = parse_manifest_from_str(json).unwrap();
        assert_eq!(item.app_name, "Sugar");
        assert_eq!(item.display_name, "Cyberpunk 2077");
        assert!(item.is_valid_installation());
        assert!(!item.is_dlc());
        assert_eq!(item.can_run_offline, Some(true));
        assert_eq!(item.requires_auth, Some(true));
    }

    #[test]
    fn test_parse_empty_or_whitespace_error() {
        assert!(parse_manifest_from_str("").is_err());
        assert!(parse_manifest_from_str("   ").is_err());
        assert!(parse_manifest_from_str("\n\t  \r\n").is_err());
    }

    #[test]
    fn test_parse_empty_app_name_error() {
        let json = r#"{
            "AppName": "",
            "CatalogItemId": "123",
            "CatalogNamespace": "ns",
            "DisplayName": "Name",
            "InstallationGuid": "guid",
            "InstallLocation": "loc",
            "LaunchExecutable": "game.exe"
        }"#;
        assert!(parse_manifest_from_str(json).is_err());
    }

    #[test]
    fn test_incomplete_install_flag() {
        let json = r#"{
            "AppName": "Fortnite",
            "CatalogItemId": "fn_id",
            "CatalogNamespace": "fn",
            "DisplayName": "Fortnite",
            "InstallationGuid": "guid",
            "InstallLocation": "C:\\Games\\Fortnite",
            "bIsIncompleteInstall": true
        }"#;
        let item = parse_manifest_from_str(json).unwrap();
        assert!(item.is_incomplete_install.unwrap());
        assert!(!item.is_valid_installation());
    }

    #[test]
    fn test_dlc_classification() {
        let json = r#"{
            "AppName": "Civ6_DLC1",
            "CatalogItemId": "dlc_id",
            "CatalogNamespace": "civ6",
            "DisplayName": "Civ6 Expansion",
            "InstallationGuid": "guid",
            "InstallLocation": "/Games/Civ6/DLC1",
            "MainGameAppName": "Civ6_Base"
        }"#;
        let item = parse_manifest_from_str(json).unwrap();
        assert!(item.is_dlc());

        let base_json = r#"{
            "AppName": "Civ6_Base",
            "CatalogItemId": "base_id",
            "CatalogNamespace": "civ6",
            "DisplayName": "Civ6 Base",
            "InstallationGuid": "guid",
            "InstallLocation": "/Games/Civ6",
            "MainGameAppName": "Civ6_Base"
        }"#;
        let base_item = parse_manifest_from_str(base_json).unwrap();
        assert!(!base_item.is_dlc());
    }

    #[test]
    fn test_macos_app_bundle_resolution() {
        let tmp = tempdir().unwrap();
        let app_dir = tmp.path().join("TestGame.app");
        let macos_dir = app_dir.join("Contents/MacOS");
        fs::create_dir_all(&macos_dir).unwrap();

        let bin_path = macos_dir.join("TestGameBinary");
        fs::write(&bin_path, b"dummy binary content").unwrap();

        let plist_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>TestGameBinary</string>
</dict>
</plist>"#;
        fs::write(app_dir.join("Contents/Info.plist"), plist_content).unwrap();

        let manifest = EpicManifestItem {
            format_version: Some(0),
            app_name: "TestApp".to_string(),
            catalog_item_id: "test_cat_id".to_string(),
            catalog_namespace: "test_ns".to_string(),
            display_name: "Test Game".to_string(),
            installation_guid: "guid".to_string(),
            install_location: tmp.path().to_string_lossy().to_string(),
            install_size: 1000,
            launch_executable: "TestGame.app".to_string(),
            launch_command: None,
            main_game_app_name: None,
            mandatory_app_folder_name: None,
            is_incomplete_install: Some(false),
            can_run_offline: Some(true),
            requires_auth: Some(true),
            app_version_string: Some("1.0".to_string()),
        };

        let resolved = manifest.resolve_executable_path();
        assert_eq!(resolved, bin_path);
    }

    #[test]
    fn test_scan_directory_resilience_to_corrupt_files() {
        let tmp = tempdir().unwrap();

        // Valid item
        let valid_json = r#"{
            "AppName": "GoodGame",
            "CatalogItemId": "good_id",
            "CatalogNamespace": "ns",
            "DisplayName": "Good Game",
            "InstallationGuid": "guid1",
            "InstallLocation": "/Games/GoodGame"
        }"#;
        fs::write(tmp.path().join("good.item"), valid_json).unwrap();

        // Corrupt item
        fs::write(tmp.path().join("corrupt.item"), b"{ AppName: broken...").unwrap();

        // Non-.item file
        fs::write(tmp.path().join("notes.txt"), b"some notes").unwrap();

        let items = scan_manifest_directory(tmp.path()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].app_name, "GoodGame");
    }

    #[test]
    fn test_scan_nonexistent_directory() {
        let items = scan_manifest_directory("/nonexistent/path/for/swch_test").unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_platform_paths() {
        let win_dirs = resolve_platform_manifest_dirs("windows");
        assert!(!win_dirs.is_empty());
        assert!(win_dirs[0].to_string_lossy().contains("EpicGamesLauncher"));

        let mac_dirs = resolve_platform_manifest_dirs("macos");
        assert!(!mac_dirs.is_empty());
        assert!(mac_dirs[0].to_string_lossy().contains("Epic Games"));

        let linux_dirs = resolve_platform_manifest_dirs("linux");
        assert!(!linux_dirs.is_empty());
    }
}
