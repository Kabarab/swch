//! Epic Games Launcher session and profile switcher management.
//!
//! Provides hot-swapping of Epic Games Launcher user profiles by backing up
//! and restoring configuration files and web authentication caches:
//! - `KeyValueStore.ini` (`AccountId`)
//! - `GameUserSettings.ini` (`[RememberMe] Data`, `[Offline] Data`)
//! - `webcache` (CEF IndexedDB, LocalStorage, and Cookies)

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::launchers::epic::models::EpicError;
use crate::storage::atomic::{atomic_copy, atomic_write_json, atomic_write_str, copy_dir_all};
use crate::sys::process::{
    find_launcher_processes, terminate_launcher, LauncherType, TerminateOptions, TerminationReport,
};
use sysinfo::System;

/// Paths to key configuration and cache directories used by Epic Games Launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicPaths {
    pub key_value_store: PathBuf,
    pub game_user_settings: PathBuf,
    pub webcache_dir: PathBuf,
    pub app_bundle_path: Option<PathBuf>,
}

impl EpicPaths {
    /// Automatically detects standard Epic Games Launcher paths based on OS and environment.
    pub fn detect() -> Result<Self, EpicError> {
        // 1. Check environment variable overrides
        let env_kvs = std::env::var("SWCH_EPIC_KEY_VALUE_STORE").ok().map(PathBuf::from);
        let env_gus = std::env::var("SWCH_EPIC_GAME_USER_SETTINGS").ok().map(PathBuf::from);
        let env_webcache = std::env::var("SWCH_EPIC_WEBCACHE_DIR").ok().map(PathBuf::from);

        #[cfg(target_os = "macos")]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                EpicError::Io("Could not determine user home directory".to_string())
            })?;

            let key_value_store = env_kvs.unwrap_or_else(|| {
                home.join("Library/Application Support/Epic/Epic Games/KeyValueStore.ini")
            });

            let game_user_settings = env_gus.unwrap_or_else(|| {
                home.join("Library/Application Support/Epic/EpicGamesLauncher/Saved/Config/MacEditor/GameUserSettings.ini")
            });

            let webcache_dir = env_webcache.unwrap_or_else(|| {
                let cache_base = home.join("Library/Caches/com.epicgames.EpicGamesLauncher");
                // Check if webcache_4430 exists or look for any webcache_* directory
                let default_webcache = cache_base.join("webcache_4430");
                if default_webcache.exists() {
                    default_webcache
                } else if let Ok(entries) = fs::read_dir(&cache_base) {
                    let mut found = None;
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with("webcache") && entry.path().is_dir() {
                            found = Some(entry.path());
                            break;
                        }
                    }
                    found.unwrap_or(default_webcache)
                } else {
                    default_webcache
                }
            });

            let app_bundle_path = {
                let candidates = [
                    PathBuf::from("/Applications/Epic Games Launcher.app"),
                    home.join("Applications/Epic Games Launcher.app"),
                ];
                candidates.into_iter().find(|p| p.exists())
            };

            Ok(Self {
                key_value_store,
                game_user_settings,
                webcache_dir,
                app_bundle_path,
            })
        }

        #[cfg(target_os = "windows")]
        {
            let local_app_data = dirs::data_local_dir().ok_or_else(|| {
                EpicError::Io("Could not determine LOCALAPPDATA directory".to_string())
            })?;

            let key_value_store = env_kvs.unwrap_or_else(|| {
                local_app_data.join("Epic Games\\Epic Games\\KeyValueStore.ini")
            });

            let game_user_settings = env_gus.unwrap_or_else(|| {
                local_app_data.join("EpicGamesLauncher\\Saved\\Config\\Windows\\GameUserSettings.ini")
            });

            let webcache_dir = env_webcache.unwrap_or_else(|| {
                let c1 = local_app_data.join("EpicGamesLauncher\\Saved\\webcache_4430");
                if c1.exists() {
                    c1
                } else {
                    local_app_data.join("EpicGamesLauncher\\Saved\\webcache")
                }
            });

            let app_bundle_path = {
                let prog = std::env::var("ProgramFiles(x86)")
                    .unwrap_or_else(|_| "C:\\Program Files (x86)".into());
                let exe = PathBuf::from(prog).join(
                    "Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe",
                );
                if exe.exists() {
                    Some(exe)
                } else {
                    None
                }
            };

            Ok(Self {
                key_value_store,
                game_user_settings,
                webcache_dir,
                app_bundle_path,
            })
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            Ok(Self {
                key_value_store: env_kvs.unwrap_or_else(|| home.join(".epic/KeyValueStore.ini")),
                game_user_settings: env_gus.unwrap_or_else(|| home.join(".epic/GameUserSettings.ini")),
                webcache_dir: env_webcache.unwrap_or_else(|| home.join(".epic/webcache")),
                app_bundle_path: None,
            })
        }
    }
}

