use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;
use tracing::{info, warn};

use crate::parsers::vdf::{VdfDocument, VdfError, VdfTable, VdfValue};
use crate::storage::atomic::{atomic_copy, atomic_write_json, AtomicError};
use crate::storage::db::{Database, DbError};
use crate::storage::models::Account;
use crate::sys::process::{terminate_by_names, ProcessError, TerminateOptions, TerminationReport};

/// Process names associated with the Steam client, web helper, and game overlay.
pub const STEAM_PROCESS_NAMES: &[&str] = &[
    "steam",
    "steam.exe",
    "steam_osx",
    "Steam",
    "steamwebhelper",
    "steamwebhelper.exe",
    "Steam Helper",
    "GameOverlayUI",
    "GameOverlayUI.exe",
];

/// Errors occurring during Steam session operations.
#[derive(Error, Debug)]
pub enum SteamError {
    #[error("Steam installation directory not found")]
    SteamNotFound,
    #[error("Steam configuration file not found at {0}")]
    ConfigNotFound(PathBuf),
    #[error("loginusers.vdf does not contain a valid 'users' block")]
    InvalidLoginUsers,
    #[error("Steam account '{0}' not found in loginusers.vdf")]
    AccountNotFound(String),
    #[error("VDF error: {0}")]
    Vdf(#[from] VdfError),
    #[error("Process error: {0}")]
    Process(#[from] ProcessError),
    #[error("Atomic file error: {0}")]
    Atomic(#[from] AtomicError),
    #[error("Database error: {0}")]
    Database(#[from] DbError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Registry update failed: {0}")]
    Registry(String),
}

/// Paths to Steam files and directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamPaths {
    pub steam_dir: PathBuf,
}

impl SteamPaths {
    pub fn new<P: Into<PathBuf>>(steam_dir: P) -> Self {
        Self {
            steam_dir: steam_dir.into(),
        }
    }

    /// Automatically detects the Steam directory based on OS standards or environment variables.
    pub fn detect() -> Result<Self, SteamError> {
        // 1. Environment variable override
        for var in ["SWCH_STEAM_DIR", "STEAM_DIR", "STEAM_PATH"] {
            if let Ok(val) = std::env::var(var) {
                let p = PathBuf::from(val.trim());
                if p.exists() {
                    return Ok(Self::new(p));
                }
            }
        }

        // 2. Platform-specific detection
        #[cfg(target_os = "macos")]
        {
            if let Some(home) = dirs::home_dir() {
                let p = home.join("Library/Application Support/Steam");
                if p.exists() {
                    return Ok(Self::new(p));
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Try Windows Registry query for SteamPath
            if let Ok(reg_path) = query_windows_steam_path() {
                if reg_path.exists() {
                    return Ok(Self::new(reg_path));
                }
            }

            // Common default installation folders
            let prog_x86 = std::env::var("ProgramFiles(x86)")
                .unwrap_or_else(|_| "C:\\Program Files (x86)".into());
            let p1 = PathBuf::from(&prog_x86).join("Steam");
            if p1.exists() {
                return Ok(Self::new(p1));
            }

            let prog = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
            let p2 = PathBuf::from(&prog).join("Steam");
            if p2.exists() {
                return Ok(Self::new(p2));
            }
        }

        #[cfg(target_os = "linux")]
        {
            if let Some(home) = dirs::home_dir() {
                let p1 = home.join(".steam/steam");
                if p1.exists() {
                    return Ok(Self::new(p1));
                }
                let p2 = home.join(".local/share/Steam");
                if p2.exists() {
                    return Ok(Self::new(p2));
                }
            }
        }

        // Fallback for macOS path even if compiled under non-macos (e.g. testing)
        if let Some(home) = dirs::home_dir() {
            let p = home.join("Library/Application Support/Steam");
            if p.exists() {
                return Ok(Self::new(p));
            }
        }

        Err(SteamError::SteamNotFound)
    }

    pub fn config_dir(&self) -> PathBuf {
        self.steam_dir.join("config")
    }

    pub fn loginusers_path(&self) -> PathBuf {
        self.config_dir().join("loginusers.vdf")
    }

    pub fn registry_vdf_path(&self) -> PathBuf {
        let config_reg = self.config_dir().join("registry.vdf");
        let root_reg = self.steam_dir.join("registry.vdf");
        if config_reg.exists() && !root_reg.exists() {
            config_reg
        } else {
            root_reg
        }
    }

    pub fn config_vdf_path(&self) -> PathBuf {
        self.config_dir().join("config.vdf")
    }

    /// Finds all sentry files (`ssfn*` or `.ssfn*`) in the Steam directory.
    pub fn find_sentry_files(&self) -> Vec<PathBuf> {
        let mut ssfn_files = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.steam_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();
                if name.starts_with("ssfn") || name.starts_with(".ssfn") {
                    ssfn_files.push(entry.path());
                }
            }
        }
        ssfn_files.sort();
        ssfn_files
    }
}

#[cfg(target_os = "windows")]
fn query_windows_steam_path() -> Result<PathBuf, ()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let queries = [
        (r#"HKCU\Software\Valve\Steam"#, "SteamPath"),
        (r#"HKCU\Software\Valve\Steam"#, "InstallPath"),
        (r#"HKLM\Software\Valve\Steam"#, "InstallPath"),
        (r#"HKLM\Software\WOW6432Node\Valve\Steam"#, "InstallPath"),
    ];

    for (key, val_name) in queries {
        if let Ok(output) = std::process::Command::new("reg")
            .args(["query", key, "/v", val_name])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    if line.contains(val_name) {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 3 {
                            let p_str = parts[2..].join(" ");
                            let p = PathBuf::from(p_str.replace('/', "\\"));
                            if p.exists() {
                                return Ok(p);
                            }
                        }
                    }
                }
            }
        }
    }
    Err(())
}

/// Metadata stored in swch local storage alongside backed-up session files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBackupManifest {
    pub account_name: String,
    pub steam_id: Option<String>,
    pub persona_name: Option<String>,
    pub backed_up_at: String,
    pub sentry_files: Vec<String>,
}

/// Result of an account switch operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchAccountResult {
    pub account_name: String,
    pub previous_account: Option<String>,
    pub steam_id: Option<String>,
    pub message: String,
}

/// Resolves the root directory for swch session backups.
pub fn resolve_swch_sessions_dir() -> Result<PathBuf, SteamError> {
    if let Ok(val) = std::env::var("SWCH_SESSIONS_DIR") {
        let p = PathBuf::from(val.trim());
        fs::create_dir_all(&p)?;
        return Ok(p);
    }
    if let Ok(val) = std::env::var("SWCH_STORAGE_DIR") {
        let p = PathBuf::from(val.trim()).join("sessions").join("steam");
        fs::create_dir_all(&p)?;
        return Ok(p);
    }

    let base = dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".swch")))
        .ok_or_else(|| {
            SteamError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Unable to resolve swch configuration directory",
            ))
        })?;

    let swch_dir = if base.ends_with(".swch") {
        base
    } else {
        base.join("swch")
    };

    let dir = swch_dir.join("sessions").join("steam");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Sanitizes account name for filesystem safety.
