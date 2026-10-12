use crate::storage::atomic::{atomic_copy, atomic_write_json, atomic_write_str, AtomicError};
use crate::sys::process::{
    find_launcher_processes, terminate_launcher, LauncherType, TerminateOptions, TerminationReport,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use sysinfo::System;
use thiserror::Error;
use tracing::info;

#[derive(Error, Debug)]
pub enum RiotError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Atomic storage error: {0}")]
    Atomic(#[from] AtomicError),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Process error: {0}")]
    Process(#[from] crate::sys::process::ProcessError),
    #[error("Riot Client configuration not found: {0}")]
    ConfigNotFound(PathBuf),
    #[error("Riot account '{0}' not found in saved sessions")]
    AccountNotFound(String),
    #[error("Сохранённая сессия Riot не содержит данных для повторного входа. Войди в Riot Client с включённым «Stay signed in» и сохрани сессию заново.")]
    LoginRequired,
    #[error("Аккаунт Riot изменился во время сохранения. Повтори сохранение текущей сессии.")]
    AccountChanged,
}

/// Detects paths for Riot Client configuration and session files.
#[derive(Debug, Clone)]
pub struct RiotPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub private_settings_yaml: PathBuf,
    pub games_private_settings_yaml: PathBuf,
    pub client_settings_yaml: PathBuf,
}

impl RiotPaths {
    pub fn detect() -> Result<Self, RiotError> {
        if let Ok(custom) = std::env::var("SWCH_RIOT_DATA_DIR") {
            let data_dir = PathBuf::from(custom.trim());
            let config_dir = data_dir.parent().unwrap_or(&data_dir).join("Config");
            return Ok(Self {
                private_settings_yaml: data_dir.join("RiotClientPrivateSettings.yaml"),
                games_private_settings_yaml: data_dir.join("RiotGamesPrivateSettings.yaml"),
                client_settings_yaml: config_dir.join("RiotClientSettings.yaml"),
                data_dir,
                config_dir,
            });
        }

        #[cfg(target_os = "windows")]
        {
            let local_app_data = PathBuf::from(
                std::env::var_os("LOCALAPPDATA")
                    .unwrap_or_else(|| "C:\\Users\\Default\\AppData\\Local".into()),
            );
            let riot_client = local_app_data.join("Riot Games").join("Riot Client");
            let data_dir = riot_client.join("Data");
            let config_dir = riot_client.join("Config");
            Ok(Self {
                private_settings_yaml: data_dir.join("RiotClientPrivateSettings.yaml"),
                games_private_settings_yaml: data_dir.join("RiotGamesPrivateSettings.yaml"),
                client_settings_yaml: config_dir.join("RiotClientSettings.yaml"),
                data_dir,
                config_dir,
            })
        }

        #[cfg(target_os = "macos")]
        {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            let riot_client = home.join("Library/Application Support/Riot Games/Riot Client");
            let data_dir = riot_client.join("Data");
            let config_dir = riot_client.join("Config");
            Ok(Self {
                private_settings_yaml: data_dir.join("RiotClientPrivateSettings.yaml"),
                games_private_settings_yaml: data_dir.join("RiotGamesPrivateSettings.yaml"),
                client_settings_yaml: config_dir.join("RiotClientSettings.yaml"),
                data_dir,
                config_dir,
            })
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            let riot_client = home.join(".riot/Riot Client");
            let data_dir = riot_client.join("Data");
            let config_dir = riot_client.join("Config");
            Ok(Self {
                private_settings_yaml: data_dir.join("RiotClientPrivateSettings.yaml"),
                games_private_settings_yaml: data_dir.join("RiotGamesPrivateSettings.yaml"),
                client_settings_yaml: config_dir.join("RiotClientSettings.yaml"),
                data_dir,
                config_dir,
            })
        }
    }
}

/// Metadata stored alongside a backed-up Riot Games session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiotSessionManifest {
    pub account_id: String,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub backed_up_at: String,
    pub has_games_settings: bool,
    pub has_client_settings: bool,
}

/// Result returned from an account switch operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchRiotSessionResult {
    pub target_account_id: String,
    pub display_name: Option<String>,
    pub restored_saved_session: bool,
    pub already_active: bool,
    pub no_saved_session: bool,
    pub message: Option<String>,
}

/// Decodes base64 string (handles standard and URL-safe base64).
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut clean = input.replace('-', "+").replace('_', "/");
    clean.retain(|c| !c.is_whitespace());
    while clean.len() % 4 != 0 {
        clean.push('=');
    }
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let decode_byte = |b: u8| -> Option<u8> { table.iter().position(|&x| x == b).map(|p| p as u8) };
    let bytes = clean.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 4 {
            break;
        }
        let b0 = decode_byte(chunk[0])?;
        let b1 = decode_byte(chunk[1])?;
        let b2 = if chunk[2] == b'=' {
            0
        } else {
            decode_byte(chunk[2])?
        };
        let b3 = if chunk[3] == b'=' {
            0
        } else {
            decode_byte(chunk[3])?
        };

        out.push((b0 << 2) | (b1 >> 4));
        if chunk[2] != b'=' {
            out.push((b1 << 4) | (b2 >> 2));
        }
        if chunk[3] != b'=' {
            out.push((b2 << 6) | b3);
        }
    }
    Some(out)
}

/// Decodes the payload portion of a JWT string.
pub fn decode_jwt_payload(jwt: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let bytes = base64_decode(parts[1])?;
    serde_json::from_slice(&bytes).ok()
}

/// Sanitizes folder name for account storage.
fn sanitize_folder_name(name: &str) -> String {
    name.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_")
        .trim()
        .to_string()
}

/// Read a block from Riot's indentation-based settings format. Keep authentication
/// scoped to riot-login/persist; device cookies and cached ID tokens are not a login.
fn settings_block<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed
            .strip_prefix(key)
            .is_some_and(|s| s.starts_with(':'))
        {
            let indent = line.len() - line.trim_start().len();
            let start = offset;
            offset += line.len();
            for next in text[offset..].split_inclusive('\n') {
                if !next.trim().is_empty() && next.len() - next.trim_start().len() <= indent {
                    break;
                }
                offset += next.len();
            }
            return Some(&text[start..offset]);
        }
        offset += line.len();
    }
    None
}

fn scalar(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
}

fn login_cookie(text: &str, wanted: &str) -> Option<String> {
    let login = settings_block(text, "riot-login")?;
    let persist = settings_block(login, "persist")?;
    let session = settings_block(persist, "session")?;
    // Riot emits indentless YAML sequences: each '-' is aligned with 'cookies:'.
    let mut name = "";
    let mut value = "";
    let mut persistent = false;
    for line in session.lines().chain(std::iter::once("-")) {
        let trimmed = line.trim();
        if trimmed.starts_with('-') {
            if name == wanted && !value.is_empty() && value != "null" && persistent {
                return Some(value.to_string());
            }
            name = "";
            value = "";
            persistent = false;
        }
        if let Some((key, raw)) = trimmed.trim_start_matches('-').trim().split_once(':') {
            match key {
                "name" => name = scalar(raw),
                "value" => value = scalar(raw),
                "persistent" => persistent = scalar(raw) == "true",
                _ => {}
            }
        }
    }
    None
}

