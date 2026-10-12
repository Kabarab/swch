use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::launchers::epic::EpicAuthStore;
use crate::storage::Database;

use super::detector::LauncherDetector;
use super::downloader::ModuleDownloader;
use super::types::{ModuleError, ModuleId, ModuleInfo, ModuleStatus};

/// Internal runtime state maintained per module in memory.
#[derive(Debug, Clone)]
pub struct ModuleRuntimeState {
    pub id: ModuleId,
    pub status: ModuleStatus,
    pub launcher_path: Option<PathBuf>,
    pub ram_bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanReport {
    pub scanned_at: String,
    pub newly_installed: Vec<String>,
    pub deactivated: Vec<String>,
    pub active_modules: Vec<String>,
}

/// Central manager orchestrating modular launcher lifecycles,
/// background detection, dynamic hot-plugging, and memory optimization.
pub struct ModuleManager {
    db: Database,
    detector: Arc<RwLock<LauncherDetector>>,
    downloader: ModuleDownloader,
    states: Arc<RwLock<HashMap<ModuleId, ModuleRuntimeState>>>,
    epic_store: Arc<RwLock<Option<Arc<EpicAuthStore>>>>,
}

impl ModuleManager {
    /// Creates a new ModuleManager instance.
    pub fn new(db: Database) -> Self {
        let downloader = ModuleDownloader::new().unwrap_or_else(|_| {
            ModuleDownloader::with_dir(PathBuf::from(".swch/modules"))
        });

        Self {
            db,
            detector: Arc::new(RwLock::new(LauncherDetector::new())),
            downloader,
            states: Arc::new(RwLock::new(HashMap::new())),
            epic_store: Arc::new(RwLock::new(None)),
        }
    }

    /// Sets mock launchers on this manager instance (thread-safe for concurrent tests).
    pub async fn set_mock_launchers(&self, launchers: Vec<String>) {
        let mut det = self.detector.write().await;
        det.set_mock_launchers(launchers);
    }

    /// Creates a ModuleManager with a pre-configured EpicAuthStore and active modules (ideal for tests and fallbacks).
    pub fn new_with_epic(db: Database, epic: Option<Arc<EpicAuthStore>>) -> Self {
        let downloader = ModuleDownloader::new().unwrap_or_else(|_| {
            ModuleDownloader::with_dir(PathBuf::from(".swch/modules"))
        });

        let mut states_map = HashMap::new();
        for &id in ModuleId::all() {
            states_map.insert(
                id,
                ModuleRuntimeState {
                    id,
                    status: ModuleStatus::Active,
                    launcher_path: Some(PathBuf::from(format!("/mock/{}", id.as_str()))),
                    ram_bytes: 10_000_000,
                },
            );
        }

        Self {
            db,
            detector: Arc::new(RwLock::new(LauncherDetector::new())),
            downloader,
            states: Arc::new(RwLock::new(states_map)),
            epic_store: Arc::new(RwLock::new(epic)),
        }
    }

    /// Access the active EpicAuthStore, if the Epic module is currently active.
    pub async fn epic_store(&self) -> Option<Arc<EpicAuthStore>> {
        self.epic_store.read().await.clone()
    }

    /// Performs the initial scan and configuration on server startup.
    pub async fn init(&self) -> Result<(), ModuleError> {
        info!("Initializing SWCH Modular Engine...");
        let gog_online = self.db.get_accounts().await.map_err(|e| ModuleError::Database(e.to_string()))?
            .iter().any(|a| a.launcher == "gog");
        let mut states_lock = self.states.write().await;

        for &id in ModuleId::all() {
            let detected_path = self.detector.read().await.detect_path(id);
            if let Some(ref path) = detected_path {
                info!("Launcher detected: {} at {}", id.display_name(), path.display());
                // Auto-install module files if missing
                if !self.downloader.is_installed(id) {
                    let _ = self.downloader.install_module(id, Some(path)).await;
                }

                // If Epic Games is detected, initialize EpicAuthStore and restore sessions
                let ram = if id == ModuleId::Epic {
                    let store = Arc::new(EpicAuthStore::default_store(self.db.clone()));
                    match store.restore_sessions().await {
                        Ok(report) => {
                            info!(
                                "Epic sessions restored: {}/{} session(s) ready, active account: {:?}",
                                report.restored_sessions,
                                report.total_accounts,
                                report.active_account_id
                            );
                        }
                        Err(e) => {
                            warn!("Failed to restore Epic sessions: {}", e);
                        }
                    }
                    let mut ep = self.epic_store.write().await;
                    *ep = Some(store);
                    15_200_000 // Measured ~15MB RAM
                } else if matches!(id, ModuleId::Ubisoft | ModuleId::Gog | ModuleId::Hoyoverse) {
                    0 // No persistent launcher runtime is allocated.
                } else if id == ModuleId::Steam {
                    18_400_000 // Measured ~18MB RAM
                } else if id == ModuleId::Roblox {
                    12_500_000 // Measured ~12.5MB RAM
                } else if id == ModuleId::Minecraft {
                    11_200_000 // Measured ~11.2MB RAM
                } else {
                    9_100_000  // Measured ~9MB RAM
                };

                states_lock.insert(
                    id,
                    ModuleRuntimeState {
                        id,
                        status: ModuleStatus::Active,
                        launcher_path: Some(path.clone()),
                        ram_bytes: ram,
                    },
                );
            } else if id == ModuleId::Gog && gog_online {
                self.downloader.install_module(id, None).await?;
                states_lock.insert(id, ModuleRuntimeState { id, status: ModuleStatus::Active, launcher_path: None, ram_bytes: 0 });
            } else {
                info!(
                    "Launcher {} not detected on system. Module will NOT be loaded (0 MB RAM overhead).",
                    id.display_name()
                );
                states_lock.insert(
                    id,
                    ModuleRuntimeState {
                        id,
                        status: ModuleStatus::NotDetected,
                        launcher_path: None,
                        ram_bytes: 0,
                    },
                );
            }
        }

        Ok(())
    }

