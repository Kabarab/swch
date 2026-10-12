#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

use super::models::EpicManifestItem;

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("I/O error reading manifest: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Manifest file has invalid structure: {0}")]
    Invalid(String),
}

/// Parses a single `.item` JSON manifest from a string
pub fn parse_manifest_from_str(content: &str) -> Result<EpicManifestItem, ManifestError> {
    if content.trim().is_empty() {
        return Err(ManifestError::Invalid("Manifest content is empty".to_string()));
    }
    let item: EpicManifestItem = serde_json::from_str(content)?;
    if item.app_name.trim().is_empty() {
        return Err(ManifestError::Invalid("AppName is empty".to_string()));
    }
    Ok(item)
}

/// Parses an `.item` file from disk
pub fn parse_manifest_file<P: AsRef<Path>>(path: P) -> Result<EpicManifestItem, ManifestError> {
    let content = fs::read_to_string(path)?;
    parse_manifest_from_str(&content)
}

fn scan_items_in_directory(dir: &Path, items: &mut Vec<EpicManifestItem>) -> Result<(), ManifestError> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(dir)?;
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
    Ok(())
}

/// Scans a directory for all `.item` files and parses them
pub fn scan_manifest_directory<P: AsRef<Path>>(dir: P) -> Result<Vec<EpicManifestItem>, ManifestError> {
    let path = dir.as_ref();
    if !path.is_dir() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    scan_items_in_directory(path, &mut items)?;

    // Scan Pending subfolder (e.g. Data\Manifests\Pending) for updating/downloading games
    let pending_dir = path.join("Pending");
    if pending_dir.is_dir() {
        scan_items_in_directory(&pending_dir, &mut items)?;
    } else {
        let pending_lower = path.join("pending");
        if pending_lower.is_dir() && pending_lower != pending_dir {
            scan_items_in_directory(&pending_lower, &mut items)?;
        }
    }

    Ok(items)
}

/// Resolves standard Epic Games manifest paths for current platform
pub fn resolve_default_manifest_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(override_dir) = std::env::var("SWCH_EPIC_MANIFESTS_DIR") {
        let p = PathBuf::from(override_dir);
        if p.is_dir() {
            dirs.push(p);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(program_data) = std::env::var("ProgramData") {
            dirs.push(PathBuf::from(program_data).join("Epic/EpicGamesLauncher/Data/Manifests"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/Users/Shared/Epic Games/EpicGamesLauncher/Data/Manifests"));
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".local/share/bottles/bottles/Epic-Games-Store/drive_c/ProgramData/Epic/EpicGamesLauncher/Data/Manifests"));
        }
    }

    dirs
}