fn cached_account_id(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let token = line.trim().strip_prefix("id_token:")?;
        decode_jwt_payload(scalar(token))?["sub"]
            .as_str()
            .map(String::from)
    })
}

/// Recent Riot Clients persist SDK authorization separately from the legacy
/// cookie jar. An ID token alone is metadata, not a reusable authorization.
fn psl_account_id(text: &str) -> Option<String> {
    let psl = settings_block(text, "psl")?;
    let authorization = settings_block(psl, "authorization")?;
    let client = settings_block(authorization, "riot-client")?;
    let refresh = client.lines().find_map(|line| {
        line.trim().strip_prefix("refresh_token:").map(scalar)
    })?;
    if refresh.is_empty() || refresh == "null" { return None; }
    cached_account_id(client).filter(|id| !id.is_empty())
}

fn has_reusable_login(text: &str) -> bool {
    psl_account_id(text).is_some() || login_cookie(text, "ssid").is_some()
}

fn login_account_id(text: &str) -> Option<String> {
    psl_account_id(text).or_else(|| login_cookie(text, "sub"))
}

/// Riot can keep the cookie jar in Client settings while Games settings holds
/// an ID token and `persist: null`. Choose the jar that can resume a login,
/// rejecting a Client jar that disagrees with Games' cached account identity.
fn login_settings(client: Option<String>, games: Option<String>) -> Option<String> {
    if games
        .as_deref()
        .is_some_and(has_reusable_login)
    {
        games
    } else {
        if let (Some(client_id), Some(games_id)) = (
            client.as_deref().and_then(login_account_id),
            games.as_deref().and_then(cached_account_id),
        ) {
            if client_id != games_id {
                return None;
            }
        }
        client
    }
}

fn saved_login(dir: &std::path::Path) -> Option<String> {
    login_settings(
        fs::read_to_string(dir.join("RiotClientPrivateSettings.yaml")).ok(),
        fs::read_to_string(dir.join("RiotGamesPrivateSettings.yaml")).ok(),
    )
}

fn has_private_settings(dir: &std::path::Path) -> bool {
    dir.join("RiotClientPrivateSettings.yaml").is_file()
        || dir.join("RiotGamesPrivateSettings.yaml").is_file()
}

fn remove_if_present(path: &std::path::Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// Information extracted from current Riot Client files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CurrentRiotAccountInfo {
    pub account_id: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub has_session: bool,
}

/// Session manager responsible for profile swapping, process lifecycle, and backups for Riot Client.
#[derive(Debug, Clone)]
pub struct RiotSessionManager {
    custom_paths: Option<RiotPaths>,
    custom_sessions_dir: Option<PathBuf>,
    skip_process_termination: bool,
    simulate_running: Option<bool>,
}

impl Default for RiotSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RiotSessionManager {
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

    pub fn with_paths(paths: RiotPaths) -> Self {
        Self {
            custom_paths: Some(paths),
            custom_sessions_dir: None,
            skip_process_termination: false,
            simulate_running: None,
        }
    }