fn sanitize_account_folder(account_name: &str) -> String {
    let sanitized: String = account_name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if sanitized.is_empty() || sanitized.chars().all(|c| c == '_') {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        account_name.hash(&mut hasher);
        format!("account_{:x}", hasher.finish())
    } else {
        sanitized
    }
}

/// Manager service for Steam Hot-Swap session switching and backups.
#[derive(Debug, Clone)]
pub struct SteamSessionManager {
    custom_paths: Option<SteamPaths>,
    custom_sessions_dir: Option<PathBuf>,
    skip_process_termination: bool,
}

impl Default for SteamSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SteamSessionManager {
    /// Creates a new manager utilizing automatic path discovery.
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

    /// Creates a manager configured with explicit Steam paths.
    pub fn with_paths(paths: SteamPaths) -> Self {
        Self {
            custom_paths: Some(paths),
            custom_sessions_dir: None,
            skip_process_termination: false,
        }
    }

    /// Configures an explicit directory for swch session backups.
    pub fn with_sessions_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.custom_sessions_dir = Some(dir.into());
        self
    }

    /// Configures whether to skip killing system Steam processes (useful during testing).
    pub fn with_skip_process_termination(mut self, skip: bool) -> Self {
        self.skip_process_termination = skip;
        self
    }

    /// Resolves Steam paths for this instance.
    pub fn paths(&self) -> Result<SteamPaths, SteamError> {
        match &self.custom_paths {
            Some(p) => Ok(p.clone()),
            None => SteamPaths::detect(),
        }
    }

    /// Resolves the sessions root directory for this instance.
    pub fn sessions_dir(&self) -> Result<PathBuf, SteamError> {
        if let Some(ref dir) = self.custom_sessions_dir {
            fs::create_dir_all(dir)?;
            return Ok(dir.clone());
        }
        resolve_swch_sessions_dir()
    }

    /// Terminates all running Steam processes (steam, steamwebhelper, GameOverlayUI).
    pub async fn terminate_processes(&self) -> Result<TerminationReport, ProcessError> {
        let options = TerminateOptions::default();
        terminate_by_names(STEAM_PROCESS_NAMES, &options).await
    }

    /// Backs up sentry files and configuration for the specified account into swch local storage.
    pub fn backup_session(
        &self,
        paths: &SteamPaths,
        account_name: &str,
        steam_id: Option<&str>,
        persona_name: Option<&str>,
    ) -> Result<PathBuf, SteamError> {
        let sessions_root = self.sessions_dir()?;
        let account_dir = sessions_root.join(sanitize_account_folder(account_name));
        fs::create_dir_all(&account_dir)?;

        // 1. Backup all sentry files (ssfn*)
        let sentry_files = paths.find_sentry_files();
        let mut backed_up_sentry = Vec::new();

        for sentry_path in &sentry_files {
            if let Some(name) = sentry_path.file_name() {
                let dest = account_dir.join(name);
                atomic_copy(sentry_path, &dest)?;
                backed_up_sentry.push(name.to_string_lossy().to_string());
            }
        }

        // 2. Backup loginusers.vdf
        let loginusers = paths.loginusers_path();
        if loginusers.exists() {
            let dest = account_dir.join("loginusers.vdf");
            atomic_copy(&loginusers, &dest)?;
        }

        // 3. Backup registry.vdf if present
        let registry = paths.registry_vdf_path();
        if registry.exists() {
            let dest = account_dir.join("registry.vdf");
            atomic_copy(&registry, &dest)?;
        }

        // 4. Save session manifest
        let manifest = SessionBackupManifest {
            account_name: account_name.to_string(),
            steam_id: steam_id.map(|s| s.to_string()),
            persona_name: persona_name.map(|s| s.to_string()),
            backed_up_at: Utc::now().to_rfc3339(),
            sentry_files: backed_up_sentry,
        };
        let manifest_path = account_dir.join("session_manifest.json");
        atomic_write_json(&manifest_path, &manifest)?;

        info!(
            "Backed up session for '{}' to {}",
            account_name,
            account_dir.display()
        );
        Ok(account_dir)
    }

    /// Restores backed-up sentry files (`ssfn*`) for the target account into Steam directory.
    pub fn restore_sentry_files(
        &self,
        paths: &SteamPaths,
        account_name: &str,
    ) -> Result<usize, SteamError> {
        let sessions_root = self.sessions_dir()?;
        let account_dir = sessions_root.join(sanitize_account_folder(account_name));

        if !account_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        if let Ok(entries) = fs::read_dir(&account_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();
                if name.starts_with("ssfn") || name.starts_with(".ssfn") {
                    let dest = paths.steam_dir.join(&file_name);
                    atomic_copy(entry.path(), &dest)?;
                    count += 1;
                }
            }
        }

        if count > 0 {
            info!(
                "Restored {} sentry (ssfn*) file(s) for '{}'",
                count, account_name
            );
        }

        Ok(count)
    }

    /// Removes sentry files from steam_dir that are already safely backed up in the account's session directory.
    pub fn clean_backed_up_sentry_files(&self, paths: &SteamPaths, account_name: &str) {
        if let Ok(sessions_root) = self.sessions_dir() {
            let account_dir = sessions_root.join(sanitize_account_folder(account_name));
            if let Ok(entries) = fs::read_dir(&account_dir) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let name = file_name.to_string_lossy();
                    if name.starts_with("ssfn") || name.starts_with(".ssfn") {
                        let active_path = paths.steam_dir.join(&file_name);
                        if active_path.exists() {
                            if let Ok(meta) = fs::metadata(&active_path) {
                                let mut perms = meta.permissions();
                                if perms.readonly() {
                                    #[cfg(unix)]
                                    {
                                        use std::os::unix::fs::PermissionsExt;
                                        perms.set_mode(perms.mode() | 0o200);
                                        let _ = fs::set_permissions(&active_path, perms);
                                    }
                                    #[cfg(not(unix))]
                                    {
                                        perms.set_readonly(false);
                                        let _ = fs::set_permissions(&active_path, perms);
                                    }
                                }
                            }
                            if let Err(e) = fs::remove_file(&active_path) {
                                warn!(
                                    "Failed to clean up sentry file '{}': {}",
                                    active_path.display(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Updates registry settings for Steam auto-login.
    ///
    /// macOS / Linux: updates "AutoLoginUser" and "RememberPassword" in `registry.vdf` (checking root and config/).
    /// Windows: updates `HKCU\Software\Valve\Steam` key values via `reg.exe`.
    pub fn update_registry(
        &self,
        paths: &SteamPaths,
        account_name: &str,
    ) -> Result<(), SteamError> {
        // macOS / Linux registry.vdf update: update both root and config/ if present
        let candidate_registry_paths = [
            paths.steam_dir.join("registry.vdf"),
            paths.config_dir().join("registry.vdf"),
        ];

        let mut any_updated = false;
        for reg_path in &candidate_registry_paths {
            if reg_path.exists() {
                if let Ok(mut doc) = VdfDocument::from_file(reg_path) {
                    Self::apply_registry_doc_keys(&mut doc, account_name);
                    doc.save_atomic(reg_path)?;
                    any_updated = true;
                }
            }
        }

        // If neither exists, ensure default registry.vdf path is initialized
        if !any_updated {
            let default_reg = paths.registry_vdf_path();
            let mut doc = VdfDocument::new();
            Self::apply_registry_doc_keys(&mut doc, account_name);
            if let Some(parent) = default_reg.parent() {
                let _ = fs::create_dir_all(parent);
            }
            doc.save_atomic(&default_reg)?;
        }

        // If config.vdf exists, update AutoLoginUser and RememberPassword there as well
        let config_vdf_path = paths.config_vdf_path();
        if config_vdf_path.exists() {
            if let Ok(mut config_doc) = VdfDocument::from_file(&config_vdf_path) {
                let steam_path: &[&str] = if config_doc
                    .find_obj_path_mut(&["InstallConfigStore", "Software", "Valve", "Steam"])
                    .is_some()
                {
                    &["InstallConfigStore", "Software", "Valve", "Steam"]
                } else {
                    &["Software", "Valve", "Steam"]
                };
                config_doc.set_path_value(steam_path, "AutoLoginUser", account_name);
                config_doc.set_path_value(steam_path, "RememberPassword", "1");
                config_doc.save_atomic(&config_vdf_path)?;
            }
        }

        // Windows Registry update
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            let status = std::process::Command::new("reg")
                .args([
                    "add",
                    r#"HKCU\Software\Valve\Steam"#,
                    "/v",
                    "AutoLoginUser",
                    "/t",
                    "REG_SZ",
                    "/d",
                    account_name,
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .status()
                .map_err(|e| SteamError::Registry(format!("Failed to execute reg.exe: {}", e)))?;

            if !status.success() {
                return Err(SteamError::Registry(format!(
                    "reg.exe returned non-zero status: {}",
                    status
                )));
            }

            let _ = std::process::Command::new("reg")
                .args([
                    "add",
                    r#"HKCU\Software\Valve\Steam"#,
                    "/v",
                    "RememberPassword",
                    "/t",
                    "REG_DWORD",
                    "/d",
                    "1",
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .status();
        }

        info!("Updated Steam AutoLoginUser registry to '{}'", account_name);
        Ok(())
    }

    fn apply_registry_doc_keys(doc: &mut VdfDocument, account_name: &str) {
        let steam_path: &[&str] = if doc
            .find_obj_path_mut(&["Registry", "HKCU", "Software", "Valve", "Steam"])
            .is_some()
        {
            &["Registry", "HKCU", "Software", "Valve", "Steam"]
        } else if doc
            .find_obj_path_mut(&["HKCU", "Software", "Valve", "Steam"])
            .is_some()
        {
            &["HKCU", "Software", "Valve", "Steam"]
        } else if doc
            .find_obj_path_mut(&["Software", "Valve", "Steam"])
            .is_some()
        {
            &["Software", "Valve", "Steam"]
        } else {
            &["Registry", "HKCU", "Software", "Valve", "Steam"]
        };
        doc.set_path_value(steam_path, "AutoLoginUser", account_name);
        doc.set_path_value(steam_path, "RememberPassword", "1");
    }
}

/// Detects the currently active Steam user based on loginusers.vdf, registry.vdf, and config.vdf.
pub fn detect_active_steam_user(
    paths: &SteamPaths,
    users_tbl: &VdfTable,
) -> Option<String> {
        // 1. Check MostRecent == "1"
        for (id, val) in users_tbl.iter() {
            if let VdfValue::Obj(u) = val {
                if u.get_str("MostRecent") == Some("1") {
                    return Some(id.clone());
                }
            }
        }

        // 2. Check AutoLogin == "1" (prevalent on modern macOS / Linux Steam)
        for (id, val) in users_tbl.iter() {
            if let VdfValue::Obj(u) = val {
                if u.get_str("AutoLogin") == Some("1") {
                    return Some(id.clone());
                }
            }
        }

        // 3. Check registry.vdf AutoLoginUser
        let reg_path = paths.registry_vdf_path();
        if let Ok(reg_doc) = VdfDocument::from_file(&reg_path) {
            let auto_login = reg_doc
                .get_path_str(
                    &["Registry", "HKCU", "Software", "Valve", "Steam"],
                    "AutoLoginUser",
                )
                .or_else(|| {
                    reg_doc
                        .get_path_str(&["HKCU", "Software", "Valve", "Steam"], "AutoLoginUser")
                })
                .or_else(|| {
                    reg_doc.get_path_str(&["Software", "Valve", "Steam"], "AutoLoginUser")
                });
            if let Some(user) = auto_login {
                let u = user.trim();
                if !u.is_empty() {
                    for (id, val) in users_tbl.iter() {
                        if let VdfValue::Obj(user_tbl) = val {
                            let acc = user_tbl.get_str("AccountName").unwrap_or("");
                            if acc.eq_ignore_ascii_case(u) || id.eq_ignore_ascii_case(u) {
                                return Some(id.clone());
                            }
                        }
                    }
                }
            }
        }

        // 4. Check config.vdf AutoLoginUser
        let cfg_path = paths.config_vdf_path();
        if let Ok(cfg_doc) = VdfDocument::from_file(&cfg_path) {
            let auto_login = cfg_doc
                .get_path_str(
                    &["InstallConfigStore", "Software", "Valve", "Steam"],
                    "AutoLoginUser",
                )
                .or_else(|| {
                    cfg_doc.get_path_str(&["Software", "Valve", "Steam"], "AutoLoginUser")
                });
            if let Some(user) = auto_login {
                let u = user.trim();
                if !u.is_empty() {
                    for (id, val) in users_tbl.iter() {
                        if let VdfValue::Obj(user_tbl) = val {
                            let acc = user_tbl.get_str("AccountName").unwrap_or("");
                            if acc.eq_ignore_ascii_case(u) || id.eq_ignore_ascii_case(u) {
                                return Some(id.clone());
                            }
                        }
                    }
                }
            }
        }

        // 5. Fallback: find user with highest Timestamp
        let mut highest: Option<(u64, String)> = None;
        for (id, val) in users_tbl.iter() {
            if let VdfValue::Obj(u) = val {
                if let Some(ts_str) = u.get_str("Timestamp") {
                    if let Ok(ts) = ts_str.parse::<u64>() {
                        match &highest {
                            Some((max_ts, _)) if ts > *max_ts => {
                                highest = Some((ts, id.clone()));
                            }
                            None => {
                                highest = Some((ts, id.clone()));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        highest.map(|(_, id)| id)
    }

impl SteamSessionManager {
    /// Executes complete Hot-Swap session switch to `target_account`:
    ///
    /// 1. Discovers Steam directory and config paths.
    /// 2. Terminates running Steam client and overlay processes.
    /// 3. Backs up previous active session into swch local storage.
    /// 4. Restores any target sentry files (ssfn*) from swch local storage.
    /// 5. Modifies `loginusers.vdf` AST: sets target "MostRecent" "1" and "RememberPassword" "1",
    ///    resets all others to "MostRecent" "0", and atomically writes changes.
    /// 6. Updates Steam registry (registry.vdf on macOS, HKCU on Windows).
    /// 7. Creates backup of newly active target session in swch local storage.
    /// 8. Updates SQLite database accounts table if database handle is provided.
    pub async fn switch_account(
        &self,
        target_account: &str,
        db: Option<&Database>,
    ) -> Result<SwitchAccountResult, SteamError> {
        let clean_target = target_account.trim();
        if clean_target.is_empty() {
            return Err(SteamError::AccountNotFound(clean_target.to_string()));
        }

        let paths = self.paths()?;
        let loginusers_path = paths.loginusers_path();

        if !loginusers_path.exists() {
            return Err(SteamError::ConfigNotFound(loginusers_path));
        }

        // Serialize changes and validate the target before stopping Steam.
        static SWITCH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _switch_guard = SWITCH_LOCK.lock().await;
        let preflight = VdfDocument::from_file(&loginusers_path)?;
        let users = preflight.get_obj("users").ok_or(SteamError::InvalidLoginUsers)?;
        if !users.iter().any(|(id, value)| id == clean_target || value.as_obj().and_then(|u| u.get_str("AccountName")).is_some_and(|name| name.eq_ignore_ascii_case(clean_target))) {
            return Err(SteamError::AccountNotFound(clean_target.into()));
        }

        let active_user_id = detect_active_steam_user(&paths, users);
        let is_already_active = users.iter().any(|(id, value)| {
            let matches_target = id == clean_target
                || value
                    .as_obj()
                    .and_then(|u| u.get_str("AccountName"))
                    .is_some_and(|name| name.eq_ignore_ascii_case(clean_target));
            matches_target && active_user_id.as_deref() == Some(id.as_str())
        });

        let mut sys = sysinfo::System::new();
        sys.refresh_processes();
        let steam_running = !crate::sys::process::find_processes_by_names(
            &sys,
            crate::sys::process::LauncherType::Steam.process_names(),
        )
        .is_empty();

        if is_already_active && steam_running {
            // Steam is already running with target account active.
            // Do not terminate processes or touch sentry files!
            let mut doc = VdfDocument::from_file(&loginusers_path)?;
            let users_tbl = doc
                .get_obj_mut("users")
                .ok_or(SteamError::InvalidLoginUsers)?;

            let mut canonical_target_name = clean_target.to_string();
            let mut target_steam_id = None;
            let mut target_persona_name = None;
            let now_ts = Utc::now().timestamp().to_string();

            for (steam_id, val) in users_tbl.iter_mut() {
                if let VdfValue::Obj(ref mut u_tbl) = val {
                    let acc = u_tbl.get_str("AccountName").unwrap_or("");
                    let matches = steam_id == clean_target || acc.eq_ignore_ascii_case(clean_target);
                    if matches {
                        if !acc.is_empty() {
                            canonical_target_name = acc.to_string();
                        }
                        target_steam_id = Some(steam_id.clone());
                        target_persona_name = u_tbl.get_str("PersonaName").map(|p| p.to_string());
                        u_tbl.set_str("MostRecent", "1");
                        u_tbl.set_str("RememberPassword", "1");
                        u_tbl.set_str("AllowAutoLogin", "1");
                        u_tbl.set_str("Timestamp", &now_ts);
                    } else {
                        u_tbl.set_str("MostRecent", "0");
                    }
                }
            }

            let _ = doc.save_atomic(&loginusers_path);
            let _ = self.update_registry(&paths, &canonical_target_name);

            if let Some(database) = db {
                let mut target_account_existed = false;
                if let Ok(accounts) = database.get_accounts().await {
                    for mut acc in accounts {
                        if acc.launcher.eq_ignore_ascii_case("steam") {
                            let is_target = acc.username.eq_ignore_ascii_case(&canonical_target_name)
                                || target_steam_id.as_deref() == Some(acc.id.as_str());
                            acc.is_active = is_target;
                            if is_target {
                                target_account_existed = true;
                                acc.last_used = Some(Utc::now().to_rfc3339());
                                if let Some(ref persona) = target_persona_name {
                                    acc.display_name = Some(persona.clone());
                                }
                            }
                            let _ = database.upsert_account(&acc).await;
                        }
                    }
                }
                if !target_account_existed {
                    let acc_id = format!("steam_{}", canonical_target_name.to_lowercase());
                    let account_record = Account {
                        id: acc_id,
                        launcher: "steam".to_string(),
                        username: canonical_target_name.clone(),
                        display_name: target_persona_name.clone(),
                        email: None,
                        is_active: true,
                        last_used: Some(Utc::now().to_rfc3339()),
                    };
                    let _ = database.upsert_account(&account_record).await;
                }
            }

            let message = format!(
                "Steam уже запущен с аккаунтом '{}'",
                canonical_target_name
            );

            return Ok(SwitchAccountResult {
                account_name: canonical_target_name.clone(),
                previous_account: Some(canonical_target_name),
                steam_id: target_steam_id,
                message,
            });
        }

        if !self.skip_process_termination {
            self.terminate_processes().await?;
        }

        // 2. Read and parse loginusers.vdf through AST
        let mut doc = VdfDocument::from_file(&loginusers_path)?;
        let users_tbl = doc
            .get_obj_mut("users")
            .ok_or(SteamError::InvalidLoginUsers)?;

        let mut target_found = false;
        let mut canonical_target_name = clean_target.to_string();
        let mut target_steam_id: Option<String> = None;
        let mut target_persona_name: Option<String> = None;

        let mut previous_account: Option<String> = None;
        let mut previous_steam_id: Option<String> = None;
        let mut previous_persona: Option<String> = None;

        // First pass: locate target and previous active user
        for (steam_id, val) in users_tbl.iter() {
            if let VdfValue::Obj(u_tbl) = val {
                let acc = u_tbl.get_str("AccountName").unwrap_or("");
                let persona = u_tbl.get_str("PersonaName");
                let is_recent = u_tbl.get_str("MostRecent") == Some("1");

                if is_recent && previous_account.is_none() {
                    previous_account = Some(if !acc.is_empty() {
                        acc.to_string()
                    } else {
                        steam_id.clone()
                    });
                    previous_steam_id = Some(steam_id.clone());
                    previous_persona = persona.map(|p| p.to_string());
                }

                let matches_acc = acc.eq_ignore_ascii_case(clean_target);
                let matches_id = steam_id.eq_ignore_ascii_case(clean_target);

                if (matches_acc || matches_id) && !target_found {
                    target_found = true;
                    canonical_target_name = if !acc.is_empty() {
                        acc.to_string()
                    } else {
                        clean_target.to_string()
                    };
                    target_steam_id = Some(steam_id.clone());
                    target_persona_name = persona.map(|p| p.to_string());
                }
            }
        }

        if !target_found {
            return Err(SteamError::AccountNotFound(clean_target.to_string()));
        }

        // If previous account wasn't flagged with MostRecent 1 in loginusers.vdf,
        // try checking AutoLoginUser in registry.vdf or config.vdf
        if previous_account.is_none() {
            let reg_path = paths.registry_vdf_path();
            if let Ok(reg_doc) = VdfDocument::from_file(&reg_path) {
                let auto_login = reg_doc
                    .get_path_str(
                        &["Registry", "HKCU", "Software", "Valve", "Steam"],
                        "AutoLoginUser",
                    )
                    .or_else(|| {
                        reg_doc
                            .get_path_str(&["HKCU", "Software", "Valve", "Steam"], "AutoLoginUser")
                    })
                    .or_else(|| {
                        reg_doc.get_path_str(&["Software", "Valve", "Steam"], "AutoLoginUser")
                    });
                if let Some(user) = auto_login {
                    let u = user.trim();
                    if !u.is_empty() {
                        previous_account = Some(u.to_string());
                    }
                }
            }
        }

        if previous_account.is_none() {
            let cfg_path = paths.config_vdf_path();
            if let Ok(cfg_doc) = VdfDocument::from_file(&cfg_path) {
                let auto_login = cfg_doc
                    .get_path_str(
                        &["InstallConfigStore", "Software", "Valve", "Steam"],
                        "AutoLoginUser",
                    )
                    .or_else(|| {
                        cfg_doc.get_path_str(&["Software", "Valve", "Steam"], "AutoLoginUser")
                    });
                if let Some(user) = auto_login {
                    let u = user.trim();
                    if !u.is_empty() {
                        previous_account = Some(u.to_string());
                    }
                }
            }
        }

        // Correlate previous_account with users_tbl to extract previous_steam_id and persona
        if let Some(ref prev_acc) = previous_account {
            if previous_steam_id.is_none() {
                for (steam_id, val) in users_tbl.iter() {
                    if let VdfValue::Obj(u_tbl) = val {
                        let acc = u_tbl.get_str("AccountName").unwrap_or("");
                        if acc.eq_ignore_ascii_case(prev_acc)
                            || steam_id.eq_ignore_ascii_case(prev_acc)
                        {
                            previous_steam_id = Some(steam_id.clone());
                            if previous_persona.is_none() {
                                previous_persona =
                                    u_tbl.get_str("PersonaName").map(|p| p.to_string());
                            }
                            break;
                        }
                    }
                }
            }
        }

        // 3. Backup previous session if one was active and differs from target
        if let Some(ref prev_acc) = previous_account {
            if !prev_acc.eq_ignore_ascii_case(&canonical_target_name) {
                if let Err(e) = self.backup_session(
                    &paths,
                    prev_acc,
                    previous_steam_id.as_deref(),
                    previous_persona.as_deref(),
                ) {
                    warn!(
                        "Notice: failed to backup previous session for '{}': {}",
                        prev_acc, e
                    );
                }
                // Clean up previous account's backed-up sentry files from steam_dir to prevent token leaking
                self.clean_backed_up_sentry_files(&paths, prev_acc);
            }
        }

        // 4. Restore sentry files for target account if backed up previously
        let _ = self.restore_sentry_files(&paths, &canonical_target_name);

        // 5. Update loginusers.vdf AST:
        // Set target "MostRecent" "1", "RememberPassword" "1", "AllowAutoLogin" "1", and update timestamp.
        // Set all other users "MostRecent" "0".
        let now_ts = Utc::now().timestamp().to_string();
        for (steam_id, val) in users_tbl.iter_mut() {
            if let VdfValue::Obj(ref mut u_tbl) = val {
                let acc = u_tbl.get_str("AccountName").unwrap_or("");
                let is_target = acc.eq_ignore_ascii_case(&canonical_target_name)
                    || target_steam_id.as_deref() == Some(steam_id.as_str());
                if is_target {
                    u_tbl.set_str("MostRecent", "1");
                    u_tbl.set_str("RememberPassword", "1");
                    u_tbl.set_str("Timestamp", &now_ts);
                    u_tbl.set_str("WantsOfflineMode", "0");
                    u_tbl.set_str("SkipOfflineModeWarning", "0");
                    u_tbl.set_str("AllowAutoLogin", "1");
                } else {
                    u_tbl.set_str("MostRecent", "0");
                }
            }
        }

        // Atomically save modified loginusers.vdf
        doc.save_atomic(&loginusers_path)?;
        info!(
            "loginusers.vdf updated atomically with active account '{}'",
            canonical_target_name
        );

        // 6. Update Steam Registry (AutoLoginUser & RememberPassword)
        self.update_registry(&paths, &canonical_target_name)?;

        // 7. Backup target account session with its new active state
        if let Err(e) = self.backup_session(
            &paths,
            &canonical_target_name,
            target_steam_id.as_deref(),
            target_persona_name.as_deref(),
        ) {
            warn!(
                "Notice: failed to backup target session for '{}': {}",
                canonical_target_name, e
            );
        }

        // 8. Synchronize with SQLite database if provided
        if let Some(database) = db {
            let mut target_account_existed = false;
            if let Ok(accounts) = database.get_accounts().await {
                for mut acc in accounts {
                    if acc.launcher.eq_ignore_ascii_case("steam") {
                        let is_target = acc.username.eq_ignore_ascii_case(&canonical_target_name);
                        acc.is_active = is_target;
                        if is_target {
                            target_account_existed = true;
                            acc.last_used = Some(Utc::now().to_rfc3339());
                            if let Some(ref persona) = target_persona_name {
                                acc.display_name = Some(persona.clone());
                            }
                        }
                        let _ = database.upsert_account(&acc).await;
                    }
                }
            }
            // Ensure an account entry exists in database only if not already present
            if !target_account_existed {
                let acc_id = format!("steam_{}", canonical_target_name.to_lowercase());
                let account_record = Account {
                    id: acc_id,
                    launcher: "steam".to_string(),
                    username: canonical_target_name.clone(),
                    display_name: target_persona_name.clone(),
                    email: None,
                    is_active: true,
                    last_used: Some(Utc::now().to_rfc3339()),
                };
                let _ = database.upsert_account(&account_record).await;
            }
        }

        let message = format!(
            "Steam account switched successfully to '{}'",
            canonical_target_name
        );

        Ok(SwitchAccountResult {
            account_name: canonical_target_name,
            previous_account,
            steam_id: target_steam_id,
            message,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_mock_steam_env() -> (tempfile::TempDir, SteamPaths) {
        let dir = tempdir().unwrap();
        let steam_dir = dir.path().to_path_buf();
        let config_dir = steam_dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();

        // Create mock loginusers.vdf
        let loginusers_content = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"user_alpha"
		"PersonaName"		"Alpha"
		"RememberPassword"		"1"
		"MostRecent"		"1"
		"Timestamp"		"1700000000"
		"WantsOfflineMode"		"0"
		"SkipOfflineModeWarning"		"0"
	}
	"76561198000000002"
	{
		"AccountName"		"user_beta"
		"PersonaName"		"Beta"
		"RememberPassword"		"1"
		"MostRecent"		"0"
		"Timestamp"		"1690000000"
	}
}
"#;
        fs::write(config_dir.join("loginusers.vdf"), loginusers_content).unwrap();

        // Create mock sentry file
        fs::write(steam_dir.join("ssfn1234567890"), b"mock_sentry_token").unwrap();

        let paths = SteamPaths::new(steam_dir);
        (dir, paths)
    }

    #[tokio::test]
    async fn test_switch_account_success() {
        let (temp_dir, paths) = setup_mock_steam_env();
        let sessions_dir = temp_dir.path().join("swch_sessions");

        let manager = SteamSessionManager::with_paths(paths.clone())
            .with_sessions_dir(&sessions_dir)
            .with_skip_process_termination(true);

        let result = manager.switch_account("user_beta", None).await.unwrap();

        assert_eq!(result.account_name, "user_beta");
        assert_eq!(result.previous_account, Some("user_alpha".to_string()));
        assert_eq!(result.steam_id, Some("76561198000000002".to_string()));

        // Verify loginusers.vdf
        let doc = VdfDocument::from_file(paths.loginusers_path()).unwrap();
        assert_eq!(
            doc.get_path_str(&["users", "76561198000000001"], "MostRecent"),
            Some("0")
        );
        assert_eq!(
            doc.get_path_str(&["users", "76561198000000002"], "MostRecent"),
            Some("1")
        );
        assert_eq!(
            doc.get_path_str(&["users", "76561198000000002"], "RememberPassword"),
            Some("1")
        );

        // Verify registry.vdf
        let reg_doc = VdfDocument::from_file(paths.registry_vdf_path()).unwrap();
        assert_eq!(
            reg_doc.get_path_str(
                &["Registry", "HKCU", "Software", "Valve", "Steam"],
                "AutoLoginUser"
            ),
            Some("user_beta")
        );
        assert_eq!(
            reg_doc.get_path_str(
                &["Registry", "HKCU", "Software", "Valve", "Steam"],
                "RememberPassword"
            ),
            Some("1")
        );

        // Verify backup was created for user_alpha and user_beta
        assert!(sessions_dir.join("user_alpha").exists());
        assert!(sessions_dir
            .join("user_alpha")
            .join("ssfn1234567890")
            .exists());
        assert!(sessions_dir.join("user_beta").exists());
        // Verify sentry token isolation: user_beta must NOT have received user_alpha's ssfn file
        assert!(
            !sessions_dir
                .join("user_beta")
                .join("ssfn1234567890")
                .exists(),
            "user_beta session must not leak user_alpha sentry token"
        );
    }

    #[tokio::test]
    async fn test_switch_account_by_steam_id() {
        let (temp_dir, paths) = setup_mock_steam_env();
        let sessions_dir = temp_dir.path().join("swch_sessions");

        let manager = SteamSessionManager::with_paths(paths.clone())
            .with_sessions_dir(&sessions_dir)
            .with_skip_process_termination(true);

        // Switch using SteamID64 instead of account name
        let result = manager
            .switch_account("76561198000000002", None)
            .await
            .unwrap();

        assert_eq!(result.account_name, "user_beta");
        assert_eq!(result.steam_id, Some("76561198000000002".to_string()));

        let doc = VdfDocument::from_file(paths.loginusers_path()).unwrap();
        assert_eq!(
            doc.get_path_str(&["users", "76561198000000002"], "MostRecent"),
            Some("1")
        );
    }

    #[tokio::test]
    async fn test_dual_registry_vdf_updates_both_root_and_config() {
        let (temp_dir, paths) = setup_mock_steam_env();
        let sessions_dir = temp_dir.path().join("swch_sessions");

        // Create both steam_dir/registry.vdf and steam_dir/config/registry.vdf
        let root_reg = paths.steam_dir.join("registry.vdf");
        let config_reg = paths.config_dir().join("registry.vdf");

        let initial_vdf = r#""Registry" { "HKCU" { "Software" { "Valve" { "Steam" { "AutoLoginUser" "old_user" } } } } }"#;
        fs::write(&root_reg, initial_vdf).unwrap();
        fs::write(&config_reg, initial_vdf).unwrap();

        let manager = SteamSessionManager::with_paths(paths.clone())
            .with_sessions_dir(&sessions_dir)
            .with_skip_process_termination(true);

        manager.switch_account("user_beta", None).await.unwrap();

        // Both registry.vdf files must be updated
        let doc_root = VdfDocument::from_file(&root_reg).unwrap();
        let doc_config = VdfDocument::from_file(&config_reg).unwrap();

        assert_eq!(
            doc_root.get_path_str(
                &["Registry", "HKCU", "Software", "Valve", "Steam"],
                "AutoLoginUser"
            ),
            Some("user_beta")
        );
        assert_eq!(
            doc_config.get_path_str(
                &["Registry", "HKCU", "Software", "Valve", "Steam"],
                "AutoLoginUser"
            ),
            Some("user_beta")
        );
    }

    #[tokio::test]
    async fn test_switch_account_not_found() {
        let (_temp_dir, paths) = setup_mock_steam_env();
        let manager = SteamSessionManager::with_paths(paths).with_skip_process_termination(true);
        let err = manager
            .switch_account("non_existent_user", None)
            .await
            .unwrap_err();

        match err {
            SteamError::AccountNotFound(name) => assert_eq!(name, "non_existent_user"),
            other => panic!("Unexpected error: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_sentry_file_restoration() {
        let (temp_dir, paths) = setup_mock_steam_env();
        let sessions_dir = temp_dir.path().join("swch_sessions");

        // Stage sentry file in backup directory for user_beta
        let beta_backup = sessions_dir.join("user_beta");
        fs::create_dir_all(&beta_backup).unwrap();
        fs::write(beta_backup.join("ssfn9999999999"), b"beta_sentry_data").unwrap();

        let manager = SteamSessionManager::with_paths(paths.clone())
            .with_sessions_dir(&sessions_dir)
            .with_skip_process_termination(true);

        let _ = manager.switch_account("user_beta", None).await.unwrap();

        // Check that ssfn9999999999 was restored into steam directory
        assert!(paths.steam_dir.join("ssfn9999999999").exists());
        let restored = fs::read(paths.steam_dir.join("ssfn9999999999")).unwrap();
        assert_eq!(restored, b"beta_sentry_data");
    }

    #[test]
    fn test_clean_backed_up_sentry_files_with_readonly_file() {
        let (temp_dir, paths) = setup_mock_steam_env();
        let sessions_dir = temp_dir.path().join("swch_sessions");

        // Backup exists for user_alpha containing ssfn1234567890
        let alpha_backup = sessions_dir.join("user_alpha");
        fs::create_dir_all(&alpha_backup).unwrap();
        fs::write(alpha_backup.join("ssfn1234567890"), b"mock_sentry_token").unwrap();

        let active_ssfn = paths.steam_dir.join("ssfn1234567890");
        assert!(active_ssfn.exists());

        // Mark active_ssfn as read-only
        let mut perms = fs::metadata(&active_ssfn).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&active_ssfn, perms).unwrap();
        assert!(fs::metadata(&active_ssfn).unwrap().permissions().readonly());

        let manager =
            SteamSessionManager::with_paths(paths.clone()).with_sessions_dir(&sessions_dir);

        // Should successfully remove the read-only active sentry file
        manager.clean_backed_up_sentry_files(&paths, "user_alpha");
        assert!(
            !active_ssfn.exists(),
            "Readonly sentry file must be cleanly deleted"
        );
    }

    #[tokio::test]
    async fn test_switch_account_detects_previous_user_from_config_vdf() {
        let dir = tempdir().unwrap();
        let steam_dir = dir.path().to_path_buf();
        let config_dir = steam_dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let sessions_dir = dir.path().join("swch_sessions");

        // loginusers.vdf has all MostRecent = "0" (e.g. after clean exit or crash)
        let loginusers_content = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"user_alpha"
		"PersonaName"		"Alpha"
		"RememberPassword"		"1"
		"MostRecent"		"0"
	}
	"76561198000000002"
	{
		"AccountName"		"user_beta"
		"PersonaName"		"Beta"
		"RememberPassword"		"1"
		"MostRecent"		"0"
	}
}
"#;
        fs::write(config_dir.join("loginusers.vdf"), loginusers_content).unwrap();

        // But config.vdf has AutoLoginUser = "user_alpha"
        let config_vdf_content = r#""InstallConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"AutoLoginUser"		"user_alpha"
			}
		}
	}
}
"#;
        fs::write(config_dir.join("config.vdf"), config_vdf_content).unwrap();

        let paths = SteamPaths::new(steam_dir);
        let manager = SteamSessionManager::with_paths(paths)
            .with_sessions_dir(&sessions_dir)
            .with_skip_process_termination(true);

        let result = manager.switch_account("user_beta", None).await.unwrap();
        assert_eq!(result.account_name, "user_beta");
        assert_eq!(result.previous_account, Some("user_alpha".to_string()));
    }

    #[test]
    fn test_sanitize_account_folder_supports_dots_and_unicode() {
        assert_eq!(sanitize_account_folder("john.doe"), "john.doe");
        assert_eq!(sanitize_account_folder("gamer_2026-pro"), "gamer_2026-pro");
        assert_eq!(sanitize_account_folder("Игрок123"), "игрок123");
        let fallback = sanitize_account_folder("???");
        assert!(fallback.starts_with("account_"));
    }
}
