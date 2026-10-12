//! SWCH-owned profiles only; never opens or copies the user's main browser data.
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

pub fn valid_choice(choice: &str) -> bool {
    matches!(choice, "auto" | "chrome" | "edge")
}

fn installed(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    for base in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(base) {
            let path = Path::new(&base).join(if name == "edge" {
                "Microsoft/Edge/Application/msedge.exe"
            } else {
                "Google/Chrome/Application/chrome.exe"
            });
            if path.is_file() {
                return Some(path);
            }
        }
    }
    #[cfg(not(windows))]
    for path in if name == "edge" {
        vec![
            "/usr/bin/microsoft-edge",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ]
    } else {
        vec![
            "/usr/bin/google-chrome",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ]
    } {
        if Path::new(path).is_file() {
            return Some(path.into());
        }
    }
    None
}

fn default_choice() -> &'static str {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if let Ok(output) = std::process::Command::new("reg.exe")
            .args(["query", r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice", "/v", "ProgId"])
            .creation_flags(0x08000000).output()
        {
            let value = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
            if value.contains("msedgehtm") { return "edge"; }
        }
    }
    "chrome"
}

pub fn executable(choice: &str) -> Result<(String, PathBuf), String> {
    if !valid_choice(choice) {
        return Err("Выбери Chrome, Edge или автоматический выбор браузера.".into());
    }
    let name = if choice == "auto" {
        default_choice()
    } else {
        choice
    };
    if let Some(path) = installed(name) {
        return Ok((name.into(), path));
    }
    if choice == "auto" {
        let fallback = if name == "chrome" { "edge" } else { "chrome" };
        if let Some(path) = installed(fallback) {
            return Ok((fallback.into(), path));
        }
    }
    Err(format!(
        "Браузер {} не установлен. Выбери другой в «Раздачи игр → Автоматизация».",
        if choice == "edge" { "Edge" } else { "Chrome" }
    ))
}

fn root() -> Result<PathBuf, String> {
    dirs::data_local_dir()
        .map(|p| p.join("SWCH").join("claim-browser-profiles"))
        .ok_or_else(|| "Не удалось найти папку профилей SWCH.".into())
}

pub fn account_key(platform: &str, id: &str) -> Result<String, String> {
    if super::platforms::platform(platform).is_none() || id.is_empty() || id.len() > 256 {
        return Err("Не удалось определить профиль аккаунта.".into());
    }
    let mut hash = Sha256::new();
    hash.update(platform.as_bytes());
    hash.update([0]);
    hash.update(id.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}

fn lock_at(root: &Path, key: &str) -> Result<File, String> {
    if std::fs::symlink_metadata(root).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Папка профилей SWCH не должна быть ссылкой.".into());
    }
    std::fs::create_dir_all(root).map_err(|_| "Не удалось создать папку профилей SWCH.")?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(format!("{key}.lock")))
        .map_err(|_| "Не удалось открыть профиль SWCH.")?;
    file.try_lock()
        .map_err(|_| "Окно этого аккаунта уже используется. Заверши вход или текущую проверку.")?;
    Ok(file)
}

pub fn prepare(platform: &str, id: &str, name: &str) -> Result<(PathBuf, File), String> {
    prepare_at(&root()?, platform, id, name)
}

pub(super) fn prepare_at(
    root: &Path,
    platform: &str,
    id: &str,
    name: &str,
) -> Result<(PathBuf, File), String> {
    if !matches!(name, "chrome" | "edge") {
        return Err("Неизвестный браузер профиля.".into());
    }
    let key = account_key(platform, id)?;
    let lock = lock_at(root, &key)?;
    let account = root.join(key);
    if std::fs::symlink_metadata(&account).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Папка профиля SWCH не должна быть ссылкой.".into());
    }
    let profile = account.join(name);
    if std::fs::symlink_metadata(&profile).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Папка браузера SWCH не должна быть ссылкой.".into());
    }
    std::fs::create_dir_all(&profile)
        .map_err(|_| "Не удалось подготовить профиль браузера SWCH.")?;
    // A browser can outlive a restarted backend. Do not forward the new URL into
    // that existing process: it may still have been launched in automated mode.
    if profile_is_running(&profile) {
        return Err("Этот профиль магазина уже открыт. Закрой его старое окно и повтори.".into());
    }
    Ok((profile, lock))
}

