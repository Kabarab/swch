//! Empirical Adversarial Test Suite for Milestone 1 (teamwork_preview_challenger_m1_2)
//!
//! Objectives:
//! 1. Simulate cold restarts: Persist accounts to real SQLite file on disk, drop all database
//!    and store handles, re-open database connection, verify `restore_sessions()` recovers state
//!    without user interaction.
//! 2. Fault injection: Keyring returns `NoEntry`, corrupted JSON, or backend failure. Verify
//!    graceful handling without panics.
//! 3. Invalidate DeviceAuth: Epic API returns `errors.com.epicgames.account.invalid_account_credentials`.
//!    Verify error categorization (`is_invalid_credentials()`).
//! 4. Keyring backend override: Test switching between OS keyring probe and `SWCH_KEYRING_BACKEND=memory`.
//! 5. Concurrency and edge-case stress testing.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{Form, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::json;
use tokio::sync::RwLock;

use swch::launchers::epic::keyring::KeyringStoreError;
use swch::launchers::epic::{
    default_store, probe_os_keyring, sanitize_auth_code, CredentialStore, DeviceAuth,
    EpicAuthStore, EpicClient, MemoryCredentialStore, EPIC_BASIC_AUTH, EPIC_CLIENT_ID,
};
use swch::storage::{Account, Database};

// ============================================================================
// Mock Server with Fault Injection & Credential Invalidation Capabilities
// ============================================================================

#[derive(Clone, Default)]
struct MockState {
    registered_auths: Arc<RwLock<HashMap<String, (String, String)>>>, // account_id -> (device_id, secret)
    revoked_accounts: Arc<RwLock<Vec<String>>>,
}

struct MockServer {
    router: Router,
    state: MockState,
}

impl MockServer {
    pub async fn new() -> Self {
        let state = MockState::default();

        let app = Router::new()
            .route("/account/api/oauth/token", post(handle_token))
            .route(
                "/account/api/public/account/:account_id/deviceAuth",
                post(handle_create_device_auth),
            )
            .route(
                "/account/api/public/account/:account_id/deviceAuth/:device_id",
                delete(handle_delete_device_auth),
            )
            .route("/account/api/oauth/exchange", get(handle_exchange))
            .with_state(state.clone());

        Self {
            router: app,
            state,
        }
    }

    pub fn router(&self) -> Router {
        self.router.clone()
    }

    pub async fn revoke_device_auth(&self, account_id: &str) {
        self.state
            .revoked_accounts
            .write()
            .await
            .push(account_id.to_string());
    }
}

