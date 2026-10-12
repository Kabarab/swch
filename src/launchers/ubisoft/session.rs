use super::UbisoftError;
use crate::storage::{atomic::atomic_write, Account};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

const FILES: [&str; 3] = ["user.dat", "ConnectSecureStorage.dat", "settings.yaml"];
const MAX_FILE: u64 = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Snapshot {
    account: Account,
    files: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct SessionManager {
    pub data_dir: PathBuf,
    pub launcher_dir: Option<PathBuf>,
    pub sessions_dir: PathBuf,
}

pub fn account_uuid(id: &str) -> Result<String, UbisoftError> {
    let id = id.strip_prefix("ubisoft_").unwrap_or(id);
    if id.len() != 36
        || !id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return Err(UbisoftError::Invalid(
            "Некорректный ID аккаунта Ubisoft".into(),
        ));
    }
    Ok(id.to_ascii_lowercase())
}

pub fn user_from_log(log: &str) -> Option<String> {
    for line in log.lines().rev() {
        // A previous process's login does not authenticate a new login window.
        if line.contains("-- Starting Ubisoft Game Launcher --") {
            return None;
        }
        if let Some((_, tail)) = line.split_once("User:") {
            if let Some(id) = tail
                .split_whitespace()
                .next()
                .and_then(|id| account_uuid(id).ok())
            {
                return Some(id);
            }
        }
    }
    None
}

pub fn user_from_process_log(log: &str, pids: &[u32]) -> Option<String> {
    let lines = log
        .lines()
        .filter(|line| {
            line.trim_start()
                .strip_prefix('[')
                .and_then(|s| s.split_once(']'))
                .and_then(|(pid, _)| pid.trim().parse::<u32>().ok())
                .is_some_and(|pid| pids.contains(&pid))
        })
        .collect::<Vec<_>>()
        .join("\n");
    user_from_log(&lines)
}

/// Remove only legacy login fields; keep language, paths and other preferences.
fn settings_for_login(bytes: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    let mut masters = false;
    text.split_inclusive('\n')
        .map(|line| {
            let trimmed = line.trim();
            if !line.starts_with(char::is_whitespace) && !trimmed.is_empty() {
                masters = trimmed == "masters:";
            }
            if masters && (trimmed.starts_with("username:") || trimmed.starts_with("password:")) {
                if let Some((key, _)) = line.split_once(':') {
                    return format!(
                        "{key}: \"\"{}",
                        if line.ends_with("\r\n") {
                            "\r\n"
                        } else if line.ends_with('\n') {
                            "\n"
                        } else {
                            ""
                        }
                    );
                }
            }
            line.to_string()
        })
        .collect::<String>()
        .into_bytes()
}

impl Default for SessionManager {
    fn default() -> Self {
        let root = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".swch"));
        Self {
            data_dir: std::env::var_os("SWCH_UBISOFT_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    dirs::data_local_dir()
                        .unwrap_or_else(|| root.clone())
                        .join("Ubisoft Game Launcher")
                }),
            launcher_dir: super::launcher_dir(),
            sessions_dir: std::env::var_os("SWCH_UBISOFT_SESSIONS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("swch/ubisoft/sessions")),
        }
    }
}

impl SessionManager {
    fn path(&self, id: &str) -> Result<PathBuf, UbisoftError> {
        Ok(self
            .sessions_dir
            .join(format!("{}.json", account_uuid(id)?)))
    }

