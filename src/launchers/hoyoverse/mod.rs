//! HoYoverse / miHoYo unified launcher and game account switcher.
//! Supports Genshin Impact, Honkai: Star Rail, Zenless Zone Zero, Honkai Impact 3rd, and HoYoPlay.

pub mod registry;
pub mod session;

use crate::storage::Game;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum HoyoverseError {
    #[error("{0}")]
    Invalid(String),
    #[error("Ошибка файлов HoYoverse: {0}")]
    Io(#[from] std::io::Error),
    #[error("Ошибка сохранения сессии HoYoverse: {0}")]
    Atomic(#[from] crate::storage::atomic::AtomicError),
    #[error("Повреждённые данные сессии HoYoverse: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Process(#[from] crate::sys::process::ProcessError),
    #[error("Ошибка базы данных: {0}")]
    Database(#[from] crate::storage::DbError),
}

/// Metadata definition for a HoYoverse game or unified launcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HoyoverseGameDef {
    pub id: &'static str,
    pub display_name: &'static str,
    pub default_process: &'static str,
    pub process_names: &'static [&'static str],
    pub registry_subkeys: &'static [&'static str],
    pub uid_key_name: Option<&'static str>,
    pub launch_scheme: Option<&'static str>,
}

pub const SUPPORTED_GAMES: &[HoyoverseGameDef] = &[
    HoyoverseGameDef {
        id: "genshin",
        display_name: "Genshin Impact",
        default_process: "GenshinImpact.exe",
        process_names: &["GenshinImpact.exe", "YuanShen.exe"],
        registry_subkeys: &[
            "Software\\miHoYo\\Genshin Impact",
            "Software\\Cognosphere\\Genshin Impact",
            "Software\\miHoYo\\原神",
            "Software\\miHoYo\\云·原神",
        ],
        uid_key_name: Some("__LastUid___h2153286551"),
        launch_scheme: Some("genshin://"),
    },
    HoyoverseGameDef {
        id: "starrail",
        display_name: "Honkai: Star Rail",
        default_process: "StarRail.exe",
        process_names: &["StarRail.exe"],
        registry_subkeys: &[
            "Software\\Cognosphere\\Star Rail",
            "Software\\miHoYo\\崩坏：星穹铁道",
            "Software\\miHoYo\\Star Rail",
        ],
        uid_key_name: Some("App_LastUserID_h2841727341"),
        launch_scheme: None,
    },
    HoyoverseGameDef {
        id: "zzz",
        display_name: "Zenless Zone Zero",
        default_process: "ZenlessZoneZero.exe",
        process_names: &["ZenlessZoneZero.exe"],
        registry_subkeys: &[
            "Software\\miHoYo\\ZenlessZoneZero",
            "Software\\Cognosphere\\ZenlessZoneZero",
            "Software\\miHoYo\\绝区零",
        ],
        uid_key_name: None,
        launch_scheme: None,
    },
    HoyoverseGameDef {
        id: "honkai3rd",
        display_name: "Honkai Impact 3rd",
        default_process: "BH3.exe",
        process_names: &["BH3.exe"],
        registry_subkeys: &[
            "Software\\miHoYo\\Honkai Impact 3rd",
            "Software\\miHoYo\\Honkai Impact 3",
            "Software\\Cognosphere\\Honkai Impact 3rd",
            "Software\\miHoYo\\崩坏3",
            "Software\\miHoYo\\崩壊3rd",
            "Software\\miHoYo\\붕괴3rd",
        ],
        uid_key_name: Some("GENERAL_DATA_V2_LastLoginUserId_h47158221"),
        launch_scheme: None,
    },
    HoyoverseGameDef {
        id: "hoyoplay",
        display_name: "HoYoPlay",
        default_process: "HYP.exe",
        process_names: &["HYP.exe", "hyp.exe", "hyp_desktop.exe", "HoYoPlay.exe"],
        registry_subkeys: &[
            "Software\\Cognosphere\\HYP\\1_0",
            "Software\\miHoYo\\HYP\\1_1",
            "Software\\Cognosphere\\HYP",
            "Software\\miHoYo\\HYP",
        ],
        uid_key_name: None,
        launch_scheme: Some("hyp://"),
    },
];

pub const SHARED_SDK_SUBKEY: &str = "Software\\miHoYoSDK";

pub const LOGIN_VALUE_NAMES: &[&str] = &[
    "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
    "MIHOYOSDK_ADL_PROD_CN_h3123967166",
    "MIHOYOSDK_ADL_0",
    "GENERAL_DATA_h2389025596",
    "GENERAL_DATA_V2_LastLoginUserId_h47158221",
    "App_LastUserID_h2841727341",
    "__LastUid___h2153286551",
];

pub fn find_game_def(game_id: &str) -> Option<&'static HoyoverseGameDef> {
    let lower = game_id.to_lowercase();
    SUPPORTED_GAMES.iter().find(|g| {
        g.id.eq_ignore_ascii_case(&lower)
            || g.display_name.eq_ignore_ascii_case(&lower)
            || (lower == "hoyo" && g.id == "hoyoplay")
            || (lower == "hyp" && g.id == "hoyoplay")
            || (lower == "gi" && g.id == "genshin")
            || (lower == "hsr" && g.id == "starrail")
            || (lower == "hi3" && g.id == "honkai3rd")
    })
}

pub fn all_games() -> &'static [HoyoverseGameDef] {
    SUPPORTED_GAMES
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