async fn handle_token(
    State(state): State<MockState>,
    headers: HeaderMap,
    Form(params): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let auth = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok());
    if auth != Some(EPIC_BASIC_AUTH) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "errorCode": "errors.com.epicgames.common.oauth.invalid_client",
                "errorMessage": "Invalid client authentication credentials",
                "numericErrorCode": 1011
            })),
        )
            .into_response();
    }

    match params.get("grant_type").map(|s| s.as_str()) {
        Some("authorization_code") => {
            let code = params.get("code").map(|s| s.as_str()).unwrap_or("");
            if code == "11111111111111111111111111111111" {
                (
                    StatusCode::OK,
                    Json(json!({
                        "access_token": "eg1~mock_token_acc_alpha",
                        "token_type": "bearer",
                        "expires_in": 28800,
                        "account_id": "acc_alpha_id",
                        "client_id": EPIC_CLIENT_ID,
                        "displayName": "AlphaPlayer"
                    })),
                )
                    .into_response()
            } else if code == "22222222222222222222222222222222" {
                (
                    StatusCode::OK,
                    Json(json!({
                        "access_token": "eg1~mock_token_acc_beta",
                        "token_type": "bearer",
                        "expires_in": 28800,
                        "account_id": "acc_beta_id",
                        "client_id": EPIC_CLIENT_ID,
                        "displayName": "BetaPlayer"
                    })),
                )
                    .into_response()
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "errorCode": "errors.com.epicgames.account.oauth.authorization_code_not_found",
                        "errorMessage": "Authorization code not found or expired",
                        "numericErrorCode": 18031
                    })),
                )
                    .into_response()
            }
        }
        Some("device_auth") => {
            let account_id = params.get("account_id").map(|s| s.as_str()).unwrap_or("");
            let device_id = params.get("device_id").map(|s| s.as_str()).unwrap_or("");
            let secret = params.get("secret").map(|s| s.as_str()).unwrap_or("");

            // Check if explicitly revoked
            let revoked = state.revoked_accounts.read().await;
            if revoked.contains(&account_id.to_string()) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "errorCode": "errors.com.epicgames.account.invalid_account_credentials",
                        "errorMessage": "Sorry the credentials you provided are not valid.",
                        "numericErrorCode": 18031
                    })),
                )
                    .into_response();
            }

            let registered = state.registered_auths.read().await;
            if let Some((saved_dev, saved_sec)) = registered.get(account_id) {
                if saved_dev == device_id && saved_sec == secret {
                    return (
                        StatusCode::OK,
                        Json(json!({
                            "access_token": format!("eg1~refreshed_token_{}", account_id),
                            "token_type": "bearer",
                            "expires_in": 28800,
                            "account_id": account_id,
                            "client_id": EPIC_CLIENT_ID,
                            "displayName": format!("DisplayName_{}", account_id),
                            "device_id": device_id
                        })),
                    )
                        .into_response();
                }
            }

            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "errorCode": "errors.com.epicgames.account.invalid_account_credentials",
                    "errorMessage": "Sorry the credentials you provided are not valid.",
                    "numericErrorCode": 18031
                })),
            )
                .into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({"errorMessage": "Unsupported grant_type"})),
        )
            .into_response(),
    }
}

async fn handle_create_device_auth(
    State(state): State<MockState>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let auth = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok());
    if auth.is_none() || !auth.unwrap().starts_with("Bearer ") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"errorMessage": "Missing or invalid bearer token"})),
        )
            .into_response();
    }

    let device_id = format!("device_{}", account_id);
    let secret = format!("secret_{}", account_id);

    state
        .registered_auths
        .write()
        .await
        .insert(account_id.clone(), (device_id.clone(), secret.clone()));

    (
        StatusCode::OK,
        Json(json!({
            "deviceId": device_id,
            "accountId": account_id,
            "secret": secret,
            "userAgent": "UELauncher/11.0.1",
            "created": {
                "dateTime": "2026-09-13T12:00:00.000Z"
            }
        })),
    )
        .into_response()
}

async fn handle_delete_device_auth(
    State(state): State<MockState>,
    Path((account_id, _device_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let auth = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok());
    if auth.is_none() || !auth.unwrap().starts_with("Bearer ") {
        return (StatusCode::UNAUTHORIZED, ()).into_response();
    }
    state.registered_auths.write().await.remove(&account_id);
    (StatusCode::NO_CONTENT, ()).into_response()
}

async fn handle_exchange(headers: HeaderMap) -> impl IntoResponse {
    let auth = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok());
    if auth.is_none() || !auth.unwrap().starts_with("Bearer ") {
        return (StatusCode::UNAUTHORIZED, ()).into_response();
    }
    (
        StatusCode::OK,
        Json(json!({
            "expiresInSeconds": 300,
            "code": "mock_exchange_token_challenger_999",
            "creatingClientId": EPIC_CLIENT_ID
        })),
    )
        .into_response()
}

// ============================================================================
// Custom Fault-Injecting CredentialStore Implementations
// ============================================================================

/// CredentialStore that simulates corrupted JSON in storage.
struct CorruptedCredentialStore {
    corrupted_accounts: Vec<String>,
    valid_store: Arc<MemoryCredentialStore>,
}

