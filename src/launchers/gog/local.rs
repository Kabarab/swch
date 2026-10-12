use super::{game, json_id, valid_id, GogError};
use crate::storage::Game;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

fn hidden_command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}

pub fn client_executable(dir: &Path) -> Option<PathBuf> {
    let exe = dir.join("GalaxyClient.exe");
    if exe.is_file() {
        return Some(exe);
    }
    let exe = dir.join("Contents/MacOS/GOG Galaxy");
    exe.is_file().then_some(exe)
}

pub fn launcher_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SWCH_GOG_DIR") {
        let path = PathBuf::from(path);
        let dir = if path.is_file() {
            path.parent()?.to_path_buf()
        } else {
            path
        };
        return client_executable(&dir).map(|_| dir);
    }
    #[cfg(windows)]
    {
        for hive in ["HKLM", "HKCU"] {
            for key in [
                "SOFTWARE\\WOW6432Node\\GOG.com\\GalaxyClient\\paths",
                "SOFTWARE\\GOG.com\\GalaxyClient\\paths",
            ] {
                if let Ok(out) = hidden_command(Path::new("reg.exe"))
                    .args(["query", &format!("{hive}\\{key}"), "/v", "client"])
                    .output()
                {
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        if let Some((_, value)) = line.split_once("REG_SZ") {
                            let dir = PathBuf::from(value.trim());
                            if client_executable(&dir).is_some() {
                                return Some(dir);
                            }
                        }
                    }
                }
            }
        }
        for env in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(root) = std::env::var_os(env) {
                for name in ["GOG Galaxy", "GalaxyClient"] {
                    let dir = PathBuf::from(&root).join(name);
                    if client_executable(&dir).is_some() {
                        return Some(dir);
                    }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut dirs = vec![PathBuf::from("/Applications/GOG Galaxy.app")];
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Applications/GOG Galaxy.app"));
        }
        return dirs
            .into_iter()
            .find(|dir| client_executable(dir).is_some());
    }
    None
}

fn installed_game(id: &str, dir: &Path, title: Option<&str>) -> Option<Game> {
    if !valid_id(id) || !dir.is_dir() {
        return None;
    }
    let manifest = dir.join(format!("goggame-{id}.info"));
    let info = if manifest.exists() {
        let raw = std::fs::read_to_string(manifest).ok()?;
        Some(serde_json::from_str::<Value>(raw.trim_start_matches('\u{feff}')).ok()?)
    } else {
        None
    };
    if let Some(info) = &info {
        if json_id(&info["gameId"]).as_deref() != Some(id) {
            return None;
        }
        if json_id(&info["rootGameId"]).is_some_and(|root| root != id) {
            return None;
        }
        if !info["playTasks"]
            .as_array()?
            .iter()
            .any(|task| task["isPrimary"] == true)
        {
            return None;
        }
    }
    let title = info
        .as_ref()
        .and_then(|v| v["name"].as_str())
        .or(title)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            dir.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let mut game = game(id, title, None);
    game.is_installed = true;
    game.install_path = Some(dir.to_string_lossy().into_owned());
    Some(game)
}

/// Read GOG game and uninstall registry records. Never execute registry launchCommand values.
pub fn games_from_registry(text: &str) -> Vec<Game> {
    let mut installs = BTreeMap::<String, (Option<String>, Option<String>, bool)>::new();
    let mut current = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("HKEY_") {
            let id = line.rsplit('\\').next().unwrap_or_default();
            let id = id.strip_suffix("_is1").unwrap_or(id);
            current = (valid_id(id)
                && (line.contains("\\GOG.com\\Games\\") || line.contains("\\Uninstall\\")))
            .then(|| id.to_owned());
            if let Some(id) = &current {
                installs.entry(id.clone()).or_default().2 |= line.contains("\\GOG.com\\Games\\");
            }
        } else if let Some(id) = &current {
            if let Some((key, value)) = line.split_once("REG_SZ") {
                let entry = installs.entry(id.clone()).or_default();
                match key.trim().to_ascii_lowercase().as_str() {
                    "path" | "installlocation" => entry.0 = Some(value.trim().into()),
                    "gamename" | "displayname" => entry.1 = Some(value.trim().into()),
                    "publisher" if value.trim().eq_ignore_ascii_case("GOG.com") => entry.2 = true,
                    _ => {}
                }
            }
        }
    }
    installs
        .into_iter()
        .filter_map(|(id, (path, title, is_gog))| {
            if !is_gog {
                return None;
            }
            installed_game(&id, Path::new(&path?), title.as_deref())
        })
        .collect()
}