/// Detects the installation directory of HoYoPlay or any HoYoverse launcher.
pub fn launcher_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SWCH_HOYOVERSE_DIR") {
        let path = PathBuf::from(path);
        let dir = if path.is_file() {
            path.parent()?.to_path_buf()
        } else {
            path
        };
        return Some(dir);
    }

    #[cfg(windows)]
    {
        // 1. Check HoYoPlay uninstall registry entries
        for hive in ["HKCU", "HKLM"] {
            for subkey in [
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\HYP_1_0_global",
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\HYP_1_1_cn",
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\HYP",
                "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\HYP_1_0_global",
                "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\HYP_1_1_cn",
            ] {
                if let Ok(Some(entry)) = registry::read_value(hive, subkey, "InstallPath") {
                    if let Some(path_str) = entry.as_str() {
                        let dir = PathBuf::from(path_str);
                        if dir.is_dir() {
                            return Some(dir);
                        }
                    }
                }
                if let Ok(Some(entry)) = registry::read_value(hive, subkey, "InstallLocation") {
                    if let Some(path_str) = entry.as_str() {
                        let dir = PathBuf::from(path_str);
                        if dir.is_dir() {
                            return Some(dir);
                        }
                    }
                }
            }
        }

        // 2. Check standard Program Files folders
        for env in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(env) {
                let p = PathBuf::from(root);
                for rel in ["HoYoPlay", "HoYoPlay Launcher", "Genshin Impact", "Star Rail"] {
                    let dir = p.join(rel);
                    if dir.is_dir() {
                        return Some(dir);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        for app in [
            "/Applications/HoYoPlay.app",
            "/Applications/Genshin Impact.app",
        ] {
            let p = PathBuf::from(app);
            if p.exists() {
                return Some(p);
            }
        }
    }

    None
}

/// Scans installed HoYoverse games.
pub fn scan_games() -> Result<Vec<Game>, HoyoverseError> {
    let mut games = Vec::new();

    // Check mock test flag or environment
    let is_mock_enabled = std::env::var("SWCH_HOYOVERSE_MOCK_GAMES").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
        || std::env::var("SWCH_MOCK_LAUNCHERS").map(|v| v.contains("hoyoverse") || v.contains("mihoyo")).unwrap_or(false)
        || registry::mock_registry_path().is_some();

    for def in SUPPORTED_GAMES {
        let mut installed = false;
        let mut install_path: Option<String> = None;
        let mut exe_path: Option<String> = None;

        if is_mock_enabled {
            installed = true;
            install_path = Some(format!("/mock/games/{}", def.id));
            exe_path = Some(format!("/mock/games/{}/{}", def.id, def.default_process));
        } else {
            // Check Windows registry for game install path
            #[cfg(windows)]
            {
                for subkey in def.registry_subkeys {
                    for hive in ["HKCU", "HKLM"] {
                        if let Ok(Some(entry)) = registry::read_value(hive, subkey, "InstallPath") {
                            if let Some(s) = entry.as_str() {
                                let path = PathBuf::from(s);
                                if path.exists() {
                                    installed = true;
                                    install_path = Some(s.to_string());
                                    let exe = path.join(def.default_process);
                                    if exe.is_file() {
                                        exe_path = Some(exe.to_string_lossy().to_string());
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    if installed {
                        break;
                    }
                }

                // If not found in game subkey, check HoYoPlay registry or standard paths
                if !installed {
                    for env in ["ProgramFiles", "ProgramFiles(x86)"] {
                        if let Some(root) = std::env::var_os(env) {
                            let candidate = PathBuf::from(root).join(def.display_name);
                            if candidate.is_dir() {
                                installed = true;
                                install_path = Some(candidate.to_string_lossy().to_string());
                                let exe = candidate.join(def.default_process);
                                if exe.is_file() {
                                    exe_path = Some(exe.to_string_lossy().to_string());
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        if installed {
            let launch_uri = def.launch_scheme.map(str::to_string)
                .or_else(|| exe_path.clone());

            games.push(Game {
                id: format!("hoyoverse_{}", def.id),
                launcher: "hoyoverse".into(),
                app_id: def.id.into(),
                name: def.display_name.into(),
                install_path,
                executable_path: exe_path,
                size_bytes: 0,
                is_installed: true,
                banner_url: None,
                launch_uri,
                namespace: None,
            });
        }
    }

    Ok(games)
}

/// Launches a specific HoYoverse game.
pub fn launch_game(game_id: &str) -> Result<(), HoyoverseError> {
    let def = find_game_def(game_id).ok_or_else(|| {
        HoyoverseError::Invalid(format!("Неизвестная игра HoYoverse: {game_id}"))
    })?;

    // Check if URI scheme is supported
    if let Some(scheme) = def.launch_scheme {
        #[cfg(windows)]
        {
            let _ = hidden_command("cmd")
                .args(["/c", "start", "", scheme])
                .spawn();
            return Ok(());
        }
    }

    // Try starting process directly if found
    let games = scan_games()?;
    if let Some(game) = games.iter().find(|g| g.app_id == def.id) {
        if let Some(ref exe) = game.executable_path {
            let path = Path::new(exe);
            if path.is_file() {
                let mut cmd = Command::new(path);
                if let Some(dir) = path.parent() {
                    cmd.current_dir(dir);
                }
                cmd.spawn()?;
                return Ok(());
            }
        }
    }

    Ok(())
}

/// Opens the unified HoYoPlay client.
pub fn open_launcher() -> Result<(), HoyoverseError> {
    launch_game("hoyoplay")
}