impl CredentialStore for CorruptedCredentialStore {
    fn get_device_auth(&self, account_id: &str) -> Result<Option<DeviceAuth>, KeyringStoreError> {
        if self.corrupted_accounts.contains(&account_id.to_string()) {
            let malformed_json = "{ \"accountId\": \"acc\", \"invalid_json\": ";
            let res: Result<DeviceAuth, _> = serde_json::from_str(malformed_json);
            return Err(KeyringStoreError::Serialization(res.unwrap_err()));
        }
        self.valid_store.get_device_auth(account_id)
    }

    fn set_device_auth(&self, auth: &DeviceAuth) -> Result<(), KeyringStoreError> {
        self.valid_store.set_device_auth(auth)
    }

    fn delete_device_auth(&self, account_id: &str) -> Result<(), KeyringStoreError> {
        self.valid_store.delete_device_auth(account_id)
    }
}

/// CredentialStore that simulates system-level platform / daemon failures.
enum FailingMode {
    Unavailable,
    LockPoisoned,
}

struct FailingCredentialStore {
    mode: FailingMode,
}

impl CredentialStore for FailingCredentialStore {
    fn get_device_auth(&self, _account_id: &str) -> Result<Option<DeviceAuth>, KeyringStoreError> {
        match self.mode {
            FailingMode::Unavailable => {
                Err(KeyringStoreError::Unavailable("D-Bus SecretService daemon not running".to_string()))
            }
            FailingMode::LockPoisoned => {
                Err(KeyringStoreError::LockPoisoned("Internal lock poisoned".to_string()))
            }
        }
    }

    fn set_device_auth(&self, _auth: &DeviceAuth) -> Result<(), KeyringStoreError> {
        match self.mode {
            FailingMode::Unavailable => {
                Err(KeyringStoreError::Unavailable("Keyring daemon crashed on write".to_string()))
            }
            FailingMode::LockPoisoned => {
                Err(KeyringStoreError::LockPoisoned("Internal lock poisoned on write".to_string()))
            }
        }
    }

    fn delete_device_auth(&self, _account_id: &str) -> Result<(), KeyringStoreError> {
        Ok(())
    }
}

// ============================================================================
// CHALLENGE 1: Cold Restarts with Real On-Disk SQLite Database
// ============================================================================

#[tokio::test]
async fn test_cold_restart_with_real_sqlite_file() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let db_path = temp_dir.path().join("swch_library.db");

    let mock = MockServer::new().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let persistent_keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    // Phase 1: Boot from scratch, insert accounts, switch active account
    {
        let db = Database::open(&db_path).expect("open sqlite on disk");
        db.run_migrations().await.expect("run migrations on disk");

        let store = EpicAuthStore::new(db.clone(), persistent_keyring.clone(), client.clone());

        // Authenticate Account 1
        let acc1 = store
            .login_with_code("11111111111111111111111111111111")
            .await
            .expect("login acc1");
        assert_eq!(acc1.account_id, "acc_alpha_id");
        assert!(acc1.is_active);

        // Authenticate Account 2
        let acc2 = store
            .login_with_code("22222222222222222222222222222222")
            .await
            .expect("login acc2");
        assert_eq!(acc2.account_id, "acc_beta_id");
        assert!(!acc2.is_active);

        // Switch to Account 2
        let switched = store
            .switch_active_account("acc_beta_id")
            .await
            .expect("switch account");
        assert_eq!(switched.account_id, "acc_beta_id");
        assert!(switched.is_active);

        // Verify active account in DB before shutdown
        let active = db.get_active_epic_account().await.unwrap().unwrap();
        assert_eq!(active.id, "acc_beta_id");

        // DROP all handles: store and db go out of scope here
    }

    // Verify DB file physically exists on disk
    assert!(db_path.exists());
    let metadata = std::fs::metadata(&db_path).expect("read db metadata");
    assert!(metadata.len() > 0, "SQLite file must not be empty");

    // Phase 2: Cold restart simulation (Backend process boots up)
    {
        // Re-open DB from disk without re-creating schema
        let reopened_db = Database::open(&db_path).expect("reopen sqlite from disk");
        let restarted_store = EpicAuthStore::new(
            reopened_db.clone(),
            persistent_keyring.clone(),
            client.clone(),
        );

        // Restore sessions across cold restart
        let report = restarted_store
            .restore_sessions()
            .await
            .expect("restore sessions after cold restart");

        assert_eq!(report.total_accounts, 2);
        assert_eq!(report.restored_sessions, 2);
        assert_eq!(report.active_account_id.as_deref(), Some("acc_beta_id"));

        // Verify active account is immediate and correct
        let active_acc = restarted_store.get_active_account().await.unwrap().unwrap();
        assert_eq!(active_acc.account_id, "acc_beta_id");
        assert!(active_acc.is_active);

        // Verify pre-warmed token is returned without user interaction
        let token = restarted_store
            .get_valid_access_token("acc_beta_id")
            .await
            .expect("get valid access token");
        assert!(!token.is_empty());
        assert!(token.contains("acc_beta_id"));
    }

    // Phase 3: Second consecutive cold restart (Idempotency test)
    {
        let reopened_db_2 = Database::open(&db_path).expect("reopen sqlite second time");
        let restarted_store_2 = EpicAuthStore::new(
            reopened_db_2.clone(),
            persistent_keyring.clone(),
            client.clone(),
        );

        let report_2 = restarted_store_2
            .restore_sessions()
            .await
            .expect("restore sessions on 2nd cold restart");

        assert_eq!(report_2.total_accounts, 2);
        assert_eq!(report_2.restored_sessions, 2);
        assert_eq!(report_2.active_account_id.as_deref(), Some("acc_beta_id"));
    }
}

