use std::path::{Path, PathBuf};
use std::time::Duration;
use serde_json::Value;
use tracing::{info, warn};

use crate::modules::detector::LauncherDetector;
use crate::storage::atomic::atomic_write_str;
use crate::storage::models::MinecraftAccount;
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

    /// Performs a zero-latency atomic hot-swap of the active Minecraft/TLauncher account:
    /// 1. Gracefully terminates any running Minecraft/TLauncher launcher processes (2s timeout).
    /// 2. Creates a `.bak` backup copy of relevant account config files.
    /// 3. Modifies active accounts in launcher_accounts.json, TlauncherProfiles.json, and properties.
    /// 4. Performs an atomic write using temporary file replacement.
    /// 5. Updates the database active status.
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

        // 3. Resolve .minecraft and .tlauncher directories
        let mc_dir = self
            .detector
            .detect_minecraft_data_dir()
            .unwrap_or_else(|| PathBuf::from(".minecraft"));
        let tl_dir = self.detector.detect_tlauncher_dir();

        let mut files_modified = 0;
        let is_tl = account.account_type.starts_with("tlauncher") || account.account_type == "offline";

        // 4. Update launcher_accounts.json if present and not pure TLauncher/offline
        let accounts_file = mc_dir.join("launcher_accounts.json");
        if accounts_file.exists() && !is_tl {
            match update_accounts_json(&accounts_file, &account.account_id) {
                Ok(modified) => {
                    if modified {
                        files_modified += 1;
                    }
                }
                Err(e) => warn!("Failed to update launcher_accounts.json: {}", e),
            }
        }

        // 5. Update launcher_accounts_microsoft_store.json if present and not pure TLauncher/offline
        let ms_accounts_file = mc_dir.join("launcher_accounts_microsoft_store.json");
        if ms_accounts_file.exists() && !is_tl {
            match update_accounts_json(&ms_accounts_file, &account.account_id) {
                Ok(modified) => {
                    if modified {
                        files_modified += 1;
                    }
                }
                Err(e) => warn!("Failed to update launcher_accounts_microsoft_store.json: {}", e),
            }
        }

        let tl_names = ["TlauncherProfiles.json", "tlauncher_profiles.json", "tlauncher-profiles.json", "tlauncherProfiles.json"];

        // 6. Update TLauncher profiles in mc_dir if present, or if account is TLauncher
        let mut tl_mc_updated = false;
        for name in &tl_names {
            let tl_mc_file = mc_dir.join(name);
            if tl_mc_file.exists() {
                match update_tlauncher_profiles_json(&tl_mc_file, &account) {
                    Ok(_) => {
                        files_modified += 1;
                        tl_mc_updated = true;
                    }
                    Err(e) => warn!("Failed to update {} in mc_dir: {}", name, e),
                }
            }
        }
        if !tl_mc_updated && (is_tl && mc_dir.exists()) {
            let tl_mc_file = mc_dir.join("TlauncherProfiles.json");
            match update_tlauncher_profiles_json(&tl_mc_file, &account) {
                Ok(_) => files_modified += 1,
                Err(e) => warn!("Failed to create TlauncherProfiles.json in mc_dir: {}", e),
            }
        }

        // 7. Update TLauncher profiles and properties in .tlauncher if directory exists
        if let Some(ref tdir) = tl_dir {
            if tdir.exists() && tdir != &mc_dir {
                for name in &tl_names {
                    let tl_file = tdir.join(name);
                    if tl_file.exists() || is_tl {
                        match update_tlauncher_profiles_json(&tl_file, &account) {
                            Ok(_) => files_modified += 1,
                            Err(e) => warn!("Failed to update {} in tl_dir: {}", name, e),
                        }
                    }
                }
                for prop_name in &["tlauncher-2.0.properties", "tlauncher.properties"] {
                    let prop_path = tdir.join(prop_name);
                    if prop_path.exists() || is_tl {
                        match update_tlauncher_properties(&prop_path, &account.username) {
                            Ok(_) => files_modified += 1,
                            Err(e) => warn!("Failed to update {}: {}", prop_name, e),
                        }
                    }
                }
            }
        }

        // 8. Update properties in mc_dir if present
        for prop_name in &["tlauncher-2.0.properties", "tlauncher.properties"] {
            let prop_path = mc_dir.join(prop_name);
            if prop_path.exists() {
                match update_tlauncher_properties(&prop_path, &account.username) {
                    Ok(_) => files_modified += 1,
                    Err(e) => warn!("Failed to update mc_dir {}: {}", prop_name, e),
                }
            }
        }

        // 9. Update database active status
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
/// Returns Ok(true) if modified, Ok(false) if target was not part of this accounts file.
pub fn update_accounts_json(path: &Path, target_account_id: &str) -> Result<bool, HotSwapError> {
    let content = std::fs::read_to_string(path)?;

    // 1. Parse JSON
    let mut root: Value = serde_json::from_str(&content)?;

    let has_target = root
        .get("accounts")
        .and_then(|a| a.as_object())
        .map(|accs| {
            accs.iter().any(|(key, acc_val)| {
                key == target_account_id
                    || acc_val.get("localId").and_then(|l| l.as_str()) == Some(target_account_id)
            })
        })
        .unwrap_or(false);

    if !has_target {
        return Ok(false);
    }

    // 2. Create .bak backup file
    let bak_path = PathBuf::from(format!("{}.bak", path.display()));
    if let Err(e) = std::fs::copy(path, &bak_path) {
        warn!("Failed to create backup file {}: {}", bak_path.display(), e);
    } else {
        info!("Saved backup: {}", bak_path.display());
    }

    // 3. Mutate JSON
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

    // 4. Serialize mutated content
    let new_json = serde_json::to_string_pretty(&root)?;

    // 5. Atomic write with fsync
    atomic_write_str(path, &new_json).map_err(|e| HotSwapError::Io(std::io::Error::other(e)))?;

    Ok(true)
}

