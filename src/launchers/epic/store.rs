use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use crate::launchers::epic::client::EpicClient;
use crate::launchers::epic::keyring::{default_store, CredentialStore};
use crate::launchers::epic::launch::{
    build_launch_parameters, mask_launch_arguments, LaunchCommand, LaunchGameOptions, LaunchResult,
};
use crate::launchers::epic::models::{
    sanitize_auth_code, DeviceAuth, EpicAccount, EpicAccountDetails, EpicError,
    FortniteAccountSummary, SyncSummary,
};
use crate::parsers::{resolve_default_manifest_dirs, scan_manifest_directory, EpicManifestItem};
use crate::storage::{Account, Database, Game, GameAccount};

/// In-memory cached OAuth access token with expiration time.
#[derive(Clone, Debug)]
pub struct CachedToken {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}

fn native_claim_paths_from_games(games: &[Game]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for game in games.iter().filter(|game| game.launcher == "epic" && game.is_installed) {
        if let Some(path) = game.executable_path.as_deref().filter(|v| !v.trim().is_empty()) {
            let path = PathBuf::from(path);
            if path.is_absolute() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

#[cfg(test)]
mod native_claim_path_tests {
    use super::*;

    #[test]
    fn only_installed_epic_absolute_executables_are_game_guards() {
        let path = std::env::temp_dir().join("native-claim-game.exe");
        let fixture = |launcher: &str, installed, executable_path| Game {
            id: "fixture".into(), launcher: launcher.into(), app_id: "fixture".into(),
            name: "fixture".into(), install_path: None, executable_path,
            size_bytes: 0, is_installed: installed, banner_url: None, launch_uri: None,
            namespace: None,
        };
        let valid = fixture("epic", true, Some(path.to_string_lossy().into_owned()));
        let games = [valid.clone(), valid, fixture("steam", true, Some(path.to_string_lossy().into_owned())),
            fixture("epic", false, Some(path.to_string_lossy().into_owned())),
            fixture("epic", true, None), fixture("epic", true, Some("game.exe".into()))];
        assert_eq!(native_claim_paths_from_games(&games), vec![path]);
    }
}

impl CachedToken {
    /// Returns true if token has at least 60 seconds of validity remaining.
    pub fn is_valid(&self) -> bool {
        Utc::now() + Duration::seconds(60) < self.expires_at
    }
}

/// Report produced by restoring persisted Epic sessions on backend startup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRestoreReport {
    pub total_accounts: usize,
    pub restored_sessions: usize,
    pub active_account_id: Option<String>,
}

/// Epic Games account, credential, and session management engine.
/// Coordinates in-memory caching, OS Keyring storage, and SQLite database persistence.
#[derive(Clone)]
pub struct EpicAuthStore {
    pub browser_auth: Arc<super::browser::BrowserAuth>,
    database: Database,
    keyring: Arc<dyn CredentialStore>,
    client: Arc<EpicClient>,
    active_account_id: Arc<RwLock<Option<String>>>,
    token_cache: Arc<RwLock<HashMap<String, CachedToken>>>,
    token_refresh_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl EpicAuthStore {
    /// Creates an EpicAuthStore with explicit dependencies (used in unit tests and production).
    pub fn new(
        database: Database,
        keyring: Arc<dyn CredentialStore>,
        client: Arc<EpicClient>,
    ) -> Self {
        Self {
            browser_auth: Arc::new(super::browser::BrowserAuth::default()),
            database,
            keyring,
            client,
            active_account_id: Arc::new(RwLock::new(None)),
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            token_refresh_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Creates a default store with production OS Keyring and standard Epic client.
    pub fn default_store(database: Database) -> Self {
        Self::new(
            database,
            default_store(),
            Arc::new(EpicClient::new()),
        )
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    pub fn keyring(&self) -> &Arc<dyn CredentialStore> {
        &self.keyring
    }

    pub fn client(&self) -> &Arc<EpicClient> {
        &self.client
    }

    /// Authenticates using a user-provided authorization code.
    ///
    /// Flow:
    /// 1. Sanitizes input code.
    /// 2. Exchanges code for initial bearer token.
    /// 3. Immediately creates permanent DeviceAuth credentials (eliminates refresh token expiry).
    /// 4. Saves permanent DeviceAuth in OS Keyring.
    /// 5. Persists account metadata in SQLite.
    /// 6. Caches access token in memory.
    /// 7. Activates account if no active account currently exists.
    pub async fn login_with_code(&self, raw_code: &str) -> Result<EpicAccount, EpicError> {
        let code = sanitize_auth_code(raw_code)?;

        // 1. Exchange authorization code for token
        let token_resp = self.client.exchange_code_for_token(&code).await?;

        // 2. Attempt permanent DeviceAuth, or fallback gracefully to refresh_token
        let device_auth = match self
            .client
            .create_device_auth(&token_resp.account_id, &token_resp.access_token)
            .await
        {
            Ok(mut da) => {
                da.refresh_token = token_resp.refresh_token.clone();
                da
            }
            Err(e) => {
                warn!(
                    "Epic create_device_auth error: {}. Falling back to refresh_token for account {}",
                    e, token_resp.account_id
                );
                DeviceAuth {
                    account_id: token_resp.account_id.clone(),
                    device_id: String::new(),
                    secret: String::new(),
                    refresh_token: token_resp.refresh_token.clone(),
                    user_agent: None,
                    created: None,
                }
            }
        };

        // 3. Store in Database and Keyring
        let json = serde_json::to_string(&device_auth)?;
        self.database.save_epic_credential(&device_auth.account_id, &json).await?;
        self.keyring.set_device_auth(&device_auth)?;

        // 4. Check if an active account already exists or if this is the active account re-logging in
        let current_active = self.database.get_active_epic_account().await?;
        let should_be_active = match &current_active {
            None => true,
            Some(active) => active.id == token_resp.account_id,
        };

        let display_name = token_resp
            .display_name
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| token_resp.account_id.clone());

        let account = Account {
            id: token_resp.account_id.clone(),
            launcher: "epic".to_string(),
            username: display_name.clone(),
            display_name: Some(display_name.clone()),
            email: None,
            is_active: should_be_active,
            last_used: Some(Utc::now().to_rfc3339()),
        };

        // 5. Persist account in SQLite
        self.database.upsert_epic_account(&account).await?;

        // 6. Cache access token in memory
        let expires_at = Utc::now() + Duration::seconds(token_resp.expires_in);
        self.token_cache.write().await.insert(
            token_resp.account_id.clone(),
            CachedToken {
                access_token: token_resp.access_token,
                expires_at,
            },
        );

        // 7. Update active in-memory pointer
        if should_be_active {
            *self.active_account_id.write().await = Some(token_resp.account_id.clone());
        }

        info!(
            "Authenticated Epic account '{}' ({}) successfully",
            token_resp.account_id, display_name
        );

        Ok(EpicAccount::from(account))
    }

    /// Restores all saved Epic sessions on startup across backend restarts.
    ///
    /// Reads accounts from SQLite, verifies DeviceAuth presence in Keyring,
    /// and pre-warms the access token cache for the active account.
    pub async fn restore_sessions(&self) -> Result<SessionRestoreReport, EpicError> {
        let accounts = self.database.get_epic_accounts().await?;
        info!("Restoring Epic sessions for {} accounts", accounts.len());

        let mut restored_count = 0;
        let mut active_id = None;

        for account in &accounts {
            let mut da_opt = None;

            if let Ok(Some(json)) = self.database.get_epic_credential(&account.id).await {
                if let Ok(da) = serde_json::from_str::<DeviceAuth>(&json) {
                    da_opt = Some(da);
                }
            }

            if da_opt.is_none() {
                da_opt = match self.keyring.get_device_auth(&account.id) {
                    Ok(Some(da)) => Some(da),
                    _ => None,
                };
            }

            match da_opt {
                Some(device_auth) => {
                    restored_count += 1;
                    let _ = self.keyring.set_device_auth(&device_auth);
                    if account.is_active {
                        active_id = Some(account.id.clone());
                        // Use the same serialized refresh/persistence path as
                        // API requests arriving immediately after a restart.
                        match self.get_valid_access_token(&account.id).await {
                            Ok(_) => {
                                info!("Pre-warmed token for active Epic account '{}'", account.id);
                            }
                            Err(e) => {
                                warn!(
                                    "Could not prepare active Epic session on boot: {}. Will retry on demand.",
                                    e
                                );
                            }
                        }
                    }
                }
                None => {
                    warn!(
                        "Epic account '{}' exists in DB but has no credentials in Keyring or DB. Requires re-login.",
                        account.id
                    );
                }
            }
        }

        // Synchronize in-memory active account pointer
        if let Some(ref id) = active_id {
            *self.active_account_id.write().await = Some(id.clone());
        } else if let Some(first) = accounts.first() {
            *self.active_account_id.write().await = Some(first.id.clone());
            let _ = self.database.set_active_epic_account(&first.id).await;
            active_id = Some(first.id.clone());
        }

        Ok(SessionRestoreReport {
            total_accounts: accounts.len(),
            restored_sessions: restored_count,
            active_account_id: active_id,
        })
    }

    /// Obtains a valid access token for the given account.
    ///
    /// High-performance fast path: returns cached token if valid (< 1us).
    /// Refresh path: uses permanent DeviceAuth from Keyring to obtain fresh token (< 200ms).
    pub async fn get_valid_access_token(&self, account_id: &str) -> Result<String, EpicError> {
        // A refresh token can be consumed once. Concurrent API requests on a
        // cold start must share one refresh and its durably saved replacement.
        let refresh_lock = self.token_refresh_locks.lock().await
            .entry(account_id.to_string()).or_default().clone();
        let _refresh_guard = refresh_lock.lock().await;
        // Fast path: memory cache hit
        {
            let cache = self.token_cache.read().await;
            if let Some(token) = cache.get(account_id) {
                if token.is_valid() {
                    return Ok(token.access_token.clone());
                }
            }
        }

        // Cache miss or expired: use permanent DeviceAuth or RefreshToken
        let mut device_auth = None;

        if let Ok(Some(json)) = self.database.get_epic_credential(account_id).await {
            if let Ok(da) = serde_json::from_str::<DeviceAuth>(&json) {
                device_auth = Some(da);
            }
        }

        if device_auth.is_none() {
            device_auth = match self.keyring.get_device_auth(account_id) {
                Ok(Some(da)) => Some(da),
                Ok(None) => None,
                Err(e) => return Err(e.into()),
            };
        }

        let device_auth = device_auth.ok_or_else(|| EpicError::DeviceAuthNotFound(account_id.to_string()))?;

        let token_resp = if !device_auth.device_id.is_empty() && !device_auth.secret.is_empty() {
            match self.client.login_with_device_auth(&device_auth).await {
                Ok(resp) => resp,
                Err(e) => {
                    if let Some(ref rt) = device_auth.refresh_token {
                        self.client.login_with_refresh_token(rt).await?
                    } else {
                        return Err(e);
                    }
                }
            }
        } else if let Some(ref rt) = device_auth.refresh_token {
            self.client.login_with_refresh_token(rt).await?
        } else {
            return Err(EpicError::DeviceAuthNotFound(account_id.to_string()));
        };

        if token_resp.account_id != account_id {
            return Err(EpicError::Auth("Epic returned a session for a different account".into()));
        }

        if let Some(ref new_rt) = token_resp.refresh_token {
            let mut updated = device_auth.clone();
            updated.refresh_token = Some(new_rt.clone());
            let json = serde_json::to_string(&updated)?;
            self.database.save_epic_credential(account_id, &json).await?;
            let _ = self.keyring.set_device_auth(&updated);
        }

        let expires_at = Utc::now() + Duration::seconds(token_resp.expires_in);

        self.token_cache.write().await.insert(
            account_id.to_string(),
            CachedToken {
                access_token: token_resp.access_token.clone(),
                expires_at,
            },
        );

        Ok(token_resp.access_token)
    }

    /// Obtain a fresh, single-use login for the native launcher. Its cookies and
    /// RememberMe tokens are independent of SWCH's OAuth session and may expire.
    pub async fn launcher_exchange_code(&self, account_id: &str) -> Result<String, EpicError> {
        let access = self.get_valid_access_token(account_id).await?;
        let exchange = self.client.get_exchange_code(&access).await?;
        if exchange.code.is_empty() || exchange.code.len() > 512
            || !exchange.code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            return Err(EpicError::Auth("Epic returned an invalid launcher login code".into()));
        }
        Ok(exchange.code)
    }

    pub async fn launcher_refresh_token(&self, account_id: &str) -> Result<String, EpicError> {
        let code = self.launcher_exchange_code(account_id).await?;
        let session = self.client.redeem_launcher_exchange(&code).await?;
        if session.account_id != account_id {
            return Err(EpicError::Auth("Launcher session belongs to a different account".into()));
        }
        session.refresh_token.filter(|t| !t.is_empty())
            .ok_or_else(|| EpicError::Auth("Epic did not return a launcher refresh token".into()))
    }

    /// Installed game executables used to avoid disrupting gameplay during
    /// native claiming. This is local metadata, never credentials or tokens.
    pub async fn native_claim_game_paths(&self) -> Result<Vec<PathBuf>, EpicError> {
        let games = self.database.get_games().await?;
        let mut paths = native_claim_paths_from_games(&games);
        // Manifests also cover installations whose library has not been synced.
        for manifest in crate::parsers::scan_all_default_manifests() {
            if manifest.is_valid_installation() && !manifest.is_dlc() {
                let path = manifest.resolve_executable_path();
                if path.is_absolute() && !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
        Ok(paths)
    }

    /// Switches the active account in SQLite and the in-memory cache.
    pub async fn switch_active_account(&self, account_id: &str) -> Result<EpicAccount, EpicError> {
        // 1. Atomic SQLite WAL update (< 2ms)
        if let Err(e) = self.database.set_active_epic_account(account_id).await {
            match &e {
                crate::storage::DbError::Io(msg) if msg.contains("not found in database") => {
                    return Err(EpicError::AccountNotFound(account_id.to_string()));
                }
                _ => return Err(EpicError::from(e)),
            }
        }

        // 2. Fetch updated account model to verify and get canonical ID
        let accounts = self.database.get_epic_accounts().await?;
        let account = accounts
            .into_iter()
            .find(|a| a.id == account_id || a.username == account_id)
            .ok_or_else(|| EpicError::AccountNotFound(account_id.to_string()))?;

        // 3. In-memory cache update (< 1us) ONLY after SQLite WAL confirmation
        *self.active_account_id.write().await = Some(account.id.clone());

        Ok(EpicAccount::from(account))
    }

    /// Retrieves the currently active Epic account.
    pub async fn get_active_account(&self) -> Result<Option<EpicAccount>, EpicError> {
        let active_id = self.active_account_id.read().await.clone();
        if let Some(id) = active_id {
            let accounts = self.database.get_epic_accounts().await?;
            if let Some(acc) = accounts.into_iter().find(|a| a.id == id) {
                return Ok(Some(EpicAccount::from(acc)));
            }
        }
        let db_acc = self.database.get_active_epic_account().await?;
        Ok(db_acc.map(EpicAccount::from))
    }

    /// Retrieves all Epic accounts from SQLite.
    pub async fn find_account(&self, target: &str) -> Result<EpicAccount, EpicError> {
        self.database.get_epic_accounts().await?.into_iter()
            .find(|a| a.id == target || a.username == target)
            .map(EpicAccount::from)
            .ok_or_else(|| EpicError::AccountNotFound(target.to_string()))
    }

    /// Retrieves all Epic accounts from SQLite.
    pub async fn get_accounts(&self) -> Result<Vec<EpicAccount>, EpicError> {
        let accounts = self.database.get_epic_accounts().await?;
        Ok(accounts.into_iter().map(EpicAccount::from).collect())
    }

    /// Fetches detailed Epic account information (2FA, verified email, country, connected platforms).
    pub async fn get_account_details(&self, account_id: &str) -> Result<EpicAccountDetails, EpicError> {
        let token = self.get_valid_access_token(account_id).await?;
        match self.client.fetch_account_details(&token, account_id).await {
            Ok(details) => Ok(details),
            Err(e) if matches!(e, EpicError::Auth(_) | EpicError::Api { status: reqwest::StatusCode::UNAUTHORIZED, .. }) => {
                // Stale token in memory; invalidate and retry once with fresh credentials
                self.token_cache.write().await.remove(account_id);
                let fresh_token = self.get_valid_access_token(account_id).await?;
                self.client.fetch_account_details(&fresh_token, account_id).await
            }
            Err(e) => Err(e),
        }
    }

    /// Fetches complete Fortnite summary (wallet, Battle Pass, skins, locker cosmetics, STW status).
    /// Caches result in SQLite for 30 minutes; bypasses cache when `force_refresh` is true.
    pub async fn get_fortnite_summary(
        &self,
        account_id: &str,
        force_refresh: bool,
    ) -> Result<FortniteAccountSummary, EpicError> {
        let cache_key = format!("epic:fortnite:summary:{}", account_id);

        if !force_refresh {
            if let Ok(Some((cached_json, updated_at))) = self.database.steam_cache_get(&cache_key).await {
                let now = chrono::Utc::now().timestamp();
                // 30-minute cache TTL
                if now - updated_at < 1800 {
                    if let Ok(summary) = serde_json::from_str::<FortniteAccountSummary>(&cached_json) {
                        return Ok(summary);
                    }
                }
            }
        }

        let token = self.get_valid_access_token(account_id).await?;
        let summary = match self.client.fetch_fortnite_summary(&token, account_id).await {
            Ok(s) => s,
            Err(e) if matches!(e, EpicError::Auth(_) | EpicError::Api { status: reqwest::StatusCode::UNAUTHORIZED, .. }) => {
                // Stale token in memory; invalidate and retry once with fresh credentials
                self.token_cache.write().await.remove(account_id);
                let fresh_token = self.get_valid_access_token(account_id).await?;
                self.client.fetch_fortnite_summary(&fresh_token, account_id).await?
            }
            Err(e) => return Err(e),
        };

        // Cache in SQLite
        if let Ok(json) = serde_json::to_string(&summary) {
            let _ = self.database.steam_cache_put(&cache_key, &json).await;
        }

        Ok(summary)
    }

    /// Removes an Epic account by account ID or username.
    ///
    /// Flow:
    /// 1. Finds the canonical account in SQLite (returns AccountNotFound if missing).
    /// 2. Performs best-effort remote DeviceAuth revocation on Epic backend if token is available.
    /// 3. Deletes permanent DeviceAuth credentials from Keyring.
    /// 4. Evicts cached OAuth tokens from memory.
    /// 5. Deletes the account row from SQLite (cascading to game_accounts).
    /// 6. If the removed account was active, automatically activates the next remaining account
    ///    or sets active to None if no accounts remain.
    pub async fn remove_account(&self, account_id: &str) -> Result<(), EpicError> {
        // 1. Locate account in database
        let accounts = self.database.get_epic_accounts().await?;
        let target_account = accounts
            .into_iter()
            .find(|a| a.id == account_id || a.username == account_id)
            .ok_or_else(|| EpicError::AccountNotFound(account_id.to_string()))?;

        let canonical_id = target_account.id;
        let was_active = target_account.is_active;

        // 2. Best-effort remote revocation on Epic backend (non-blocking for offline use)
        let mut device_auth_opt = match self.database.get_epic_credential(&canonical_id).await {
            Ok(Some(json)) => serde_json::from_str::<DeviceAuth>(&json).ok(),
            _ => None,
        };
        if device_auth_opt.is_none() {
            device_auth_opt = self.keyring.get_device_auth(&canonical_id).ok().flatten();
        }

        if let Some(device_auth) = device_auth_opt {
            if !device_auth.device_id.is_empty() {
                if let Ok(token) = self.get_valid_access_token(&canonical_id).await {
                    let _ = self
                        .client
                        .delete_device_auth(&canonical_id, &device_auth.device_id, &token)
                        .await;
                }
            }
        }

        // 3. Remove DeviceAuth credential from Database and Keyring
        let _ = self.database.delete_epic_credential(&canonical_id).await;
        let _ = self.keyring.delete_device_auth(&canonical_id);

        // 4. Invalidate memory token cache
        {
            let mut cache = self.token_cache.write().await;
            cache.remove(&canonical_id);
            cache.remove(account_id);
        }

        // 5. Delete account record from SQLite
        self.database.delete_epic_account(&canonical_id).await?;

        // 6. Handle active account state transition
        let is_active_target =
            was_active || self.active_account_id.read().await.as_deref() == Some(&canonical_id);

        if is_active_target {
            let remaining = self.database.get_epic_accounts().await?;
            if let Some(next) = remaining.first() {
                self.database.set_active_epic_account(&next.id).await?;
                *self.active_account_id.write().await = Some(next.id.clone());
                info!(
                    "Removed active Epic account '{}'; promoted '{}' as active",
                    canonical_id, next.id
                );
            } else {
                *self.active_account_id.write().await = None;
                info!(
                    "Removed active Epic account '{}'; no remaining accounts",
                    canonical_id
                );
            }
        } else {
            info!("Removed inactive Epic account '{}'", canonical_id);
        }

        Ok(())
    }

    /// Synchronizes cloud entitlements and local manifest files into the SQLite database.
    ///
    /// Algorithm:
    /// 1. Obtains a valid access token for `account_id` via `get_valid_access_token` (using memory cache or Keyring DeviceAuth).
    /// 2. Fetches cloud entitlements via `client.fetch_all_entitlements(&access_token, account_id)`.
    /// 3. Scans local `.item` manifests (from `manifests_dir` if provided, else from platform default directories).
    /// 4. Builds an O(1) correlation map indexing manifests by `catalog_item_id`, `app_name`, and `display_name`.
    /// 5. Separates DLC/add-on entitlements (`entitlement_type != "EXECUTABLE"`) from base games.
    /// 6. Correlates base game entitlements with local manifests to set `is_installed`, `install_path`, and `executable_path`.
    /// 7. Atomically persists all games and `game_accounts` ownership links in a single SQLite WAL transaction.
    /// 8. Returns `SyncSummary` containing detailed counts.
    pub async fn sync_library(
        &self,
        account_id: &str,
        manifests_dir: Option<&Path>,
    ) -> Result<SyncSummary, EpicError> {
        // 1. Obtain valid access token (refreshes via DeviceAuth if expired)
        let access_token = self.get_valid_access_token(account_id).await?;

        // 2. Fetch cloud entitlements from Epic backend
        let entitlements = self
            .client
            .fetch_all_entitlements(&access_token, account_id)
            .await?;

        let total_entitlements = entitlements.len();

        // 3. Scan local manifests
        let local_manifests = if let Some(dir) = manifests_dir {
            scan_manifest_directory(dir).unwrap_or_default()
        } else if self.client.is_mock() {
            Vec::new()
        } else {
            let mut manifests = Vec::new();
            for dir in resolve_default_manifest_dirs() {
                if let Ok(items) = scan_manifest_directory(&dir) {
                    manifests.extend(items);
                }
            }
            manifests
        };

        // 4. Index manifests for O(1) matching
        let mut manifest_map: HashMap<String, EpicManifestItem> = HashMap::new();
        for m in &local_manifests {
            let keys = [
                m.catalog_item_id.clone(),
                m.catalog_item_id.to_lowercase(),
                m.app_name.clone(),
                m.app_name.to_lowercase(),
                m.display_name.clone(),
                m.display_name.to_lowercase(),
            ];
            for k in keys {
                if let Some(existing) = manifest_map.get(&k) {
                    // Prefer incomplete/updating manifest so updating state is not lost
                    if !existing.is_incomplete_install.unwrap_or(false) && m.is_incomplete_install.unwrap_or(false) {
                        manifest_map.insert(k, m.clone());
                    }
                } else {
                    manifest_map.insert(k, m.clone());
                }
            }
        }

        // 5. Fetch catalog offers concurrently across all unique namespaces for rich metadata (real titles & cover posters)
        let unique_namespaces: HashSet<String> = entitlements
            .iter()
            .filter(|e| e.is_executable())
            .map(|e| e.namespace.clone())
            .filter(|ns| ns != "ue")
            .collect();

        info!(
            "Fetching Epic catalog offers concurrently across {} namespaces for account '{}'",
            unique_namespaces.len(),
            account_id
        );

        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        let mut offer_tasks = JoinSet::new();
        for ns in unique_namespaces {
            let http = http.clone();
            let token = access_token.clone();
            offer_tasks.spawn(async move {
                let url = format!(
                    "https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace/{}/offers",
                    ns
                );
                let resp = http
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .header("User-Agent", "UERefreshed/1.0")
                    .send()
                    .await;
                if let Ok(r) = resp {
                    if let Ok(val) = r.json::<serde_json::Value>().await {
                        return Some((ns, val));
                    }
                }
                None
            });
        }

        #[derive(Debug, Clone)]
        struct EpicOfferMeta {
            title: String,
            poster_url: Option<String>,
            offer_type: String,
            namespace: String,
        }

        let mut offers_by_item_id: HashMap<String, EpicOfferMeta> = HashMap::new();
        while let Some(res) = offer_tasks.join_next().await {
            if let Ok(Some((ns, val))) = res {
                if let Some(elements) = val.get("elements").and_then(|v| v.as_array()) {
                    for offer in elements {
                        let title = offer
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if title.is_empty() {
                            continue;
                        }
                        let offer_type = offer
                            .get("offerType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        let mut tall_img = None;
                        let mut wide_img = None;

                        if let Some(imgs) = offer.get("keyImages").and_then(|v| v.as_array()) {
                            for img in imgs {
                                let itype = img.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                let iurl = img.get("url").and_then(|v| v.as_str());
                                if itype == "OfferImageTall"
                                    || itype == "DieselGameBoxTall"
                                    || itype == "Thumbnail"
                                {
                                    if tall_img.is_none() || itype == "OfferImageTall" {
                                        tall_img = iurl.map(|s| s.to_string());
                                    }
                                } else if itype == "OfferImageWide"
                                    || itype == "DieselGameBox"
                                    || itype == "HorizontalDetailImage"
                                {
                                    if wide_img.is_none() || itype == "OfferImageWide" {
                                        wide_img = iurl.map(|s| s.to_string());
                                    }
                                }
                            }
                        }

                        let poster_url = tall_img.or(wide_img);

                        let meta = EpicOfferMeta {
                            title,
                            poster_url,
                            offer_type: offer_type.clone(),
                            namespace: ns.clone(),
                        };

                        if let Some(items) = offer.get("items").and_then(|v| v.as_array()) {
                            for item in items {
                                if let Some(item_id) = item.get("id").and_then(|v| v.as_str()) {
                                    if let Some(existing) = offers_by_item_id.get(item_id) {
                                        if (existing.offer_type == "BASE_GAME"
                                            || existing.offer_type == "EDITION")
                                            && offer_type != "BASE_GAME"
                                            && offer_type != "EDITION"
                                        {
                                            continue;
                                        }
                                    }
                                    offers_by_item_id.insert(item_id.to_string(), meta.clone());
                                    offers_by_item_id.insert(item_id.to_lowercase(), meta.clone());
                                }
                            }
                        }

                        if let Some(oid) = offer.get("id").and_then(|v| v.as_str()) {
                            offers_by_item_id
                                .entry(oid.to_string())
                                .or_insert_with(|| meta.clone());
                            offers_by_item_id
                                .entry(oid.to_lowercase())
                                .or_insert_with(|| meta.clone());
                        }
                    }
                }
            }
        }

        let mut base_games_count = 0;
        let mut dlc_count = 0;
        let mut installed_count = 0;

        let mut games_to_persist = Vec::new();
        let mut links_to_persist = Vec::new();

        let accounts = self.database.get_epic_accounts().await?;
        let canonical_account_id = accounts
            .iter()
            .find(|a| a.id == account_id || a.username == account_id)
            .map(|a| a.id.clone())
            .unwrap_or_else(|| account_id.to_string());

        let mut seen_titles: HashSet<String> = HashSet::new();

        for ent in &entitlements {
            info!("Entitlement: '{}' type='{}' id='{}'", ent.entitlement_name, ent.entitlement_type, ent.id);
            // 1. DLCs and add-ons are classified by entitlement_type != "EXECUTABLE"
            if !ent.is_executable() {
                dlc_count += 1;
                continue;
            }

            // 2. Correlate with public catalog offer metadata
            let offer_meta = offers_by_item_id
                .get(&ent.catalog_item_id)
                .or_else(|| offers_by_item_id.get(&ent.catalog_item_id.to_lowercase()))
                .or_else(|| offers_by_item_id.get(&ent.entitlement_name))
                .or_else(|| offers_by_item_id.get(&ent.entitlement_name.to_lowercase()));

            if let Some(meta) = offer_meta {
                let ot = meta.offer_type.to_uppercase();
                if ot == "DLC" || ot == "ADD_ON" || ot == "VIRTUAL_CURRENCY" || ot == "CONSUMABLE" {
                    dlc_count += 1;
                    continue;
                }
                let lower_title = meta.title.to_lowercase();
                if lower_title.contains("soundtrack")
                    || lower_title.contains("digital artbook")
                    || lower_title.contains("season pass")
                {
                    dlc_count += 1;
                    continue;
                }
            } else {
                // If not found in offers, skip Fortnite cosmetics / mtx and UE developer items
                if ent.namespace == "ue" {
                    continue;
                }
                if ent.namespace == "fn" && ent.entitlement_name.starts_with("FN_") {
                    dlc_count += 1;
                    continue;
                }
            }

            // 3. Correlate with local manifest
            let local_item = manifest_map
                .get(&ent.catalog_item_id)
                .or_else(|| manifest_map.get(&ent.catalog_item_id.to_lowercase()))
                .or_else(|| manifest_map.get(&ent.entitlement_name))
                .or_else(|| manifest_map.get(&ent.entitlement_name.to_lowercase()));

            let is_installed = local_item.is_some_and(|m| m.is_valid_installation());
            if is_installed {
                installed_count += 1;
            }

            // 4. Resolve human-readable game title
            let is_raw_hex = ent.entitlement_name.len() == 32
                && ent.entitlement_name.chars().all(|c| c.is_ascii_hexdigit());

            // Skip unresolvable internal assets, cosmetic packs, and DLCs that have no catalog offer and no local installation
            if offer_meta.is_none() && local_item.is_none() {
                if is_raw_hex || ent.entitlement_name.starts_with("EGS_") || ent.namespace == "fn" {
                    dlc_count += 1;
                    continue;
                }
            }

            let title = if let Some(meta) = offer_meta {
                meta.title.clone()
            } else if let Some(local) = local_item {
                local.display_name.clone()
            } else if !is_raw_hex {
                ent.entitlement_name.clone()
            } else {
                ent.entitlement_name.clone()
            };

            // 5. Deduplicate games with identical titles
            let norm_title = title.trim().to_lowercase();
            if !seen_titles.insert(norm_title) {
                continue;
            }

            base_games_count += 1;

            let game_id = format!("epic_{}", ent.catalog_item_id);
            let install_path = local_item.map(|m| m.install_location.clone());
            let executable_path = local_item
                .map(|m| m.resolve_executable_path().to_string_lossy().to_string());
            let size_bytes = local_item.map_or(0, |m| m.install_size);
            let banner_url = offer_meta.and_then(|m| m.poster_url.clone());
            let namespace = offer_meta
                .map(|m| m.namespace.clone())
                .unwrap_or_else(|| ent.namespace.clone());

            let game = Game {
                id: game_id.clone(),
                launcher: "epic".to_string(),
                app_id: ent.catalog_item_id.clone(),
                name: title,
                install_path,
                executable_path,
                size_bytes,
                is_installed,
                banner_url,
                launch_uri: Some(format!("epic://launch/{}", ent.catalog_item_id)),
                namespace: Some(namespace),
            };

            games_to_persist.push(game);

            let link = GameAccount {
                game_id,
                account_id: canonical_account_id.clone(),
                launcher: "epic".to_string(),
            };
            links_to_persist.push(link);
        }

        // 6. Include local manifest games not matched by cloud entitlements (e.g. Fortnite, local/offline installs, pending updates)
        for m in &local_manifests {
            if m.is_dlc() {
                continue;
            }
            let norm_title = m.display_name.trim().to_lowercase();
            if !seen_titles.insert(norm_title) {
                continue;
            }
            base_games_count += 1;
            let is_installed = m.is_valid_installation();
            if is_installed {
                installed_count += 1;
            }
            let game_id = format!("epic_{}", m.catalog_item_id);
            let game = Game {
                id: game_id.clone(),
                launcher: "epic".to_string(),
                app_id: m.catalog_item_id.clone(),
                name: m.display_name.clone(),
                install_path: Some(m.install_location.clone()),
                executable_path: Some(m.resolve_executable_path().to_string_lossy().to_string()),
                size_bytes: m.install_size,
                is_installed,
                banner_url: None,
                launch_uri: Some(format!("epic://launch/{}", m.catalog_item_id)),
                namespace: Some(m.catalog_namespace.clone()),
            };
            games_to_persist.push(game);
            let link = GameAccount {
                game_id,
                account_id: canonical_account_id.clone(),
                launcher: "epic".to_string(),
            };
            links_to_persist.push(link);
        }

        // 7. Atomic batch persist in SQLite WAL
        self.database
            .sync_epic_library_batch(&games_to_persist, &links_to_persist)
            .await?;

        info!(
            "Synced library for account '{}': {} entitlements ({} games, {} dlc, {} installed)",
            account_id, total_entitlements, base_games_count, dlc_count, installed_count
        );

        Ok(SyncSummary {
            total_entitlements,
            base_games_count,
            dlc_count,
            installed_count,
        })
    }

    /// Launches an Epic game by name or ID.
    ///
    /// If `account_id` is None, resolves the currently active Epic account.
    /// Obtains a fresh access token, generates a single-use exchange code,
    /// resolves local manifest / library metadata, constructs launch parameters,
    /// masks sensitive passwords in logs, and executes the game binary.
    pub async fn launch_game(
        &self,
        account_id: Option<&str>,
        app_name: &str,
        namespace: Option<&str>,
        additional_args: Option<Vec<String>>,
    ) -> Result<LaunchResult, EpicError> {
        self.launch_game_with_options(LaunchGameOptions {
            account_id: account_id.map(|s| s.to_string()),
            app_name: app_name.to_string(),
            namespace: namespace.map(|s| s.to_string()),
            locale: None,
            additional_args,
            dry_run: false,
        })
        .await
    }

    /// Full options-driven game launch implementation.
    pub async fn launch_game_with_options(
        &self,
        options: LaunchGameOptions,
    ) -> Result<LaunchResult, EpicError> {
        let app_name = options.app_name.trim();
        if app_name.is_empty() {
            return Err(EpicError::Database("app_name must not be empty".to_string()));
        }

        // 1. Resolve Target Epic Account
        let (canonical_account_id, display_name) = match &options.account_id {
            Some(id) if !id.trim().is_empty() => {
                let clean_id = id.trim();
                let accounts = self.database.get_epic_accounts().await?;
                let acc = accounts
                    .into_iter()
                    .find(|a| a.id == clean_id || a.username == clean_id)
                    .ok_or_else(|| EpicError::AccountNotFound(clean_id.to_string()))?;
                let name = acc.display_name.unwrap_or(acc.username);
                (acc.id, name)
            }
            _ => {
                let active = self.get_active_account().await?;
                match active {
                    Some(acc) => (acc.account_id, acc.display_name),
                    None => return Err(EpicError::NoActiveAccount),
                }
            }
        };

        // 2. Obtain Fresh Bearer Access Token
        let access_token = self.get_valid_access_token(&canonical_account_id).await?;

        // 3. Obtain Single-Use Exchange Code (valid 300s)
        let exchange_resp = self.client.get_exchange_code(&access_token).await?;
        let exchange_code = exchange_resp.code;

        // 4. Resolve Game Metadata, Manifest, and Namespace
        let mut manifest_item: Option<EpicManifestItem> = None;
        for dir in resolve_default_manifest_dirs() {
            if let Ok(items) = scan_manifest_directory(&dir) {
                for item in items {
                    if item.app_name.eq_ignore_ascii_case(app_name)
                        || item.catalog_item_id.eq_ignore_ascii_case(app_name)
                        || item.display_name.eq_ignore_ascii_case(app_name)
                    {
                        manifest_item = Some(item);
                        break;
                    }
                }
            }
            if manifest_item.is_some() {
                break;
            }
        }

        if let Some(ref m) = manifest_item {
            if m.is_incomplete_install.unwrap_or(false) {
                return Err(EpicError::Database(format!(
                    "Game '{}' is marked as incomplete installation",
                    m.display_name
                )));
            }
            if m.is_dlc() {
                return Err(EpicError::Database(format!(
                    "Cannot directly launch DLC '{}'",
                    m.display_name
                )));
            }
        }

        let is_simulated = options.dry_run || std::env::var("SWCH_EPIC_SIMULATE_LAUNCH").is_ok();

        let (namespace, executable_path, working_dir) = if let Some(ref m) = manifest_item {
            let ns = options
                .namespace
                .clone()
                .unwrap_or_else(|| m.catalog_namespace.clone());
            let exe = m.resolve_executable_path();
            let work_dir = if exe.is_file() {
                exe.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from(&m.install_location))
            } else {
                PathBuf::from(&m.install_location)
            };
            (ns, Some(exe), Some(work_dir))
        } else {
            let db_games = self
                .database
                .get_epic_library(Some(&canonical_account_id))
                .await
                .unwrap_or_default();
            let db_game = db_games.into_iter().find(|g| {
                g.app_name.eq_ignore_ascii_case(app_name)
                    || g.catalog_item_id.eq_ignore_ascii_case(app_name)
                    || g.title.eq_ignore_ascii_case(app_name)
            });

            // Enforce installation / manifest existence in live mode
            if !is_simulated {
                match &db_game {
                    Some(g) if !g.is_installed || g.executable_path.is_none() => {
                        return Err(EpicError::Database(format!(
                            "Manifest or installation not found for app '{}'",
                            app_name
                        )));
                    }
                    None => {
                        return Err(EpicError::Database(format!(
                            "Manifest or installation not found for app '{}'",
                            app_name
                        )));
                    }
                    _ => {}
                }
            }

            let ns = options
                .namespace
                .clone()
                .or_else(|| {
                    db_game
                        .as_ref()
                        .map(|g| g.namespace.clone())
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or_else(|| "fn".to_string());

            let exe = db_game
                .as_ref()
                .and_then(|g| g.executable_path.clone())
                .map(PathBuf::from);
            let work_dir = db_game
                .as_ref()
                .and_then(|g| g.install_path.clone())
                .map(PathBuf::from);

            (ns, exe, work_dir)
        };

        // 5. Construct CLI Launch Parameters
        let custom_args_slice = options.additional_args.as_deref();
        let raw_args = build_launch_parameters(
            &exchange_code,
            app_name,
            &canonical_account_id,
            &display_name,
            &namespace,
            options.locale.as_deref().or(Some("en")),
            custom_args_slice,
        );

        // 6. Mask Sensitive Secrets
        let display_args = mask_launch_arguments(&raw_args);

        info!(
            "Prepared Epic launch for app '{}' under account '{}' (namespace: '{}'): {}",
            app_name,
            canonical_account_id,
            namespace,
            display_args.join(" ")
        );

        // 7. Process Spawning / Simulation
        let spawned_pid = if is_simulated {
            Some(42424)
        } else if let (Some(ref exe), Some(ref work_dir)) = (&executable_path, &working_dir) {
            if exe.exists() {
                let mut cmd = std::process::Command::new(exe);
                cmd.current_dir(work_dir);
                cmd.args(&raw_args);
                cmd.env("EPIC_APP_NAME", app_name);
                cmd.env("EPIC_NAMESPACE", &namespace);
                cmd.env("EPIC_ACCOUNT_ID", &canonical_account_id);
                cmd.env("EPIC_DISPLAY_NAME", &display_name);

                match cmd.spawn() {
                    Ok(child) => {
                        let pid = child.id();
                        info!("Spawned process for Epic game '{}' with PID {}", app_name, pid);
                        Some(pid)
                    }
                    Err(e) => {
                        error!("Failed to spawn game process for '{}': {}", app_name, e);
                        return Err(EpicError::Io(format!("Process spawn failed: {}", e)));
                    }
                }
            } else {
                return Err(EpicError::Database(format!(
                    "Manifest or installation not found for app '{}'",
                    app_name
                )));
            }
        } else {
            return Err(EpicError::Database(format!(
                "Manifest or installation not found for app '{}'",
                app_name
            )));
        };

        Ok(LaunchResult {
            app_name: app_name.to_string(),
            launch_arguments: raw_args,
            display_arguments: display_args,
            pid: spawned_pid,
            executable_path,
            working_dir,
        })
    }

    /// Prepares a game launch command for a specific account and app name.
    pub async fn prepare_launch(
        &self,
        account_id: &str,
        app_name: &str,
        locale: Option<&str>,
        custom_args: Option<&[String]>,
    ) -> Result<LaunchCommand, EpicError> {
        let accounts = self.database.get_epic_accounts().await?;
        let account = accounts
            .iter()
            .find(|a| a.id == account_id || a.username == account_id)
            .ok_or_else(|| EpicError::AccountNotFound(account_id.to_string()))?;
        let display_name = account
            .display_name
            .clone()
            .unwrap_or_else(|| account.username.clone());

        let access_token = self.get_valid_access_token(&account.id).await?;
        let exchange = self.client.get_exchange_code(&access_token).await?;

        let manifest_dirs = resolve_default_manifest_dirs();
        let mut found_manifest: Option<EpicManifestItem> = None;
        for dir in manifest_dirs {
            if let Ok(manifests) = scan_manifest_directory(&dir) {
                for m in manifests {
                    if m.app_name.eq_ignore_ascii_case(app_name)
                        || m.catalog_item_id.eq_ignore_ascii_case(app_name)
                        || m.display_name.eq_ignore_ascii_case(app_name)
                    {
                        found_manifest = Some(m);
                        break;
                    }
                }
            }
            if found_manifest.is_some() {
                break;
            }
        }

        let manifest = found_manifest.ok_or_else(|| {
            EpicError::Database(format!("Manifest not found for app '{}'", app_name))
        })?;

        let cmd = crate::launchers::epic::launch::prepare_launch_command(
            &manifest,
            &exchange.code,
            &account.id,
            &display_name,
            locale,
            custom_args,
        )?;

        Ok(cmd)
    }
}