#[tokio::test]
async fn test_cold_restart_with_empty_database() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let db_path = temp_dir.path().join("empty_swch.db");

    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let client = Arc::new(EpicClient::new());
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let store = EpicAuthStore::new(db, keyring, client);
    let report = store.restore_sessions().await.expect("restore empty DB");
    assert_eq!(report.total_accounts, 0);
    assert_eq!(report.restored_sessions, 0);
    assert_eq!(report.active_account_id, None);
    assert!(store.get_active_account().await.unwrap().is_none());
}

#[tokio::test]
async fn test_cold_restart_with_no_active_flag_in_db() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let db_path = temp_dir.path().join("no_active_flag.db");

    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let keyring = Arc::new(MemoryCredentialStore::new());
    let auth1 = DeviceAuth::new("acc_fallback_1", "dev_1", "sec_1");
    let auth2 = DeviceAuth::new("acc_fallback_2", "dev_2", "sec_2");
    keyring.set_device_auth(&auth1).unwrap();
    keyring.set_device_auth(&auth2).unwrap();

    // Insert 2 accounts with is_active = false
    let a1 = Account {
        id: "acc_fallback_1".to_string(),
        launcher: "epic".to_string(),
        username: "UserOne".to_string(),
        display_name: Some("UserOne".to_string()),
        email: None,
        is_active: false,
        last_used: Some("2026-09-01T00:00:00Z".to_string()),
    };
    let a2 = Account {
        id: "acc_fallback_2".to_string(),
        launcher: "epic".to_string(),
        username: "UserTwo".to_string(),
        display_name: Some("UserTwo".to_string()),
        email: None,
        is_active: false,
        last_used: Some("2026-09-02T00:00:00Z".to_string()),
    };
    db.upsert_epic_account(&a1).await.unwrap();
    db.upsert_epic_account(&a2).await.unwrap();

    let mock = MockServer::new().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client.clone());
    let report = store.restore_sessions().await.unwrap();

    assert_eq!(report.total_accounts, 2);
    assert_eq!(report.restored_sessions, 2);
    // When no account has is_active=true, restore_sessions activates the first account (ordered by last_used desc: acc_fallback_2)
    assert!(report.active_account_id.is_some());
}

// ============================================================================
// CHALLENGE 2: Fault Injection (Missing Keyring, Corrupted JSON, Backend Dead)
// ============================================================================

