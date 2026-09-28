pub mod profile;
pub mod valorant;
pub mod local;
// Local Riot Client integration. Authentication stays in the official client.
use crate::storage::Game;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct Library {
    pub games: Vec<Game>,
    pub client_installed: bool,
    pub supported: bool,
    pub capabilities: Capabilities,
    pub message: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub launch: bool,
    pub account_switch: bool,
    pub cloud_library: bool,
    pub inventory: bool,
    pub profile_sync: bool,
}

pub fn product(id: &str) -> Option<&'static str> {
    match id {
        "league_of_legends" => Some("League of Legends"),
        "valorant" => Some("VALORANT"),
        "bacon" => Some("Legends of Runeterra"),
        _ => None,
    }
}

pub fn supported_product(id: &str, os: &str) -> bool {
    product(id).is_some() && (os == "windows" || (os == "macos" && id == "league_of_legends"))
}

fn roots() -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        vec![PathBuf::from(
            std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into()),
        )
        .join("Riot Games")]
    } else {
        vec![PathBuf::from("/Users/Shared/Riot Games")]
    }
}

fn manifests() -> Vec<serde_json::Value> {
    roots()
        .iter()
        .filter_map(|root| {
            let data = std::fs::read(root.join("RiotClientInstalls.json")).ok()?;
            serde_json::from_slice(&data).ok()
        })
        .collect()
}

fn valid_client(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    path.is_absolute()
        && path.is_file()
        && if cfg!(target_os = "windows") {
            name.eq_ignore_ascii_case("RiotClientServices.exe")
        } else {
            name == "RiotClientServices"
        }
}

pub fn client() -> Option<PathBuf> {
    if !cfg!(any(target_os = "windows", target_os = "macos")) {
        return None;
    }
    let mut candidates = Vec::new();
    if let Some(custom) = std::env::var_os("SWCH_RIOT_CLIENT") {
        candidates.push(PathBuf::from(custom));
    }
    for manifest in manifests() {
        for key in ["rc_default", "rc_live"] {
            if let Some(path) = manifest[key].as_str() {
                candidates.push(PathBuf::from(path));
            }
        }
    }
    if cfg!(target_os = "windows") {
        candidates.push(PathBuf::from(
            "C:\\Riot Games\\Riot Client\\RiotClientServices.exe",
        ));
    } else {
        for root in ["/Users/Shared/Riot Games", "/Applications"] {
            candidates
                .push(Path::new(root).join("Riot Client.app/Contents/MacOS/RiotClientServices"));
        }
    }
    candidates.into_iter().find(|p| valid_client(p))
}

fn game_path(id: &str) -> Option<PathBuf> {
    let relative = match id {
        "league_of_legends" if cfg!(target_os = "macos") => "Contents/LoL/LeagueClient.app",
        "league_of_legends" => "LeagueClient.exe",
        "valorant" => "live/VALORANT.exe",
        "bacon" => "live/Game/LoR.exe",
        _ => return None,
    };
    let mut candidates = Vec::new();
    for manifest in manifests() {
        if let Some(installs) = manifest["associated_client"].as_object() {
            candidates.extend(installs.keys().map(PathBuf::from));
        }
    }
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/Applications/League of Legends.app"));
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join("Applications/League of Legends.app"));
        }
    } else {
        let folder = match id {
            "league_of_legends" => "League of Legends",
            "valorant" => "VALORANT",
            _ => "LoR",
        };
        candidates.push(PathBuf::from("C:\\Riot Games").join(folder));
    }
    candidates.into_iter().find(|p| {
        p.join(relative).exists()
            || match id {
                "valorant" => p.join("VALORANT.exe").is_file(),
                "bacon" => p.join("Game/LoR.exe").is_file(),
                _ => false,
            }
    })
}

pub fn library() -> Library {
    let supported = cfg!(any(target_os = "windows", target_os = "macos"));
    let client_installed = client().is_some();
    let games = ["league_of_legends", "valorant", "bacon"]
        .into_iter()
        .filter(|id| supported_product(id, std::env::consts::OS))
        .map(|id| {
            let path = game_path(id);
            Game {
                id: format!("riot_{id}"),
                launcher: "riot".into(),
                app_id: id.into(),
                name: product(id).unwrap().into(),
                is_installed: path.is_some(),
                install_path: path.map(|p| p.to_string_lossy().into_owned()),
                executable_path: None,
                size_bytes: 0,
                banner_url: None,
                launch_uri: None,
                namespace: None,
            }
        })
        .collect();
    Library {
        games,
        client_installed,
        supported,
        capabilities: Capabilities {
            launch: client_installed,
            account_switch: false,
            cloud_library: false,
            inventory: true,
            profile_sync: supported,
        },
        message: if !supported {
            "Riot Client не поддерживается на этой ОС."
        } else if !client_installed {
            "Riot Client не найден. Установи официальный клиент или укажи SWCH_RIOT_CLIENT."
        } else {
            "Вход и выбор аккаунта — в Riot Client. Коллекции и историю матчей обновляй из вошедшего клиента игры. TFT открывается через League of Legends."
        },
    }
}

/// Construct literal arguments only; never accept command lines from HTTP callers.
pub fn launch_args(id: Option<&str>) -> Result<Vec<String>, &'static str> {
    match id {
        None => Ok(vec![]),
        Some(id) if product(id).is_none() => Err("unknown_product"),
        Some(id) if !supported_product(id, std::env::consts::OS) => Err("unsupported_platform"),
        Some(id) => Ok(vec![
            format!("--launch-product={id}"),
            "--launch-patchline=live".into(),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_matrix_and_injection() {
        assert!(supported_product("league_of_legends", "macos"));
        assert!(!supported_product("valorant", "macos"));
        assert!(supported_product("valorant", "windows"));
        assert!(!supported_product("league_of_legends", "linux"));
        assert_eq!(launch_args(Some("valorant --evil")), Err("unknown_product"));
        assert_eq!(launch_args(None).unwrap(), Vec::<String>::new());
    }
    #[test]
    fn missing_executable_is_not_client() {
        assert!(!valid_client(Path::new(
            "/does-not-exist/RiotClientServices"
        )));
    }
}
