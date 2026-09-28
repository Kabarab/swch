#![allow(dead_code)]

use chrono::Utc;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::RwLock;

use super::client::{EpicClient, EpicClientError};
use super::keyring::{CredentialStore, KeyringError, MemoryCredentialStore};
use super::manifest::{scan_manifest_directory, ManifestError};
use super::models::*;
use swch::storage::{Account, Database, DbError, Game, GameAccount};

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Database error: {0}")]
    Db(#[from] DbError),
    #[error("Keyring error: {0}")]
    Keyring(#[from] KeyringError),
    #[error("Epic client error: {0}")]
    Client(#[from] EpicClientError),
    #[error("Manifest error: {0}")]
    Manifest(#[from] ManifestError),
    #[error("Authorization code format error: {0}")]
    AuthCode(#[from] AuthCodeError),
    #[error("Account not found: {0}")]
    AccountNotFound(String),
    #[error("No active account selected")]
    NoActiveAccount,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSummary {
    pub total_entitlements: usize,
    pub base_games_count: usize,
    pub dlc_count: usize,
    pub installed_count: usize,
}

#[derive(Clone)]
pub struct EpicAuthStore {
    db: Database,
    keyring: Arc<dyn CredentialStore>,
    client: EpicClient,
    active_account_id: Arc<RwLock<Option<String>>>,
}

impl EpicAuthStore {
    pub fn new(
        db: Database,
        keyring: Arc<dyn CredentialStore>,
        client: EpicClient,
    ) -> Self {
        Self {
            db,
            keyring,
            client,
            active_account_id: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_memory_store(db: Database, client: EpicClient) -> Self {
        let keyring = Arc::new(MemoryCredentialStore::new());
        Self::new(db, keyring, client)
    }

    pub fn keyring(&self) -> &Arc<dyn CredentialStore> {
        &self.keyring
    }

    pub fn client(&self) -> &EpicClient {
        &self.client
    }

    /// Authenticates with an authorization code, stores permanent DeviceAuth in keyring, and saves to DB.
    pub async fn login_with_code(&self, raw_input: &str) -> Result<EpicAccount, StoreError> {
        let code = sanitize_auth_code(raw_input)?;
        let token_resp = self.client.exchange_code_for_token(&code).await?;

        // Immediately create permanent DeviceAuth
        let device_auth = self
            .client
            .create_device_auth(&token_resp.account_id, &token_resp.access_token)
            .await?;

        // Save encrypted DeviceAuth in Keyring
        self.keyring.set_device_auth(&device_auth)?;

        let now = Utc::now().to_rfc3339();
        let epic_acc_id = format!("acc_epic_{}", token_resp.account_id);

        // Deactivate any currently active Epic accounts
        let existing = self.get_accounts().await?;
        for acc in existing {
            if acc.is_active {
                let db_acc = Account {
                    id: format!("acc_epic_{}", acc.account_id),
                    launcher: "epic".to_string(),
                    username: acc.account_id.clone(),
                    display_name: acc.display_name.clone(),
                    email: acc.email.clone(),
                    is_active: false,
                    last_used: acc.last_used.clone(),
                };
                self.db.upsert_account(&db_acc).await?;
            }
        }

        // Insert new active account
        let db_account = Account {
            id: epic_acc_id.clone(),
            launcher: "epic".to_string(),
            username: token_resp.account_id.clone(),
            display_name: token_resp.display_name.clone(),
            email: None,
            is_active: true,
            last_used: Some(now.clone()),
        };
        self.db.upsert_account(&db_account).await?;

        // Update in-memory active cache
        *self.active_account_id.write().await = Some(token_resp.account_id.clone());

        Ok(EpicAccount {
            account_id: token_resp.account_id,
            display_name: token_resp.display_name,
            email: None,
            is_active: true,
            last_used: Some(now),
        })
    }

    /// Retrieves all Epic accounts from SQLite
    pub async fn get_accounts(&self) -> Result<Vec<EpicAccount>, StoreError> {
        let all = self.db.get_accounts().await?;
        let epic_accounts = all
            .into_iter()
            .filter(|a| a.launcher == "epic")
            .map(|a| EpicAccount {
                account_id: a.username,
                display_name: a.display_name,
                email: a.email,
                is_active: a.is_active,
                last_used: a.last_used,
            })
            .collect();
        Ok(epic_accounts)
    }

    /// Retrieves the currently active Epic account
    pub async fn get_active_account(&self) -> Result<Option<EpicAccount>, StoreError> {
        let accounts = self.get_accounts().await?;
        Ok(accounts.into_iter().find(|a| a.is_active))
    }

    /// Switches active account in SQLite and in-memory cache, measuring latency
    pub async fn switch_active_account(
        &self,
        account_id: &str,
    ) -> Result<(EpicAccount, std::time::Duration), StoreError> {
        let accounts = self.get_accounts().await?;
        let target = accounts
            .iter()
            .find(|a| a.account_id == account_id)
            .cloned()
            .ok_or_else(|| StoreError::AccountNotFound(account_id.to_string()))?;

        let now = Utc::now().to_rfc3339();
        let start = Instant::now();

        // 1. Update in-memory active cache (< 1us)
        *self.active_account_id.write().await = Some(account_id.to_string());

        // 2. Atomic SQLite WAL update (< 1ms)
        self.db.set_active_epic_account(account_id).await?;

        let elapsed = start.elapsed();

        let updated_account = EpicAccount {
            account_id: target.account_id,
            display_name: target.display_name,
            email: target.email,
            is_active: true,
            last_used: Some(now),
        };

        Ok((updated_account, elapsed))
    }

    /// Gets a fresh valid access token via stored DeviceAuth credentials
    pub async fn get_valid_access_token(&self, account_id: &str) -> Result<String, StoreError> {
        let device_auth = self
            .keyring
            .get_device_auth(account_id)?
            .ok_or_else(|| StoreError::AccountNotFound(account_id.to_string()))?;

        let token_resp = self.client.login_with_device_auth(&device_auth).await?;
        Ok(token_resp.access_token)
    }

    /// Synchronizes cloud entitlements and local manifest files into the SQLite library
    pub async fn sync_library(
        &self,
        account_id: &str,
        manifests_dir: Option<&Path>,
    ) -> Result<SyncSummary, StoreError> {
        let access_token = self.get_valid_access_token(account_id).await?;
        let entitlements = self
            .client
            .fetch_all_entitlements(&access_token, account_id)
            .await?;

        let total_entitlements = entitlements.len();

        // Scan local manifests if directory is provided
        let local_manifests = if let Some(dir) = manifests_dir {
            scan_manifest_directory(dir)?
        } else {
            Vec::new()
        };

        let mut manifest_map: HashMap<String, EpicManifestItem> = HashMap::new();
        for m in local_manifests {
            manifest_map.insert(m.catalog_item_id.clone(), m.clone());
            manifest_map.insert(m.app_name.clone(), m);
        }

        let mut base_games_count = 0;
        let mut dlc_count = 0;
        let mut installed_count = 0;

        for ent in &entitlements {
            let is_dlc = ent.entitlement_type != "EXECUTABLE";
            if is_dlc {
                dlc_count += 1;
                continue;
            }

            base_games_count += 1;
            let local_item = manifest_map.get(&ent.catalog_item_id);
            let is_installed = local_item.map_or(false, |m| m.is_valid_installation());
            if is_installed {
                installed_count += 1;
            }

            let game_id = format!("epic_{}", ent.catalog_item_id);
            let game = Game {
                id: game_id.clone(),
                launcher: "epic".to_string(),
                app_id: ent.catalog_item_id.clone(),
                name: ent.entitlement_name.clone(),
                install_path: local_item.map(|m| m.install_location.clone()),
                executable_path: local_item.map(|m| m.resolve_executable_path().to_string_lossy().to_string()),
                size_bytes: local_item.map_or(0, |m| m.install_size),
                is_installed,
                banner_url: None,
                launch_uri: Some(format!("epic://launch/{}", ent.catalog_item_id)),
                namespace: None,
            };

            self.db.upsert_game(&game).await?;

            let link = GameAccount {
                game_id,
                account_id: format!("acc_epic_{}", account_id),
                launcher: "epic".to_string(),
            };
            self.db.link_game_account(&link).await?;
        }

        Ok(SyncSummary {
            total_entitlements,
            base_games_count,
            dlc_count,
            installed_count,
        })
    }
}