#[tokio::test]
async fn test_fault_injection_missing_keyring_entry_no_panic() {
    let mock = MockServer::new().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let db = Database::open_in_memory().expect("open memory db");
    db.run_migrations().await.expect("run migrations");

    // Manually insert an account into SQLite that has NO entry in Keyring
    let phantom_acc = Account {
        id: "phantom_user_id".to_string(),
        launcher: "epic".to_string(),
        username: "PhantomGamer".to_string(),
        display_name: Some("PhantomGamer".to_string()),
        email: None,
        is_active: true,
        last_used: None,
    };
    db.upsert_epic_account(&phantom_acc).await.expect("upsert phantom account");

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client.clone());

    // 1. restore_sessions must handle NoEntry gracefully (warn and continue, no panic)
    let report = store.restore_sessions().await.expect("restore_sessions must not fail on NoEntry");
    assert_eq!(report.total_accounts, 1);
    assert_eq!(report.restored_sessions, 0); // No valid session restored
    assert_eq!(report.active_account_id.as_deref(), Some("phantom_user_id"));

    // 2. get_valid_access_token must return DeviceAuthNotFound without panicking
    let err = store.get_valid_access_token("phantom_user_id").await.unwrap_err();
    match err {
        swch::launchers::epic::EpicError::DeviceAuthNotFound(id) => {
            assert_eq!(id, "phantom_user_id");
        }
        other => panic!("Expected DeviceAuthNotFound, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_fault_injection_corrupted_keyring_json_no_panic() {
    let mock = MockServer::new().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let valid_mem = Arc::new(MemoryCredentialStore::new());

    // Seed a valid account
    let valid_auth = DeviceAuth::new("corrupt_acc_id", "dev_corrupt", "sec_corrupt");
    valid_mem.set_device_auth(&valid_auth).unwrap();

    // Wrap with corrupting store
    let corrupted_keyring: Arc<dyn CredentialStore> = Arc::new(CorruptedCredentialStore {
        corrupted_accounts: vec!["corrupt_acc_id".to_string()],
        valid_store: valid_mem,
    });

    let db = Database::open_in_memory().expect("open memory db");
    db.run_migrations().await.expect("run migrations");

    let acc = Account {
        id: "corrupt_acc_id".to_string(),
        launcher: "epic".to_string(),
        username: "CorruptUser".to_string(),
        display_name: Some("CorruptUser".to_string()),
        email: None,
        is_active: true,
        last_used: None,
    };
    db.upsert_epic_account(&acc).await.expect("upsert account");

    let store = EpicAuthStore::new(db.clone(), corrupted_keyring.clone(), client.clone());

    // 1. Direct get_device_auth returns Serialization error
    let raw_res = corrupted_keyring.get_device_auth("corrupt_acc_id");
    assert!(matches!(raw_res, Err(KeyringStoreError::Serialization(_))));

    // 2. restore_sessions handles serialization corruption without panic
    let report = store.restore_sessions().await.expect("restore_sessions must not crash");
    assert_eq!(report.total_accounts, 1);
    assert_eq!(report.restored_sessions, 0);

    // 3. get_valid_access_token returns EpicError::Keyring without panic
    let err = store.get_valid_access_token("corrupt_acc_id").await.unwrap_err();
    match err {
        swch::launchers::epic::EpicError::Keyring(msg) => {
            assert!(msg.contains("serialization error"));
        }
        other => panic!("Expected EpicError::Keyring, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_fault_injection_keyring_backend_failure_no_panic() {
    let mock = MockServer::new().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));

    let failing_keyring: Arc<dyn CredentialStore> = Arc::new(FailingCredentialStore {
        mode: FailingMode::Unavailable,
    });

    let db = Database::open_in_memory().expect("open memory db");
    db.run_migrations().await.expect("run migrations");

    let acc = Account {
        id: "dbus_down_acc".to_string(),
        launcher: "epic".to_string(),
        username: "DbusUser".to_string(),
        display_name: Some("DbusUser".to_string()),
        email: None,
        is_active: true,
        last_used: None,
    };
    db.upsert_epic_account(&acc).await.unwrap();

    let store = EpicAuthStore::new(db.clone(), failing_keyring.clone(), client.clone());

    // restore_sessions must handle backend failure gracefully
    let report = store.restore_sessions().await.expect("restore_sessions must survive backend error");
    assert_eq!(report.total_accounts, 1);
    assert_eq!(report.restored_sessions, 0);

    // login_with_code should propagate the keyring failure cleanly without crashing
    let login_err = store.login_with_code("11111111111111111111111111111111").await.unwrap_err();
    match login_err {
        swch::launchers::epic::EpicError::Keyring(msg) => {
            assert!(msg.contains("crashed on write"));
        }
        other => panic!("Expected EpicError::Keyring, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_fault_injection_lock_poisoned_no_panic() {
    let mock = MockServer::new().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));

    let poisoned_keyring: Arc<dyn CredentialStore> = Arc::new(FailingCredentialStore {
        mode: FailingMode::LockPoisoned,
    });

    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let store = EpicAuthStore::new(db, poisoned_keyring, client);
    let err = store.get_valid_access_token("some_account").await.unwrap_err();
    match err {
        swch::launchers::epic::EpicError::Keyring(msg) => {
            assert!(msg.contains("lock poisoned"));
        }
        other => panic!("Expected EpicError::Keyring with lock poisoned, got: {:?}", other),
    }
}

// ============================================================================
// CHALLENGE 3: Invalidate DeviceAuth & Error Classification
// ============================================================================

#[tokio::test]
async fn test_invalidate_device_auth_and_error_classification() {
    let mock = MockServer::new().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let db = Database::open_in_memory().expect("open memory db");
    db.run_migrations().await.expect("run migrations");

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client.clone());

    // 1. Normal login
    let acc = store
        .login_with_code("11111111111111111111111111111111")
        .await
        .expect("initial login");
    assert_eq!(acc.account_id, "acc_alpha_id");

    // 2. Explicitly revoke the DeviceAuth on Epic's mock backend
    mock.revoke_device_auth("acc_alpha_id").await;

    // 3. Directly call EpicClient::login_with_device_auth with the revoked credential
    let saved_auth = keyring.get_device_auth("acc_alpha_id").unwrap().unwrap();
    let client_err = client.login_with_device_auth(&saved_auth).await.unwrap_err();

    // Verify error classification
    assert!(
        client_err.is_invalid_credentials(),
        "Error must be classified as is_invalid_credentials(): {:?}",
        client_err
    );
    assert!(
        !client_err.is_auth_code_expired(),
        "Must NOT be classified as is_auth_code_expired()"
    );

    // 4. Invalidate token cache by constructing a fresh EpicAuthStore (simulating backend reboot)
    let fresh_store = EpicAuthStore::new(db.clone(), keyring.clone(), client.clone());

    // restore_sessions should not panic when token refresh fails
    let report = fresh_store.restore_sessions().await.expect("restore_sessions should survive revoked auth");
    assert_eq!(report.total_accounts, 1);
    assert_eq!(report.restored_sessions, 1); // DeviceAuth was in keyring

    // 5. Subsequent token acquisition should hit Epic and return invalid_account_credentials
    let token_err = fresh_store.get_valid_access_token("acc_alpha_id").await.unwrap_err();
    assert!(
        token_err.is_invalid_credentials(),
        "Token acquisition error must be classified as is_invalid_credentials(): {:?}",
        token_err
    );
}

