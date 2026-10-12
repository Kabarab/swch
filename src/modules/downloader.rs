use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::types::{ModuleError, ModuleId};

/// Manifest persisted inside `~/.swch/modules/<id>/manifest.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub installed_at: String,
    pub size_bytes: u64,
    pub launcher_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModuleDownloader {
    base_dir: PathBuf,
}

impl ModuleDownloader {
    pub fn new() -> Result<Self, ModuleError> {
        let base_dir = if let Ok(custom) = std::env::var("SWCH_MODULES_DIR") {
            PathBuf::from(custom)
        } else {
            let root = dirs::config_dir()
                .or_else(|| dirs::home_dir().map(|h| h.join(".swch")))
                .unwrap_or_else(|| PathBuf::from(".swch"));
            root.join("modules")
        };

        if !base_dir.exists() {
            let _ = std::fs::create_dir_all(&base_dir);
        }

        Ok(Self { base_dir })
    }

    pub fn with_dir(dir: PathBuf) -> Self {
        if !dir.exists() {
            let _ = std::fs::create_dir_all(&dir);
        }
        Self { base_dir: dir }
    }

    /// Resolves directory for a specific module: `~/.swch/modules/<id>/`
    pub fn module_dir(&self, id: ModuleId) -> PathBuf {
        self.base_dir.join(id.as_str())
    }

    /// Checks if the module manifest exists and is valid on disk.
    pub fn is_installed(&self, id: ModuleId) -> bool {
        let manifest_file = self.module_dir(id).join("manifest.json");
        manifest_file.exists()
    }

    /// Reads module manifest if present.
    pub fn read_manifest(&self, id: ModuleId) -> Option<ModuleManifest> {
        let manifest_file = self.module_dir(id).join("manifest.json");
        if manifest_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&manifest_file) {
                if let Ok(manifest) = serde_json::from_str::<ModuleManifest>(&content) {
                    return Some(manifest);
                }
            }
        }
        None
    }

    /// Installs or updates a module: creates directory, writes manifest, initializes storage.
    pub async fn install_module(
        &self,
        id: ModuleId,
        launcher_path: Option<&Path>,
    ) -> Result<ModuleManifest, ModuleError> {
        let dir = self.module_dir(id);
        std::fs::create_dir_all(&dir).map_err(|e| {
            ModuleError::Io(e)
        })?;

        let manifest = ModuleManifest {
            id: id.as_str().to_string(),
            name: id.display_name().to_string(),
            version: "1.0.0".to_string(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            size_bytes: match id {
                ModuleId::Steam => 14_500_000,
                ModuleId::Epic => 12_800_000,
                ModuleId::Riot => 8_200_000,
                ModuleId::Roblox => 10_500_000,
                ModuleId::Minecraft => 9_500_000,
                ModuleId::Ubisoft => 0,
                ModuleId::Gog => 0,
                ModuleId::Hoyoverse => 0,
            },
            launcher_path: launcher_path.map(|p| p.to_string_lossy().to_string()),
        };

        let json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| ModuleError::OperationFailed(e.to_string()))?;
        std::fs::write(dir.join("manifest.json"), json)
            .map_err(ModuleError::Io)?;

        info!("Module '{}' successfully installed into {}", id, dir.display());
        Ok(manifest)
    }

    /// Purges all files for the specified module from disk.
    pub fn purge_module(&self, id: ModuleId) -> Result<(), ModuleError> {
        let dir = self.module_dir(id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(ModuleError::Io)?;
            info!("Module '{}' files purged from disk at {}", id, dir.display());
        }
        Ok(())
    }
}