/// Reads or creates TlauncherProfiles.json, creates a `.bak` backup, updates active account, and atomically writes back.
pub fn update_tlauncher_profiles_json(
    path: &Path,
    target_account: &MinecraftAccount,
) -> Result<(), HotSwapError> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut root: Value = if path.exists() {
        let content = std::fs::read_to_string(path)?;
        let bak_path = PathBuf::from(format!("{}.bak", path.display()));
        if let Err(e) = std::fs::copy(path, &bak_path) {
            warn!("Failed to create backup file {}: {}", bak_path.display(), e);
        } else {
            info!("Saved backup: {}", bak_path.display());
        }
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let target_user = &target_account.username;
    let target_uuid = &target_account.uuid;
    let target_id = &target_account.account_id;

    if let Some(obj) = root.as_object_mut() {
        if let Some(existing_user) = obj.get("selectedUser") {
            if existing_user.is_string() {
                obj.insert("selectedUser".to_string(), Value::String(target_user.to_string()));
            } else {
                obj.insert(
                    "selectedUser".to_string(),
                    serde_json::json!({
                        "account": target_user,
                        "profile": target_user
                    }),
                );
            }
        } else {
            obj.insert(
                "selectedUser".to_string(),
                serde_json::json!({
                    "account": target_user,
                    "profile": target_user
                }),
            );
        }
        obj.insert("selected_account".to_string(), Value::String(target_user.to_string()));
        obj.insert("selectedAccount".to_string(), Value::String(target_user.to_string()));
        obj.insert("activeAccountLocalId".to_string(), Value::String(target_id.to_string()));

        let raw_type = if target_account.account_type.contains("free") || target_account.account_type == "offline" {
            "FREE"
        } else if target_account.account_type.contains("mojang") {
            "MOJANG"
        } else if target_account.account_type.contains("msa") {
            "MSA"
        } else {
            "TLAUNCHER"
        };

        if let Some(accs_map) = obj.get_mut("accounts").and_then(|a| a.as_object_mut()) {
            let mut found = false;
            for (key, acc_val) in accs_map.iter_mut() {
                if let Some(acc_obj) = acc_val.as_object_mut() {
                    let is_match = key.eq_ignore_ascii_case(target_id)
                        || key.eq_ignore_ascii_case(target_user)
                        || key.eq_ignore_ascii_case(target_uuid)
                        || acc_obj.get("username").and_then(|u| u.as_str()).map_or(false, |u| u.eq_ignore_ascii_case(target_user))
                        || acc_obj.get("displayName").and_then(|u| u.as_str()).map_or(false, |u| u.eq_ignore_ascii_case(target_user))
                        || acc_obj.get("name").and_then(|u| u.as_str()).map_or(false, |u| u.eq_ignore_ascii_case(target_user))
                        || acc_obj.get("uuid").and_then(|u| u.as_str()).map_or(false, |u| u.eq_ignore_ascii_case(target_uuid));

                    acc_obj.insert("active".to_string(), Value::Bool(is_match));
                    if is_match {
                        found = true;
                    }
                }
            }
            if !found {
                accs_map.insert(
                    target_user.to_string(),
                    serde_json::json!({
                        "username": target_user,
                        "displayName": target_user,
                        "type": raw_type,
                        "uuid": target_uuid,
                        "active": true
                    }),
                );
            }
        } else if let Some(accs_arr) = obj.get_mut("accounts").and_then(|a| a.as_array_mut()) {
            let mut found = false;
            for acc_val in accs_arr.iter_mut() {
                if let Some(acc_obj) = acc_val.as_object_mut() {
                    let is_match = acc_obj.get("username").and_then(|u| u.as_str()).map_or(false, |u| u.eq_ignore_ascii_case(target_user))
                        || acc_obj.get("displayName").and_then(|u| u.as_str()).map_or(false, |u| u.eq_ignore_ascii_case(target_user))
                        || acc_obj.get("name").and_then(|u| u.as_str()).map_or(false, |u| u.eq_ignore_ascii_case(target_user))
                        || acc_obj.get("uuid").and_then(|u| u.as_str()).map_or(false, |u| u.eq_ignore_ascii_case(target_uuid));

                    acc_obj.insert("active".to_string(), Value::Bool(is_match));
                    if is_match {
                        found = true;
                    }
                }
            }
            if !found {
                accs_arr.push(serde_json::json!({
                    "username": target_user,
                    "displayName": target_user,
                    "type": raw_type,
                    "uuid": target_uuid,
                    "active": true
                }));
            }
        } else {
            let mut new_map = serde_json::Map::new();
            new_map.insert(
                target_user.to_string(),
                serde_json::json!({
                    "username": target_user,
                    "displayName": target_user,
                    "type": raw_type,
                    "uuid": target_uuid,
                    "active": true
                }),
            );
            obj.insert("accounts".to_string(), Value::Object(new_map));
        }
    }

    let new_json = serde_json::to_string_pretty(&root)?;
    atomic_write_str(path, &new_json).map_err(|e| HotSwapError::Io(std::io::Error::other(e)))?;

    Ok(())
}