#[test]
fn test_error_classification_oracle_matrix() {
    use axum::http::StatusCode;
    use swch::launchers::epic::EpicError;

    // Matrix test of EpicError classification helpers
    let invalid_cred_err = EpicError::Api {
        status: StatusCode::BAD_REQUEST,
        code: Some("errors.com.epicgames.account.invalid_account_credentials".to_string()),
        message: "Invalid credentials".to_string(),
        numeric: Some(18031),
    };
    assert!(invalid_cred_err.is_invalid_credentials());
    assert!(!invalid_cred_err.is_auth_code_expired());

    let expired_code_err = EpicError::Api {
        status: StatusCode::BAD_REQUEST,
        code: Some("errors.com.epicgames.account.oauth.authorization_code_not_found".to_string()),
        message: "Auth code expired".to_string(),
        numeric: Some(18031),
    };
    assert!(!expired_code_err.is_invalid_credentials());
    assert!(expired_code_err.is_auth_code_expired());

    let client_err = EpicError::Api {
        status: StatusCode::UNAUTHORIZED,
        code: Some("errors.com.epicgames.common.oauth.invalid_client".to_string()),
        message: "Invalid client".to_string(),
        numeric: Some(1011),
    };
    assert!(!client_err.is_invalid_credentials());
    assert!(!client_err.is_auth_code_expired());

    let non_api_err = EpicError::DeviceAuthNotFound("acc_xyz".to_string());
    assert!(!non_api_err.is_invalid_credentials());
    assert!(!non_api_err.is_auth_code_expired());
}