/// Metadata stored alongside a backed-up Epic Games session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpicSessionManifest {
    pub account_id: String,
    pub display_name: Option<String>,
    pub backed_up_at: String,
    pub has_webcache: bool,
}

/// Result returned from an account switch operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchEpicSessionResult {
    pub target_account_id: String,
    pub restored_saved_session: bool,
    pub already_active: bool,
}

/// Session manager responsible for profile swapping, process lifecycle, and backups.
#[derive(Debug, Clone)]
pub struct EpicSessionManager {
    custom_paths: Option<EpicPaths>,
    custom_sessions_dir: Option<PathBuf>,
    skip_process_termination: bool,
}

impl Default for EpicSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl EpicSessionManager {
    /// Creates a new manager with default auto-detected paths.
    pub fn new() -> Self {
        let skip_proc = std::env::var("SWCH_SKIP_PROCESS_TERMINATION")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Self {
            custom_paths: None,
            custom_sessions_dir: None,
            skip_process_termination: skip_proc,
        }
    }

    /// Creates a manager configured with explicit paths.
    pub fn with_paths(paths: EpicPaths) -> Self {
        Self {
            custom_paths: Some(paths),
            custom_sessions_dir: None,
            skip_process_termination: false,
        }
    }

    /// Configures an explicit sessions directory for swch profile snapshots.
    pub fn with_sessions_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.custom_sessions_dir = Some(dir.into());
        self
    }

    /// Configures whether to skip process termination (useful in automated tests).
    pub fn with_skip_process_termination(mut self, skip: bool) -> Self {
        self.skip_process_termination = skip;
        self
    }

    /// Resolves active `EpicPaths`.
    pub fn paths(&self) -> Result<EpicPaths, EpicError> {
        match &self.custom_paths {
            Some(p) => Ok(p.clone()),
            None => EpicPaths::detect(),
        }
    }

    /// Resolves directory storing profile snapshots.
    pub fn sessions_dir(&self) -> Result<PathBuf, EpicError> {
        if let Some(ref dir) = self.custom_sessions_dir {
            fs::create_dir_all(dir)?;
            return Ok(dir.clone());
        }

        if let Ok(dir) = std::env::var("SWCH_EPIC_SESSIONS_DIR") {
            let p = PathBuf::from(dir.trim());
            fs::create_dir_all(&p)?;
            return Ok(p);
        }

        if let Some(base) = dirs::data_local_dir() {
            let p = base.join("swch").join("sessions").join("epic");
            fs::create_dir_all(&p)?;
            return Ok(p);
        }

        let fallback = PathBuf::from(".swch").join("sessions").join("epic");
        fs::create_dir_all(&fallback)?;
        Ok(fallback)
    }

    /// Checks if any Epic Games Launcher processes are currently active in the OS.
    pub fn is_launcher_running(&self) -> bool {
        if self.skip_process_termination {
            return false;
        }
        let mut system = System::new();
        system.refresh_processes();
        !find_launcher_processes(&system, LauncherType::EpicGames).is_empty()
    }

    /// Terminates all running Epic Games Launcher and EpicWebHelper processes.
    pub async fn terminate_processes(&self) -> Result<TerminationReport, EpicError> {
        if self.skip_process_termination {
            return Ok(TerminationReport::default());
        }
        let options = TerminateOptions::default();
        let report = terminate_launcher(LauncherType::EpicGames, &options).await?;
        Ok(report)
    }

    /// Reads the account ID currently active according to the launcher's `KeyValueStore.ini`.
    pub fn read_current_launcher_account(&self) -> Option<String> {
        let paths = self.paths().ok()?;
        read_account_id_from_ini(&paths.key_value_store)
    }

    /// Resolves the currently active account ID, consulting KeyValueStore.ini and current_session.json.
    pub fn active_account_id(&self) -> Option<String> {
        if let Some(id) = self.read_current_launcher_account() {
            let clean = id.trim();
            if !clean.is_empty() {
                return Some(clean.to_string());
            }
        }
        if let Ok(sessions_root) = self.sessions_dir() {
            let meta_path = sessions_root.join("current_session.json");
            if let Ok(content) = fs::read_to_string(&meta_path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(id) = val["active_account_id"].as_str() {
                        let clean = id.trim();
                        if !clean.is_empty() {
                            return Some(clean.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Records the currently active account ID into `current_session.json`.
    pub fn set_active_account_id(&self, account_id: &str) -> Result<(), EpicError> {
        let sessions_root = self.sessions_dir()?;
        let meta_path = sessions_root.join("current_session.json");
        let json = serde_json::json!({
            "active_account_id": account_id.trim(),
            "switched_at": Utc::now().to_rfc3339(),
        });
        let _ = atomic_write_json(&meta_path, &json);
        Ok(())
    }

    /// Checks if the current on-disk Epic configuration and cache files contain valid authenticating data.
    pub fn is_disk_authenticated(&self) -> bool {
        let Ok(paths) = self.paths() else {
            return false;
        };
        let cookies_path = paths.webcache_dir.join("Cookies");
        let has_cookies = cookies_path.is_file()
            && fs::metadata(&cookies_path).map(|m| m.len() > 0).unwrap_or(false);
        let has_remember_me = ini_has_remember_me(&paths.game_user_settings);
        has_cookies || has_remember_me
    }

    /// Checks if a saved session snapshot exists on disk for the specified account ID.
    pub fn has_session(&self, account_id: &str) -> bool {
        let clean_id = account_id.trim();
        if clean_id.is_empty() {
            return false;
        }
        let Ok(sessions_root) = self.sessions_dir() else {
            return false;
        };
        let account_dir = sessions_root.join(clean_id);
        if !account_dir.is_dir() {
            return false;
        }

        // A valid saved session must have authenticating data:
        // 1. webcache with non-empty Cookies file, OR
        // 2. GameUserSettings.ini with [RememberMe] Data= token
        let cookies_path = account_dir.join("webcache").join("Cookies");
        let has_cookies = cookies_path.is_file()
            && fs::metadata(&cookies_path).map(|m| m.len() > 0).unwrap_or(false);
        let has_remember_me = ini_has_remember_me(&account_dir.join("GameUserSettings.ini"));

        has_cookies || has_remember_me
    }

    /// Backs up the current on-disk Epic session files for the specified account ID.
    pub fn backup_session(
        &self,
        account_id: &str,
        display_name: Option<&str>,
    ) -> Result<PathBuf, EpicError> {
        let clean_id = account_id.trim();
        if clean_id.is_empty() {
            return Err(EpicError::Database("Cannot backup session for empty account ID".into()));
        }

        let paths = self.paths()?;
        let sessions_root = self.sessions_dir()?;
        let account_dir = sessions_root.join(clean_id);
        fs::create_dir_all(&account_dir)?;

        // 1. Backup KeyValueStore.ini
        if paths.key_value_store.exists() {
            let dest = account_dir.join("KeyValueStore.ini");
            let _ = atomic_copy(&paths.key_value_store, &dest);
        }

        // 2. Backup GameUserSettings.ini
        if paths.game_user_settings.exists() {
            let dest = account_dir.join("GameUserSettings.ini");
            let _ = atomic_copy(&paths.game_user_settings, &dest);
            let _ = ensure_remember_me_enabled_in_ini(&dest);
        }

        // 3. Backup webcache directory
        let mut has_webcache = false;
        if paths.webcache_dir.exists() && paths.webcache_dir.is_dir() {
            let dest_cache = account_dir.join("webcache");
            if let Err(e) = copy_dir_all(&paths.webcache_dir, &dest_cache) {
                warn!("Warning during webcache backup for {}: {}", clean_id, e);
            } else {
                has_webcache = true;
            }
        }

        // Preserve previous display_name if none is provided
        let resolved_display_name = display_name
            .map(|s| s.to_string())
            .or_else(|| {
                let manifest_path = account_dir.join("session_manifest.json");
                if manifest_path.exists() {
                    let content = fs::read_to_string(&manifest_path).ok()?;
                    let prev: EpicSessionManifest = serde_json::from_str(&content).ok()?;
                    prev.display_name
                } else {
                    None
                }
            });

        // 4. Save session manifest
        let manifest = EpicSessionManifest {
            account_id: clean_id.to_string(),
            display_name: resolved_display_name,
            backed_up_at: Utc::now().to_rfc3339(),
            has_webcache,
        };
        let manifest_path = account_dir.join("session_manifest.json");
        let _ = atomic_write_json(&manifest_path, &manifest);

        info!("Backed up Epic session for '{}' to {}", clean_id, account_dir.display());
        Ok(account_dir)
    }

    /// Updates display name in manifest if a session snapshot already exists for the account.
    pub fn update_manifest_display_name(
        &self,
        account_id: &str,
        display_name: &str,
    ) -> Result<(), EpicError> {
        let sessions_root = self.sessions_dir()?;
        let account_dir = sessions_root.join(account_id.trim());
        let manifest_path = account_dir.join("session_manifest.json");
        if manifest_path.exists() {
            if let Ok(content) = fs::read_to_string(&manifest_path) {
                if let Ok(mut manifest) = serde_json::from_str::<EpicSessionManifest>(&content) {
                    manifest.display_name = Some(display_name.to_string());
                    let _ = atomic_write_json(&manifest_path, &manifest);
                }
            }
        }
        Ok(())
    }

    /// Restores a session snapshot for `account_id` into the launcher's directory.
    ///
    /// If a snapshot exists, it is restored and returns `Ok(true)`.
    /// If no snapshot exists yet, `KeyValueStore.ini` is set to `account_id`,
    /// stale tokens/webcache are cleared to guarantee a clean login screen, and returns `Ok(false)`.
    pub fn restore_session(&self, account_id: &str) -> Result<bool, EpicError> {
        let clean_id = account_id.trim();
        let paths = self.paths()?;
        let sessions_root = self.sessions_dir()?;
        let account_dir = sessions_root.join(clean_id);

        if self.has_session(clean_id) {
            info!("Restoring existing Epic session snapshot for '{}'", clean_id);

            // Restore KeyValueStore.ini
            let src_kvs = account_dir.join("KeyValueStore.ini");
            if src_kvs.exists() {
                if let Some(parent) = paths.key_value_store.parent() {
                    fs::create_dir_all(parent)?;
                }
                atomic_copy(&src_kvs, &paths.key_value_store)?;
            }
            write_account_id_to_ini(&paths.key_value_store, clean_id)?;

            // Restore GameUserSettings.ini
            let src_gus = account_dir.join("GameUserSettings.ini");
            if src_gus.exists() {
                if let Some(parent) = paths.game_user_settings.parent() {
                    fs::create_dir_all(parent)?;
                }
                atomic_copy(&src_gus, &paths.game_user_settings)?;
                let _ = ensure_remember_me_enabled_in_ini(&paths.game_user_settings);
            }

            // Restore webcache directory
            let src_webcache = account_dir.join("webcache");
            if src_webcache.exists() && src_webcache.is_dir() {
                safe_remove_dir_all(&paths.webcache_dir);
                if let Some(parent) = paths.webcache_dir.parent() {
                    fs::create_dir_all(parent)?;
                }
                if let Err(e) = copy_dir_all(&src_webcache, &paths.webcache_dir) {
                    warn!("Failed restoring webcache for {}: {}", clean_id, e);
                }
            }

            Ok(true)
        } else {
            info!(
                "No saved session snapshot found for '{}'; preparing clean login state in launcher",
                clean_id
            );

            // Set AccountId in KeyValueStore.ini
            write_account_id_to_ini(&paths.key_value_store, clean_id)?;

            // Clear old RememberMe and Offline in GameUserSettings.ini so old user session is not revived
            if paths.game_user_settings.exists() {
                let _ = clear_remember_me_in_ini(&paths.game_user_settings);
            }

            // Clear stale webcache so old login cookies do not interfere
            safe_remove_dir_all(&paths.webcache_dir);
            if let Some(cache_base) = paths.webcache_dir.parent() {
                if let Ok(entries) = fs::read_dir(cache_base) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with("webcache") && entry.path().is_dir() {
                            safe_remove_dir_all(&entry.path());
                        }
                    }
                }
            }

            Ok(false)
        }
    }

    /// Dispatches the native Epic Games Launcher application.
    pub fn launch_client(&self) -> Result<(), EpicError> {
        let paths = self.paths()?;

        #[cfg(target_os = "macos")]
        {
            if let Some(ref app) = paths.app_bundle_path {
                if app.exists() {
                    let _ = std::process::Command::new("open")
                        .arg(app)
                        .status();
                    return Ok(());
                }
            }

            if open::that("com.epicgames.launcher://").is_err() {
                let _ = std::process::Command::new("open")
                    .arg("-a")
                    .arg("Epic Games Launcher")
                    .status();
            }
            Ok(())
        }

        #[cfg(target_os = "windows")]
        {
            if let Some(ref exe) = paths.app_bundle_path {
                if exe.exists() {
                    let _ = std::process::Command::new(exe).spawn();
                    return Ok(());
                }
            }

            if open::that("com.epicgames.launcher://").is_err() {
                let prog = std::env::var("ProgramFiles(x86)")
                    .unwrap_or_else(|_| "C:\\Program Files (x86)".into());
                let exe = PathBuf::from(prog).join(
                    "Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe",
                );
                if exe.exists() {
                    let _ = std::process::Command::new(exe).spawn();
                }
            }
            Ok(())
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = paths;
            let _ = open::that("com.epicgames.launcher://");
            Ok(())
        }
    }

    /// Performs complete account switching in Epic Games Launcher:
    /// 1. Checks if the launcher is currently running.
    /// 2. If already running with `target_account_id`, simply focuses the window.
    /// 3. Otherwise, gracefully terminates running processes to release locks.
    /// 4. Backs up the outgoing account's session (if authenticated).
    /// 5. Restores (or prepares initial login for) the incoming target account.
    /// 6. Launches Epic Games Launcher with the target account active.
    pub async fn switch_account(
        &self,
        target_account_id: &str,
        display_name: Option<&str>,
    ) -> Result<SwitchEpicSessionResult, EpicError> {
        let clean_target = target_account_id.trim();
        if clean_target.is_empty() {
            return Err(EpicError::Database("Target account ID must not be empty".into()));
        }

        let is_running = self.is_launcher_running();
        let current_account = self.active_account_id();

        // If launcher is already open with the target account, ensure snapshot is saved if missing, then bring window to front
        if is_running && current_account.as_deref() == Some(clean_target) {
            info!(
                "Epic Games Launcher is already running with account '{}'. Bringing window to front.",
                clean_target
            );
            if !self.has_session(clean_target) && self.is_disk_authenticated() {
                let _ = self.backup_session(clean_target, display_name);
            }
            if !self.skip_process_termination {
                let _ = self.launch_client();
            }
            return Ok(SwitchEpicSessionResult {
                target_account_id: clean_target.to_string(),
                restored_saved_session: true,
                already_active: true,
            });
        }

        // Launcher must be closed before files are modified
        if is_running && !self.skip_process_termination {
            info!("Terminating running Epic Games Launcher processes before switching session...");
            self.terminate_processes().await?;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        // Snapshot current account's session if identified and actually authenticated on disk
        if let Some(ref outgoing_id) = current_account {
            if !outgoing_id.is_empty() && outgoing_id != clean_target && self.is_disk_authenticated() {
                info!("Saving session snapshot for outgoing Epic account '{}'...", outgoing_id);
                let _ = self.backup_session(outgoing_id, None);
            }
        }

        // Restore target session (or clean slate)
        let restored = if self.has_session(clean_target) {
            self.restore_session(clean_target)?
        } else if current_account.as_deref() == Some(clean_target) && self.is_disk_authenticated() {
            info!(
                "Live Epic session is already authenticated for '{}'. Saving initial snapshot.",
                clean_target
            );
            let _ = self.backup_session(clean_target, display_name);
            true
        } else {
            self.restore_session(clean_target)?
        };

        // Update active account tracking
        let _ = self.set_active_account_id(clean_target);

        // If target account already has a saved snapshot and display name is provided, update manifest metadata only
        if let Some(name) = display_name {
            let _ = self.update_manifest_display_name(clean_target, name);
        }

        // Dispatch launcher
        if !self.skip_process_termination {
            self.launch_client()?;
        }

        Ok(SwitchEpicSessionResult {
            target_account_id: clean_target.to_string(),
            restored_saved_session: restored,
            already_active: false,
        })
    }
}

/// Reads the `AccountId` value from `KeyValueStore.ini`.
pub fn read_account_id_from_ini(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("AccountId=") {
            let val = rest.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// Writes or updates `AccountId` in `KeyValueStore.ini`.
pub fn write_account_id_to_ini(path: &Path, account_id: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let existing = if path.exists() {
        fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut lines: Vec<String> = existing.lines().map(|s| s.to_string()).collect();
    let mut found = false;

    for line in &mut lines {
        if line.trim().starts_with("AccountId=") {
            *line = format!("AccountId={}", account_id.trim());
            found = true;
            break;
        }
    }

    if !found {
        let mut section_idx = None;
        for (i, line) in lines.iter().enumerate() {
            if line.trim().eq_ignore_ascii_case("[Unreal Engine/Identifiers]") {
                section_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = section_idx {
            lines.insert(idx + 1, format!("AccountId={}", account_id.trim()));
        } else {
            lines.push("[Unreal Engine/Identifiers]".to_string());
            lines.push(format!("AccountId={}", account_id.trim()));
            lines.push(String::new());
        }
    }

    let mut output = lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }

    atomic_write_str(path, &output).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
}

/// Resets or removes `[RememberMe]` Data and `[Offline]` Data in `GameUserSettings.ini` so stale tokens are not loaded.
pub fn clear_remember_me_in_ini(path: &Path) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(path)?;
    let mut lines = Vec::new();
    let mut in_remember_me = false;
    let mut in_offline = false;
    let mut had_remember_me = false;
    let mut had_enable = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_remember_me && !had_enable {
                lines.push("Enable=False".to_string());
                had_enable = true;
            }
            in_remember_me = trimmed.eq_ignore_ascii_case("[RememberMe]");
            in_offline = trimmed.eq_ignore_ascii_case("[Offline]");
            if in_remember_me {
                had_remember_me = true;
                had_enable = false;
            }
            lines.push(line.to_string());
            continue;
        }

        if in_remember_me {
            if trimmed.starts_with("Enable=") {
                lines.push("Enable=False".to_string());
                had_enable = true;
                continue;
            }
            if trimmed.starts_with("Data=") {
                // Skip RememberMe Data token
                continue;
            }
        }

        if in_offline && trimmed.starts_with("Data=") {
            // Skip Offline Data token so stale offline auth is purged
            continue;
        }

        lines.push(line.to_string());
    }

    if in_remember_me && !had_enable {
        lines.push("Enable=False".to_string());
    }

    if !had_remember_me {
        lines.push("[RememberMe]".to_string());
        lines.push("Enable=False".to_string());
        lines.push(String::new());
    }

    let mut output = lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }

    atomic_write_str(path, &output).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
}

/// Guarantees that if a non-empty `Data=` token exists under `[RememberMe]`,
/// `Enable=True` is explicitly set so Epic Games Launcher automatically authenticates the user.
pub fn ensure_remember_me_enabled_in_ini(path: &Path) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(path)?;
    let mut lines = Vec::new();
    let mut in_remember_me = false;
    let mut has_data = false;
    let mut enable_idx = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_remember_me = trimmed.eq_ignore_ascii_case("[RememberMe]");
            lines.push(line.to_string());
            continue;
        }

        if in_remember_me {
            if trimmed.starts_with("Enable=") {
                enable_idx = Some(lines.len());
                lines.push("Enable=True".to_string());
                continue;
            }
            if trimmed.starts_with("Data=") {
                let val = trimmed["Data=".len()..].trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    has_data = true;
                }
            }
        }

        lines.push(line.to_string());
    }

    if has_data && enable_idx.is_none() {
        for (i, line) in lines.iter().enumerate() {
            if line.trim().eq_ignore_ascii_case("[RememberMe]") {
                lines.insert(i + 1, "Enable=True".to_string());
                break;
            }
        }
    }

    let mut output = lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }

    atomic_write_str(path, &output).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
}