    pub fn with_sessions_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.custom_sessions_dir = Some(dir.into());
        self
    }

    pub fn with_skip_process_termination(mut self, skip: bool) -> Self {
        self.skip_process_termination = skip;
        self
    }

    pub fn with_simulate_running(mut self, running: bool) -> Self {
        self.simulate_running = Some(running);
        self
    }

    pub fn paths(&self) -> Result<RiotPaths, RiotError> {
        match &self.custom_paths {
            Some(p) => Ok(p.clone()),
            None => RiotPaths::detect(),
        }
    }

    pub fn sessions_dir(&self) -> Result<PathBuf, RiotError> {
        if let Some(ref dir) = self.custom_sessions_dir {
            fs::create_dir_all(dir)?;
            return Ok(dir.clone());
        }

        if let Ok(dir) = std::env::var("SWCH_RIOT_SESSIONS_DIR") {
            let p = PathBuf::from(dir.trim());
            fs::create_dir_all(&p)?;
            return Ok(p);
        }

        if let Some(base) = dirs::data_local_dir() {
            let p = base.join("swch").join("sessions").join("riot");
            fs::create_dir_all(&p)?;
            return Ok(p);
        }

        let fallback = PathBuf::from(".swch").join("sessions").join("riot");
        fs::create_dir_all(&fallback)?;
        Ok(fallback)
    }

    /// Resolves legacy Riot accounts directory (e.g. %APPDATA%\swch\riot_accounts) if present.
    pub fn legacy_sessions_dir(&self) -> Option<PathBuf> {
        if self.custom_sessions_dir.is_some() {
            return None;
        }
        if let Ok(dir) = std::env::var("SWCH_RIOT_LEGACY_ACCOUNTS_DIR") {
            let p = PathBuf::from(dir.trim());
            if p.is_dir() {
                return Some(p);
            }
        }
        if let Some(base) = dirs::data_dir().or_else(dirs::config_dir) {
            let p = base.join("swch").join("riot_accounts");
            if p.is_dir() {
                return Some(p);
            }
        }
        None
    }

    pub fn is_launcher_running(&self) -> bool {
        if let Some(simulated) = self.simulate_running {
            return simulated;
        }
        if self.skip_process_termination {
            return false;
        }
        let mut system = System::new();
        system.refresh_processes();
        !find_launcher_processes(&system, LauncherType::RiotGames).is_empty()
    }

    pub async fn terminate_processes(&self) -> Result<TerminationReport, RiotError> {
        if self.skip_process_termination {
            return Ok(TerminationReport::default());
        }
        let mut options = TerminateOptions::default();
        options.graceful_timeout = std::time::Duration::from_millis(3000);
        options.force_timeout = std::time::Duration::from_millis(3000);
        let report = terminate_launcher(LauncherType::RiotGames, &options).await;
        match report {
            Ok(r) => Ok(r),
            Err(e) => {
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    for img in &[
                        "RiotClientServices.exe",
                        "RiotClientUx.exe",
                        "RiotClientUxRender.exe",
                        "RiotClientCrashHandler.exe",
                    ] {
                        let _ = std::process::Command::new("taskkill.exe")
                            .args(["/F", "/IM", img, "/T"])
                            .creation_flags(CREATE_NO_WINDOW)
                            .status();
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    let mut sys = System::new();
                    sys.refresh_processes();
                    let remaining = find_launcher_processes(&sys, LauncherType::RiotGames);
                    if remaining.is_empty() {
                        return Ok(TerminationReport::default());
                    }
                }
                Err(e.into())
            }
        }
    }

    /// Inspects live Riot Client files on disk and extracts account information (puuid, username, Riot ID).
    pub fn read_current_account(&self) -> CurrentRiotAccountInfo {
        let paths = match self.paths() {
            Ok(p) => p,
            Err(_) => return CurrentRiotAccountInfo::default(),
        };

        let mut info = CurrentRiotAccountInfo::default();

        // 1. Try reading RiotGamesPrivateSettings.yaml for id_token
        if paths.games_private_settings_yaml.is_file() {
            if let Ok(content) = fs::read_to_string(&paths.games_private_settings_yaml) {
                // Look for id_token: "..."
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("id_token:") {
                        let token = trimmed
                            .trim_start_matches("id_token:")
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'');
                        if let Some(payload) = decode_jwt_payload(token) {
                            if let Some(sub) = payload["sub"].as_str() {
                                info.account_id = Some(sub.to_string());
                                info.has_session = true;
                            }
                            if let Some(uname) = payload["uname"].as_str() {
                                info.username = Some(uname.to_string());
                            }
                            if let Some(acct) = payload.get("acct") {
                                let gname = acct["game_name"].as_str().unwrap_or("");
                                let tag = acct["tag_line"].as_str().unwrap_or("");
                                if !gname.is_empty() {
                                    info.display_name = if tag.is_empty() {
                                        Some(gname.to_string())
                                    } else {
                                        Some(format!("{}#{}", gname, tag))
                                    };
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }

        let login = login_settings(
            fs::read_to_string(&paths.private_settings_yaml).ok(),
            fs::read_to_string(&paths.games_private_settings_yaml).ok(),
        );
        info.has_session = login.as_deref().is_some_and(has_reusable_login);
        if let Some(id) = login.as_deref().and_then(login_account_id) {
            // Cached ID tokens may belong to a previous account.
            if info.account_id.as_deref().is_some_and(|old| old != id) {
                info.username = None;
                info.display_name = None;
            }
            info.account_id = Some(id);
        }
        if !info.has_session {
            info = CurrentRiotAccountInfo::default();
        }

        // 3. Fallback display_name from username
        if info.display_name.is_none() {
            info.display_name = info.username.clone();
        }

        info
    }

    /// Checks if a saved session snapshot exists for this account name or ID.
    pub fn has_session(&self, account_identifier: &str) -> bool {
        self.resolve_session_dir(account_identifier).is_ok()
    }

    /// Capture token rotation even when Riot is closed or SWCH has just restarted.
    /// Only refresh an existing, connected account, and never replace a newer
    /// snapshot with older live files left behind by an earlier restore.
    pub fn refresh_saved_session(&self) -> Result<bool, RiotError> {
        let current = self.read_current_account();
        let Some(id) = current.account_id.as_deref() else { return Ok(false); };
        if !current.has_session || self.is_removed(id) { return Ok(false); }
        let Ok(saved_dir) = self.resolve_session_dir(id) else { return Ok(false); };
        let paths = self.paths()?;
        let live_client = fs::read_to_string(&paths.private_settings_yaml).ok();
        let live_games = fs::read_to_string(&paths.games_private_settings_yaml).ok();
        let Some(live) = login_settings(live_client, live_games.clone()) else { return Ok(false); };
        let saved = saved_login(&saved_dir);
        if saved.as_deref() == Some(&live) { return Ok(false); }
        if login_account_id(&live).as_deref() != Some(id) { return Ok(false); }
        if saved.as_deref().and_then(login_account_id).is_some_and(|saved_id| !saved_id.eq_ignore_ascii_case(id)) {
            return Ok(false);
        }
        let live_path = if live_games.as_deref() == Some(&live) {
            &paths.games_private_settings_yaml
        } else {
            &paths.private_settings_yaml
        };
        if let Some(ref saved) = saved {
            let saved_games = fs::read_to_string(saved_dir.join("RiotGamesPrivateSettings.yaml")).ok();
            let saved_path = saved_dir.join(if saved_games.as_deref() == Some(saved) {
                "RiotGamesPrivateSettings.yaml"
            } else {
                "RiotClientPrivateSettings.yaml"
            });
            if fs::metadata(live_path)?.modified()? <= fs::metadata(saved_path)?.modified()? {
                return Ok(false);
            }
        }
        let manifest = fs::read(saved_dir.join("session_manifest.json")).ok()
            .and_then(|bytes| serde_json::from_slice::<RiotSessionManifest>(&bytes).ok());
        self.backup_session(
            id,
            manifest.as_ref().and_then(|m| m.username.as_deref()).or(current.username.as_deref()),
            manifest.as_ref().and_then(|m| m.display_name.as_deref()).or(current.display_name.as_deref()),
        )?;
        Ok(true)
    }

    fn removed_identifiers(&self) -> Result<Vec<String>, RiotError> {
        match fs::read(self.sessions_dir()?.join("removed_accounts.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn is_removed(&self, identifier: &str) -> bool {
        let clean = identifier
            .trim()
            .strip_prefix("riot_")
            .unwrap_or(identifier.trim());
        self.removed_identifiers()
            .unwrap_or_default()
            .iter()
            .any(|id| id.eq_ignore_ascii_case(clean))
    }

    /// Forget SWCH snapshots, including legacy copies, without changing the
    /// installed Riot Client's active login. Remember the removal so discovery
    /// and automatic backups cannot immediately add the account again.
    pub fn remove_session(&self, identifier: &str) -> Result<(), RiotError> {
        let clean = identifier
            .trim()
            .strip_prefix("riot_")
            .unwrap_or(identifier.trim());
        if clean.is_empty() || clean == "." || clean == ".." || clean.contains(['/', '\\']) {
            return Err(RiotError::AccountNotFound(identifier.to_string()));
        }
        let root = self.sessions_dir()?;
        let mut aliases = vec![clean.to_string()];
        if let Ok(dir) = self.resolve_session_dir(identifier) {
            if let Ok(bytes) = fs::read(dir.join("session_manifest.json")) {
                if let Ok(manifest) = serde_json::from_slice::<RiotSessionManifest>(&bytes) {
                    aliases.push(manifest.account_id);
                    aliases.extend(manifest.username);
                    aliases.extend(manifest.display_name);
                }
            }
            if let Some(id) = saved_login(&dir)
                .as_deref()
                .and_then(login_account_id)
            {
                aliases.push(id);
            }
        }
        let mut roots = vec![root.clone()];
        roots.extend(self.legacy_sessions_dir());
        let mut targets = Vec::new();
        for storage_root in roots {
            let canonical_root = storage_root.canonicalize()?;
            for entry in fs::read_dir(&storage_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let path = entry.path();
                if !has_private_settings(&path) && !path.join("session_manifest.json").is_file() {
                    continue;
                }
                let folder = entry.file_name().to_string_lossy().into_owned();
                let manifest = fs::read(path.join("session_manifest.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<RiotSessionManifest>(&b).ok());
                let legacy_name = fs::read(path.join("meta.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                    .and_then(|v| v["name"].as_str().map(String::from));
                let cookie_id = saved_login(&path)
                    .as_deref()
                    .and_then(login_account_id);
                let matches = aliases.iter().any(|alias| {
                    folder.eq_ignore_ascii_case(&sanitize_folder_name(alias))
                        || manifest
                            .as_ref()
                            .is_some_and(|m| m.account_id.eq_ignore_ascii_case(alias))
                        || legacy_name
                            .as_deref()
                            .is_some_and(|name| name.eq_ignore_ascii_case(alias))
                        || cookie_id
                            .as_deref()
                            .is_some_and(|id| id.eq_ignore_ascii_case(alias))
                });
                if matches {
                    let canonical = path.canonicalize()?;
                    if canonical == canonical_root || !canonical.starts_with(&canonical_root) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "Riot snapshot is outside its storage directory",
                        )
                        .into());
                    }
                    targets.push(path);
                }
            }
        }
        for path in targets {
            fs::remove_dir_all(path)?;
        }
        let mut removed = self.removed_identifiers()?;
        removed.extend(aliases);
        removed.sort();
        removed.dedup();
        atomic_write_json(&root.join("removed_accounts.json"), &removed)?;
        Ok(())
    }

    /// Lists all saved Riot session manifests.
    pub fn list_sessions(&self) -> Vec<RiotSessionManifest> {
        let mut list = Vec::new();
        let dir = match self.sessions_dir() {
            Ok(d) => d,
            Err(_) => return list,
        };

        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest_file = path.join("session_manifest.json");
                    if let Ok(data) = fs::read(&manifest_file) {
                        if let Ok(mut m) = serde_json::from_slice::<RiotSessionManifest>(&data) {
                            if self.is_removed(&m.account_id) {
                                continue;
                            }
                            if m.username.is_none() {
                                m.username = m
                                    .display_name
                                    .as_ref()
                                    .map(|d| d.split('#').next().unwrap_or(d).to_string());
                            }
                            list.push(m);
                        }
                    }
                }
            }
        }

        // Also discover legacy sessions from %APPDATA%\swch\riot_accounts and migrate them
        if let Some(legacy) = self.legacy_sessions_dir() {
            if let Ok(entries) = fs::read_dir(&legacy) {
                for entry in entries.flatten() {
                    let leg_path = entry.path();
                    if leg_path.is_dir() && has_private_settings(&leg_path) {
                        let folder_name = entry.file_name().to_string_lossy().to_string();
                        let mut name = folder_name.clone();
                        let meta_file = leg_path.join("meta.json");
                        if let Ok(d) = fs::read_to_string(&meta_file) {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&d) {
                                if let Some(n) = v["name"].as_str().filter(|s| !s.is_empty()) {
                                    name = n.to_string();
                                }
                            }
                        }

                        let already = list.iter().any(|m| {
                            m.account_id.eq_ignore_ascii_case(&folder_name)
                                || m.account_id.eq_ignore_ascii_case(&name)
                                || m.username.as_deref().is_some_and(|u| {
                                    u.eq_ignore_ascii_case(&name)
                                        || u.eq_ignore_ascii_case(&folder_name)
                                })
                                || m.display_name.as_deref().is_some_and(|d| {
                                    d.eq_ignore_ascii_case(&name)
                                        || d.eq_ignore_ascii_case(&folder_name)
                                })
                        });

                        if self.is_removed(&folder_name)
                            || self.is_removed(&name)
                            || saved_login(&leg_path)
                                .as_deref()
                                .and_then(login_account_id)
                                .is_some_and(|id| self.is_removed(&id))
                        {
                            continue;
                        }

                        if !already {
                            let target_dir = dir.join(sanitize_folder_name(&name));
                            let _ = fs::create_dir_all(&target_dir);
                            let _ = atomic_copy(
                                leg_path.join("RiotClientPrivateSettings.yaml"),
                                target_dir.join("RiotClientPrivateSettings.yaml"),
                            );
                            if leg_path.join("RiotGamesPrivateSettings.yaml").is_file() {
                                let _ = atomic_copy(
                                    leg_path.join("RiotGamesPrivateSettings.yaml"),
                                    target_dir.join("RiotGamesPrivateSettings.yaml"),
                                );
                            }
                            if leg_path.join("RiotClientSettings.yaml").is_file() {
                                let _ = atomic_copy(
                                    leg_path.join("RiotClientSettings.yaml"),
                                    target_dir.join("RiotClientSettings.yaml"),
                                );
                            }
                            let manifest = RiotSessionManifest {
                                account_id: name.clone(),
                                username: Some(name.clone()),
                                display_name: Some(name.clone()),
                                backed_up_at: Utc::now().to_rfc3339(),
                                has_games_settings: target_dir
                                    .join("RiotGamesPrivateSettings.yaml")
                                    .is_file(),
                                has_client_settings: target_dir
                                    .join("RiotClientSettings.yaml")
                                    .is_file(),
                            };
                            let _ = atomic_write_json(
                                &target_dir.join("session_manifest.json"),
                                &manifest,
                            );
                            list.push(manifest);
                        }
                    }
                }
            }
        }

        list.sort_by(|a, b| b.backed_up_at.cmp(&a.backed_up_at));
        list
    }

    /// Backs up active Riot Client session files on disk to swch storage.
    pub fn backup_session(
        &self,
        account_identifier: &str,
        username: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<PathBuf, RiotError> {
        let paths = self.paths()?;
        let client_content = fs::read_to_string(&paths.private_settings_yaml).ok();
        let games_content = fs::read_to_string(&paths.games_private_settings_yaml).ok();
        let login = login_settings(client_content.clone(), games_content.clone());
        if !login.as_deref().is_some_and(has_reusable_login) {
            return Err(RiotError::LoginRequired);
        }
        let login_id = login.as_deref().and_then(login_account_id);
        if login_id.as_deref().is_some_and(|id| !id.eq_ignore_ascii_case(account_identifier.strip_prefix("riot_").unwrap_or(account_identifier))) {
            return Err(RiotError::AccountChanged);
        }
        let sessions_root = self.sessions_dir()?;
        let folder_name = sanitize_folder_name(account_identifier);
        let target_dir = self.resolve_session_dir(account_identifier)
            .unwrap_or_else(|_| sessions_root.join(folder_name));
        fs::create_dir_all(&target_dir)?;

        let previous = fs::read(target_dir.join("session_manifest.json")).ok()
            .and_then(|bytes| serde_json::from_slice::<RiotSessionManifest>(&bytes).ok());

        let mut has_games_settings = false;
        let mut has_client_settings = false;

        // 1. Copy RiotClientPrivateSettings.yaml
        let client_id = client_content.as_deref().and_then(login_account_id);
        if let Some(content) = client_content.filter(|_| client_id == login_id) {
            let dest = target_dir.join("RiotClientPrivateSettings.yaml");
            atomic_write_str(&dest, &content)?;
        } else {
            remove_if_present(&target_dir.join("RiotClientPrivateSettings.yaml"))?;
        }

        // 2. Copy RiotGamesPrivateSettings.yaml
        if let Some(content) = games_content {
            let dest = target_dir.join("RiotGamesPrivateSettings.yaml");
            atomic_write_str(&dest, &content)?;
            has_games_settings = true;
        } else {
            remove_if_present(&target_dir.join("RiotGamesPrivateSettings.yaml"))?;
        }

        // 3. Copy RiotClientSettings.yaml
        if paths.client_settings_yaml.is_file() {
            let dest = target_dir.join("RiotClientSettings.yaml");
            if atomic_copy(&paths.client_settings_yaml, &dest).is_ok() {
                has_client_settings = true;
            }
        }

        // 4. Save session manifest
        let manifest = RiotSessionManifest {
            account_id: account_identifier.to_string(),
            username: username.map(String::from).or_else(|| previous.as_ref().and_then(|m| m.username.clone())),
            display_name: display_name.map(String::from).or_else(|| previous.as_ref().and_then(|m| m.display_name.clone())),
            backed_up_at: Utc::now().to_rfc3339(),
            has_games_settings,
            has_client_settings,
        };
        let manifest_path = target_dir.join("session_manifest.json");
        atomic_write_json(&manifest_path, &manifest)?;
        let mut removed = self.removed_identifiers()?;
        removed.retain(|id| {
            !id.eq_ignore_ascii_case(account_identifier)
                && !username.is_some_and(|name| id.eq_ignore_ascii_case(name))
                && !display_name.is_some_and(|name| id.eq_ignore_ascii_case(name))
        });
        atomic_write_json(&sessions_root.join("removed_accounts.json"), &removed)?;

        info!(
            "Backed up Riot session for '{}' to {}",
            account_identifier,
            target_dir.display()
        );

        Ok(target_dir)
    }

    fn resolve_session_dir(&self, account_identifier: &str) -> Result<PathBuf, RiotError> {
        let clean = account_identifier.trim();
        let stripped = clean.strip_prefix("riot_").unwrap_or(clean);
        if stripped.is_empty() || stripped == "." || stripped == ".." {
            return Err(RiotError::AccountNotFound(clean.to_string()));
        }
        let sessions_root = self.sessions_dir()?;

        // Locate session directory
        let direct = sessions_root.join(sanitize_folder_name(clean));
        let direct_stripped = sessions_root.join(sanitize_folder_name(stripped));
        let session_dir = if has_private_settings(&direct) {
            direct
        } else if has_private_settings(&direct_stripped) {
            direct_stripped
        } else {
            let mut found = None;
            if let Ok(entries) = fs::read_dir(&sessions_root) {
                for entry in entries.flatten() {
                    let manifest_file = entry.path().join("session_manifest.json");
                    if let Ok(data) = fs::read(&manifest_file) {
                        if let Ok(m) = serde_json::from_slice::<RiotSessionManifest>(&data) {
                            if m.account_id.eq_ignore_ascii_case(clean)
                                || m.account_id.eq_ignore_ascii_case(stripped)
                                || m.username.as_deref().is_some_and(|u| {
                                    u.eq_ignore_ascii_case(clean)
                                        || u.eq_ignore_ascii_case(stripped)
                                })
                                || m.display_name.as_deref().is_some_and(|d| {
                                    d.eq_ignore_ascii_case(clean)
                                        || d.eq_ignore_ascii_case(stripped)
                                })
                            {
                                found = Some(entry.path());
                                break;
                            }
                        }
                    }
                }
            }
            if found.is_none() {
                if let Some(legacy) = self.legacy_sessions_dir() {
                    let leg_dir = legacy.join(sanitize_folder_name(clean));
                    if has_private_settings(&leg_dir) {
                        found = Some(leg_dir);
                    } else {
                        let leg_strip = legacy.join(sanitize_folder_name(stripped));
                        if has_private_settings(&leg_strip) {
                            found = Some(leg_strip);
                        } else if let Ok(entries) = fs::read_dir(&legacy) {
                            for entry in entries.flatten() {
                                let p = entry.path();
                                if p.is_dir() && has_private_settings(&p) {
                                    let meta = p.join("meta.json");
                                    if let Ok(d) = fs::read_to_string(&meta) {
                                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&d)
                                        {
                                            if let Some(n) = v["name"].as_str() {
                                                if n.eq_ignore_ascii_case(clean)
                                                    || n.eq_ignore_ascii_case(stripped)
                                                {
                                                    found = Some(p);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            found.ok_or_else(|| RiotError::AccountNotFound(clean.to_string()))?
        };

        if !has_private_settings(&session_dir) {
            return Err(RiotError::AccountNotFound(clean.to_string()));
        }
        Ok(session_dir)
    }

    /// Restores a backed-up Riot session to active Riot Client paths.
    pub fn restore_session(&self, account_identifier: &str) -> Result<bool, RiotError> {
        let session_dir = self.resolve_session_dir(account_identifier)?;
        let clean = account_identifier.trim();
        let paths = self.paths()?;
        if !saved_login(&session_dir).as_deref().is_some_and(has_reusable_login) {
            return Err(RiotError::LoginRequired);
        }

        // Ensure target directories exist
        fs::create_dir_all(&paths.data_dir)?;
        fs::create_dir_all(&paths.config_dir)?;

        // 1. Restore RiotClientPrivateSettings.yaml
        let src_private = session_dir.join("RiotClientPrivateSettings.yaml");
        if src_private.is_file() {
            atomic_copy(&src_private, &paths.private_settings_yaml)?;
        } else {
            remove_if_present(&paths.private_settings_yaml)?;
        }

        // 2. Restore RiotGamesPrivateSettings.yaml
        let src_games = session_dir.join("RiotGamesPrivateSettings.yaml");
        if src_games.is_file() {
            atomic_copy(&src_games, &paths.games_private_settings_yaml)?;
        } else {
            remove_if_present(&paths.games_private_settings_yaml)?;
        }

        // 3. Restore RiotClientSettings.yaml if present in backup
        let src_settings = session_dir.join("RiotClientSettings.yaml");
        if src_settings.is_file() {
            let _ = atomic_copy(&src_settings, &paths.client_settings_yaml);
        }

        info!(
            "Restored Riot session for '{}' from {}",
            clean,
            session_dir.display()
        );
        Ok(true)
    }

    /// Performs the full account switch workflow:
    /// 1. Terminate Riot Client processes if active
    /// 2. Backup current session if one is active on disk
    /// 3. Restore target session files
    pub async fn switch_account(
        &self,
        target_account: &str,
        display_name: Option<&str>,
    ) -> Result<SwitchRiotSessionResult, RiotError> {
        let clean_target = target_account.trim();
        let stripped_target = clean_target.strip_prefix("riot_").unwrap_or(clean_target);
        if clean_target.is_empty() {
            return Err(RiotError::AccountNotFound(clean_target.to_string()));
        }

        let is_running = self.is_launcher_running();
        let current_info = self.read_current_account();

        // Riot may have rotated its token while SWCH was closed. Save that
        // before any restore, including a switch to the same account after boot.
        self.refresh_saved_session()?;

        // Check if target is already active
        let is_already_active = current_info.account_id.as_deref().is_some_and(|id| {
            id.eq_ignore_ascii_case(clean_target) || id.eq_ignore_ascii_case(stripped_target)
        }) || current_info.username.as_deref().is_some_and(|u| {
            u.eq_ignore_ascii_case(clean_target) || u.eq_ignore_ascii_case(stripped_target)
        }) || current_info.display_name.as_deref().is_some_and(|d| {
            d.eq_ignore_ascii_case(clean_target) || d.eq_ignore_ascii_case(stripped_target)
        });

        if is_already_active && current_info.has_session && is_running {
            return Ok(SwitchRiotSessionResult {
                target_account_id: clean_target.to_string(),
                display_name: display_name.map(String::from).or(current_info.display_name),
                restored_saved_session: false,
                already_active: true,
                no_saved_session: false,
                message: Some(format!(
                    "Riot Client уже запущен с выбранным аккаунтом '{}'",
                    display_name.unwrap_or(clean_target)
                )),
            });
        }

        let has_saved = self.has_session(clean_target);

        // Safe switch guard: If target has no saved session snapshot and launcher is running,
        // do not kill running processes.
        if !has_saved {
            let name = display_name.unwrap_or(clean_target);
            let msg = format!(
                "Сессия для аккаунта '{}' ещё не сохранена в swch. Выполните вход в Riot Client и нажмите «Сохранить текущую сессию».",
                name
            );
            return Ok(SwitchRiotSessionResult {
                target_account_id: clean_target.to_string(),
                display_name: display_name.map(String::from),
                restored_saved_session: false,
                already_active: false,
                no_saved_session: true,
                message: Some(msg),
            });
        }

        // Validate before stopping the current client or writing any live files.
        let target_dir = self.resolve_session_dir(clean_target)?;
        if !saved_login(&target_dir).as_deref().is_some_and(has_reusable_login) {
            return Err(RiotError::LoginRequired);
        }

        // 1. Stop Riot Client processes
        if is_running && !self.skip_process_termination {
            self.terminate_processes().await?;
        }

        // 2. Backup current session if present on disk and distinct from target
        let current_info = self.read_current_account();
        if current_info.has_session {
            if let Some(ref cur_id) = current_info.account_id {
                if !cur_id.eq_ignore_ascii_case(stripped_target)
                    && !self.is_removed(cur_id)
                    && self.resolve_session_dir(cur_id).ok().as_ref() != Some(&target_dir)
                {
                    self.backup_session(
                        cur_id,
                        current_info.username.as_deref(),
                        current_info.display_name.as_deref(),
                    )?;
                }
            }
        }

        // 3. Restore target session if saved
        let restored = if has_saved {
            self.restore_session(clean_target)?
        } else {
            false
        };

        let message = if restored {
            format!(
                "Сессия Riot Client успешно переключена на '{}'",
                display_name.unwrap_or(clean_target)
            )
        } else {
            format!(
                "Аккаунт '{}' выбран. Запусти Riot Client для входа.",
                display_name.unwrap_or(clean_target)
            )
        };

        Ok(SwitchRiotSessionResult {
            target_account_id: clean_target.to_string(),
            display_name: display_name.map(String::from),
            restored_saved_session: restored,
            already_active: false,
            no_saved_session: !has_saved,
            message: Some(message),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn session_fixture(account: &str) -> String {
        format!("riot-login:\n    persist:\n        session:\n            cookies:\n            -   domain: \"auth.riotgames.com\"\n                name: \"sub\"\n                persistent: true\n                value: \"{account}\"\n            -   domain: \"auth.riotgames.com\"\n                name: \"ssid\"\n                persistent: true\n                value: \"test-session-{account}\"\n")
    }

    fn psl_fixture(account: &str) -> String {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::json!({"sub":account,"uname":"sdk-user","exp":1}).to_string());
        format!("psl:\n    authorization:\n        riot-client:\n            id_token: \"header.{payload}.signature\"\n            refresh_token: \"fixture-refresh\"\n            is_dpop_bound: false\nriot-login:\n    persist: null\n")
    }

    #[tokio::test]
    async fn sdk_refresh_authorization_round_trips_without_stale_cookie_account() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path());
        let paths = mgr.paths().unwrap();
        let modern = psl_fixture("modern");
        fs::write(&paths.private_settings_yaml, session_fixture("old")).unwrap();
        fs::write(&paths.games_private_settings_yaml, &modern).unwrap();
        let current = mgr.read_current_account();
        assert!(current.has_session);
        assert_eq!(current.account_id.as_deref(), Some("modern"));
        assert_eq!(current.username.as_deref(), Some("sdk-user"));
        let saved = mgr.backup_session("modern", None, None).unwrap();
        assert!(!saved.join("RiotClientPrivateSettings.yaml").exists());
        assert_eq!(fs::read_to_string(saved.join("RiotGamesPrivateSettings.yaml")).unwrap(), modern);
        fs::write(&paths.games_private_settings_yaml, psl_fixture("other")).unwrap();
        let result = mgr.switch_account("riot_modern", None).await.unwrap();
        assert!(result.restored_saved_session);
        assert!(mgr.has_session("other"));
        assert!(!paths.private_settings_yaml.exists());
        assert_eq!(mgr.read_current_account().account_id.as_deref(), Some("modern"));
        assert_eq!(fs::read_to_string(&paths.games_private_settings_yaml).unwrap(), modern);
        mgr.remove_session("riot_modern").unwrap();
        assert!(!saved.exists());
    }

    #[test]
    fn sdk_metadata_or_other_product_tokens_are_not_reusable_riot_login() {
        let valid = psl_fixture("modern");
        assert!(has_reusable_login(&valid));
        for invalid in [
            valid.replace("refresh_token: \"fixture-refresh\"", "refresh_token: null"),
            valid.replace("refresh_token: \"fixture-refresh\"", "refresh_token: \"\""),
            valid.replace("id_token:", "cached_metadata:"),
            valid.replace("riot-client:", "another-product:"),
            valid.replace("psl:", "unrelated-plugin:"),
        ] {
            assert!(!has_reusable_login(&invalid));
        }
    }
    #[test]
    fn saving_rotated_sdk_token_does_not_overwrite_another_account() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path());
        let paths = mgr.paths().unwrap();
        let original = psl_fixture("saved");
        fs::write(&paths.games_private_settings_yaml, &original).unwrap();
        let dir = mgr.backup_session("saved", None, None).unwrap();
        let rotated = original.replace("fixture-refresh", "rotated-refresh");
        fs::write(&paths.games_private_settings_yaml, &rotated).unwrap();
        mgr.backup_session("saved", None, None).unwrap();
        assert_eq!(fs::read_to_string(dir.join("RiotGamesPrivateSettings.yaml")).unwrap(), rotated);
        fs::write(&paths.games_private_settings_yaml, psl_fixture("other")).unwrap();
        assert!(matches!(mgr.backup_session("saved", None, None), Err(RiotError::AccountChanged)));
        assert_eq!(fs::read_to_string(dir.join("RiotGamesPrivateSettings.yaml")).unwrap(), rotated);
    }

    fn test_manager(root: &std::path::Path) -> RiotSessionManager {
        let data = root.join("Data");
        let config = root.join("Config");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&config).unwrap();
        RiotSessionManager::with_paths(RiotPaths {
            private_settings_yaml: data.join("RiotClientPrivateSettings.yaml"),
            games_private_settings_yaml: data.join("RiotGamesPrivateSettings.yaml"),
            client_settings_yaml: config.join("RiotClientSettings.yaml"),
            data_dir: data,
            config_dir: config,
        })
        .with_sessions_dir(root.join("sessions"))
        .with_skip_process_termination(true)
    }

    #[tokio::test]
    async fn rotated_login_survives_restart_and_switch_to_same_stopped_account() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path());
        let paths = mgr.paths().unwrap();
        let old = psl_fixture("saved");
        fs::write(&paths.games_private_settings_yaml, &old).unwrap();
        let saved = mgr.backup_session("saved", Some("login"), Some("Player#123")).unwrap();
        // Deterministic ordering independent of the filesystem clock resolution.
        fs::OpenOptions::new().write(true).open(saved.join("RiotGamesPrivateSettings.yaml")).unwrap()
            .set_times(fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))).unwrap();
        let rotated = old.replace("fixture-refresh", "rotated-refresh");
        fs::write(&paths.games_private_settings_yaml, &rotated).unwrap();
        drop(mgr);

        let restarted = test_manager(tmp.path()).with_simulate_running(false);
        let result = restarted.switch_account("riot_saved", None).await.unwrap();
        assert!(result.restored_saved_session);
        assert_eq!(fs::read_to_string(&paths.games_private_settings_yaml).unwrap(), rotated);
        assert_eq!(fs::read_to_string(saved.join("RiotGamesPrivateSettings.yaml")).unwrap(), rotated);
        let manifest: RiotSessionManifest = serde_json::from_slice(&fs::read(saved.join("session_manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest.display_name.as_deref(), Some("Player#123"));
        assert!(!restarted.refresh_saved_session().unwrap(), "Unchanged tokens must not rewrite the snapshot");
        fs::write(&paths.games_private_settings_yaml, psl_fixture("other")).unwrap();
        restarted.switch_account("riot_saved", None).await.unwrap();
        assert_eq!(fs::read_to_string(&paths.games_private_settings_yaml).unwrap(), rotated);
    }

    #[test]
    fn offline_refresh_preserves_removed_signed_out_and_unconnected_accounts() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path()).with_simulate_running(false);
        let paths = mgr.paths().unwrap();
        fs::write(&paths.games_private_settings_yaml, psl_fixture("saved")).unwrap();
        let saved = mgr.backup_session("saved", None, None).unwrap();
        let before = fs::read(saved.join("RiotGamesPrivateSettings.yaml")).unwrap();
        fs::write(&paths.games_private_settings_yaml, "riot-login:\n    persist: null\n").unwrap();
        assert!(!mgr.refresh_saved_session().unwrap());
        assert_eq!(fs::read(saved.join("RiotGamesPrivateSettings.yaml")).unwrap(), before);
        fs::write(&paths.games_private_settings_yaml, psl_fixture("unknown")).unwrap();
        assert!(!mgr.refresh_saved_session().unwrap());
        assert!(!mgr.has_session("unknown"));
        mgr.remove_session("saved").unwrap();
        fs::write(&paths.games_private_settings_yaml, psl_fixture("saved")).unwrap();
        assert!(!mgr.refresh_saved_session().unwrap());
        assert!(!saved.exists());
    }

    #[test]
    fn offline_refresh_keeps_alias_folder_and_newer_saved_token() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path()).with_simulate_running(false);
        let paths = mgr.paths().unwrap();
        let original = psl_fixture("saved");
        fs::write(&paths.games_private_settings_yaml, &original).unwrap();
        let saved = mgr.backup_session("saved", None, Some("Alias#123")).unwrap();
        let alias = saved.with_file_name("legacy-alias");
        fs::rename(saved, &alias).unwrap();
        let newer = original.replace("fixture-refresh", "newer-saved-refresh");
        fs::write(alias.join("RiotGamesPrivateSettings.yaml"), &newer).unwrap();
        fs::OpenOptions::new().write(true).open(&paths.games_private_settings_yaml).unwrap()
            .set_times(fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))).unwrap();
        assert!(!mgr.refresh_saved_session().unwrap());
        assert_eq!(fs::read_to_string(alias.join("RiotGamesPrivateSettings.yaml")).unwrap(), newer);
        fs::OpenOptions::new().write(true).open(alias.join("RiotGamesPrivateSettings.yaml")).unwrap()
            .set_times(fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))).unwrap();
        let rotated = original.replace("fixture-refresh", "latest-live-refresh");
        fs::write(&paths.games_private_settings_yaml, &rotated).unwrap();
        assert!(mgr.refresh_saved_session().unwrap());
        assert_eq!(fs::read_to_string(alias.join("RiotGamesPrivateSettings.yaml")).unwrap(), rotated);
        assert!(!mgr.sessions_dir().unwrap().join("saved").exists());
    }

    #[test]
    fn games_login_takes_precedence_over_stale_client_and_cached_token() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path());
        let paths = mgr.paths().unwrap();
        fs::write(&paths.private_settings_yaml, session_fixture("old")).unwrap();
        fs::write(&paths.games_private_settings_yaml, session_fixture("new")).unwrap();
        let current = mgr.read_current_account();
        assert_eq!(current.account_id.as_deref(), Some("new"));
        assert!(current.has_session);
        fs::write(
            &paths.games_private_settings_yaml,
            "riot-login:\n    persist: null\n",
        )
        .unwrap();
        let current = mgr.read_current_account();
        assert!(current.has_session);
        assert_eq!(current.account_id.as_deref(), Some("old"));
    }

    #[test]
    fn client_login_with_games_token_and_null_persist_can_be_saved() {
        use base64::Engine;
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path());
        let paths = mgr.paths().unwrap();
        fs::write(&paths.private_settings_yaml, session_fixture("current")).unwrap();
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"sub":"current","uname":"current-user"}"#);
        let games = format!(
            "riot-login:\n    persist: null\n    id_token: \"header.{payload}.signature\"\n"
        );
        fs::write(&paths.games_private_settings_yaml, &games).unwrap();
        let current = mgr.read_current_account();
        assert!(current.has_session);
        assert_eq!(current.account_id.as_deref(), Some("current"));
        assert_eq!(current.username.as_deref(), Some("current-user"));
        let saved = mgr.backup_session("current", None, None).unwrap();
        assert_eq!(
            fs::read_to_string(saved.join("RiotGamesPrivateSettings.yaml")).unwrap(),
            games
        );
        let other = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"sub":"other"}"#);
        fs::write(
            &paths.games_private_settings_yaml,
            games.replace(&payload, &other),
        )
        .unwrap();
        assert!(!mgr.read_current_account().has_session);
        assert!(matches!(
            mgr.backup_session("current", None, None),
            Err(RiotError::LoginRequired)
        ));
    }

    #[test]
    fn games_only_snapshot_round_trips_without_old_client_cookies() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path());
        let paths = mgr.paths().unwrap();
        fs::write(&paths.games_private_settings_yaml, session_fixture("new")).unwrap();
        mgr.backup_session("new", None, None).unwrap();
        assert!(mgr.has_session("riot_new"));
        fs::write(&paths.private_settings_yaml, session_fixture("other")).unwrap();
        fs::write(&paths.games_private_settings_yaml, session_fixture("other")).unwrap();
        mgr.restore_session("riot_new").unwrap();
        assert!(!paths.private_settings_yaml.exists());
        assert_eq!(
            mgr.read_current_account().account_id.as_deref(),
            Some("new")
        );
    }

    #[test]
    fn client_only_snapshot_removes_previous_games_login() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path());
        let paths = mgr.paths().unwrap();
        fs::write(&paths.private_settings_yaml, session_fixture("old")).unwrap();
        mgr.backup_session("old", None, None).unwrap();
        fs::write(&paths.games_private_settings_yaml, session_fixture("other")).unwrap();
        mgr.restore_session("old").unwrap();
        assert!(!paths.games_private_settings_yaml.exists());
        assert_eq!(
            mgr.read_current_account().account_id.as_deref(),
            Some("old")
        );
    }

    #[tokio::test]
    async fn cleared_saved_login_is_rejected_before_touching_current_session() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path()).with_simulate_running(true);
        let paths = mgr.paths().unwrap();
        fs::write(&paths.private_settings_yaml, session_fixture("target")).unwrap();
        let saved = mgr.backup_session("target", None, None).unwrap();
        fs::write(
            saved.join("RiotClientPrivateSettings.yaml"),
            "riot-login:\n    persist: null\n",
        )
        .unwrap();
        fs::write(
            saved.join("RiotGamesPrivateSettings.yaml"),
            "riot-login:\n    persist: null\n",
        )
        .unwrap();
        fs::write(&paths.private_settings_yaml, session_fixture("current")).unwrap();
        assert!(matches!(
            mgr.switch_account("target", None).await,
            Err(RiotError::LoginRequired)
        ));
        assert_eq!(
            fs::read_to_string(&paths.private_settings_yaml).unwrap(),
            session_fixture("current")
        );
        assert!(!paths.games_private_settings_yaml.exists());
    }

    #[test]
    fn signed_out_backup_does_not_overwrite_good_snapshot() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path());
        let paths = mgr.paths().unwrap();
        fs::write(
            &paths.games_private_settings_yaml,
            session_fixture("target"),
        )
        .unwrap();
        let saved = mgr.backup_session("target", None, None).unwrap();
        fs::write(
            &paths.games_private_settings_yaml,
            "riot-login:\n    persist: null\n",
        )
        .unwrap();
        assert!(matches!(
            mgr.backup_session("target", None, None),
            Err(RiotError::LoginRequired)
        ));
        assert_eq!(
            fs::read_to_string(saved.join("RiotGamesPrivateSettings.yaml")).unwrap(),
            session_fixture("target")
        );
    }

    #[test]
    fn device_cookies_and_nonpersistent_login_are_not_saved_sessions() {
        assert!(login_cookie("rso-authenticator:\n    tdid:\n        name: ssid\n        persistent: true\n        value: device\n", "ssid").is_none());
        let transient = session_fixture("target").replace("persistent: true", "persistent: false");
        assert!(login_cookie(&transient, "ssid").is_none());
        assert!(login_cookie("riot-login: {}\n", "ssid").is_none());
    }

    #[tokio::test]
    async fn switching_same_account_with_prefix_keeps_saved_snapshot() {
        let tmp = tempdir().unwrap();
        let mgr = test_manager(tmp.path());
        let paths = mgr.paths().unwrap();
        fs::write(
            &paths.games_private_settings_yaml,
            session_fixture("target"),
        )
        .unwrap();
        let saved = mgr.backup_session("target", None, None).unwrap();
        let refreshed =
            session_fixture("target").replace("test-session-target", "refreshed-session");
        fs::write(saved.join("RiotGamesPrivateSettings.yaml"), &refreshed).unwrap();
        mgr.switch_account("riot_target", None).await.unwrap();
        assert_eq!(
            fs::read_to_string(&paths.games_private_settings_yaml).unwrap(),
            refreshed
        );
    }

    #[test]
    fn test_jwt_decode() {
        // A minimal valid JWT with payload {"sub":"12345","uname":"testuser","acct":{"game_name":"Test","tag_line":"RU1"}}
        // Header: eyJhbGciOiJIUzI1NiJ9 ({"alg":"HS256"})
        // Payload: eyJzdWIiOiIxMjM0NSIsInVuYW1lIjoidGVzdHVzZXIiLCJhY2N0Ijp7ImdhbWVfbmFtZSI6IlRlc3QiLCJ0YWdfbGluZSI6IlJUMSJ9fQ
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NSIsInVuYW1lIjoidGVzdHVzZXIiLCJhY2N0Ijp7ImdhbWVfbmFtZSI6IlRlc3QiLCJ0YWdfbGluZSI6IlJUMSJ9fQ.signature";
        let payload = decode_jwt_payload(jwt).expect("Should decode payload");
        assert_eq!(payload["sub"], "12345");
        assert_eq!(payload["uname"], "testuser");
        assert_eq!(payload["acct"]["game_name"], "Test");
        assert_eq!(payload["acct"]["tag_line"], "RT1");
    }

    #[tokio::test]
    async fn test_backup_and_restore_session() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path().join("Data");
        let config_dir = tmp.path().join("Config");
        let sessions_dir = tmp.path().join("sessions");
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let private_yaml = data_dir.join("RiotClientPrivateSettings.yaml");
        let games_yaml = data_dir.join("RiotGamesPrivateSettings.yaml");
        let settings_yaml = config_dir.join("RiotClientSettings.yaml");

        fs::write(&private_yaml, &session_fixture("user1")).unwrap();
        fs::write(&games_yaml, "id_token: \"test.id.token.user1\"\n").unwrap();
        fs::write(&settings_yaml, "locale: en_US\n").unwrap();

        let paths = RiotPaths {
            data_dir: data_dir.clone(),
            config_dir: config_dir.clone(),
            private_settings_yaml: private_yaml.clone(),
            games_private_settings_yaml: games_yaml.clone(),
            client_settings_yaml: settings_yaml.clone(),
        };

        let mgr = RiotSessionManager::with_paths(paths)
            .with_sessions_dir(&sessions_dir)
            .with_skip_process_termination(true);

        // Backup user1
        mgr.backup_session("user1", Some("user1_login"), Some("UserOne#1234"))
            .unwrap();

        assert!(mgr.has_session("user1"));
        assert!(mgr.has_session("UserOne#1234"));

        // Overwrite disk with user2 content
        fs::write(&private_yaml, &session_fixture("user2")).unwrap();
        fs::write(&games_yaml, "id_token: \"test.id.token.user2\"\n").unwrap();

        // Backup user2
        mgr.backup_session("user2", Some("user2_login"), Some("UserTwo#5678"))
            .unwrap();
        assert!(mgr.has_session("user2"));

        let sessions = mgr.list_sessions();
        assert_eq!(sessions.len(), 2);

        // Restore user1
        mgr.restore_session("user1").unwrap();
        assert_eq!(
            fs::read_to_string(&private_yaml).unwrap(),
            session_fixture("user1")
        );
        assert_eq!(
            fs::read_to_string(&games_yaml).unwrap(),
            "id_token: \"test.id.token.user1\"\n"
        );
    }

    #[tokio::test]
    async fn test_switch_account_safe_no_saved_session() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path().join("Data");
        let config_dir = tmp.path().join("Config");
        let sessions_dir = tmp.path().join("sessions");
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let paths = RiotPaths {
            data_dir: data_dir.clone(),
            config_dir: config_dir.clone(),
            private_settings_yaml: data_dir.join("RiotClientPrivateSettings.yaml"),
            games_private_settings_yaml: data_dir.join("RiotGamesPrivateSettings.yaml"),
            client_settings_yaml: config_dir.join("RiotClientSettings.yaml"),
        };

        let mgr = RiotSessionManager::with_paths(paths)
            .with_sessions_dir(&sessions_dir)
            .with_simulate_running(true);

        let res = mgr
            .switch_account("unknown_account", Some("Unknown"))
            .await
            .unwrap();

        assert!(res.no_saved_session);
        assert!(!res.restored_saved_session);
        assert!(res.message.unwrap().contains("ещё не сохранена"));
    }
}