// ============================================================================
// CHALLENGE 4: Keyring Backend Override Switching
// ============================================================================

#[test]
fn test_keyring_backend_override_switching() {
    // 1. Explicit memory override (lowercase)
    std::env::set_var("SWCH_KEYRING_BACKEND", "memory");
    let store_mem = default_store();
    let auth = DeviceAuth::new("acc_override_1", "dev_1", "sec_1");
    store_mem.set_device_auth(&auth).unwrap();
    assert_eq!(
        store_mem.get_device_auth("acc_override_1").unwrap().unwrap().secret,
        "sec_1"
    );

    // 2. Explicit memory override (uppercase MEMORY)
    std::env::set_var("SWCH_KEYRING_BACKEND", "MEMORY");
    let store_mem_upper = default_store();
    assert!(store_mem_upper.get_device_auth("acc_override_1").unwrap().is_none()); // New instance is empty
    store_mem_upper.set_device_auth(&auth).unwrap();
    assert_eq!(
        store_mem_upper.get_device_auth("acc_override_1").unwrap().unwrap().secret,
        "sec_1"
    );

    // 3. Explicit OS override (lowercase os)
    std::env::set_var("SWCH_KEYRING_BACKEND", "os");
    let _store_os = default_store();

    // 4. Explicit OS override (uppercase OS)
    std::env::set_var("SWCH_KEYRING_BACKEND", "OS");
    let _store_os_upper = default_store();

    // 5. Unknown override value -> falls back gracefully to probe without panic
    std::env::set_var("SWCH_KEYRING_BACKEND", "unsupported_backend_xyz");
    let store_fallback = default_store();
    let _ = store_fallback.get_device_auth("acc_test");

    // Clean up environment
    std::env::remove_var("SWCH_KEYRING_BACKEND");

    // 6. Probing without env var produces valid boolean
    let _ = probe_os_keyring();
}

// ============================================================================
// CHALLENGE 5: Stress & Concurrency Under Rapid Switching
// ============================================================================

