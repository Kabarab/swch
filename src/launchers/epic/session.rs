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
    pub saved_data_dir: Option<PathBuf>,
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

            let saved_data_dir = Some(home.join("Library/Application Support/Epic/EpicGamesLauncher/Saved/Data"));

            Ok(Self {
                key_value_store,
                game_user_settings,
                webcache_dir,
                app_bundle_path,
                saved_data_dir,
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
                let editor = local_app_data.join("EpicGamesLauncher\\Saved\\Config\\WindowsEditor\\GameUserSettings.ini");
                if editor.exists() {
                    editor
                } else {
                    let win = local_app_data.join("EpicGamesLauncher\\Saved\\Config\\Windows\\GameUserSettings.ini");
                    if win.exists() {
                        win
                    } else {
                        editor
                    }
                }
            });

            let webcache_dir = env_webcache.unwrap_or_else(|| {
                let saved = local_app_data.join("EpicGamesLauncher\\Saved");
                let c1 = saved.join("webcache_4430");
                if c1.exists() {
                    c1
                } else if let Ok(entries) = fs::read_dir(&saved) {
                    let mut found = None;
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with("webcache") && entry.path().is_dir() {
                            found = Some(entry.path());
                            break;
                        }
                    }
                    found.unwrap_or_else(|| saved.join("webcache"))
                } else {
                    saved.join("webcache")
                }
            });

            let app_bundle_path = detect_windows_epic_exe();
            let saved_data_dir = Some(local_app_data.join("EpicGamesLauncher\\Saved\\Data"));

            Ok(Self {
                key_value_store,
                game_user_settings,
                webcache_dir,
                app_bundle_path,
                saved_data_dir,
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
                saved_data_dir: None,
            })
        }
    }
}

