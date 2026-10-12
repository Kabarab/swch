//! Local Ubisoft Connect integration. Authentication stays in the official client.
pub mod session;
pub mod profile;
mod cache;

use crate::storage::Game;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum UbisoftError {
    #[error("{0}")]
    Invalid(String),
    #[error("Ошибка файлов Ubisoft Connect: {0}")]
    Io(#[from] std::io::Error),
    #[error("Ошибка сохранения сессии Ubisoft: {0}")]
    Atomic(#[from] crate::storage::atomic::AtomicError),
    #[error("Повреждённая сессия Ubisoft: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Process(#[from] crate::sys::process::ProcessError),
}

pub(crate) fn hidden_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn executable(dir: &Path) -> Option<PathBuf> {
    ["upc.exe", "UbisoftConnect.exe", "Uplay.exe"]
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

pub fn launcher_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SWCH_UBISOFT_DIR") {
        let path = PathBuf::from(path);
        let dir = if path.is_file() {
            path.parent()?.to_path_buf()
        } else {
            path
        };
        return executable(&dir).map(|_| dir);
    }
    #[cfg(windows)]
    {
        for hive in ["HKLM", "HKCU"] {
            for key in [
                "SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher",
                "SOFTWARE\\Ubisoft\\Launcher",
            ] {
                if let Ok(out) = hidden_command("reg.exe")
                    .args(["query", &format!("{hive}\\{key}"), "/v", "InstallDir"])
                    .output()
                {
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        if let Some((_, value)) = line.split_once("REG_SZ") {
                            let dir = PathBuf::from(value.trim());
                            if executable(&dir).is_some() {
                                return Some(dir);
                            }
                        }
                    }
                }
            }
        }
        for env in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(root) = std::env::var_os(env) {
                let dir = PathBuf::from(root).join("Ubisoft/Ubisoft Game Launcher");
                if executable(&dir).is_some() {
                    return Some(dir);
                }
            }
        }
    }
    None
}

/// Parse `reg query ... /s` output; only numeric install keys with real directories qualify.
pub fn games_from_registry(text: &str) -> Vec<Game> {
    let mut installs = BTreeMap::<String, (Option<String>, Option<String>)>::new();
    let mut current = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("HKEY_") {
            current = line.rsplit_once("\\Installs\\").and_then(|(_, id)| {
                (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then(|| id.to_string())
            });
        } else if let Some(id) = &current {
            if let Some((key, value)) = line.split_once("REG_SZ") {
                let entry = installs.entry(id.clone()).or_default();
                match key.trim() {
                    "InstallDir" => entry.0 = Some(value.trim().to_string()),
                    "DisplayName" => entry.1 = Some(value.trim().to_string()),
                    _ => {}
                }
            }
        }
    }
    installs
        .into_iter()
        .filter_map(|(id, (path, title))| {
            let path = path?;
            if !Path::new(&path).is_dir() {
                return None;
            }
            let name = title.filter(|s| !s.is_empty()).unwrap_or_else(|| {
                path.trim_end_matches(['/', '\\'])
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&path)
                    .to_string()
            });
            Some(Game {
                id: format!("ubisoft_{id}"),
                launcher: "ubisoft".into(),
                app_id: id.clone(),
                name,
                install_path: Some(path),
                executable_path: None,
                size_bytes: 0,
                is_installed: true,
                banner_url: None,
                launch_uri: Some(format!("uplay://launch/{id}/0")),
                namespace: None,
            })
        })
        .collect()
}

pub fn scan_games() -> Result<Vec<Game>, UbisoftError> {
    // A registry export also supports fixture tests and installations outside standard paths.
    if let Some(path) = std::env::var_os("SWCH_UBISOFT_REGISTRY_FILE") {
        return Ok(games_from_registry(&std::fs::read_to_string(path)?));
    }
    #[cfg(windows)]
    {
        let mut text = String::new();
        for hive in ["HKLM", "HKCU"] {
            for key in [
                "SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs",
                "SOFTWARE\\Ubisoft\\Launcher\\Installs",
            ] {
                let out = hidden_command("reg.exe")
                    .args(["query", &format!("{hive}\\{key}"), "/s"])
                    .output()?;
                text.push_str(&String::from_utf8_lossy(&out.stdout));
                text.push('\n');
            }
        }
        return Ok(games_from_registry(&text));
    }
    #[cfg(not(windows))]
    Ok(Vec::new())
}

pub fn launch_uri(app_id: &str) -> Result<String, UbisoftError> {
    if app_id.is_empty() || app_id.len() > 12 || !app_id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(UbisoftError::Invalid("Некорректный ID игры Ubisoft".into()));
    }
    Ok(format!("uplay://launch/{app_id}/0"))
}

pub fn open_client(app_id: Option<&str>) -> Result<(), UbisoftError> {
    let uri = app_id.map(launch_uri).transpose()?;
    if !cfg!(windows) {
        return Err(UbisoftError::Invalid(
            "Запуск Ubisoft Connect поддерживается на Windows".into(),
        ));
    }
    let dir = launcher_dir().ok_or_else(|| {
        UbisoftError::Invalid("Ubisoft Connect не найден. Установи официальный клиент.".into())
    })?;
    let exe = executable(&dir)
        .ok_or_else(|| UbisoftError::Invalid("Клиент Ubisoft Connect не найден".into()))?;
    let mut cmd = hidden_command(exe.to_string_lossy().as_ref());
    cmd.current_dir(&dir);
    if let Some(uri) = uri {
        cmd.arg(uri);
    }
    cmd.spawn()?;
    Ok(())
}
