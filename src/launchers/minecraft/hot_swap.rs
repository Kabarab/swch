use std::path::{Path, PathBuf};
use std::time::Duration;
use serde_json::Value;
use tracing::{info, warn};

use crate::modules::detector::LauncherDetector;
use crate::storage::atomic::atomic_write_str;
use crate::storage::Database;
use crate::sys::process::{terminate_launcher, LauncherType, TerminateOptions};

#[derive(Debug, thiserror::Error)]
pub enum HotSwapError {
    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Minecraft directory not found")]
    DirectoryNotFound,

    #[error("I/O error during hot-swap: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Process error: {0}")]
    Process(String),
}

/// Hot-swap manager for atomically switching the active Minecraft account.
#[derive(Clone)]
pub struct MinecraftHotSwap {
    db: Database,
    detector: LauncherDetector,
}

impl MinecraftHotSwap {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            detector: LauncherDetector::new(),
        }
    }

    /// Performs a zero-latency atomic hot-swap of the active Minecraft account:
    /// 1. Gracefully terminates any running Minecraft launcher processes (2s timeout).
    /// 2. Creates a `.bak` backup copy of launcher_accounts.json.
    /// 3. Modifies activeAccountLocalId.
    /// 4. Performs an atomic write using temporary file replacement.
    /// 5. Updates the database and base accounts state.
    pub async fn switch_account(&self, target_account_id: &str) -> Result<String, HotSwapError> {
        // 1. Verify account exists in DB
        let account = self
            .db
            .get_minecraft_account(target_account_id)
            .await
            .map_err(|e| HotSwapError::Database(e.to_string()))?
            .ok_or_else(|| HotSwapError::AccountNotFound(target_account_id.to_string()))?;

        info!("Initiating hot-swap to Minecraft account: {} ({})", account.username, account.account_id);

        // 2. Soft termination of launcher processes (2s graceful timeout)
        let term_options = TerminateOptions {
            graceful_timeout: Duration::from_secs(2),
            poll_interval: Duration::from_millis(100),
            force_timeout: Duration::from_millis(1500),
            settle_delay: Duration::from_millis(50),
            include_descendants: true,
        };

        if let Err(e) = terminate_launcher(LauncherType::Minecraft, &term_options).await {
            warn!("Notice during launcher process termination: {:?}", e);
        }

        // 3. Resolve .minecraft data directory
        let mc_dir = self
            .detector
            .detect_minecraft_data_dir()
            .unwrap_or_else(|| PathBuf::from(".minecraft"));

        let mut files_modified = 0;

        // 4. Update launcher_accounts.json if present
        let accounts_file = mc_dir.join("launcher_accounts.json");
        if accounts_file.exists() {
            match update_accounts_json(&accounts_file, &account.account_id) {
                Ok(_) => files_modified += 1,
                Err(e) => warn!("Failed to update launcher_accounts.json: {}", e),
            }
        }

        // 5. Update launcher_accounts_microsoft_store.json if present
        let ms_accounts_file = mc_dir.join("launcher_accounts_microsoft_store.json");
        if ms_accounts_file.exists() {
            match update_accounts_json(&ms_accounts_file, &account.account_id) {
                Ok(_) => files_modified += 1,
                Err(e) => warn!("Failed to update launcher_accounts_microsoft_store.json: {}", e),
            }
        }

        // 6. Update database active status
        self.db
            .set_active_minecraft_account(&account.account_id)
            .await
            .map_err(|e| HotSwapError::Database(e.to_string()))?;

        info!(
            "Successfully hot-swapped to account {} (modified {} file(s))",
            account.username, files_modified
        );

        Ok(format!(
            "Активный аккаунт Minecraft переключен на {}",
            account.username
        ))
    }
}

/// Reads the accounts file, writes a `.bak` backup, updates `activeAccountLocalId`, and atomically writes back.
pub fn update_accounts_json(path: &Path, target_account_id: &str) -> Result<(), HotSwapError> {
    let content = std::fs::read_to_string(path)?;

    // 1. Create .bak backup file
    let bak_path = PathBuf::from(format!("{}.bak", path.display()));
    if let Err(e) = std::fs::copy(path, &bak_path) {
        warn!("Failed to create backup file {}: {}", bak_path.display(), e);
    } else {
        info!("Saved backup: {}", bak_path.display());
    }

    // 2. Parse and mutate JSON
    let mut root: Value = serde_json::from_str(&content)?;

    if let Some(obj) = root.as_object_mut() {
        obj.insert(
            "activeAccountLocalId".to_string(),
            Value::String(target_account_id.to_string()),
        );

        // Also check if account is in accounts object and set active flag if present
        if let Some(accs) = obj.get_mut("accounts").and_then(|a| a.as_object_mut()) {
            for (key, acc_val) in accs.iter_mut() {
                if let Some(acc_obj) = acc_val.as_object_mut() {
                    let is_target = key == target_account_id
                        || acc_obj.get("localId").and_then(|l| l.as_str()) == Some(target_account_id);
                    acc_obj.insert("active".to_string(), Value::Bool(is_target));
                }
            }
        }
    }

    // 3. Serialize mutated content
    let new_json = serde_json::to_string_pretty(&root)?;

    // 4. Atomic write with fsync
    atomic_write_str(path, &new_json).map_err(|e| HotSwapError::Io(std::io::Error::other(e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_update_accounts_json_creates_backup_and_swaps_active_id() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("launcher_accounts.json");
        let bak_path = dir.path().join("launcher_accounts.json.bak");

        let original = r#"{
            "activeAccountLocalId": "acc-1",
            "accounts": {
                "acc-1": { "localId": "acc-1", "username": "steve" },
                "acc-2": { "localId": "acc-2", "username": "alex" }
            }
        }"#;

        std::fs::write(&file_path, original).unwrap();

        update_accounts_json(&file_path, "acc-2").unwrap();

        // Verify backup exists with original content
        assert!(bak_path.exists());
        let bak_content = std::fs::read_to_string(&bak_path).unwrap();
        assert!(bak_content.contains("\"activeAccountLocalId\": \"acc-1\""));

        // Verify updated file has new active ID
        let updated = std::fs::read_to_string(&file_path).unwrap();
        let parsed: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(
            parsed.get("activeAccountLocalId").and_then(|v| v.as_str()),
            Some("acc-2")
        );
    }
}