/// Detects the Epic Games Launcher executable on Windows via Registry, environment, and known paths.
#[cfg(target_os = "windows")]
pub fn detect_windows_epic_exe() -> Option<PathBuf> {
    // 1. Environment variable overrides
    if let Ok(p) = std::env::var("SWCH_EPIC_CLIENT").or_else(|_| std::env::var("SWCH_EPIC_APP_BUNDLE_PATH")) {
        let pb = PathBuf::from(p.trim().trim_matches('"'));
        if pb.is_file() {
            return Some(pb);
        }
    }

    // 2. Query Windows Registry
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        if let Ok(output) = std::process::Command::new("reg")
            .args(["query", r#"HKCR\com.epicgames.launcher\shell\open\command"#])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    let trimmed = line.trim();
                    if let Some(pos) = trimmed.to_lowercase().find(".exe") {
                        let candidate_part = &trimmed[..pos + 4];
                        let candidate_str = candidate_part.trim_matches('"').trim();
                        let raw = if let Some(idx) = candidate_str.find(r#":\"#) {
                            &candidate_str[idx - 1..]
                        } else {
                            candidate_str
                        };
                        let p = PathBuf::from(raw);
                        if p.is_file() {
                            return Some(p);
                        }
                    }
                }
            }
        }

        let reg_queries = [
            (r#"HKLM\SOFTWARE\EpicGames\EpicGamesLauncher"#, "AppDataPath"),
            (r#"HKLM\SOFTWARE\WOW6432Node\EpicGames\EpicGamesLauncher"#, "AppDataPath"),
            (r#"HKCU\Software\Epic Games\EOS"#, "InstallPath"),
        ];
        for (key, val_name) in reg_queries {
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
                                let base = PathBuf::from(p_str);
                                let exe = base.join(r#"Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"#);
                                if exe.is_file() {
                                    return Some(exe);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Known drive installation paths
    let sub = r#"Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"#;
    let prog86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r#"C:\Program Files (x86)"#.into());
    let prog64 = std::env::var("ProgramFiles").unwrap_or_else(|_| r#"C:\Program Files"#.into());

    let candidates = [
        PathBuf::from(r#"D:\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"#),
        PathBuf::from(&prog86).join(sub),
        PathBuf::from(&prog64).join(sub),
        PathBuf::from(r#"E:\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"#),
        PathBuf::from(r#"C:\Games\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"#),
        PathBuf::from(r#"D:\Games\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"#),
        PathBuf::from(r#"E:\Games\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"#),
    ];

    for c in &candidates {
        if c.is_file() {
            return Some(c.clone());
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
pub fn detect_windows_epic_exe() -> Option<PathBuf> {
    None
}

/// Reads the AccountId from the Windows Registry key HKCU\Software\Epic Games\Unreal Engine\Identifiers.
#[cfg(target_os = "windows")]
pub fn read_windows_epic_registry_account_id() -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let output = std::process::Command::new("reg")
        .args(["query", r#"HKCU\Software\Epic Games\Unreal Engine\Identifiers"#, "/v", "AccountId"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.contains("AccountId") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let id = parts[2..].join(" ").trim().to_string();
                if !id.is_empty() {
                    return Some(id);
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn read_windows_epic_registry_account_id() -> Option<String> {
    None
}

/// Writes or updates AccountId in the Windows Registry key HKCU\Software\Epic Games\Unreal Engine\Identifiers.
#[cfg(target_os = "windows")]
pub fn write_windows_epic_registry_account_id(account_id: &str) -> Result<(), std::io::Error> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let status = std::process::Command::new("reg")
        .args([
            "add",
            r#"HKCU\Software\Epic Games\Unreal Engine\Identifiers"#,
            "/v",
            "AccountId",
            "/t",
            "REG_SZ",
            "/d",
            account_id.trim(),
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to write AccountId to Windows Registry",
        ))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn write_windows_epic_registry_account_id(_account_id: &str) -> Result<(), std::io::Error> {
    Ok(())
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
    #[serde(default)]
    pub no_saved_session: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Session manager responsible for profile swapping, process lifecycle, and backups.
#[derive(Debug, Clone)]
pub struct EpicSessionManager {
    custom_paths: Option<EpicPaths>,
    custom_sessions_dir: Option<PathBuf>,
    skip_process_termination: bool,
    simulate_running: Option<bool>,
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
            simulate_running: None,
        }
    }

    /// Creates a manager configured with explicit paths.
    pub fn with_paths(paths: EpicPaths) -> Self {
        Self {
            custom_paths: Some(paths),
            custom_sessions_dir: None,
            skip_process_termination: false,
            simulate_running: None,
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

    /// Simulates whether the Epic Games Launcher is running (useful in automated unit tests).
    pub fn with_simulate_running(mut self, running: bool) -> Self {
        self.simulate_running = Some(running);
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
        if let Some(simulated) = self.simulate_running {
            return simulated;
        }
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
        let mut options = TerminateOptions::default();
        options.graceful_timeout = std::time::Duration::from_millis(3000);
        options.force_timeout = std::time::Duration::from_millis(3000);
        let report = terminate_launcher(LauncherType::EpicGames, &options).await;
        match report {
            Ok(r) => Ok(r),
            Err(e) => {
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    for img in &["EpicGamesLauncher.exe", "EpicWebHelper.exe", "EpicOnlineServicesUserHelper.exe"] {
                        let _ = std::process::Command::new("taskkill.exe")
                            .args(["/F", "/IM", img, "/T"])
                            .creation_flags(CREATE_NO_WINDOW)
                            .status();
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    let mut sys = System::new();
                    sys.refresh_processes();
                    let remaining = find_launcher_processes(&sys, LauncherType::EpicGames);
                    if remaining.is_empty() {
                        return Ok(TerminationReport::default());
                    }
                }
                Err(e.into())
            }
        }
    }

    /// Reads the account ID currently active according to the launcher's registry (Windows) or `KeyValueStore.ini`.
    pub fn read_current_launcher_account(&self) -> Option<String> {
        #[cfg(target_os = "windows")]
        if self.custom_paths.is_none() {
            if let Some(reg_id) = read_windows_epic_registry_account_id() {
                let clean = reg_id.trim();
                if !clean.is_empty() {
                    return Some(clean.to_string());
                }
            }
        }
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
        let net_cookies = paths.webcache_dir.join("Network").join("Cookies");
        let has_cookies = (cookies_path.is_file()
            && fs::metadata(&cookies_path).map(|m| m.len() > 0).unwrap_or(false))
            || (net_cookies.is_file()
                && fs::metadata(&net_cookies).map(|m| m.len() > 0).unwrap_or(false));
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
        // 1. webcache with non-empty Cookies file (or Network/Cookies), OR
        // 2. GameUserSettings.ini with [RememberMe] Data= token, OR
        // 3. session_manifest.json with has_webcache flag
        let cookies_path = account_dir.join("webcache").join("Cookies");
        let net_cookies = account_dir.join("webcache").join("Network").join("Cookies");
        let has_cookies = (cookies_path.is_file()
            && fs::metadata(&cookies_path).map(|m| m.len() > 0).unwrap_or(false))
            || (net_cookies.is_file()
                && fs::metadata(&net_cookies).map(|m| m.len() > 0).unwrap_or(false));
        let has_remember_me = ini_has_remember_me(&account_dir.join("GameUserSettings.ini"));
        let has_manifest_cache = account_dir.join("session_manifest.json").is_file()
            && fs::read_to_string(account_dir.join("session_manifest.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<EpicSessionManifest>(&s).ok())
                .map(|m| m.has_webcache)
                .unwrap_or(false);
        let has_dat = account_dir.join(format!("{}.dat", clean_id)).is_file()
            || account_dir.join(format!("OC_{}.dat", clean_id)).is_file()
            || fs::read_dir(&account_dir).ok().map(|entries| {
                entries.flatten().any(|e| {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    n.ends_with(".dat") && n.contains(&clean_id.to_lowercase())
                })
            }).unwrap_or(false);

        has_cookies || has_remember_me || has_manifest_cache || has_dat
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
            let mut copied = atomic_copy(&paths.game_user_settings, &dest).is_ok();
            if !copied {
                for _ in 0..5 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    if fs::copy(&paths.game_user_settings, &dest).is_ok() {
                        copied = true;
                        break;
                    }
                }
            }
            if copied || dest.exists() {
                let _ = ensure_remember_me_enabled_in_ini(&dest);
            }
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

        // 4. Backup Saved/Data .dat files (tokens and offline caches on Windows)
        if let Some(ref data_dir) = paths.saved_data_dir {
            if data_dir.is_dir() {
                if let Ok(entries) = fs::read_dir(data_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let fname = entry.file_name();
                            let fname_str = fname.to_string_lossy();
                            let lower = fname_str.to_lowercase();
                            if lower.contains(&clean_id.to_lowercase()) && lower.ends_with(".dat") {
                                let dest = account_dir.join(&fname);
                                let _ = atomic_copy(&path, &dest);
                            }
                        }
                    }
                }
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

            // Update Windows Registry AccountId
            #[cfg(target_os = "windows")]
            if self.custom_paths.is_none() {
                let _ = write_windows_epic_registry_account_id(clean_id);
            }

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
            } else if paths.game_user_settings.exists() {
                let has_cookies = account_dir.join("webcache").join("Cookies").exists()
                    || account_dir.join("webcache").join("Network").join("Cookies").exists();
                if !has_cookies {
                    let _ = clear_remember_me_in_ini(&paths.game_user_settings);
                }
            }

            // Sync GameUserSettings.ini across both WindowsEditor and Windows configs if present
            #[cfg(target_os = "windows")]
            if let Some(config_root) = paths.game_user_settings.parent().and_then(Path::parent) {
                let alt_dirs = [
                    config_root.join("WindowsEditor"),
                    config_root.join("Windows"),
                ];
                for dir in &alt_dirs {
                    if dir.is_dir() {
                        let target_ini = dir.join("GameUserSettings.ini");
                        if src_gus.exists() {
                            atomic_copy(&src_gus, &target_ini)?;
                            ensure_remember_me_enabled_in_ini(&target_ini)?;
                        }
                    }
                }
            }

            // Restore webcache directory
            let src_webcache = account_dir.join("webcache");
            if src_webcache.exists() && src_webcache.is_dir() {
                let mut cache_targets = vec![paths.webcache_dir.clone()];
                if let Some(parent) = paths.webcache_dir.parent() {
                    if let Ok(entries) = fs::read_dir(parent) {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if name.starts_with("webcache") && entry.path().is_dir() {
                                if !cache_targets.contains(&entry.path()) {
                                    cache_targets.push(entry.path());
                                }
                                safe_remove_dir_all(&entry.path());
                                if entry.path().exists() {
                                    return Err(EpicError::Io("Epic webcache is still locked".into()));
                                }
                            }
                        }
                    }
                    let _ = fs::create_dir_all(parent);
                } else {
                    safe_remove_dir_all(&paths.webcache_dir);
                }

                for target in cache_targets {
                    copy_dir_all(&src_webcache, &target)?;
                }
            }

            // Restore Saved/Data .dat files
            if let Some(ref data_dir) = paths.saved_data_dir {
                fs::create_dir_all(data_dir)?;
                if let Ok(entries) = fs::read_dir(data_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let fname = entry.file_name().to_string_lossy().to_string();
                            if fname.to_lowercase().ends_with(".dat") {
                                fs::remove_file(&path)?;
                            }
                        }
                    }
                }
                if let Ok(entries) = fs::read_dir(&account_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let fname = entry.file_name().to_string_lossy().to_string();
                            if fname.to_lowercase().ends_with(".dat") {
                                let dest = data_dir.join(&fname);
                                atomic_copy(&path, &dest)?;
                            }
                        }
                    }
                }
            }

            Ok(true)
        } else {
            info!(
                "No saved session snapshot found for '{}'; preparing clean login state in launcher",
                clean_id
            );

            // Update Windows Registry AccountId
            #[cfg(target_os = "windows")]
            if self.custom_paths.is_none() {
                let _ = write_windows_epic_registry_account_id(clean_id);
            }

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

            // Clear Saved/Data tokens for clean login
            if let Some(ref data_dir) = paths.saved_data_dir {
                if data_dir.is_dir() {
                    if let Ok(entries) = fs::read_dir(data_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_file() {
                                let fname = entry.file_name().to_string_lossy().to_string();
                                if fname.to_lowercase().ends_with(".dat") {
                                    let _ = fs::remove_file(&path);
                                }
                            }
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
                    std::process::Command::new(exe).spawn()?;
                    return Ok(());
                }
            }

            open::that("com.epicgames.launcher://")?;
            Ok(())
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = paths;
            let _ = open::that("com.epicgames.launcher://");
            Ok(())
        }
    }

    /// Install a dedicated native refresh token before starting the launcher.
    /// The portal ignores game AUTH_* and ExchangeCode command-line arguments.
    pub async fn login_with_refresh_token(&self, account_id: &str, display_name: &str, token: &str) -> Result<(), EpicError> {
        let paths = self.paths()?;
        let data = encode_launcher_remember_me(display_name, token)?;
        let mut command = launcher_login_command(&paths)?;
        if !self.skip_process_termination {
            self.terminate_processes().await?;
        }
        let started_at = Utc::now();
        let previous = match fs::read(&paths.game_user_settings) {
            Ok(contents) => Some(contents),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if let Some(ref contents) = previous {
            crate::storage::atomic::atomic_write(self.sessions_dir()?.join("launcher-before-login.ini"), contents)
                .map_err(|e| EpicError::Io(e.to_string()))?;
        }
        let existing = previous.as_deref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let updated = replace_launcher_login_sections(&existing, &data);
        atomic_write_str(&paths.game_user_settings, &updated).map_err(|e| EpicError::Io(e.to_string()))?;
        if !self.skip_process_termination {
            if let Err(e) = command.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
                if let Some(contents) = previous {
                    crate::storage::atomic::atomic_write(&paths.game_user_settings, contents)
                        .map_err(|e| EpicError::Io(e.to_string()))?;
                }
                return Err(e.into());
            }
        }
        if !self.skip_process_termination {
            let saved = paths.game_user_settings.ancestors()
                .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("Saved")))
                .ok_or_else(|| EpicError::Io("Epic launcher log directory was not found".into()))?;
            let log = saved.join("Logs/EpicGamesLauncher.log");
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
            loop {
                if fs::read_to_string(&log).ok().is_some_and(|text| native_login_confirmed(&text, account_id, started_at)) {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(EpicError::Auth("Epic не подтвердил вход в выбранный аккаунт. Повтори переключение.".into()));
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        info!("Confirmed native Epic login for account {}", account_id);
        Ok(())
    }

    /// Binds native claiming to the installed executable, process lifetime,
    /// and latest confirmed login. SQLite/registry account hints are not proof.
    pub fn native_claim_binding(
        &self,
        account_id: &str,
        game_paths: &[PathBuf],
    ) -> Result<Option<(u32, u64)>, EpicError> {
        #[cfg(target_os = "windows")]
        {
            if !native_claim_account_id(account_id) {
                return Err(native_claim_error("Некорректный идентификатор аккаунта Epic."));
            }
            let paths = self.paths()?;
            let Some(root) = native_claim_process_snapshot(&paths, game_paths)? else {
                return Ok(None);
            };
            let log = fs::read_to_string(native_claim_log_path(&paths)?)
                .map_err(|_| native_claim_error("Не удалось проверить текущий вход лаунчера Epic."))?;
            let owner = native_claim_latest_owner(&log, root.started)?;
            Ok((owner == account_id).then_some((root.pid, root.started)))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (account_id, game_paths);
            Err(EpicError::Io("Получение через лаунчер поддерживается только в Windows.".into()))
        }
    }

    /// A native-claim-only login: closes a verified idle launcher with WM_CLOSE,
    /// never force kills it or a game. Caller must hold the global launcher lock.
    pub async fn login_for_claim(
        &self,
        account_id: &str,
        display_name: &str,
        token: &str,
        game_paths: &[PathBuf],
    ) -> Result<(), EpicError> {
        #[cfg(target_os = "windows")]
        {
            use std::time::Duration;
            if !native_claim_account_id(account_id) {
                return Err(native_claim_error("Некорректный идентификатор аккаунта Epic."));
            }
            let paths = self.paths()?;
            let log_path = native_claim_log_path(&paths)?;
            // Validate credentials and executable before touching the live client.
            let _ = encode_launcher_remember_me(display_name, token)?;
            let mut command = launcher_login_command(&paths)?;
            if let Some(root) = native_claim_process_snapshot(&paths, game_paths)? {
                let log = fs::read_to_string(&log_path)
                    .map_err(|_| native_claim_error("Не удалось проверить текущий вход лаунчера Epic."))?;
                let owner = native_claim_latest_owner(&log, root.started)?;
                if owner == account_id {
                    return Ok(());
                }
                // Recheck the complete safety snapshot immediately before closing.
                let current = native_claim_process_snapshot(&paths, game_paths)?;
                if current.as_ref() != Some(&root) {
                    return Err(native_claim_error("Состояние лаунчера Epic изменилось. Повтори получение."));
                }
                let latest_log = fs::read_to_string(&log_path)
                    .map_err(|_| native_claim_error("Не удалось проверить текущий вход лаунчера Epic."))?;
                if native_claim_latest_owner(&latest_log, root.started)? != owner {
                    return Err(native_claim_error("Аккаунт лаунчера Epic изменился. Повтори получение."));
                }
                if !crate::sys::process::send_sigterm(root.pid) {
                    return Err(native_claim_error("Не удалось штатно закрыть Epic. Закрой лаунчер и повтори получение."));
                }
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                loop {
                    let mut system = System::new();
                    system.refresh_processes();
                    let alive = system.process(sysinfo::Pid::from_u32(root.pid))
                        .is_some_and(|p| p.start_time() == root.started);
                    if !alive { break; }
                    if tokio::time::Instant::now() >= deadline {
                        return Err(native_claim_error("Epic не закрылся штатно. Закрой лаунчер и повтори получение."));
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            // Includes games started while the launcher was closing and orphan helpers.
            if native_claim_process_snapshot(&paths, game_paths)?.is_some() {
                return Err(native_claim_error("Epic уже запущен. Повтори получение после завершения переключения."));
            }
            let previous = fs::read(&paths.game_user_settings).ok();
            self.clone().with_skip_process_termination(true)
                .login_with_refresh_token(account_id, display_name, token).await?;
            // Do not launch if a game or another launcher appeared while writing config.
            if native_claim_process_snapshot(&paths, game_paths)?.is_some() {
                return Err(native_claim_error("Epic уже запущен. Повтори получение после завершения переключения."));
            }
            if command.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
                .spawn().is_err() {
                if let Some(contents) = previous {
                    crate::storage::atomic::atomic_write(&paths.game_user_settings, contents)
                        .map_err(|_| native_claim_error("Не удалось восстановить настройки Epic."))?;
                }
                return Err(native_claim_error("Не удалось запустить Epic Games Launcher."));
            }
            let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
            loop {
                if let Some(root) = native_claim_process_snapshot(&paths, game_paths)? {
                    if let Ok(log) = fs::read_to_string(&log_path) {
                        if native_claim_latest_owner(&log, root.started).ok().as_deref() == Some(account_id) {
                            // Recheck process, game and identity guards after successful login.
                            if self.native_claim_binding(account_id, game_paths)? == Some((root.pid, root.started)) {
                                return Ok(());
                            }
                        }
                    }
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(native_claim_error("Epic не подтвердил вход в выбранный аккаунт. Заверши вход в лаунчере."));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (account_id, display_name, token, game_paths);
            Err(EpicError::Io("Получение через лаунчер поддерживается только в Windows.".into()))
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
                no_saved_session: false,
                message: None,
            });
        }

        // Safe switch without saved profile:
        // If target account has no saved session and is not already live active on disk,
        // and the launcher is currently running, do NOT kill running processes.
        let has_saved = self.has_session(clean_target);
        let is_live_target = current_account.as_deref() == Some(clean_target) && self.is_disk_authenticated();
        if !has_saved && !is_live_target && is_running {
            let name = display_name.unwrap_or(clean_target);
            let msg = format!(
                "Сессия для аккаунта '{}' ещё не сохранена в swch. Пожалуйста, выполните вход в этот аккаунт в Epic Games Launcher, чтобы swch сохранил сессию.",
                name
            );
            info!(
                "No saved session snapshot for Epic account '{}' and launcher is running. Preventing idle shutdown.",
                clean_target
            );
            return Ok(SwitchEpicSessionResult {
                target_account_id: clean_target.to_string(),
                restored_saved_session: false,
                already_active: false,
                no_saved_session: true,
                message: Some(msg),
            });
        }

        // Launcher must be closed before files are modified
        if is_running && !self.skip_process_termination {
            info!("Terminating running Epic Games Launcher processes before switching session...");
            if let Err(e) = self.terminate_processes().await {
                warn!("Epic termination initial attempt: {}. Verifying process status...", e);
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let mut sys = sysinfo::System::new();
                sys.refresh_processes();
                let remaining = find_launcher_processes(&sys, LauncherType::EpicGames);
                if !remaining.is_empty() {
                    self.terminate_processes().await?;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
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
            no_saved_session: false,
            message: None,
        })
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeClaimRoot {
    pid: u32,
    started: u64,
}

#[cfg(any(target_os = "windows", test))]
struct NativeClaimProcess {
    pid: u32,
    parent: Option<u32>,
    name: String,
    exe: Option<String>,
    started: u64,
    trusted_overlay: bool,
}

#[cfg(any(target_os = "windows", test))]
fn native_claim_error(message: &str) -> EpicError {
    EpicError::Auth(message.into())
}

#[cfg(any(target_os = "windows", test))]
fn native_claim_account_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(target_os = "windows")]
fn native_claim_path(path: &Path) -> Option<String> {
    if !path.is_absolute() { return None; }
    Some(fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy().trim_start_matches(r"\\?\").replace('/', "\\").to_lowercase())
}

#[cfg(target_os = "windows")]
fn native_claim_log_path(paths: &EpicPaths) -> Result<PathBuf, EpicError> {
    paths.game_user_settings.ancestors()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("Saved")))
        .map(|saved| saved.join("Logs/EpicGamesLauncher.log"))
        .ok_or_else(|| native_claim_error("Не найден журнал входа Epic Games Launcher."))
}

#[cfg(target_os = "windows")]
fn native_claim_process_snapshot(paths: &EpicPaths, games: &[PathBuf]) -> Result<Option<NativeClaimRoot>, EpicError> {
    let executable = paths.app_bundle_path.as_deref()
        .filter(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("EpicGamesLauncher.exe")))
        .ok_or_else(|| native_claim_error("Epic Games Launcher не установлен."))?;
    let canonical = fs::canonicalize(executable)
        .map_err(|_| native_claim_error("Не удалось проверить файл Epic Games Launcher."))?;
    let exe = native_claim_path(&canonical)
        .ok_or_else(|| native_claim_error("Не удалось проверить путь Epic Games Launcher."))?;
    let installation = canonical.ancestors()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("Launcher")))
        .or_else(|| canonical.parent())
        .and_then(native_claim_path)
        .ok_or_else(|| native_claim_error("Не удалось проверить каталог Epic Games Launcher."))?;
    let game_paths: Vec<_> = games.iter().filter_map(|p| native_claim_path(p)).collect();
    let mut system = System::new();
    system.refresh_processes();
    let managed_artifacts = std::env::var_os("ProgramFiles(x86)")
        .map(|base| PathBuf::from(base).join("Epic Games/Epic Online Services/managedArtifacts"))
        .and_then(|path| native_claim_path(&path));
    let processes: Vec<_> = system.processes().iter().map(|(pid, process)| {
        let executable = process.exe().and_then(native_claim_path);
        let trusted_overlay = executable.as_deref().zip(managed_artifacts.as_deref())
            .is_some_and(|(exe, root)| native_claim_overlay_layout(exe, root)
                && native_claim_epic_signed(Path::new(exe)));
        NativeClaimProcess {
            pid: pid.as_u32(), parent: process.parent().map(|p| p.as_u32()),
            name: process.name().to_ascii_lowercase(), exe: executable,
            started: process.start_time(), trusted_overlay,
        }
    }).collect();
    native_claim_guard_processes(&processes, &exe, &installation, &game_paths)
}

/// Pure safety decision. Unknown descendants are deliberately treated as busy.
#[cfg(any(target_os = "windows", test))]
fn native_claim_guard_processes(
    processes: &[NativeClaimProcess],
    installed_exe: &str,
    installation: &str,
    games: &[String],
) -> Result<Option<NativeClaimRoot>, EpicError> {
    if processes.iter().any(|p| {
        p.exe.as_ref().is_some_and(|exe| native_claim_is_game_executable(exe, games))
            || (p.exe.is_none() && games.iter().any(|exe| {
                exe.rsplit('\\').next().is_some_and(|name| name.eq_ignore_ascii_case(&p.name))
            }))
    }) {
        return Err(native_claim_error("Запущена игра Epic. Получение отложено до её закрытия."));
    }
    let candidates: Vec<_> = processes.iter().filter(|p| {
        p.name.eq_ignore_ascii_case("EpicGamesLauncher.exe")
            || p.exe.as_deref() == Some(installed_exe)
    }).collect();
    if candidates.len() > 1 {
        return Err(native_claim_error("Запущено несколько процессов лаунчера Epic. Получение отложено."));
    }
    let prefix = format!("{}\\", installation.trim_end_matches('\\'));
    let trusted_helper = |p: &NativeClaimProcess| {
        p.trusted_overlay || p.exe.as_deref().is_some_and(|exe| {
            exe.starts_with(&prefix)
                && matches!(exe.rsplit('\\').next(), Some("epicwebhelper.exe" | "crashreportclient.exe"))
        })
    };
    let Some(root) = candidates.first() else {
        if processes.iter().any(trusted_helper) {
            return Err(native_claim_error("Epic ещё завершает работу. Повтори получение после закрытия лаунчера."));
        }
        return Ok(None);
    };
    if root.exe.as_deref() != Some(installed_exe) || root.started == 0 {
        return Err(native_claim_error("Не удалось подтвердить процесс установленного лаунчера Epic."));
    }
    let mut descendants = std::collections::HashSet::from([root.pid]);
    loop {
        let previous = descendants.len();
        for process in processes {
            if process.parent.is_some_and(|pid| descendants.contains(&pid)) {
                descendants.insert(process.pid);
            }
        }
        if descendants.len() == previous { break; }
    }
    if processes.iter().any(|p| p.pid != root.pid && descendants.contains(&p.pid) && !trusted_helper(p)) {
        return Err(native_claim_error("Лаунчер Epic занят игрой или другим процессом. Получение отложено."));
    }
    if processes.iter().any(|p| trusted_helper(p) && !descendants.contains(&p.pid)) {
        return Err(native_claim_error("Обнаружен отдельный процесс Epic. Получение отложено."));
    }
    Ok(Some(NativeClaimRoot { pid: root.pid, started: root.started }))
}

/// The EOS social overlay is a launcher helper only in this installed package
/// layout; its filename alone cannot make an arbitrary child trusted.
#[cfg(any(target_os = "windows", test))]
fn native_claim_overlay_layout(executable: &str, managed_artifacts: &str) -> bool {
    let prefix = format!("{}\\", managed_artifacts.trim_end_matches('\\'));
    let Some(relative) = executable.strip_prefix(&prefix) else { return false; };
    let parts: Vec<_> = relative.split('\\').collect();
    parts.len() == 2 && native_claim_account_id(parts[0])
        && parts[1] == "eosoverlayrenderer-win64-shipping.exe"
}

/// Verify this exact file using Windows Authenticode. No application is launched
/// or injected. File metadata changes invalidate the process-local cache.
#[cfg(target_os = "windows")]
fn native_claim_epic_signed(path: &Path) -> bool {
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant, SystemTime};
    type SignatureCache = std::collections::HashMap<PathBuf, (u64, SystemTime, bool)>;
    static CACHE: OnceLock<Mutex<SignatureCache>> = OnceLock::new();
    let Ok(metadata) = fs::metadata(path) else { return false; };
    if !metadata.is_file() { return false; }
    let Ok(modified) = metadata.modified() else { return false; };
    let cache = CACHE.get_or_init(|| Mutex::new(SignatureCache::new()));
    if let Ok(entries) = cache.lock() {
        if let Some((size, timestamp, valid)) = entries.get(path) {
            if *size == metadata.len() && *timestamp == modified { return *valid; }
        }
    }
    let Some(powershell) = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe"))
        .filter(|exe| exe.is_file()) else { return false; };
    let Some(mut command) = native_claim_signature_command(&powershell, path) else { return false; };
    let Ok(mut child) = command.spawn() else { return false; };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                // Only this verification subprocess is terminated on timeout.
                // Launcher, overlays, games and any existing processes are untouched.
                if child.kill().is_ok() { let _ = child.wait(); }
                return false;
            }
        }
    }
    let Ok(output) = child.wait_with_output() else { return false; };
    if !output.status.success() { return false; }
    let valid = output.stdout == b"SWCH_EPIC_SIGNED";
    // Refuse a result if the file changed while Windows verified it.
    let unchanged = fs::metadata(path).ok().is_some_and(|now| {
        now.len() == metadata.len() && now.modified().ok() == Some(modified)
    });
    if !unchanged { return false; }
    if let Ok(mut entries) = cache.lock() {
        entries.insert(path.to_path_buf(), (metadata.len(), modified, valid));
    }
    valid
}

#[cfg(target_os = "windows")]
fn native_claim_signature_command(powershell: &Path, file: &Path) -> Option<std::process::Command> {
    use std::os::windows::process::CommandExt;
    // PowerShell 5.1 must load its own Security module. A PSModulePath inherited
    // from PowerShell 7 can select an incompatible module and falsely reject a
    // correctly signed Epic package. Exclude user and third-party module paths.
    let modules = powershell.parent()?.join("Modules");
    // A fixed command reads a task-specific environment variable; no filename
    // is interpolated into PowerShell source or exposed in command-line logs.
    const SCRIPT: &str = "$ErrorActionPreference='Stop'; try { $swchSignature=Get-AuthenticodeSignature -LiteralPath $env:SWCH_NATIVE_EPIC_SIGNER_PATH; $swchSigner=$swchSignature.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName,$false); if ($swchSignature.Status -eq 'Valid' -and $swchSigner -in @('Epic Games Inc','Epic Games Inc.')) { [Console]::Write('SWCH_EPIC_SIGNED') } } catch { exit 1 }";
    let mut command = std::process::Command::new(powershell);
    command.args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("SWCH_NATIVE_EPIC_SIGNER_PATH", file).env("PSModulePath", modules)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null()).creation_flags(0x08000000);
    Some(command)
}

/// Includes game bootstrap descendants whose original parent has already
/// exited. A drive/share root is never used as a whole-directory game guard.
#[cfg(any(target_os = "windows", test))]
fn native_claim_is_game_executable(executable: &str, games: &[String]) -> bool {
    games.iter().any(|game| {
        if executable == game { return true; }
        let Some((directory, _)) = game.rsplit_once('\\') else { return false; };
        let directory = directory.trim_end_matches('\\');
        let is_root = directory.is_empty()
            || (directory.len() == 2 && directory.ends_with(':'))
            || (directory.starts_with(r"\\")
                && directory.trim_start_matches('\\').split('\\').filter(|v| !v.is_empty()).count() <= 2);
        !is_root && executable.starts_with(&format!("{directory}\\"))
    })
}

/// Latest authentication event for this process lifetime; old successful log
/// lines cannot override a newer logout, failed login, or different account.
#[cfg(any(target_os = "windows", test))]
fn native_claim_latest_owner(log: &str, started: u64) -> Result<String, EpicError> {
    let mut latest: Option<(i64, usize, Option<String>)> = None;
    for (index, line) in log.lines().enumerate() {
        let login = line.contains("FAccountService::OnLoginComplete");
        let lower = line.to_ascii_lowercase();
        let logout = line.contains("FAccountService::")
            && (lower.contains("logout") || lower.contains("loggedout") || lower.contains("logged_out"));
        let auth_failed = line.contains("FAccountService::")
            && lower.contains("auth") && lower.contains("fail");
        if !login && !logout && !auth_failed { continue; }
        let timestamp = line.strip_prefix('[').and_then(|s| s.split(']').next())
            .and_then(|v| chrono::NaiveDateTime::parse_from_str(v, "%Y.%m.%d-%H.%M.%S:%3f").ok())
            .map(|t| t.and_utc().timestamp_millis())
            .ok_or_else(|| native_claim_error("Журнал Epic не позволяет подтвердить текущий вход."))?;
        let started_ms = i64::try_from(started).ok().and_then(|t| t.checked_mul(1000))
            .ok_or_else(|| native_claim_error("Не удалось проверить время запуска Epic."))?;
        if timestamp < started_ms { continue; }
        let owner = if login {
            let result = line.split_once("FAccountService::OnLoginComplete").unwrap().1;
            let fields: Vec<_> = result.split_whitespace().collect();
            if fields.len() >= 4 && fields[0] == "-" && fields[2] == "TRUE" && native_claim_account_id(fields[3]) {
                Some(fields[3].to_string())
            } else { None }
        } else { None };
        if latest.as_ref().is_none_or(|(t, i, _)| (timestamp, index) >= (*t, *i)) {
            latest = Some((timestamp, index, owner));
        }
    }
    latest.and_then(|(_, _, owner)| owner)
        .ok_or_else(|| native_claim_error("Текущий вход Epic не подтверждён. Заверши вход в лаунчере."))
}

fn native_login_confirmed(log: &str, account_id: &str, started_at: chrono::DateTime<Utc>) -> bool {
    log.lines().any(|line| {
        let Some((_, result)) = line.split_once("FAccountService::OnLoginComplete") else { return false; };
        let fields: Vec<_> = result.split_whitespace().collect();
        if fields.len() < 4 || fields[0] != "-" || fields[2] != "TRUE" || fields[3] != account_id {
            return false;
        }
        let Some(timestamp) = line.strip_prefix('[').and_then(|s| s.split(']').next()) else { return false; };
        chrono::NaiveDateTime::parse_from_str(timestamp, "%Y.%m.%d-%H.%M.%S:%3f").ok()
            .is_some_and(|t| t.and_utc() >= started_at)
    })
}

fn launcher_login_command(paths: &EpicPaths) -> Result<std::process::Command, EpicError> {
    let executable = paths.app_bundle_path.as_ref().filter(|p| p.exists())
        .ok_or_else(|| EpicError::Io("Epic Games Launcher executable was not found".into()))?;
    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("open");
        command.arg("-a").arg(executable);
        Ok(command)
    }
    #[cfg(not(target_os = "macos"))]
    { Ok(std::process::Command::new(executable)) }
}

fn encode_launcher_remember_me(display_name: &str, token: &str) -> Result<String, EpicError> {
    use aes::{Aes256, cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray}};
    use base64::{Engine, engine::general_purpose::STANDARD};
    if token.is_empty() || token.len() > 16384 || token.chars().any(char::is_control) {
        return Err(EpicError::Auth("Invalid native launcher token".into()));
    }
    // EGL's config format: UTF-8 JSON, trailing NUL, AES-256-ECB/PKCS7, Base64.
    // This public format key is shared by EGL and Legendary, not a user secret.
    let cipher = Aes256::new_from_slice(b"A09C853C9E95409BB94D707EADEFA52E")
        .map_err(|_| EpicError::Auth("Invalid launcher config key".into()))?;
    let mut bytes = serde_json::to_vec(&serde_json::json!([{
        "Region": "Prod", "Email": "", "Name": "", "LastName": "",
        "DisplayName": display_name, "Token": token,
        "bHasPasswordAuth": true, "TokenUseCount": 0
    }]))?;
    bytes.push(0);
    let padding = 16 - bytes.len() % 16;
    bytes.resize(bytes.len() + padding, padding as u8);
    for block in bytes.chunks_exact_mut(16) {
        cipher.encrypt_block(GenericArray::from_mut_slice(block));
    }
    Ok(STANDARD.encode(bytes))
}

fn replace_launcher_login_sections(existing: &str, data: &str) -> String {
    let mut lines = Vec::new();
    let mut skip = false;
    for line in existing.trim_start_matches('\u{feff}').lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            skip = trimmed.eq_ignore_ascii_case("[RememberMe]") || trimmed.eq_ignore_ascii_case("[Offline]");
        }
        if !skip { lines.push(line.to_string()); }
    }
    lines.push(format!("[RememberMe]\nEnable=True\nData={data}"));
    lines.join("\n") + "\n"
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

/// Safely removes a directory tree, clearing read-only attributes on Windows and retrying upon lock contention.
pub fn safe_remove_dir_all(dir: &Path) {
    if !dir.exists() {
        return;
    }

    #[cfg(windows)]
    fn clear_readonly_recursive(p: &Path) {
        if let Ok(meta) = fs::metadata(p) {
            let mut perms = meta.permissions();
            if perms.readonly() {
                perms.set_readonly(false);
                let _ = fs::set_permissions(p, perms);
            }
        }
        if p.is_dir() {
            if let Ok(entries) = fs::read_dir(p) {
                for entry in entries.flatten() {
                    clear_readonly_recursive(&entry.path());
                }
            }
        }
    }

    #[cfg(windows)]
    clear_readonly_recursive(dir);

    for attempt in 0..10 {
        if fs::remove_dir_all(dir).is_ok() || !dir.exists() {
            return;
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                #[cfg(windows)]
                {
                    if let Ok(meta) = fs::metadata(&p) {
                        let mut perms = meta.permissions();
                        if perms.readonly() {
                            perms.set_readonly(false);
                            let _ = fs::set_permissions(&p, perms);
                        }
                    }
                }
                if p.is_dir() {
                    safe_remove_dir_all(&p);
                } else {
                    let _ = fs::remove_file(&p);
                }
            }
        }
        if fs::remove_dir_all(dir).is_ok() || !dir.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20 * (attempt + 1)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn native_claim_uses_latest_process_login_and_rejects_logout_or_failure() {
        let started = chrono::DateTime::parse_from_rfc3339("2026-10-01T05:29:05Z")
            .unwrap().timestamp() as u64;
        let account = "0123456789abcdef0123456789abcdef";
        let other = "abcdef0123456789abcdef0123456789";
        let first = format!("[2026.10.01-05.29.06:685][ 42]FAccountService::OnLoginComplete - 0 TRUE {account}");
        assert_eq!(native_claim_latest_owner(&first, started).unwrap(), account);
        assert!(native_claim_latest_owner(&first, started + 5).is_err());
        assert!(native_claim_latest_owner(&first.replace(account, "selected"), started).is_err());
        assert!(native_claim_latest_owner(&first.replace("TRUE", "FALSE"), started).is_err());
        let second = format!("[2026.10.01-05.29.07:000][ 42]FAccountService::OnLoginComplete - 0 TRUE {other}");
        assert_eq!(native_claim_latest_owner(&format!("{first}\n{second}"), started).unwrap(), other);
        assert_eq!(native_claim_latest_owner(&format!("{second}\n{first}"), started).unwrap(), other);
        for event in ["FAccountService::OnLogoutComplete", "FAccountService::OnAuthFailure", "FAccountService::OnLoginComplete - 0 FALSE"] {
            let failed = format!("{first}\n[2026.10.01-05.29.08:000][ 42]{event}");
            assert!(native_claim_latest_owner(&failed, started).is_err());
        }
        assert!(native_claim_latest_owner("FAccountService::OnLoginComplete - 0 TRUE 0123456789abcdef0123456789abcdef", started).is_err());
    }

    #[test]
    fn native_claim_process_guard_rejects_games_unknown_children_and_spoofed_roots() {
        let executable = r"c:\epic\launcher\portal\binaries\win64\epicgameslauncher.exe";
        let installation = r"c:\epic\launcher";
        let process = |pid, parent, name: &str, exe: Option<&str>| NativeClaimProcess {
            pid, parent, name: name.into(), exe: exe.map(str::to_string), started: 123,
            trusted_overlay: false,
        };
        let root = || process(1, None, "EpicGamesLauncher.exe", Some(executable));
        let helper = || process(2, Some(1), "EpicWebHelper.exe", Some(r"c:\epic\launcher\engine\binaries\win64\epicwebhelper.exe"));
        assert_eq!(native_claim_guard_processes(&[root(), helper()], executable, installation, &[]).unwrap(), Some(NativeClaimRoot { pid: 1, started: 123 }));
        assert!(native_claim_guard_processes(&[], executable, installation, &[]).unwrap().is_none());
        assert!(native_claim_guard_processes(&[root(), root()], executable, installation, &[]).is_err());
        assert!(native_claim_guard_processes(&[process(1, None, "EpicGamesLauncher.exe", Some(r"c:\fake\epicgameslauncher.exe"))], executable, installation, &[]).is_err());
        assert!(native_claim_guard_processes(&[root(), process(2, Some(1), "EpicWebHelper.exe", Some(r"c:\fake\epicwebhelper.exe"))], executable, installation, &[]).is_err());
        assert!(native_claim_guard_processes(&[root(), process(2, Some(1), "unknown.exe", None)], executable, installation, &[]).is_err());
        assert!(native_claim_guard_processes(&[root(), helper(), process(3, Some(2), "game.exe", Some(r"c:\games\game.exe"))], executable, installation, &[]).is_err());
        assert!(native_claim_guard_processes(&[helper()], executable, installation, &[]).is_err());
        assert!(native_claim_guard_processes(&[process(3, None, "game.exe", Some(r"c:\games\game.exe"))], executable, installation, &[r"c:\games\game.exe".into()]).is_err());
        assert!(native_claim_guard_processes(&[root(), process(4, None, "EpicWebHelper.exe", Some(r"c:\epic\launcher\engine\binaries\win64\epicwebhelper.exe"))], executable, installation, &[]).is_err());
        // UE Shipping process can outlive its bootstrap and lose the ancestry.
        let shipping = process(5, Some(999), "game-win64-shipping.exe", Some(r"c:\games\game\binaries\win64\game-win64-shipping.exe"));
        assert!(native_claim_guard_processes(&[root(), shipping], executable, installation, &[r"c:\games\game\game.exe".into()]).is_err());
        assert!(!native_claim_is_game_executable(r"c:\games\game-other\tool.exe", &[r"c:\games\game\game.exe".into()]));
        assert!(!native_claim_is_game_executable(r"c:\windows\explorer.exe", &[r"c:\game.exe".into()]));
        assert!(native_claim_is_game_executable(r"c:\game.exe", &[r"c:\game.exe".into()]));
        assert!(!native_claim_is_game_executable(r"\\server\share\other\tool.exe", &[r"\\server\share\game.exe".into()]));
        assert!(!native_claim_is_game_executable(r"\bin\tool.exe", &[r"\game.exe".into()]));
    }

    #[test]
    fn native_claim_overlay_requires_installed_layout_and_valid_epic_signature() {
        let executable = r"c:\epic\launcher\portal\binaries\win64\epicgameslauncher.exe";
        let installation = r"c:\epic\launcher";
        let managed = r"c:\program files (x86)\epic games\epic online services\managedartifacts";
        let overlay_exe = format!(r"{managed}\98bc04bc842e4906993fd6d6644ffb8d\eosoverlayrenderer-win64-shipping.exe");
        assert!(native_claim_overlay_layout(&overlay_exe, managed));
        for invalid in [
            r"c:\fake\eosoverlayrenderer-win64-shipping.exe".to_string(),
            format!(r"{managed}\eosoverlayrenderer-win64-shipping.exe"),
            format!(r"{managed}\not-a-package\eosoverlayrenderer-win64-shipping.exe"),
            format!(r"{managed}\98bc04bc842e4906993fd6d6644ffb8d\sub\eosoverlayrenderer-win64-shipping.exe"),
            format!(r"{managed}-other\98bc04bc842e4906993fd6d6644ffb8d\eosoverlayrenderer-win64-shipping.exe"),
            format!(r"{managed}\98bc04bc842e4906993fd6d6644ffb8d\other.exe"),
        ] {
            assert!(!native_claim_overlay_layout(&invalid, managed));
        }
        let root = || NativeClaimProcess {
            pid: 1, parent: None, name: "EpicGamesLauncher.exe".into(),
            exe: Some(executable.into()), started: 123, trusted_overlay: false,
        };
        let overlay = |trusted_overlay| NativeClaimProcess {
            pid: 2, parent: Some(1), name: "EOSOverlayRenderer-Win64-Shipping.exe".into(),
            exe: Some(overlay_exe.clone()), started: 123, trusted_overlay,
        };
        // The runtime sets this role only after successful Authenticode proof.
        assert!(native_claim_guard_processes(&[root(), overlay(true)], executable, installation, &[]).unwrap().is_some());
        assert!(native_claim_guard_processes(&[root(), overlay(false)], executable, installation, &[]).is_err());
        assert!(native_claim_guard_processes(&[overlay(true)], executable, installation, &[]).is_err());
        let mut unrelated_overlay = overlay(true);
        unrelated_overlay.parent = Some(999);
        assert!(native_claim_guard_processes(&[root(), unrelated_overlay], executable, installation, &[]).is_err());
        let mut filename_only = overlay(false);
        filename_only.exe = Some(r"c:\fake\eosoverlayrenderer-win64-shipping.exe".into());
        assert!(native_claim_guard_processes(&[root(), filename_only], executable, installation, &[]).is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_claim_signature_verifier_overrides_host_powershell_modules() {
        let powershell = Path::new(r"c:\windows\system32\windowspowershell\v1.0\powershell.exe");
        let file = Path::new(r"c:\epic\overlay.exe");
        let command = native_claim_signature_command(powershell, file).unwrap();
        let module_path = command.get_envs().find(|(name, _)| *name == "PSModulePath")
            .and_then(|(_, value)| value).unwrap();
        assert_eq!(Path::new(module_path), powershell.parent().unwrap().join("Modules"));
        let target = command.get_envs().find(|(name, _)| *name == "SWCH_NATIVE_EPIC_SIGNER_PATH")
            .and_then(|(_, value)| value).unwrap();
        assert_eq!(Path::new(target), file);
        // The path remains a data argument, never PowerShell source.
        assert!(command.get_args().all(|argument| !argument.to_string_lossy().contains("overlay.exe")));
    }

    #[tokio::test]
    async fn fresh_launcher_login_replaces_stale_identity_preserving_settings_and_cache() {
        use aes::{Aes256, cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray}};
        use base64::{Engine, engine::general_purpose::STANDARD};
        let dir = tempdir().unwrap();
        let executable = dir.path().join("EpicGamesLauncher.exe");
        fs::write(&executable, b"test fixture, never executed").unwrap();
        let paths = EpicPaths {
            key_value_store: dir.path().join("KeyValueStore.ini"),
            game_user_settings: dir.path().join("GameUserSettings.ini"),
            webcache_dir: dir.path().join("webcache_9999"),
            app_bundle_path: Some(executable), saved_data_dir: None,
        };
        fs::create_dir_all(&paths.webcache_dir).unwrap();
        fs::write(paths.webcache_dir.join("Cookies"), b"preserve cache").unwrap();
        let old = "[Launcher]\nLanguage=ru\n[RememberMe]\nData=stale_identity\n[Offline]\nData=stale_offline\n[Other]\nKeep=True\n";
        fs::write(&paths.game_user_settings, old).unwrap();
        let manager = EpicSessionManager::with_paths(paths.clone())
            .with_sessions_dir(dir.path().join("sessions")).with_skip_process_termination(true);
        manager.login_with_refresh_token("selected", "Selected Привет", "dedicated_refresh").await.unwrap();
        let config = fs::read_to_string(&paths.game_user_settings).unwrap();
        assert!(config.contains("Language=ru"));
        assert!(config.contains("Keep=True"));
        assert!(!config.contains("stale_"));
        assert!(!config.contains("dedicated_refresh"));
        assert_eq!(fs::read_to_string(dir.path().join("sessions/launcher-before-login.ini")).unwrap(), old);
        let data = config.lines().find_map(|l| l.strip_prefix("Data=")).unwrap();
        let mut plain = STANDARD.decode(data).unwrap();
        let cipher = Aes256::new_from_slice(b"A09C853C9E95409BB94D707EADEFA52E").unwrap();
        for block in plain.chunks_exact_mut(16) { cipher.decrypt_block(GenericArray::from_mut_slice(block)); }
        let padding = *plain.last().unwrap() as usize;
        plain.truncate(plain.len() - padding - 1);
        let users: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(users[0]["Token"], "dedicated_refresh");
        assert_eq!(users[0]["DisplayName"], "Selected Привет");
        assert_eq!(users[0]["Region"], "Prod");
        assert_eq!(fs::read(paths.webcache_dir.join("Cookies")).unwrap(), b"preserve cache");
        assert!(encode_launcher_remember_me("test", "bad\nsecret").is_err());
    }

    #[test]
    fn native_confirmation_rejects_old_wrong_or_failed_login() {
        let started = chrono::DateTime::parse_from_rfc3339("2026-10-01T05:29:05Z").unwrap().with_timezone(&Utc);
        let success = "[2026.10.01-05.29.06:685][ 42]FAccountService::OnLoginComplete - 0 TRUE selected";
        assert!(native_login_confirmed(success, "selected", started));
        assert!(!native_login_confirmed(success, "different", started));
        assert!(!native_login_confirmed(&success.replace("TRUE", "FALSE"), "selected", started));
        assert!(!native_login_confirmed(&success.replace("05.29.06", "05.29.04"), "selected", started));
        assert!(!native_login_confirmed("FAccountService::OnLoginComplete - 0 TRUE selected", "selected", started));
    }

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
            saved_data_dir: None,
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

        // A client upgrade can leave several cache versions on Windows.
        // Restore the target into every existing version, without reviving
        // the outgoing account in the version used by the current client.
        let new_cache = dir.path().join("webcache_9999");
        fs::create_dir_all(&new_cache).unwrap();
        fs::write(new_cache.join("Cookies"), b"stale_other_account").unwrap();

        // Switch back to acc_1 (should snapshot acc_2 as outgoing and restore acc_1 snapshot!)
        let res1 = manager.switch_account("acc_1", Some("Account 1")).await.unwrap();
        assert_eq!(res1.target_account_id, "acc_1");
        assert!(res1.restored_saved_session);
        assert_eq!(manager.read_current_launcher_account().as_deref(), Some("acc_1"));
        assert_eq!(fs::read(webcache.join("Cookies")).unwrap(), b"fake_cookies_account_1");
        assert_eq!(fs::read(new_cache.join("Cookies")).unwrap(), b"fake_cookies_account_1");

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
            saved_data_dir: None,
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
            saved_data_dir: None,
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
            saved_data_dir: None,
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

    #[tokio::test]
    async fn test_switch_account_safe_no_saved_profile_when_running() {
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
            saved_data_dir: None,
        };

        // Active account on disk is acc_1
        write_account_id_to_ini(&kvs, "acc_1").unwrap();
        fs::write(&gus, "[RememberMe]\nEnable=True\nData=tok1\n").unwrap();
        fs::create_dir_all(&webcache).unwrap();
        fs::write(webcache.join("Cookies"), b"live_cookies_acc_1").unwrap();

        // Simulate launcher IS running
        let manager = EpicSessionManager::with_paths(paths)
            .with_sessions_dir(&sessions)
            .with_simulate_running(true);

        // Attempt switch to acc_2, which has NO saved session snapshot
        assert!(!manager.has_session("acc_2"));

        let res = manager.switch_account("acc_2", Some("Account Two")).await.unwrap();

        // Should return safe result without terminating or wiping
        assert_eq!(res.target_account_id, "acc_2");
        assert!(res.no_saved_session);
        assert!(!res.restored_saved_session);
        assert!(!res.already_active);
        assert!(res.message.is_some());
        assert!(res.message.unwrap().contains("Account Two"));

        // Verify active account on disk was NOT wiped
        assert_eq!(read_account_id_from_ini(&kvs).as_deref(), Some("acc_1"));
        assert!(fs::read_to_string(&gus).unwrap().contains("Enable=True"));
        assert_eq!(fs::read(webcache.join("Cookies")).unwrap(), b"live_cookies_acc_1");
    }

    #[tokio::test]
    async fn test_switch_account_saved_data_tokens_roundtrip() {
        let dir = tempdir().unwrap();
        let kvs = dir.path().join("KeyValueStore.ini");
        let gus = dir.path().join("GameUserSettings.ini");
        let webcache = dir.path().join("webcache");
        let saved_data = dir.path().join("Saved").join("Data");
        let sessions = dir.path().join("sessions");
        fs::create_dir_all(&saved_data).unwrap();

        // Account 1 files
        fs::write(saved_data.join("acc_win1.dat"), b"win1_token_data").unwrap();
        fs::write(saved_data.join("OC_acc_win1.dat"), b"win1_offline_cache").unwrap();
        write_account_id_to_ini(&kvs, "acc_win1").unwrap();
        fs::write(&gus, "[RememberMe]\nEnable=True\nData=tok_win1\n").unwrap();

        let paths = EpicPaths {
            key_value_store: kvs.clone(),
            game_user_settings: gus.clone(),
            webcache_dir: webcache.clone(),
            app_bundle_path: None,
            saved_data_dir: Some(saved_data.clone()),
        };

        let manager = EpicSessionManager::with_paths(paths)
            .with_sessions_dir(&sessions)
            .with_skip_process_termination(true);

        // Backup acc_win1
        manager.backup_session("acc_win1", Some("Windows User 1")).unwrap();
        assert!(manager.has_session("acc_win1"));
        assert!(sessions.join("acc_win1").join("acc_win1.dat").is_file());
        assert!(sessions.join("acc_win1").join("OC_acc_win1.dat").is_file());

        // Prepare acc_win2
        fs::write(saved_data.join("acc_win2.dat"), b"win2_token_data").unwrap();
        fs::write(saved_data.join("OC_acc_win2.dat"), b"win2_offline_cache").unwrap();
        write_account_id_to_ini(&kvs, "acc_win2").unwrap();
        fs::write(&gus, "[RememberMe]\nEnable=True\nData=tok_win2\n").unwrap();
        manager.backup_session("acc_win2", Some("Windows User 2")).unwrap();
        assert!(manager.has_session("acc_win2"));

        // Switch to acc_win1
        let res = manager.switch_account("acc_win1", Some("Windows User 1")).await.unwrap();
        assert_eq!(res.target_account_id, "acc_win1");
        assert!(res.restored_saved_session);

        // Verify acc_win1.dat was restored into saved_data dir
        assert_eq!(fs::read(saved_data.join("acc_win1.dat")).unwrap(), b"win1_token_data");
        assert_eq!(fs::read(saved_data.join("OC_acc_win1.dat")).unwrap(), b"win1_offline_cache");
        // Verify old acc_win2 tokens were cleaned up from saved_data
        assert!(!saved_data.join("acc_win2.dat").exists());
        assert!(!saved_data.join("OC_acc_win2.dat").exists());
    }
}

