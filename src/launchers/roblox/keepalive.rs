use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::storage::Database;
use super::client::RobloxClient;
use super::scanner::RobloxScanner;

/// Background keep-alive service that prevents Roblox session expiration.
/// It sends periodic heartbeats to Roblox API for each saved account,
/// ensuring Roblox servers never terminate sessions due to inactivity.
/// If an account token needs refresh, it automatically attempts silent re-harvesting
/// from local browser profiles.
pub struct RobloxKeepAliveWorker {
    db: Database,
    client: RobloxClient,
    scanner: Arc<RobloxScanner>,
}

impl RobloxKeepAliveWorker {
    pub fn new(db: Database) -> Self {
        let scanner = Arc::new(RobloxScanner::new(db.clone()));
        Self {
            db,
            client: RobloxClient::new(),
            scanner,
        }
    }

    /// Starts the background heartbeat loop.
    pub fn start(self: Arc<Self>, interval: Duration) -> JoinHandle<()> {
        tokio::spawn(async move {
            info!("Roblox Keep-Alive background worker started with interval: {:?}", interval);
            loop {
                tokio::time::sleep(interval).await;
                if let Err(e) = self.ping_all_accounts().await {
                    debug!("Roblox keep-alive cycle error: {}", e);
                }
            }
        })
    }

    /// Iterates over all saved Roblox accounts and sends an authenticated ping.
    pub async fn ping_all_accounts(&self) -> Result<(), String> {
        let accounts = self.db.get_roblox_accounts().await.map_err(|e| e.to_string())?;
        if accounts.is_empty() {
            return Ok(());
        }

        let mut needs_rescan = false;

        for mut acc in accounts {
            match self.client.get_authenticated_user(&acc.cookie).await {
                Ok(user) => {
                    debug!("Roblox keep-alive heartbeat successful for @{}", user.name);
                    acc.last_validated = chrono::Utc::now().timestamp();
                    acc.is_banned = false;
                    acc.username = user.name;
                    acc.display_name = user.display_name;
                    let _ = self.db.save_roblox_account(&acc).await;
                }
                Err(err) => {
                    warn!("Roblox session ping failed for {}: {}", acc.account_id, err);
                    needs_rescan = true;
                }
            }
        }

        // If any session failed, trigger silent auto-scan to recover latest cookie from browser
        if needs_rescan {
            info!("Triggering silent browser re-scan to refresh expiring Roblox cookies...");
            self.scanner.scan_and_import().await;
        }

        Ok(())
    }
}
