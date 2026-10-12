pub mod hot_swap;
pub mod launch;
pub mod nbt;
pub mod scanner;

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::modules::detector::LauncherDetector;
use crate::storage::models::{Game, MinecraftAccount, MinecraftProfile, MinecraftServer, MinecraftWorld};
use crate::storage::Database;

pub use hot_swap::MinecraftHotSwap;
pub use launch::{LaunchMinecraftRequest, LaunchMinecraftResponse, MinecraftLauncher};
pub use scanner::MinecraftScanner;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftLauncherOption {
    pub id: String,
    pub name: String,
    pub is_installed: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftLibraryResponse {
    pub games: Vec<Game>,
    pub client_installed: bool,
    pub official_installed: bool,
    pub tlauncher_installed: bool,
    pub available_launchers: Vec<MinecraftLauncherOption>,
    pub accounts: Vec<MinecraftAccount>,
    pub profiles: Vec<MinecraftProfile>,
    pub servers: Vec<MinecraftServer>,
    pub worlds: Vec<MinecraftWorld>,
    pub message: String,
}

/// Central manager orchestrating the Minecraft ecosystem in SWCH.
#[derive(Clone)]
pub struct MinecraftManager {
    db: Database,
    detector: LauncherDetector,
    scanner: Arc<MinecraftScanner>,
    hot_swap: Arc<MinecraftHotSwap>,
    launcher: Arc<MinecraftLauncher>,
}

impl MinecraftManager {
    pub fn new(db: Database) -> Self {
        let detector = LauncherDetector::new();
        let scanner = Arc::new(MinecraftScanner::new(db.clone()));
        let hot_swap = Arc::new(MinecraftHotSwap::new(db.clone()));
        let launcher = Arc::new(MinecraftLauncher::new(db.clone()));

        Self {
            db,
            detector,
            scanner,
            hot_swap,
            launcher,
        }
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    pub fn scanner(&self) -> &MinecraftScanner {
        &self.scanner
    }

    pub fn hot_swap(&self) -> &MinecraftHotSwap {
        &self.hot_swap
    }

    pub fn launcher(&self) -> &MinecraftLauncher {
        &self.launcher
    }

    /// Starts initial background scan to import accounts, profiles, servers, and worlds.
    pub fn start_background_tasks(self: Arc<Self>) {
        let scanner = self.scanner.clone();

        tokio::spawn(async move {
            info!("Running initial background Minecraft scan...");
            scanner.scan_and_import().await;
        });
    }

    /// Builds the library summary response for the frontend.
    pub async fn get_library_summary(&self) -> MinecraftLibraryResponse {
        let official_path = self.detector.detect_minecraft();
        let tlauncher_path = self.detector.detect_tlauncher();
        let official_installed = official_path.is_some();
        let tlauncher_installed = tlauncher_path.is_some();
        let client_installed = official_installed || tlauncher_installed;

        let available_launchers = vec![
            MinecraftLauncherOption {
                id: "official".to_string(),
                name: "Официальный лаунчер Minecraft".to_string(),
                is_installed: official_installed,
                path: official_path.as_ref().map(|p| p.to_string_lossy().to_string()),
            },
            MinecraftLauncherOption {
                id: "tlauncher".to_string(),
                name: "T-Launcher".to_string(),
                is_installed: tlauncher_installed,
                path: tlauncher_path.as_ref().map(|p| p.to_string_lossy().to_string()),
            },
        ];

        let accounts = self.db.get_minecraft_accounts().await.unwrap_or_default();
        let profiles = self.db.get_minecraft_profiles().await.unwrap_or_default();
        let servers = self.db.get_minecraft_servers().await.unwrap_or_default();
        let worlds = self.db.get_minecraft_worlds().await.unwrap_or_default();

        let primary_exe = official_path.or(tlauncher_path);

        let mut games = vec![Game {
            id: "minecraft_java".to_string(),
            launcher: "minecraft".to_string(),
            app_id: "minecraft_java".to_string(),
            name: "Minecraft: Java Edition".to_string(),
            install_path: self.detector.detect_minecraft_data_dir().map(|p| p.to_string_lossy().to_string()),
            executable_path: primary_exe.map(|p| p.to_string_lossy().to_string()),
            size_bytes: 1_250_000_000,
            is_installed: client_installed,
            banner_url: Some("https://launchercontent.mojang.com/gameDetails/javaEdition/hero.jpg".to_string()),
            launch_uri: Some("minecraft:".to_string()),
            namespace: Some("Mojang Studios".to_string()),
        }];

        // Add Minecraft servers as independent game cards
        for server in &servers {
            games.push(Game {
                id: format!("minecraft_server_{}", server.id),
                launcher: "minecraft".to_string(),
                app_id: server.id.clone(),
                name: server.name.clone(),
                install_path: None,
                executable_path: None,
                size_bytes: 0,
                is_installed: client_installed,
                banner_url: server.icon.clone().or_else(|| {
                    Some("https://launchercontent.mojang.com/gameDetails/javaEdition/hero.jpg".to_string())
                }),
                launch_uri: Some(format!("minecraft://connect/{}", server.ip)),
                namespace: Some(server.ip.clone()),
            });
        }

        let message = if official_installed && tlauncher_installed {
            "Официальный лаунчер Minecraft и T-Launcher обнаружены. Доступен выбор лаунчера и быстрая смена аккаунтов.".to_string()
        } else if tlauncher_installed {
            "T-Launcher обнаружен. Быстрая смена аккаунтов T-Launcher активна.".to_string()
        } else if official_installed {
            "Официальный лаунчер Minecraft, аккаунты, профили и сервера активны.".to_string()
        } else {
            "Лаунчеры Minecraft не обнаружены на компьютере.".to_string()
        };

        MinecraftLibraryResponse {
            games,
            client_installed,
            official_installed,
            tlauncher_installed,
            available_launchers,
            accounts,
            profiles,
            servers,
            worlds,
            message,
        }
    }
}
