//! Session management and account switching for HoYoverse / miHoYo games.
use super::{
    find_game_def, registry, HoyoverseError, HoyoverseGameDef, LOGIN_VALUE_NAMES,
    SHARED_SDK_SUBKEY,
};
use crate::storage::{atomic::atomic_write, Account};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

const MAX_SNAPSHOT_FILE: u64 = 32 * 1024 * 1024;

/// Serialized session snapshot persisted in `~/.swch/hoyoverse/sessions/<account_id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoyoverseSnapshot {
    pub account: Account,
    pub game_id: String,
    pub captured_at: String,
    /// Hive\Subkey -> (ValueName -> RegistryEntry)
    pub registry_entries: BTreeMap<String, BTreeMap<String, registry::RegistryEntry>>,
    /// File path -> base64 file content (for HoYoPlay app configs)
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct HoyoverseSessionManager {
    pub sessions_dir: PathBuf,
}

impl Default for HoyoverseSessionManager {
    fn default() -> Self {
        let root = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".swch"));
        Self {
            sessions_dir: std::env::var_os("SWCH_HOYOVERSE_SESSIONS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("swch/hoyoverse/sessions")),
        }
    }
}

impl HoyoverseSessionManager {
    pub fn new(sessions_dir: PathBuf) -> Self {
        Self { sessions_dir }
    }

    pub fn path(&self, account_id: &str) -> PathBuf {
        let clean = account_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        self.sessions_dir.join(format!("{clean}.json"))
    }

    /// Closes any running processes for the target game (or all HoYoverse processes).
    pub fn close_processes(&self, game_id: Option<&str>) -> Result<(), HoyoverseError> {
        if registry::mock_registry_path().is_some()
            || std::env::var_os("SWCH_HOYOVERSE_MOCK_GAMES").is_some()
        {
            return Ok(());
        }

        let opts = crate::sys::process::TerminateOptions::default();
        if let Some(gid) = game_id {
            if let Some(def) = find_game_def(gid) {
                let _ = crate::sys::process::terminate_by_names_blocking(def.process_names, &opts);
                return Ok(());
            }
        }

        let _ = crate::sys::process::terminate_launcher_blocking(
            crate::sys::process::LauncherType::Hoyoverse,
            &opts,
        );
        Ok(())
    }

    /// Captures the current logged-in session for a specific game and persists it locally.
    pub fn save_current(
        &self,
        game_id: &str,
        display_name: Option<&str>,
    ) -> Result<Account, HoyoverseError> {
        let def = find_game_def(game_id).ok_or_else(|| {
            HoyoverseError::Invalid(format!("Неизвестная игра HoYoverse: {game_id}"))
        })?;

        let mut registry_entries = BTreeMap::new();
        let mut has_login_data = false;

        // 1. Capture keys from game registry subkeys
        for subkey in def.registry_subkeys {
            let key_id = format!("HKCU\\{subkey}");
            let values = registry::read_key_values("HKCU", subkey)?;
            if !values.is_empty() {
                for name in LOGIN_VALUE_NAMES {
                    if values.contains_key(*name) {
                        has_login_data = true;
                        break;
                    }
                }
                registry_entries.insert(key_id, values);
            }
        }

        // 2. Capture shared SDK values
        let sdk_key_id = format!("HKCU\\{SHARED_SDK_SUBKEY}");
        let sdk_values = registry::read_key_values("HKCU", SHARED_SDK_SUBKEY)?;
        if !sdk_values.is_empty() {
            for name in LOGIN_VALUE_NAMES {
                if sdk_values.contains_key(*name) {
                    has_login_data = true;
                    break;
                }
            }
            registry_entries.insert(sdk_key_id, sdk_values);
        }

        // 3. For HoYoPlay, also capture app config files if present
        let mut files = BTreeMap::new();
        if def.id == "hoyoplay" {
            files = self.capture_hoyoplay_files()?;
            if !files.is_empty() {
                has_login_data = true;
            }
        }

        if !has_login_data && registry_entries.is_empty() && files.is_empty() {
            return Err(HoyoverseError::Invalid(
                "Нет активных данных входа HoYoverse. Запустите игру/клиент и авторизуйтесь.".into(),
            ));
        }

        // 4. Resolve UID or stable account identifier
        let uid = extract_uid_from_session(def, &registry_entries)
            .unwrap_or_else(|| generate_fallback_uid(&registry_entries, &files));

        let account_id = format!("hoyoverse_{}_{}", def.id, uid);
        let username = uid.clone();
        let account_display = display_name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{} ({})", def.display_name, uid));

        // 5. Build snapshot & account model
        let now = chrono::Utc::now().to_rfc3339();
        let account = Account {
            id: account_id.clone(),
            launcher: "hoyoverse".to_string(),
            username,
            display_name: Some(account_display),
            email: None,
            is_active: true,
            last_used: Some(now.clone()),
        };

        let snapshot = HoyoverseSnapshot {
            account: account.clone(),
            game_id: def.id.to_string(),
            captured_at: now,
            registry_entries,
            files,
        };

        // 6. Ensure directory and mark previous accounts for this game as inactive
        std::fs::create_dir_all(&self.sessions_dir)?;
        self.deactivate_other_game_accounts(def.id, &account_id)?;

        // 7. Write atomically
        let path = self.path(&account_id);
        let bytes = serde_json::to_vec_pretty(&snapshot)?;
        atomic_write(&path, bytes)?;

        Ok(account)
    }