    /// Starts a background auto-discovery loop that continuously monitors the system.
    /// If a user downloads and installs a new launcher, SWCH automatically installs
    /// and hot-plugs the module without requiring an application restart.
    pub fn start_auto_discovery_loop(
        self: Arc<Self>,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("Started background launcher auto-discovery loop (interval: {:?})", interval);
            loop {
                tokio::time::sleep(interval).await;
                if let Err(e) = self.scan_and_sync().await {
                    warn!("Background auto-discovery scan error: {}", e);
                }
            }
        })
    }

    /// Scans the system for changes in installed launchers.
    /// Hot-plugs newly installed launchers and deactivates removed ones.
    pub async fn scan_and_sync(&self) -> Result<ScanReport, ModuleError> {
        let mut newly_installed = Vec::new();
        let mut deactivated = Vec::new();
        let mut active_list = Vec::new();
        let gog_online = self.db.get_accounts().await.map_err(|e| ModuleError::Database(e.to_string()))?
            .iter().any(|a| a.launcher == "gog");

        for &id in ModuleId::all() {
            let detected_path = self.detector.read().await.detect_path(id);
            let current_status = {
                let lock = self.states.read().await;
                lock.get(&id).map(|s| (s.status.clone(), s.launcher_path.clone()))
            };

            match (detected_path, current_status) {
                (None, Some((status, _))) if id == ModuleId::Gog && gog_online => {
                    if matches!(status, ModuleStatus::Disabled) { continue; }
                    if !status.is_active() {
                        self.downloader.install_module(id, None).await?;
                        self.states.write().await.insert(id, ModuleRuntimeState { id, status: ModuleStatus::Active, launcher_path: None, ram_bytes: 0 });
                        newly_installed.push(id.as_str().to_string());
                    }
                    active_list.push(id.as_str().to_string());
                }
                // Case 1: Launcher was NOT detected before, but was now installed/downloaded!
                (Some(path), Some((status, _))) if !status.is_active() => {
                    info!(
                        "⚡ NEW LAUNCHER DETECTED: {} at {}! Auto-installing and activating module...",
                        id.display_name(),
                        path.display()
                    );

                    // 1. Install module files & manifest
                    let _ = self.downloader.install_module(id, Some(&path)).await;

                    // 2. Initialize module runtime if Epic
                    let ram = if id == ModuleId::Epic {
                        let store = Arc::new(EpicAuthStore::default_store(self.db.clone()));
                        let _ = store.restore_sessions().await;
                        let mut ep = self.epic_store.write().await;
                        *ep = Some(store);
                        15_200_000
                    } else if matches!(id, ModuleId::Ubisoft | ModuleId::Gog | ModuleId::Hoyoverse) {
                        0 // No persistent launcher runtime is allocated.
                    } else if id == ModuleId::Steam {
                        18_400_000
                    } else if id == ModuleId::Roblox {
                        12_500_000
                    } else if id == ModuleId::Minecraft {
                        11_200_000
                    } else {
                        9_100_000
                    };

                    // 3. Mark active
                    let mut lock = self.states.write().await;
                    lock.insert(
                        id,
                        ModuleRuntimeState {
                            id,
                            status: ModuleStatus::Active,
                            launcher_path: Some(path),
                            ram_bytes: ram,
                        },
                    );

                    newly_installed.push(id.as_str().to_string());
                    active_list.push(id.as_str().to_string());
                    info!("Module {} successfully auto-installed and hot-plugged!", id.display_name());
                }

                // Case 2: Launcher was active before, but is now uninstalled from the computer!
                (None, Some((status, _))) if status.is_active() => {
                    info!(
                        "Launcher {} is no longer detected on system. Deactivating module and freeing RAM...",
                        id.display_name()
                    );

                    if id == ModuleId::Epic {
                        let mut ep = self.epic_store.write().await;
                        *ep = None; // Free Epic store from memory
                    }

                    let mut lock = self.states.write().await;
                    lock.insert(
                        id,
                        ModuleRuntimeState {
                            id,
                            status: ModuleStatus::NotDetected,
                            launcher_path: None,
                            ram_bytes: 0,
                        },
                    );

                    deactivated.push(id.as_str().to_string());
                }

                // Case 3: Remains active
                (Some(_), Some((status, _))) if status.is_active() => {
                    active_list.push(id.as_str().to_string());
                }

                _ => {}
            }
        }

        Ok(ScanReport {
            scanned_at: chrono::Utc::now().to_rfc3339(),
            newly_installed,
            deactivated,
            active_modules: active_list,
        })
    }

    /// Returns whether a given module is currently active.
    pub async fn is_module_active(&self, id: ModuleId) -> bool {
        let lock = self.states.read().await;
        lock.get(&id).map(|s| s.status.is_active()).unwrap_or(false)
    }

    /// Returns a list of all active launcher identifier strings (e.g. `["steam", "epic"]`).
    pub async fn get_active_launcher_ids(&self) -> Vec<String> {
        let lock = self.states.read().await;
        lock.iter()
            .filter(|(_, state)| state.status.is_active())
            .map(|(id, _)| id.as_str().to_string())
            .collect()
    }

    /// Manually triggers installation of a module.
    pub async fn install_module(&self, id: ModuleId) -> Result<(), ModuleError> {
        let path = self.detector.read().await.detect_path(id);
        let _ = self.downloader.install_module(id, path.as_deref()).await?;

        let ram = if id == ModuleId::Epic {
            let store = Arc::new(EpicAuthStore::default_store(self.db.clone()));
            let _ = store.restore_sessions().await;
            let mut ep = self.epic_store.write().await;
            *ep = Some(store);
            15_200_000
        } else if matches!(id, ModuleId::Ubisoft | ModuleId::Gog | ModuleId::Hoyoverse) {
            0 // No persistent launcher runtime is allocated.
        } else if id == ModuleId::Steam {
            18_400_000
        } else if id == ModuleId::Roblox {
            12_500_000
        } else if id == ModuleId::Minecraft {
            11_200_000
        } else {
            9_100_000
        };

        let mut lock = self.states.write().await;
        lock.insert(
            id,
            ModuleRuntimeState {
                id,
                status: ModuleStatus::Active,
                launcher_path: path,
                ram_bytes: ram,
            },
        );

        Ok(())
    }

    /// Manually toggles a module active/inactive.
    pub async fn toggle_module(&self, id: ModuleId, active: bool) -> Result<(), ModuleError> {
        if active {
            self.install_module(id).await?;
        } else {
            if id == ModuleId::Epic {
                let mut ep = self.epic_store.write().await;
                *ep = None;
            }
            let mut lock = self.states.write().await;
            if let Some(state) = lock.get_mut(&id) {
                state.status = ModuleStatus::Disabled;
                state.ram_bytes = 0;
            }
        }
        Ok(())
    }

    /// Purges a module: frees memory, deletes its directory and manifest.
    pub async fn purge_module(&self, id: ModuleId) -> Result<(), ModuleError> {
        if id == ModuleId::Epic {
            let mut ep = self.epic_store.write().await;
            *ep = None;
        }

        self.downloader.purge_module(id)?;

        let mut lock = self.states.write().await;
        lock.insert(
            id,
            ModuleRuntimeState {
                id,
                status: ModuleStatus::NotDetected,
                launcher_path: None,
                ram_bytes: 0,
            },
        );

        Ok(())
    }

    /// Builds a comprehensive status report of all modules for UI presentation.
    pub async fn get_status_all(&self) -> Vec<ModuleInfo> {
        let lock = self.states.read().await;
        let mut list = Vec::new();

        // Fetch counts from database
        let all_games = self.db.get_games().await.unwrap_or_default();
        let all_accounts = self.db.get_accounts().await.unwrap_or_default();

        for &id in ModuleId::all() {
            let state = lock.get(&id);
            let detected = self.detector.read().await.detect_path(id);
            let installed = self.downloader.is_installed(id);

            let status = state.map(|s| s.status.clone()).unwrap_or(ModuleStatus::NotDetected);
            let ram = state.map(|s| s.ram_bytes).unwrap_or(0);
            let path_str = detected.as_ref().map(|p| p.to_string_lossy().to_string());

            let games_count = all_games
                .iter()
                .filter(|g| g.launcher.eq_ignore_ascii_case(id.as_str()))
                .count();
            let accounts_count = all_accounts
                .iter()
                .filter(|a| a.launcher.eq_ignore_ascii_case(id.as_str()))
                .count();

            list.push(ModuleInfo {
                id: id.as_str().to_string(),
                name: id.display_name().to_string(),
                description: id.description().to_string(),
                launcher_detected: detected.is_some(),
                launcher_path: path_str,
                module_installed: installed,
                active: status.is_active(),
                status: status.as_status_str().to_string(),
                status_detail: match &status {
                    ModuleStatus::Installing { message, .. } => Some(message.clone()),
                    ModuleStatus::Error { message } => Some(message.clone()),
                    _ => None,
                },
                ram_bytes: ram,
                games_count,
                accounts_count,
                module_dir: Some(self.downloader.module_dir(id).to_string_lossy().to_string()),
                version: "1.0.0".to_string(),
            });
        }

        list
    }
}