fn profile_is_running(profile: &Path) -> bool {
    let expected = profile
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let system = sysinfo::System::new_all();
    system
        .processes()
        .values()
        .filter(|p| {
            matches!(
                p.name().to_ascii_lowercase().as_str(),
                "chrome.exe" | "msedge.exe" | "chrome" | "msedge"
            )
        })
        .any(|p| {
            p.cmd().iter().any(|arg| {
                arg.trim_matches('"')
                    .strip_prefix("--user-data-dir=")
                    .is_some_and(|path| {
                        path.trim_matches('"')
                            .replace('\\', "/")
                            .to_ascii_lowercase()
                            == expected
                    })
            })
        })
}

pub fn delete(platform: &str, id: &str) -> Result<(), String> {
    delete_at(&root()?, platform, id)
}

/// A recovery window for the user: no CDP connection, scripts, or automatic clicks.
/// The profile lock lasts until its browser process exits. The user's main profile is never used.
pub async fn open_manual(
    account: &crate::storage::Account,
    url: &str,
    choice: &str,
) -> Result<(), String> {
    if !super::platforms::trusted_url(&account.launcher, url) {
        return Err("Адрес не относится к выбранному магазину.".into());
    }
    let platform = account.launcher.clone();
    let id = account.id.clone();
    let choice = choice.to_string();
    let (exe, profile, lock) = tokio::task::spawn_blocking(move || {
        let (name, exe) = executable(&choice)?;
        let (profile, lock) = prepare(&platform, &id, &name)?;
        Ok::<_, String>((exe, profile, lock))
    })
    .await
    .map_err(|_| "Не удалось подготовить окно магазина.")??;
    let mut child = tokio::process::Command::new(exe)
        .args(manual_arguments(&profile, url))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(false)
        .spawn()
        .map_err(|_| "Не удалось открыть обычное окно магазина.")?;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    if child
        .try_wait()
        .map_err(|_| "Не удалось проверить окно магазина.")?
        .is_some()
    {
        return Err("Профиль магазина уже открыт. Закрой его окно и повтори.".into());
    }
    tokio::spawn(async move {
        let _lock = lock;
        let _ = child.wait().await;
    });
    Ok(())
}

fn manual_arguments(profile: &Path, url: &str) -> Vec<std::ffi::OsString> {
    let mut profile_arg = std::ffi::OsString::from("--user-data-dir=");
    profile_arg.push(profile);
    vec![
        profile_arg,
        "--new-window".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        url.into(),
    ]
}

pub(super) fn delete_at(root: &Path, platform: &str, id: &str) -> Result<(), String> {
    let key = account_key(platform, id)?;
    let _lock = lock_at(root, &key)?;
    // The fixed-size hash is a single path component. remove_dir_all never follows links.
    let profile = root.join(key);
    match std::fs::remove_dir_all(profile) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => {
            Err("Не удалось удалить профиль магазина. Закрой его окно и повтори отключение.".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_window_has_no_automation_flags_or_credentials() {
        let args = manual_arguments(
            Path::new("profile with spaces"),
            "https://www.epicgames.com/id/login",
        );
        assert_eq!(args[0], "--user-data-dir=profile with spaces");
        assert_eq!(args.last().unwrap(), "https://www.epicgames.com/id/login");
        assert!(args.iter().all(|a| {
            let a = a.to_string_lossy();
            !a.contains("remote-debugging")
                && !a.contains("headless")
                && !a.contains("disable-blink")
                && !a.contains("exchangeCode")
        }));
    }
    #[test]
    fn profiles_are_distinct_locked_and_deletion_is_scoped() {
        let root = tempfile::tempdir().unwrap();
        let (a, lock) = prepare_at(root.path(), "epic", "../account", "chrome").unwrap();
        let (b, _other) = prepare_at(root.path(), "epic", "other", "edge").unwrap();
        assert_ne!(a.parent(), b.parent());
        assert!(a.starts_with(root.path()));
        assert!(prepare_at(root.path(), "epic", "../account", "edge").is_err());
        assert!(delete_at(root.path(), "epic", "../account").is_err());
        std::fs::write(a.join("fixture"), "test").unwrap();
        drop(lock);
        delete_at(root.path(), "epic", "../account").unwrap();
        assert!(!a.exists());
        assert!(b.exists());
        assert_ne!(
            account_key("epic", "same").unwrap(),
            account_key("gog", "same").unwrap()
        );
        assert!(account_key("unknown", "x").is_err());
    }
}