    /// Restores a previously saved HoYoverse account session with automatic rollback on error.
    pub fn restore(&self, account_id: &str) -> Result<Account, HoyoverseError> {
        let path = self.find_snapshot_path(account_id)?;
        if std::fs::metadata(&path)?.len() > MAX_SNAPSHOT_FILE {
            return Err(HoyoverseError::Invalid("Файл сессии слишком большой".into()));
        }

        let bytes = std::fs::read(&path)?;
        let mut snapshot: HoyoverseSnapshot = serde_json::from_slice(&bytes)?;

        let def = find_game_def(&snapshot.game_id).ok_or_else(|| {
            HoyoverseError::Invalid(format!("Неизвестная игра: {}", snapshot.game_id))
        })?;

        // 1. Terminate running game processes before writing registry
        self.close_processes(Some(def.id))?;

        // 2. Capture current state for ROLLBACK in case of write failure
        let backup = self.capture_backup_for_keys(&snapshot.registry_entries);

        // 3. Write registry entries with rollback safety
        if let Err(err) = self.apply_registry_entries(&snapshot.registry_entries) {
            // ROLLBACK
            let _ = self.apply_registry_entries(&backup);
            return Err(err);
        }

        // 4. Restore files if HoYoPlay
        if !snapshot.files.is_empty() {
            if let Err(err) = self.restore_hoyoplay_files(&snapshot.files) {
                let _ = self.apply_registry_entries(&backup);
                return Err(err);
            }
        }

        // 5. Update active state across saved sessions for this game
        self.deactivate_other_game_accounts(def.id, &snapshot.account.id)?;

        let now = chrono::Utc::now().to_rfc3339();
        snapshot.account.is_active = true;
        snapshot.account.last_used = Some(now.clone());

        let updated_bytes = serde_json::to_vec_pretty(&snapshot)?;
        atomic_write(&path, updated_bytes)?;

        Ok(snapshot.account)
    }