pub fn games_from_dirs(roots: &[PathBuf]) -> Vec<Game> {
    let mut games = BTreeMap::new();
    for root in roots {
        for entry in walkdir::WalkDir::new(root)
            .max_depth(3)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            let Some(id) = name
                .strip_prefix("goggame-")
                .and_then(|s| s.strip_suffix(".info"))
            else {
                continue;
            };
            if let Some(game) = entry
                .path()
                .parent()
                .and_then(|dir| installed_game(id, dir, None))
            {
                games.insert(game.app_id.clone(), game);
            }
        }
    }
    games.into_values().collect()
}

pub fn scan_games() -> Result<Vec<Game>, GogError> {
    let mut games = BTreeMap::new();
    if let Some(file) = std::env::var_os("SWCH_GOG_REGISTRY_FILE") {
        for game in games_from_registry(&std::fs::read_to_string(file)?) {
            games.insert(game.app_id.clone(), game);
        }
    } else {
        #[cfg(windows)]
        for hive in ["HKLM", "HKCU"] {
            for key in [
                "SOFTWARE\\WOW6432Node\\GOG.com\\Games",
                "SOFTWARE\\GOG.com\\Games",
                "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            ] {
                let out = hidden_command(Path::new("reg.exe"))
                    .args(["query", &format!("{hive}\\{key}"), "/s"])
                    .output()?;
                for game in games_from_registry(&String::from_utf8_lossy(&out.stdout)) {
                    games.insert(game.app_id.clone(), game);
                }
            }
        }
    }
    let mut roots: Vec<PathBuf> = std::env::var_os("SWCH_GOG_GAMES_DIR")
        .map(|s| std::env::split_paths(&s).collect())
        .unwrap_or_default();
    if let Some(client) = launcher_dir() {
        roots.push(client.join("Games"));
    }
    #[cfg(windows)]
    if let Some(drive) = std::env::var_os("SystemDrive") {
        roots.push(PathBuf::from(drive).join("/GOG Games"));
    }
    for game in games_from_dirs(&roots) {
        games.insert(game.app_id.clone(), game);
    }
    Ok(games.into_values().collect())
}

pub fn launch_arguments(game: &Game) -> Result<Vec<String>, GogError> {
    if game.launcher != "gog" || !valid_id(&game.app_id) || !game.is_installed {
        return Err(GogError::Invalid(
            "Игра GOG не установлена. Установи её в GALAXY и обнови библиотеку.".into(),
        ));
    }
    let dir = game
        .install_path
        .as_deref()
        .filter(|s| Path::new(s).is_dir())
        .ok_or_else(|| GogError::Invalid("Папка игры GOG не найдена. Обнови библиотеку.".into()))?;
    Ok(vec![
        "/launchViaAutostart".into(),
        "/command=runGame".into(),
        format!("/gameId={}", game.app_id),
        format!("/path={dir}"),
    ])
}

pub fn open_client(game: Option<&Game>) -> Result<(), GogError> {
    let args = game.map(launch_arguments).transpose()?;
    if !cfg!(windows) && game.is_some() {
        return Err(GogError::Invalid(
            "Запуск игр GOG через GALAXY пока поддерживается на Windows.".into(),
        ));
    }
    let dir = launcher_dir().ok_or_else(|| {
        GogError::Invalid("GOG GALAXY не найден. Установи официальный клиент.".into())
    })?;
    let exe = client_executable(&dir)
        .ok_or_else(|| GogError::Invalid("Клиент GOG GALAXY не найден.".into()))?;
    let mut command = hidden_command(&exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x01000000);
    }
    command.current_dir(&dir);
    if let Some(args) = args {
        command.args(args);
    }
    command.spawn()?;
    Ok(())
}