#[tokio::test]
async fn test_rapid_concurrency_and_stress() {
    let mock = MockServer::new().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let db = Database::open_in_memory().expect("open memory db");
    db.run_migrations().await.expect("run migrations");

    let store = Arc::new(EpicAuthStore::new(db.clone(), keyring.clone(), client.clone()));

    // Setup two accounts
    store.login_with_code("11111111111111111111111111111111").await.unwrap();
    store.login_with_code("22222222222222222222222222222222").await.unwrap();

    let start = Instant::now();
    let mut handles = Vec::new();

    // Spawn 20 concurrent tasks switching between accounts and querying active account
    for i in 0..20 {
        let store_clone = store.clone();
        handles.push(tokio::spawn(async move {
            let target = if i % 2 == 0 { "acc_alpha_id" } else { "acc_beta_id" };
            let switched = store_clone.switch_active_account(target).await.unwrap();
            assert!(switched.is_active);

            let active = store_clone.get_active_account().await.unwrap().unwrap();
            assert!(!active.account_id.is_empty());

            let token = store_clone.get_valid_access_token(&active.account_id).await.unwrap();
            assert!(!token.is_empty());
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let elapsed = start.elapsed();
    println!("20 concurrent switch & token fetch operations completed in: {:?}", elapsed);
    assert!(elapsed.as_millis() < 500, "Concurrent operations exceeded timeout: {:?}", elapsed);
}

// ============================================================================
// CHALLENGE 6: Adversarial Code Sanitization Stress Test
// ============================================================================

#[test]
fn test_adversarial_auth_code_sanitization() {
    // 1. Extreme whitespaces and exotic whitespace characters
    let code_with_tabs = "\t\r\n  3b2a0c4f8d914652a92db1029c73e1a0 \r\n\t ";
    assert_eq!(
        sanitize_auth_code(code_with_tabs).unwrap(),
        "3b2a0c4f8d914652a92db1029c73e1a0"
    );

    // 2. Nested JSON / complex JSON object
    let complex_json = r#"{
        "authorizationCode": "3b2a0c4f8d914652a92db1029c73e1a0",
        "redirectUrl": "https://localhost",
        "nested": { "deep": 123 }
    }"#;
    assert_eq!(
        sanitize_auth_code(complex_json).unwrap(),
        "3b2a0c4f8d914652a92db1029c73e1a0"
    );

    // 3. URLs with query string and multiple parameters
    let url = "https://www.epicgames.com/id/api/redirect?state=random_state_123&code=3b2a0c4f8d914652a92db1029c73e1a0&client_id=xyz";
    assert_eq!(
        sanitize_auth_code(url).unwrap(),
        "3b2a0c4f8d914652a92db1029c73e1a0"
    );

    // 4. Fragment URL format
    let frag_url = "https://redirect.internal/#code=3b2a0c4f8d914652a92db1029c73e1a0&scope=basic_profile";
    assert_eq!(
        sanitize_auth_code(frag_url).unwrap(),
        "3b2a0c4f8d914652a92db1029c73e1a0"
    );

    // 5. Malicious input strings: HTML injection, SQL injection, null bytes, long garbage
    assert!(sanitize_auth_code("<script>alert(1)</script>").is_err());
    assert!(sanitize_auth_code("'; DROP TABLE accounts; --").is_err());
    assert!(sanitize_auth_code("3b2a0c4f8d914652a92db1029c73e1a\0").is_err());
    let huge_string = "a".repeat(100_000);
    assert!(sanitize_auth_code(&huge_string).is_err());
}

// ============================================================================
// CHALLENGE 7: Empirical Verification of Discovered Vulnerabilities
// ============================================================================

#[test]
fn test_adversarial_vulnerability_multibyte_slice_panic() {
    // VULNERABILITY A (FIXED): In models.rs, safe multi-byte char slicing prevents panics on multibyte UTF-8 input.
    let result = std::panic::catch_unwind(|| sanitize_auth_code("123456789012345©"));
    assert!(result.is_ok(), "sanitize_auth_code must not panic on multibyte UTF-8 input");
    assert!(result.unwrap().is_err(), "Must return Err on invalid input");
}

#[tokio::test]
async fn test_adversarial_vulnerability_relogin_deactivates_active_account() {
    // VULNERABILITY B (FIXED): In store.rs:111-128, re-logging into an already active account preserves `is_active = true`.
    let mock = MockServer::new().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client.clone());

    // 1. First login: account is active
    let acc1 = store.login_with_code("11111111111111111111111111111111").await.unwrap();
    assert!(acc1.is_active);
    assert_eq!(db.get_active_epic_account().await.unwrap().unwrap().id, "acc_alpha_id");

    // 2. Re-login to the same account
    let acc_relogin = store.login_with_code("11111111111111111111111111111111").await.unwrap();

    // 3. Confirm that SQLite is_active remains 1 (active)
    let active_in_db = db.get_active_epic_account().await.unwrap();
    assert!(
        active_in_db.is_some(),
        "Active account must remain active in SQLite after re-login"
    );
    assert_eq!(active_in_db.unwrap().id, "acc_alpha_id");
    assert!(acc_relogin.is_active);
}