/// Checks if `GameUserSettings.ini` contains a non-empty `Data=` token under `[RememberMe]`.
pub fn ini_has_remember_me(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    let mut in_remember_me = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_remember_me = trimmed.eq_ignore_ascii_case("[RememberMe]");
            continue;
        }
        if in_remember_me && trimmed.starts_with("Data=") {
            let val = trimmed["Data=".len()..].trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return true;
            }
        }
    }
    false
}

/// Safely removes a directory tree, falling back to per-entry removal if atomic remove fails.
pub fn safe_remove_dir_all(dir: &Path) {
    if !dir.exists() {
        return;
    }
    if fs::remove_dir_all(dir).is_err() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    safe_remove_dir_all(&p);
                } else {
                    let _ = fs::remove_file(&p);
                }
            }
        }
        let _ = fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_ini_account_id_roundtrip() {
        let dir = tempdir().unwrap();
        let ini_path = dir.path().join("KeyValueStore.ini");

        assert_eq!(read_account_id_from_ini(&ini_path), None);

        write_account_id_to_ini(&ini_path, "acc_12345").unwrap();
        assert_eq!(read_account_id_from_ini(&ini_path).as_deref(), Some("acc_12345"));

        write_account_id_to_ini(&ini_path, "acc_67890").unwrap();
        assert_eq!(read_account_id_from_ini(&ini_path).as_deref(), Some("acc_67890"));
    }

    #[test]
    fn test_clear_remember_me() {
        let dir = tempdir().unwrap();
        let ini_path = dir.path().join("GameUserSettings.ini");

        let sample = r#";METADATA=(Diff=true)
[Launcher]
LastActiveVersion=20.3.0

[RememberMe]
Enable=True
Data="secret_token_12345"

[WindowSettings]
IsMaximised="false"
"#;
        fs::write(&ini_path, sample).unwrap();

        clear_remember_me_in_ini(&ini_path).unwrap();

        let updated = fs::read_to_string(&ini_path).unwrap();
        assert!(updated.contains("Enable=False"));
        assert!(!updated.contains("secret_token_12345"));
        assert!(updated.contains("[WindowSettings]"));
    }

    #[tokio::test]
    async fn test_session_manager_backup_and_restore() {
        let dir = tempdir().unwrap();
        let kvs = dir.path().join("KeyValueStore.ini");
        let gus = dir.path().join("GameUserSettings.ini");
        let webcache = dir.path().join("webcache");
        let sessions = dir.path().join("sessions");

        fs::create_dir_all(&webcache).unwrap();
        fs::write(webcache.join("Cookies"), b"fake_cookies_account_1").unwrap();
        write_account_id_to_ini(&kvs, "acc_1").unwrap();
        fs::write(&gus, "[RememberMe]\nEnable=True\nData=tok1\n").unwrap();

        let paths = EpicPaths {
            key_value_store: kvs.clone(),
            game_user_settings: gus.clone(),
            webcache_dir: webcache.clone(),
            app_bundle_path: None,
        };

        let manager = EpicSessionManager::with_paths(paths)
            .with_sessions_dir(&sessions)
            .with_skip_process_termination(true);

        // Backup acc_1
        manager.backup_session("acc_1", Some("Account 1")).unwrap();
        assert!(manager.has_session("acc_1"));

        // Switch to acc_2 (no snapshot yet)
        let res2 = manager.switch_account("acc_2", Some("Account 2")).await.unwrap();
        assert_eq!(res2.target_account_id, "acc_2");
        assert!(!res2.restored_saved_session);
        assert_eq!(manager.read_current_launcher_account().as_deref(), Some("acc_2"));
        // CRITICAL: Ensure no premature snapshot was created for acc_2!
        assert!(!manager.has_session("acc_2"));

        // Simulate login for acc_2 by creating cookies
        fs::create_dir_all(&webcache).unwrap();
        fs::write(webcache.join("Cookies"), b"fake_cookies_account_2").unwrap();

        // Switch back to acc_1 (should snapshot acc_2 as outgoing and restore acc_1 snapshot!)
        let res1 = manager.switch_account("acc_1", Some("Account 1")).await.unwrap();
        assert_eq!(res1.target_account_id, "acc_1");
        assert!(res1.restored_saved_session);
        assert_eq!(manager.read_current_launcher_account().as_deref(), Some("acc_1"));
        assert_eq!(fs::read(webcache.join("Cookies")).unwrap(), b"fake_cookies_account_1");

        // Now acc_2 has a real session snapshot saved when switched away from
        assert!(manager.has_session("acc_2"));

        // Switch back to acc_2 (should restore acc_2 snapshot!)
        let res2_again = manager.switch_account("acc_2", Some("Account 2")).await.unwrap();
        assert_eq!(res2_again.target_account_id, "acc_2");
        assert!(res2_again.restored_saved_session);
        assert_eq!(manager.read_current_launcher_account().as_deref(), Some("acc_2"));
        assert_eq!(fs::read(webcache.join("Cookies")).unwrap(), b"fake_cookies_account_2");
    }

    #[test]
    fn test_clear_remember_me_and_offline_data() {
        let dir = tempdir().unwrap();
        let ini_path = dir.path().join("GameUserSettings.ini");

        let sample = r#"[Launcher]
LastActiveVersion=20.3.1

[RememberMe]
Enable=True
Data="secret_token_abc"

[Offline]
Data=offline_token_xyz

[CustomSection]
Key=Value
"#;
        fs::write(&ini_path, sample).unwrap();

        clear_remember_me_in_ini(&ini_path).unwrap();

        let updated = fs::read_to_string(&ini_path).unwrap();
        assert!(updated.contains("Enable=False"));
        assert!(!updated.contains("secret_token_abc"));
        assert!(!updated.contains("offline_token_xyz"));
        assert!(updated.contains("[CustomSection]"));
        assert!(!ini_has_remember_me(&ini_path));
    }

    #[test]
    fn test_has_session_rejects_empty_or_stale_directory() {
        let dir = tempdir().unwrap();
        let paths = EpicPaths {
            key_value_store: dir.path().join("KeyValueStore.ini"),
            game_user_settings: dir.path().join("GameUserSettings.ini"),
            webcache_dir: dir.path().join("webcache"),
            app_bundle_path: None,
        };
        let sessions = dir.path().join("sessions");
        let manager = EpicSessionManager::with_paths(paths)
            .with_sessions_dir(&sessions);

        // Account with no directory at all
        assert!(!manager.has_session("ghost_account"));

        // Account with only empty directory
        let ghost_dir = sessions.join("empty_ghost");
        fs::create_dir_all(&ghost_dir).unwrap();
        assert!(!manager.has_session("empty_ghost"));

        // Account with only KeyValueStore.ini (no cookies, no rememberme data)
        write_account_id_to_ini(&ghost_dir.join("KeyValueStore.ini"), "empty_ghost").unwrap();
        assert!(!manager.has_session("empty_ghost"));

        // Account with GameUserSettings.ini but RememberMe Data cleared
        fs::write(ghost_dir.join("GameUserSettings.ini"), "[RememberMe]\nEnable=False\n").unwrap();
        assert!(!manager.has_session("empty_ghost"));

        // Once valid RememberMe Data or Cookies exist, has_session becomes true
        fs::write(ghost_dir.join("GameUserSettings.ini"), "[RememberMe]\nEnable=True\nData=valid_token\n").unwrap();
        assert!(manager.has_session("empty_ghost"));
    }

    #[tokio::test]
    async fn test_switch_account_preserves_live_login_if_already_authenticated_for_target() {
        let dir = tempdir().unwrap();
        let kvs = dir.path().join("KeyValueStore.ini");
        let gus = dir.path().join("GameUserSettings.ini");
        let webcache = dir.path().join("webcache");
        let sessions = dir.path().join("sessions");

        // Simulate user having just logged into "acc_new" in launcher directly
        write_account_id_to_ini(&kvs, "acc_new").unwrap();
        fs::write(&gus, "[RememberMe]\nEnable=True\nData=secret_live_token\n").unwrap();
        fs::create_dir_all(&webcache).unwrap();
        fs::write(webcache.join("Cookies"), b"fresh_session_cookies").unwrap();

        let paths = EpicPaths {
            key_value_store: kvs.clone(),
            game_user_settings: gus.clone(),
            webcache_dir: webcache.clone(),
            app_bundle_path: None,
        };

        let manager = EpicSessionManager::with_paths(paths)
            .with_sessions_dir(&sessions)
            .with_skip_process_termination(true);

        assert!(!manager.has_session("acc_new"));

        // Switch to acc_new: should recognize on-disk login and snapshot it, NOT wipe it!
        let res = manager.switch_account("acc_new", Some("New User")).await.unwrap();
        assert_eq!(res.target_account_id, "acc_new");
        assert!(res.restored_saved_session);
        assert!(manager.has_session("acc_new"));
        assert_eq!(fs::read(webcache.join("Cookies")).unwrap(), b"fresh_session_cookies");
    }

    #[tokio::test]
    async fn test_switch_account_does_not_backup_unauthenticated_outgoing() {
        let dir = tempdir().unwrap();
        let kvs = dir.path().join("KeyValueStore.ini");
        let gus = dir.path().join("GameUserSettings.ini");
        let webcache = dir.path().join("webcache");
        let sessions = dir.path().join("sessions");

        let paths = EpicPaths {
            key_value_store: kvs.clone(),
            game_user_settings: gus.clone(),
            webcache_dir: webcache.clone(),
            app_bundle_path: None,
        };

        let manager = EpicSessionManager::with_paths(paths)
            .with_sessions_dir(&sessions)
            .with_skip_process_termination(true);

        // Pre-seed a valid backup for acc_1
        let acc1_dir = sessions.join("acc_1");
        fs::create_dir_all(acc1_dir.join("webcache")).unwrap();
        fs::write(acc1_dir.join("webcache").join("Cookies"), b"good_cookies_acc_1").unwrap();
        fs::write(acc1_dir.join("GameUserSettings.ini"), "[RememberMe]\nEnable=True\nData=tok1\n").unwrap();
        write_account_id_to_ini(&acc1_dir.join("KeyValueStore.ini"), "acc_1").unwrap();
        assert!(manager.has_session("acc_1"));

        // Now suppose on-disk launcher was left in an unauthenticated/cleared state for acc_1
        write_account_id_to_ini(&kvs, "acc_1").unwrap();
        fs::write(&gus, "[RememberMe]\nEnable=False\n").unwrap();
        // webcache has no Cookies file

        // Switch to acc_2
        let _ = manager.switch_account("acc_2", Some("Account 2")).await.unwrap();

        // Ensure acc_1's good backup was NOT corrupted by the unauthenticated state
        assert!(manager.has_session("acc_1"));
        assert_eq!(
            fs::read(acc1_dir.join("webcache").join("Cookies")).unwrap(),
            b"good_cookies_acc_1"
        );
    }
}