    fn read(&self, id: &str) -> Result<Snapshot, UbisoftError> {
        let path = self.path(id)?;
        if std::fs::metadata(&path)?.len() > MAX_FILE * 5 {
            return Err(UbisoftError::Invalid(
                "Слишком большой файл сессии Ubisoft".into(),
            ));
        }
        let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(path)?)?;
        if snapshot.account.launcher != "ubisoft"
            || account_uuid(&snapshot.account.id)? != account_uuid(id)?
        {
            return Err(UbisoftError::Invalid(
                "Сессия принадлежит другому аккаунту".into(),
            ));
        }
        Self::decode(&snapshot)?;
        Ok(snapshot)
    }

    fn decode(snapshot: &Snapshot) -> Result<BTreeMap<String, Vec<u8>>, UbisoftError> {
        let mut decoded = BTreeMap::new();
        for (name, content) in &snapshot.files {
            if !FILES.contains(&name.as_str()) || content.len() as u64 > MAX_FILE * 2 {
                return Err(UbisoftError::Invalid(
                    "Некорректный файл в сессии Ubisoft".into(),
                ));
            }
            let bytes = STANDARD
                .decode(content)
                .map_err(|_| UbisoftError::Invalid("Повреждены данные сессии Ubisoft".into()))?;
            if bytes.len() as u64 > MAX_FILE {
                return Err(UbisoftError::Invalid(
                    "Слишком большой файл сессии Ubisoft".into(),
                ));
            }
            decoded.insert(name.clone(), bytes);
        }
        if !["user.dat", "ConnectSecureStorage.dat"]
            .iter()
            .any(|name| decoded.get(*name).is_some_and(|bytes| !bytes.is_empty()))
        {
            return Err(UbisoftError::Invalid("Нет сохранённых данных входа. Войди в Ubisoft Connect с включённым «Запомнить меня».".into()));
        }
        Ok(decoded)
    }

    fn current_files(&self) -> Result<BTreeMap<String, Vec<u8>>, UbisoftError> {
        let mut files = BTreeMap::new();
        for name in FILES {
            let path = self.data_dir.join(name);
            match std::fs::metadata(&path) {
                Ok(meta) if meta.len() <= MAX_FILE => {
                    files.insert(name.into(), std::fs::read(path)?);
                }
                Ok(_) => {
                    return Err(UbisoftError::Invalid(
                        "Слишком большой файл данных Ubisoft".into(),
                    ))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(files)
    }

    /// A successful login in the latest client process, not historical cache data.
    pub fn authenticated_id(&self) -> Option<String> {
        if !self
            .current_files()
            .ok()?
            .iter()
            .any(|(name, bytes)| name != "settings.yaml" && !bytes.is_empty())
        {
            return None;
        }
        let log = self.launcher_dir.as_ref()?.join("logs/launcher_log.txt");
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(log).ok()?;
        let size = file.metadata().ok()?.len();
        file.seek(SeekFrom::Start(size.saturating_sub(1024 * 1024)))
            .ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        let log = String::from_utf8_lossy(&bytes);
        let system = sysinfo::System::new_all();
        let pids: Vec<_> = self
            .client_processes(&system)
            .iter()
            .filter(|p| is_main_client(&p.name))
            .map(|p| p.pid)
            .collect();
        let id = if pids.is_empty() {
            user_from_log(&log)
        } else {
            user_from_process_log(&log, &pids)
        };
        id.map(|id| format!("ubisoft_{id}"))
    }

    fn client_processes(&self, system: &sysinfo::System) -> Vec<crate::sys::process::ProcessInfo> {
        let Some(dir) = self
            .launcher_dir
            .as_ref()
            .and_then(|d| d.canonicalize().ok())
        else {
            return Vec::new();
        };
        crate::sys::process::find_processes_by_names_exact(
            system,
            &[
                "upc.exe",
                "UbisoftConnect.exe",
                "Uplay.exe",
                "UplayService.exe",
                "UplayWebCore.exe",
            ],
        )
        .into_iter()
        .filter(|p| {
            p.exe
                .as_ref()
                .and_then(|p| p.canonicalize().ok())
                .is_some_and(|p| p.starts_with(&dir))
        })
        .collect()
    }

    pub fn current_id(&self) -> Option<String> {
        let live_auth: BTreeMap<_, _> = self
            .current_files()
            .ok()?
            .into_iter()
            .filter(|(name, _)| name != "settings.yaml")
            .collect();
        if !live_auth.values().any(|bytes| !bytes.is_empty()) {
            return None;
        }
        let system = sysinfo::System::new_all();
        if self
            .client_processes(&system)
            .iter()
            .any(|p| is_main_client(&p.name))
        {
            return self.authenticated_id();
        }
        let mut matching = Vec::new();
        // A restored session is authoritative until the client writes a fresh login.
        if let Ok(entries) = std::fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if let Ok(snapshot) = self.read(id) {
                    let saved_auth: BTreeMap<_, _> = Self::decode(&snapshot)
                        .ok()?
                        .into_iter()
                        .filter(|(name, _)| name != "settings.yaml")
                        .collect();
                    if saved_auth == live_auth {
                        matching.push(snapshot.account.id);
                    }
                }
            }
        }
        if matching.len() == 1 {
            return matching.pop();
        }
        self.authenticated_id()
    }

    pub fn accounts(&self) -> Result<Vec<Account>, UbisoftError> {
        let entries = match std::fs::read_dir(&self.sessions_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let current = self.current_id();
        let mut accounts = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                match self.read(id) {
                    Ok(snapshot) => {
                        let mut account = snapshot.account;
                        account.is_active = current.as_deref() == Some(&account.id);
                        accounts.push(account);
                    }
                    Err(e) => tracing::warn!("Could not read Ubisoft session {}: {}", id, e),
                }
            }
        }
        accounts.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(accounts)
    }

    /// Refresh public profile metadata only; never replace stored or live login data.
    pub fn refresh_names(&self) -> Result<(), UbisoftError> {
        for account in self.accounts()? {
            let Some(name) = super::profile::cached_name(&self.data_dir, &account.id) else {
                continue;
            };
            let mut snapshot = self.read(&account.id)?;
            let uuid = account_uuid(&account.id)?;
            let fallback = format!("Ubisoft {}", &uuid[..8]);
            let replace_label = snapshot
                .account
                .display_name
                .as_deref()
                .is_none_or(|label| label.is_empty() || label == fallback || label == uuid);
            let changed = snapshot.account.username != name
                || (replace_label && snapshot.account.display_name.as_deref() != Some(&name));
            if !changed {
                continue;
            }
            snapshot.account.username = name.clone();
            if replace_label {
                snapshot.account.display_name = Some(name);
            }
            atomic_write(self.path(&account.id)?, serde_json::to_vec(&snapshot)?)?;
        }
        Ok(())
    }

    /// Call after closing the client so its files are flushed and stable.
    pub fn save_current(&self, display_name: Option<&str>) -> Result<Account, UbisoftError> {
        let id = self.authenticated_id().ok_or_else(|| {
            UbisoftError::Invalid(
                "Войди в Ubisoft Connect с включённым «Запомнить меня», затем сохрани сессию."
                    .into(),
            )
        })?;
        let old = self.read(&id).ok();
        let detected_name = super::profile::cached_name(&self.data_dir, &id);
        let fallback = format!("Ubisoft {}", &account_uuid(&id)?[..8]);
        let name = display_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                old.as_ref()
                    .and_then(|s| s.account.display_name.clone())
                    .filter(|s| s != &fallback)
            })
            .or_else(|| detected_name.clone())
            .unwrap_or(fallback);
        if name.len() > 200 {
            return Err(UbisoftError::Invalid(
                "Название аккаунта слишком длинное".into(),
            ));
        }
        let account = Account {
            id: id.clone(),
            launcher: "ubisoft".into(),
            username: detected_name
                .or_else(|| old.as_ref().map(|s| s.account.username.clone()))
                .unwrap_or(account_uuid(&id)?),
            display_name: Some(name),
            email: None,
            is_active: true,
            last_used: Some(chrono::Utc::now().to_rfc3339()),
        };
        let snapshot = Snapshot {
            account: account.clone(),
            files: self
                .current_files()?
                .into_iter()
                .map(|(key, bytes)| (key, STANDARD.encode(bytes)))
                .collect(),
        };
        Self::decode(&snapshot)?;
        std::fs::create_dir_all(&self.sessions_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.sessions_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        atomic_write(self.path(&id)?, serde_json::to_vec(&snapshot)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(self.path(&id)?, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(account)
    }

    /// Validate everything before changing any live file; roll back partial writes.
    pub fn restore(&self, id: &str) -> Result<Account, UbisoftError> {
        let snapshot = self.read(id)?;
        let target = Self::decode(&snapshot)?;
        let previous = self.current_files()?;
        let result = self.write_files(&target);
        if let Err(error) = result {
            self.write_files(&previous).map_err(|rollback| {
                UbisoftError::Invalid(format!(
                    "{error}; ошибка восстановления исходной сессии: {rollback}"
                ))
            })?;
            return Err(error);
        }
        let mut account = snapshot.account;
        account.is_active = true;
        account.last_used = Some(chrono::Utc::now().to_rfc3339());
        Ok(account)
    }

    fn write_files(&self, files: &BTreeMap<String, Vec<u8>>) -> Result<(), UbisoftError> {
        std::fs::create_dir_all(&self.data_dir)?;
        for name in FILES {
            let path = self.data_dir.join(name);
            if let Some(bytes) = files.get(name) {
                atomic_write(path, bytes)?;
            } else {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Ok(())
    }

    pub fn validate_saved(&self, id: &str) -> Result<(), UbisoftError> {
        self.read(id).map(|_| ())
    }

    /// Call with the client stopped and its authenticated session saved first.
    /// Detach local remembered login without invoking Ubisoft's sign-out API.
    pub fn prepare_login(&self) -> Result<(), UbisoftError> {
        let previous = self.current_files()?;
        let mut target = BTreeMap::new();
        if let Some(settings) = previous.get("settings.yaml") {
            target.insert("settings.yaml".into(), settings_for_login(settings));
        }
        if let Err(error) = self.write_files(&target) {
            self.write_files(&previous)?;
            return Err(error);
        }
        Ok(())
    }

    pub fn remove(&self, id: &str) -> Result<(), UbisoftError> {
        std::fs::remove_file(self.path(id)?)?;
        Ok(())
    }

    pub async fn close_client(&self) -> Result<(), UbisoftError> {
        let mut system = sysinfo::System::new_all();
        self.launcher_dir
            .as_ref()
            .ok_or_else(|| UbisoftError::Invalid("Ubisoft Connect не найден".into()))?
            .canonicalize()?;
        // Closing CEF windows before upc causes the client to restart itself and
        // rewrite user.dat. Stop the main process tree first, then orphan helpers.
        for _ in 0..3 {
            system.refresh_processes();
            let processes = self.client_processes(&system);
            if processes.is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                system.refresh_processes();
                if self.client_processes(&system).is_empty() {
                    return Ok(());
                }
                continue;
            }
            let main: Vec<_> = processes
                .iter()
                .filter(|p| is_main_client(&p.name))
                .collect();
            let targets: Vec<_> = if main.is_empty() {
                processes.iter().collect()
            } else {
                main
            };
            for process in targets {
                #[cfg(windows)]
                {
                    let pid = process.pid;
                    tokio::task::spawn_blocking(move || {
                        super::hidden_command("taskkill.exe")
                            .args(["/F", "/PID", &pid.to_string(), "/T"])
                            .output()
                    })
                    .await
                    .map_err(|e| UbisoftError::Invalid(e.to_string()))??;
                }
                #[cfg(not(windows))]
                crate::sys::process::sys_kill(&system, sysinfo::Pid::from_u32(process.pid));
            }
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        }
        system.refresh_processes();
        if self.client_processes(&system).is_empty() {
            Ok(())
        } else {
            Err(UbisoftError::Invalid("Не удалось закрыть Ubisoft Connect. Закрой клиент через значок в трее и повтори действие.".into()))
        }
    }
}

fn is_main_client(name: &str) -> bool {
    ["upc.exe", "UbisoftConnect.exe", "Uplay.exe"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
}