/// Reads or creates tlauncher-2.0.properties / tlauncher.properties, creates a `.bak` backup,
/// updates `selected.account` / `login.account`, and atomically writes back.
pub fn update_tlauncher_properties(path: &Path, username: &str) -> Result<(), HotSwapError> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut lines = Vec::new();
    let mut found_selected = false;
    let mut found_login = false;
    let mut found_account_selected = false;

    if path.exists() {
        let content = std::fs::read_to_string(path)?;
        let bak_path = PathBuf::from(format!("{}.bak", path.display()));
        if let Err(e) = std::fs::copy(path, &bak_path) {
            warn!("Failed to create backup file {}: {}", bak_path.display(), e);
        } else {
            info!("Saved backup: {}", bak_path.display());
        }

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.starts_with(';') {
                lines.push(line.to_string());
                continue;
            }
            if let Some((k, _)) = trimmed.split_once('=') {
                let key_norm = k.trim().to_lowercase();
                if key_norm == "selected.account" {
                    lines.push(format!("selected.account={}", username));
                    found_selected = true;
                } else if key_norm == "login.account" {
                    lines.push(format!("login.account={}", username));
                    found_login = true;
                } else if key_norm == "account.selected" {
                    lines.push(format!("account.selected={}", username));
                    found_account_selected = true;
                } else if key_norm == "user.login" {
                    lines.push(format!("user.login={}", username));
                } else {
                    lines.push(line.to_string());
                }
            } else {
                lines.push(line.to_string());
            }
        }
    }

    if !found_selected {
        lines.push(format!("selected.account={}", username));
    }
    if !found_login {
        lines.push(format!("login.account={}", username));
    }
    if !found_account_selected {
        lines.push(format!("account.selected={}", username));
    }

    let new_content = lines.join("\n") + "\n";
    atomic_write_str(path, &new_content).map_err(|e| HotSwapError::Io(std::io::Error::other(e)))?;

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

    #[test]
    fn test_update_tlauncher_profiles_json_and_properties() {
        let dir = tempdir().unwrap();
        let profiles_path = dir.path().join("TlauncherProfiles.json");
        let props_path = dir.path().join("tlauncher-2.0.properties");

        let initial_json = r#"{
            "selected_account": "OldPlayer",
            "accounts": {
                "OldPlayer": {
                    "username": "OldPlayer",
                    "type": "FREE",
                    "active": true
                }
            }
        }"#;
        std::fs::write(&profiles_path, initial_json).unwrap();

        let account = MinecraftAccount {
            account_id: "tl_newplayer".to_string(),
            username: "NewPlayer".to_string(),
            account_type: "tlauncher_free".to_string(),
            uuid: "11111111-2222-3333-4444-555555555555".to_string(),
            avatar_url: None,
            skin_url: None,
            is_active: true,
            access_token: None,
            raw_json: None,
            last_used: None,
            updated_at: "2026-10-07T00:00:00Z".to_string(),
        };

        update_tlauncher_profiles_json(&profiles_path, &account).unwrap();

        assert!(dir.path().join("TlauncherProfiles.json.bak").exists());
        let content = std::fs::read_to_string(&profiles_path).unwrap();
        let val: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(val["selected_account"], "NewPlayer");
        assert_eq!(val["accounts"]["NewPlayer"]["active"], true);
        assert_eq!(val["accounts"]["OldPlayer"]["active"], false);

        // Properties update
        update_tlauncher_properties(&props_path, "NewPlayer").unwrap();
        let prop_content = std::fs::read_to_string(&props_path).unwrap();
        assert!(prop_content.contains("selected.account=NewPlayer"));
        assert!(prop_content.contains("login.account=NewPlayer"));
    }
}