    /// Clears login tokens from registry to allow a fresh login to another account.
    pub fn prepare_login(&self, game_id: &str) -> Result<(), HoyoverseError> {
        let def = find_game_def(game_id).ok_or_else(|| {
            HoyoverseError::Invalid(format!("Неизвестная игра HoYoverse: {game_id}"))
        })?;

        // Terminate processes
        self.close_processes(Some(def.id))?;

        // Clear login values from game keys
        for subkey in def.registry_subkeys {
            for val_name in LOGIN_VALUE_NAMES {
                let _ = registry::delete_value("HKCU", subkey, val_name);
            }
            if let Some(uid_key) = def.uid_key_name {
                let _ = registry::delete_value("HKCU", subkey, uid_key);
            }
        }

        // Clear login values from shared SDK
        for val_name in LOGIN_VALUE_NAMES {
            let _ = registry::delete_value("HKCU", SHARED_SDK_SUBKEY, val_name);
        }

        // Clear active flag on all accounts for this game
        self.deactivate_all_game_accounts(def.id)?;

        Ok(())
    }

    /// Returns all saved accounts, optionally filtered by `game_id`.
    pub fn accounts(&self, game_id: Option<&str>) -> Result<Vec<Account>, HoyoverseError> {
        if !self.sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut list = Vec::new();
        let entries = std::fs::read_dir(&self.sessions_dir)?;

        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |ext| ext == "json") {
                if let Ok(bytes) = std::fs::read(&p) {
                    if let Ok(snap) = serde_json::from_slice::<HoyoverseSnapshot>(&bytes) {
                        if let Some(gid) = game_id {
                            if !snap.game_id.eq_ignore_ascii_case(gid) {
                                continue;
                            }
                        }
                        list.push(snap.account);
                    }
                }
            }
        }

        // Sort: active first, then last_used descending
        list.sort_by(|a, b| {
            b.is_active
                .cmp(&a.is_active)
                .then_with(|| b.last_used.cmp(&a.last_used))
        });

        Ok(list)
    }

    /// Returns the currently active account ID for a game, if detected in registry.
    pub fn current_id(&self, game_id: &str) -> Option<String> {
        let def = find_game_def(game_id)?;

        let mut entries = BTreeMap::new();
        for subkey in def.registry_subkeys {
            if let Ok(vals) = registry::read_key_values("HKCU", subkey) {
                if !vals.is_empty() {
                    entries.insert(format!("HKCU\\{subkey}"), vals);
                }
            }
        }

        if let Ok(vals) = registry::read_key_values("HKCU", SHARED_SDK_SUBKEY) {
            if !vals.is_empty() {
                entries.insert(format!("HKCU\\{SHARED_SDK_SUBKEY}"), vals);
            }
        }

        let uid = extract_uid_from_session(def, &entries)?;
        Some(format!("hoyoverse_{}_{}", def.id, uid))
    }

    /// Removes a saved account snapshot from disk.
    pub fn remove(&self, account_id: &str) -> Result<(), HoyoverseError> {
        let path = self.find_snapshot_path(account_id)?;
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    // --- Internal Helpers ---

    fn find_snapshot_path(&self, account_id: &str) -> Result<PathBuf, HoyoverseError> {
        let direct = self.path(account_id);
        if direct.exists() {
            return Ok(direct);
        }

        // Try stripping hoyoverse_ prefix or fuzzy search
        if !self.sessions_dir.exists() {
            return Err(HoyoverseError::Invalid("Аккаунт не найден".into()));
        }

        let entries = std::fs::read_dir(&self.sessions_dir)?;
        for entry in entries.flatten() {
            let p = entry.path();
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                if stem.eq_ignore_ascii_case(account_id)
                    || stem.ends_with(account_id)
                    || account_id.ends_with(stem)
                {
                    return Ok(p);
                }
            }
        }

        Err(HoyoverseError::Invalid(format!(
            "Сессия аккаунта '{account_id}' не найдена"
        )))
    }

    fn capture_backup_for_keys(
        &self,
        targets: &BTreeMap<String, BTreeMap<String, registry::RegistryEntry>>,
    ) -> BTreeMap<String, BTreeMap<String, registry::RegistryEntry>> {
        let mut backup = BTreeMap::new();
        for key_path in targets.keys() {
            if let Some((hive, subkey)) = key_path.split_once('\\') {
                if let Ok(current_values) = registry::read_key_values(hive, subkey) {
                    backup.insert(key_path.clone(), current_values);
                }
            }
        }
        backup
    }

    fn apply_registry_entries(
        &self,
        entries: &BTreeMap<String, BTreeMap<String, registry::RegistryEntry>>,
    ) -> Result<(), HoyoverseError> {
        for (key_path, values) in entries {
            let (hive, subkey) = key_path.split_once('\\').ok_or_else(|| {
                HoyoverseError::Invalid(format!("Некорректный путь реестра: {key_path}"))
            })?;

            for (val_name, entry) in values {
                registry::write_value(hive, subkey, val_name, entry)?;
            }
        }
        Ok(())
    }

    fn deactivate_other_game_accounts(
        &self,
        game_id: &str,
        except_account_id: &str,
    ) -> Result<(), HoyoverseError> {
        if !self.sessions_dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&self.sessions_dir)?.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |ext| ext == "json") {
                if let Ok(bytes) = std::fs::read(&p) {
                    if let Ok(mut snap) = serde_json::from_slice::<HoyoverseSnapshot>(&bytes) {
                        if snap.game_id.eq_ignore_ascii_case(game_id)
                            && snap.account.id != except_account_id
                            && snap.account.is_active
                        {
                            snap.account.is_active = false;
                            let _ = atomic_write(&p, serde_json::to_vec_pretty(&snap)?);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn deactivate_all_game_accounts(&self, game_id: &str) -> Result<(), HoyoverseError> {
        if !self.sessions_dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&self.sessions_dir)?.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |ext| ext == "json") {
                if let Ok(bytes) = std::fs::read(&p) {
                    if let Ok(mut snap) = serde_json::from_slice::<HoyoverseSnapshot>(&bytes) {
                        if snap.game_id.eq_ignore_ascii_case(game_id) && snap.account.is_active {
                            snap.account.is_active = false;
                            let _ = atomic_write(&p, serde_json::to_vec_pretty(&snap)?);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn capture_hoyoplay_files(&self) -> Result<BTreeMap<String, String>, HoyoverseError> {
        let mut map = BTreeMap::new();
        let appdata = dirs::data_dir().or_else(dirs::config_dir);
        if let Some(root) = appdata {
            for vendor in ["Cognosphere", "miHoYo"] {
                let dir = root.join(vendor).join("HYP");
                if dir.is_dir() {
                    for filename in ["config.ini", "user.json", "launcher_info.json"] {
                        let path = dir.join(filename);
                        if let Ok(bytes) = std::fs::read(&path) {
                            if bytes.len() <= 2 * 1024 * 1024 {
                                let key = format!("{vendor}/HYP/{filename}");
                                map.insert(key, STANDARD.encode(bytes));
                            }
                        }
                    }
                }
            }
        }
        Ok(map)
    }

    fn restore_hoyoplay_files(
        &self,
        files: &BTreeMap<String, String>,
    ) -> Result<(), HoyoverseError> {
        let appdata = dirs::data_dir().or_else(dirs::config_dir);
        let Some(root) = appdata else { return Ok(()) };

        for (rel, b64) in files {
            if let Ok(bytes) = STANDARD.decode(b64) {
                let path = root.join(rel);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = atomic_write(&path, bytes);
            }
        }
        Ok(())
    }
}

/// Extracts UID from parsed registry values according to game-specific structures.
pub fn extract_uid_from_session(
    def: &HoyoverseGameDef,
    entries: &BTreeMap<String, BTreeMap<String, registry::RegistryEntry>>,
) -> Option<String> {
    // 1. Check game-specific UID key in game subkeys
    if let Some(uid_key) = def.uid_key_name {
        for subkey in def.registry_subkeys {
            let key_id = format!("HKCU\\{subkey}");
            if let Some(vals) = entries.get(&key_id) {
                if let Some(entry) = vals.get(uid_key) {
                    if let Some(uid) = parse_entry_as_uid(entry) {
                        return Some(uid);
                    }
                }
            }
        }
    }

    // 2. Search SDK tokens in game subkeys and shared SDK subkey
    for key_path in entries.keys() {
        if let Some(vals) = entries.get(key_path) {
            for sdk_token_key in [
                "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
                "MIHOYOSDK_ADL_PROD_CN_h3123967166",
                "GENERAL_DATA_h2389025596",
            ] {
                if let Some(entry) = vals.get(sdk_token_key) {
                    if let Some(bytes) = entry.as_bytes() {
                        let text = String::from_utf8_lossy(bytes);
                        if let Some(uid) = extract_account_id_from_sdk_json(&text) {
                            return Some(uid);
                        }
                    } else if let Some(text) = entry.as_str() {
                        if let Some(uid) = extract_account_id_from_sdk_json(text) {
                            return Some(uid);
                        }
                    }
                }
            }
        }
    }

    None
}

fn parse_entry_as_uid(entry: &registry::RegistryEntry) -> Option<String> {
    match entry {
        registry::RegistryEntry::DWord(d) if *d > 0 => Some(d.to_string()),
        registry::RegistryEntry::QWord(q) if *q > 0 => Some(q.to_string()),
        registry::RegistryEntry::String(s) => {
            let clean: String = s.trim_matches('\0').trim().to_string();
            if !clean.is_empty() {
                Some(clean)
            } else {
                None
            }
        }
        registry::RegistryEntry::Binary(bytes) => {
            let text = String::from_utf8_lossy(bytes);
            let clean = text.trim_matches('\0').trim();
            if clean.bytes().all(|b| b.is_ascii_digit()) && !clean.is_empty() {
                Some(clean.to_string())
            } else {
                extract_account_id_from_sdk_json(clean)
            }
        }
        _ => None,
    }
}

fn extract_account_id_from_sdk_json(text: &str) -> Option<String> {
    // 1. Direct JSON parse
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(id) = val.get("account_id").or_else(|| val.get("uid")) {
            if let Some(s) = id.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
            if let Some(n) = id.as_i64() {
                if n > 0 {
                    return Some(n.to_string());
                }
            }
        }
        // Sub-object or string in "data"
        if let Some(data_val) = val.get("data") {
            if let Some(inner_str) = data_val.as_str() {
                if let Some(uid) = extract_account_id_from_sdk_json(inner_str) {
                    return Some(uid);
                }
            } else if let Some(id) = data_val.get("account_id").or_else(|| data_val.get("uid")) {
                if let Some(s) = id.as_str() {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
                if let Some(n) = id.as_i64() {
                    if n > 0 {
                        return Some(n.to_string());
                    }
                }
            }
        }
    }

    // 2. Pattern matching fallback
    for marker in [
        "\"account_id\":",
        "\"account_id\" :",
        "\"uid\":",
        "\"uid\" :",
        "\"user_id\":",
    ] {
        if let Some(idx) = text.find(marker) {
            let tail = text[idx + marker.len()..].trim_start();
            let digits: String = tail
                .chars()
                .skip_while(|c| *c == '"')
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !digits.is_empty() && digits != "0" {
                return Some(digits);
            }
        }
    }

    None
}

fn generate_fallback_uid(
    entries: &BTreeMap<String, BTreeMap<String, registry::RegistryEntry>>,
    files: &BTreeMap<String, String>,
) -> String {
    let mut hasher = Sha256::new();
    for (key, vals) in entries {
        hasher.update(key.as_bytes());
        for (vname, ventry) in vals {
            hasher.update(vname.as_bytes());
            if let Some(b) = ventry.as_bytes() {
                hasher.update(b);
            } else if let Some(s) = ventry.as_str() {
                hasher.update(s.as_bytes());
            } else if let Some(u) = ventry.as_u64() {
                hasher.update(&u.to_le_bytes());
            }
        }
    }
    for (fname, fcontent) in files {
        hasher.update(fname.as_bytes());
        hasher.update(fcontent.as_bytes());
    }
    let hash = hasher.finalize();
    let hex: String = hash[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("id_{hex}")
}
