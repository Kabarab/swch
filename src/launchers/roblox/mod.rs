pub mod client;
pub mod keepalive;
pub mod launch;
pub mod multi_instance;
pub mod scanner;

use std::sync::Arc;
use std::time::Duration;
use serde::Serialize;
use tracing::info;

use crate::storage::{Database, Game};
use self::client::RobloxClient;
use self::keepalive::RobloxKeepAliveWorker;
use self::launch::RobloxLauncher;
use self::multi_instance::MultiInstanceManager;
use self::scanner::RobloxScanner;

#[derive(Debug, Serialize)]
pub struct RobloxLibraryResponse {
    pub games: Vec<Game>,
    pub client_installed: bool,
    pub supported: bool,
    pub active_accounts_count: usize,
    pub multi_instance_enabled: bool,
    pub message: &'static str,
}

/// Central manager orchestrating the Roblox ecosystem within SWCH.
#[derive(Clone)]
pub struct RobloxManager {
    db: Database,
    client: Arc<RobloxClient>,
    scanner: Arc<RobloxScanner>,
    launcher: Arc<RobloxLauncher>,
    multi_instance: Arc<MultiInstanceManager>,
    keepalive: Arc<RobloxKeepAliveWorker>,
}

impl RobloxManager {
    pub fn new(db: Database) -> Self {
        let client = Arc::new(RobloxClient::new());
        let scanner = Arc::new(RobloxScanner::new(db.clone()));
        let launcher = Arc::new(RobloxLauncher::new());
        let multi_instance = Arc::new(MultiInstanceManager::new(db.clone()));
        let keepalive = Arc::new(RobloxKeepAliveWorker::new(db.clone()));

        Self {
            db,
            client,
            scanner,
            launcher,
            multi_instance,
            keepalive,
        }
    }

    pub fn client(&self) -> &RobloxClient {
        &self.client
    }

    pub fn scanner(&self) -> &RobloxScanner {
        &self.scanner
    }

    pub fn launcher(&self) -> &RobloxLauncher {
        &self.launcher
    }

    pub fn multi_instance(&self) -> &MultiInstanceManager {
        &self.multi_instance
    }

    pub fn keepalive(&self) -> &RobloxKeepAliveWorker {
        &self.keepalive
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    /// Starts background services: keep-alive heartbeat and auto-discovery.
    pub fn start_background_tasks(self: Arc<Self>) {
        // 1. Keep-Alive heartbeat worker (every 20 minutes)
        self.keepalive.clone().start(Duration::from_secs(20 * 60));

        // 2. Initial silent scan for accounts
        let scanner = self.scanner.clone();
        tokio::spawn(async move {
            info!("Running initial background Roblox account scan...");
            scanner.scan_and_import().await;
        });
    }

    /// Builds the library summary response for the frontend.
    pub async fn get_library_summary(&self) -> RobloxLibraryResponse {
        let detector = crate::modules::detector::LauncherDetector::new();
        let client_installed = detector.detect_roblox().is_some();
        let accounts = self.db.get_roblox_accounts().await.unwrap_or_default();
        let multi_enabled = self.multi_instance.is_multi_instance_enabled().await;

        let mut games = vec![Game {
            id: "roblox_client".to_string(),
            launcher: "roblox".to_string(),
            app_id: "roblox".to_string(),
            name: "Roblox".to_string(),
            install_path: detector.detect_roblox().map(|p| p.to_string_lossy().to_string()),
            executable_path: detector.detect_roblox().map(|p| p.to_string_lossy().to_string()),
            size_bytes: 0,
            is_installed: client_installed,
            banner_url: Some("https://images.rbxcdn.com/f9a888c38a1fd8728a3ec3ff124d9e03.jpg".into()),
            launch_uri: Some("roblox-player:".into()),
            namespace: None,
        }];

        // Also add favorite/manual places as library items
        if let Ok(places) = self.db.get_roblox_places().await {
            for place in places {
                games.push(Game {
                    id: format!("roblox_place_{}", place.place_id),
                    launcher: "roblox".to_string(),
                    app_id: place.place_id.clone(),
                    name: place.name,
                    install_path: None,
                    executable_path: None,
                    size_bytes: 0,
                    is_installed: client_installed,
                    banner_url: place.icon_url,
                    launch_uri: Some(format!("roblox-player:1+placeId:{}", place.place_id)),
                    namespace: place.creator_name,
                });
            }
        }

        RobloxLibraryResponse {
            games,
            client_installed,
            supported: true,
            active_accounts_count: accounts.len(),
            multi_instance_enabled: multi_enabled,
            message: "Поддержка мульти-аккаунтов, скинов, инвентаря и плейсов Roblox активна.",
        }
    }
}
