//! Comprehensive test suite for Milestone 1 (M1):
//! Native Epic Games Multi-Account DeviceAuth Store, Keyring, Client, Session Persistence, and REST API.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::Body,
    extract::{Form, Json, Path, State},
    http::{header, HeaderMap, Request, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tower::ServiceExt;

use swch::launchers::epic::{
    default_store, probe_os_keyring, sanitize_auth_code, CredentialStore, DeviceAuth,
    EpicAuthStore, EpicClient, EpicError, MemoryCredentialStore, OAuthTokenResponse, EPIC_BASIC_AUTH,
    EPIC_CLIENT_ID,
};
use swch::server::{create_app_with_state, AppState};
use swch::storage::{Account, Database, Game, GameAccount};

// ============================================================================
// Section 1: Mock Epic Games OAuth & Account Server
// ============================================================================

#[derive(Clone, Default)]
struct MockServerState {
    registered_device_auths: Arc<tokio::sync::RwLock<HashMap<String, (String, String)>>>, // account_id -> (device_id, secret)
    active_tokens: Arc<tokio::sync::RwLock<HashMap<String, String>>>, // access_token -> account_id
}

struct MockEpicServer {
    pub _base_url: String,
    router: Router,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockEpicServer {
    pub async fn start() -> Self {
        let state = MockServerState::default();

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
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral port");
        let local_addr: SocketAddr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{}", local_addr);

        let app_clone = app.clone();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app_clone).await.ok();
        });

        Self {
            _base_url: base_url,
            router: app,
            _handle: handle,
        }
    }

    pub fn router(&self) -> Router {
        self.router.clone()
    }
}

async fn handle_token(
    State(state): State<MockServerState>,
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
            if code == "3b2a0c4f8d914652a92db1029c73e1a0" {
                let token = "eg1~mock_token_user_1";
                state
                    .active_tokens
                    .write()
                    .await
                    .insert(token.to_string(), "4a6313cbbe7d432b8132b5ee67ad53fa".to_string());

                (
                    StatusCode::OK,
                    Json(json!({
                        "access_token": token,
                        "token_type": "bearer",
                        "expires_in": 28800,
                        "account_id": "4a6313cbbe7d432b8132b5ee67ad53fa",
                        "client_id": EPIC_CLIENT_ID,
                        "displayName": "JediMaster99"
                    })),
                )
                    .into_response()
            } else if code == "7f8e9d0a1b2c3d4e5f6a7b8c9d0e1f2a" {
                let token = "eg1~mock_token_user_2";
                state
                    .active_tokens
                    .write()
                    .await
                    .insert(token.to_string(), "9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d".to_string());

                (
                    StatusCode::OK,
                    Json(json!({
                        "access_token": token,
                        "token_type": "bearer",
                        "expires_in": 28800,
                        "account_id": "9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d",
                        "client_id": EPIC_CLIENT_ID,
                        "displayName": "SithLord42"
                    })),
                )
                    .into_response()
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "errorCode": "errors.com.epicgames.account.oauth.authorization_code_not_found",
                        "errorMessage": "Sorry the code you provided is not valid, or has expired",
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

            let registered = state.registered_device_auths.read().await;
            if let Some((saved_dev, saved_secret)) = registered.get(account_id) {
                if saved_dev == device_id && saved_secret == secret {
                    let token = format!("eg1~refreshed_token_{}", account_id);
                    let display_name = if account_id == "4a6313cbbe7d432b8132b5ee67ad53fa" {
                        "JediMaster99"
                    } else {
                        "SithLord42"
                    };
                    return (
                        StatusCode::OK,
                        Json(json!({
                            "access_token": token,
                            "token_type": "bearer",
                            "expires_in": 28800,
                            "account_id": account_id,
                            "client_id": EPIC_CLIENT_ID,
                            "displayName": display_name,
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
            Json(json!({"errorMessage": "invalid or unsupported grant_type"})),
        )
            .into_response(),
    }
}

async fn handle_create_device_auth(
    State(state): State<MockServerState>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let auth = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok());
    let token = auth.and_then(|h| h.strip_prefix("Bearer "));

    let active_tokens = state.active_tokens.read().await;
    let expected_account = token.and_then(|t| active_tokens.get(t));

    if expected_account != Some(&account_id) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"errorMessage": "Invalid or expired bearer token"})),
        )
            .into_response();
    }

    let device_id = format!("device_{}", &account_id[..8]);
    let secret = format!("secret_{}", &account_id[..8]);

    state
        .registered_device_auths
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
                "location": "Coruscant",
                "ipAddress": "127.0.0.1",
                "dateTime": "2026-09-13T12:00:00.000Z"
            }
        })),
    )
        .into_response()
}

async fn handle_delete_device_auth(
    State(state): State<MockServerState>,
    Path((account_id, _device_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let auth = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok());
    if auth.is_none() || !auth.unwrap().starts_with("Bearer ") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"errorMessage": "Unauthorized"})),
        )
            .into_response();
    }

    state.registered_device_auths.write().await.remove(&account_id);
    (StatusCode::NO_CONTENT, ()).into_response()
}

async fn handle_exchange(headers: HeaderMap) -> impl IntoResponse {
    let auth = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok());
    if auth.is_none() || !auth.unwrap().starts_with("Bearer ") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"errorMessage": "Unauthorized"})),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(json!({
            "expiresInSeconds": 300,
            "code": "mock_exchange_code_456_xyz",
            "creatingClientId": EPIC_CLIENT_ID
        })),
    )
        .into_response()
}

// ============================================================================
// Section 2: Unit Tests for Auth Code Sanitizer
// ============================================================================

#[test]
fn test_sanitize_auth_code_hex_raw() {
    let valid_code = "3b2a0c4f8d914652a92db1029c73e1a0";
    let sanitized = sanitize_auth_code(valid_code).unwrap();
    assert_eq!(sanitized, "3b2a0c4f8d914652a92db1029c73e1a0");

    // Uppercase hex should be lowercased
    let upper = "3B2A0C4F8D914652A92DB1029C73E1A0";
    assert_eq!(sanitize_auth_code(upper).unwrap(), "3b2a0c4f8d914652a92db1029c73e1a0");
}

#[test]
fn test_sanitize_auth_code_quoted() {
    let double_quoted = "  \"3b2a0c4f8d914652a92db1029c73e1a0\"  ";
    assert_eq!(
        sanitize_auth_code(double_quoted).unwrap(),
        "3b2a0c4f8d914652a92db1029c73e1a0"
    );

    let single_quoted = "'3b2a0c4f8d914652a92db1029c73e1a0'";
    assert_eq!(
        sanitize_auth_code(single_quoted).unwrap(),
        "3b2a0c4f8d914652a92db1029c73e1a0"
    );
}

#[test]
fn test_sanitize_auth_code_json() {
    let json_payload = r#"{"authorizationCode": "3b2a0c4f8d914652a92db1029c73e1a0", "sid": "123"}"#;
    assert_eq!(
        sanitize_auth_code(json_payload).unwrap(),
        "3b2a0c4f8d914652a92db1029c73e1a0"
    );
}

#[test]
fn test_sanitize_auth_code_url_fragment_and_query() {
    let url_query = "https://www.epicgames.com/id/api/redirect?code=3b2a0c4f8d914652a92db1029c73e1a0&state=none";
    assert_eq!(
        sanitize_auth_code(url_query).unwrap(),
        "3b2a0c4f8d914652a92db1029c73e1a0"
    );

    let url_hash = "https://localhost/callback#code=3b2a0c4f8d914652a92db1029c73e1a0";
    assert_eq!(
        sanitize_auth_code(url_hash).unwrap(),
        "3b2a0c4f8d914652a92db1029c73e1a0"
    );
}

#[test]
fn test_sanitize_auth_code_invalid_inputs() {
    assert!(sanitize_auth_code("").is_err());
    assert!(sanitize_auth_code("   ").is_err());
    assert!(sanitize_auth_code("too_short").is_err());
    assert!(sanitize_auth_code("3b2a0c4f8d914652a92db1029c73e1ag").is_err()); // non-hex 'g'
}

// ============================================================================
// Section 3: Unit Tests for Models & Secret Redaction
// ============================================================================

#[test]
fn test_device_auth_debug_redaction() {
    let auth = DeviceAuth::new("acc_42", "dev_42", "super_secret_password_123");
    let debug_output = format!("{:?}", auth);
    assert!(debug_output.contains("[REDACTED]"));
    assert!(!debug_output.contains("super_secret_password_123"));
}

#[test]
fn test_oauth_token_debug_redaction() {
    let token = OAuthTokenResponse {
        access_token: "eg1~sensitive_bearer_token".to_string(),
        token_type: "bearer".to_string(),
        expires_in: 28800,
        expires_at: None,
        account_id: "acc_42".to_string(),
        client_id: EPIC_CLIENT_ID.to_string(),
        display_name: Some("Gamer".to_string()),
        refresh_token: None,
        refresh_expires: None,
        refresh_expires_at: None,
        device_id: None,
        app: None,
        in_app_id: None,
        internal_client: None,
        client_service: None,
    };
    let debug_output = format!("{:?}", token);
    assert!(debug_output.contains("[REDACTED]"));
    assert!(!debug_output.contains("sensitive_bearer_token"));
}

// ============================================================================
// Section 4: Keyring & CredentialStore Tests
// ============================================================================

#[tokio::test]
async fn test_memory_credential_store_crud_and_concurrency() {
    let store = Arc::new(MemoryCredentialStore::new());
    assert!(store.is_empty());

    let auth1 = DeviceAuth::new("acc_1", "dev_1", "sec_1");
    let auth2 = DeviceAuth::new("acc_2", "dev_2", "sec_2");

    store.set_device_auth(&auth1).unwrap();
    store.set_device_auth(&auth2).unwrap();
    assert_eq!(store.len(), 2);

    let retrieved = store.get_device_auth("acc_1").unwrap().unwrap();
    assert_eq!(retrieved.account_id, "acc_1");
    assert_eq!(retrieved.secret, "sec_1");

    assert!(store.get_device_auth("unknown").unwrap().is_none());

    // Idempotent delete
    store.delete_device_auth("acc_1").unwrap();
    assert!(store.get_device_auth("acc_1").unwrap().is_none());
    store.delete_device_auth("acc_1").unwrap(); // Second delete succeeds

    // Concurrent operations
    let mut handles = Vec::new();
    for i in 0..8 {
        let store_clone = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let id = format!("thread_acc_{}", i);
            let a = DeviceAuth::new(&id, "dev", "sec");
            store_clone.set_device_auth(&a).unwrap();
            let got = store_clone.get_device_auth(&id).unwrap().unwrap();
            assert_eq!(got.account_id, id);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(store.len(), 9); // acc_2 + 8 thread accounts
}

#[test]
fn test_keyring_probe_and_env_override() {
    std::env::set_var("SWCH_KEYRING_BACKEND", "memory");
    let store = default_store();
    let auth = DeviceAuth::new("env_test", "dev", "sec");
    store.set_device_auth(&auth).unwrap();
    assert_eq!(store.get_device_auth("env_test").unwrap().unwrap().secret, "sec");
    std::env::remove_var("SWCH_KEYRING_BACKEND");

    // Probe returns a boolean without panicking
    let _ = probe_os_keyring();
}

// ============================================================================
// Section 5: EpicClient against Mock Epic Server
// ============================================================================

#[tokio::test]
async fn test_epic_client_oauth_and_device_auth_flow() {
    let mock = MockEpicServer::start().await;
    let client = EpicClient::from_router(mock.router());

    // 1. Exchange valid authorization code
    let token = client
        .exchange_code_for_token("3b2a0c4f8d914652a92db1029c73e1a0")
        .await
        .expect("token exchange failed");
    assert_eq!(token.account_id, "4a6313cbbe7d432b8132b5ee67ad53fa");
    assert_eq!(token.display_name.as_deref(), Some("JediMaster99"));

    // 2. Exchange invalid authorization code
    let err = client
        .exchange_code_for_token("00000000000000000000000000000000")
        .await
        .unwrap_err();
    assert!(err.is_auth_code_expired());

    // 3. Create permanent DeviceAuth
    let device_auth = client
        .create_device_auth(&token.account_id, &token.access_token)
        .await
        .expect("create device auth failed");
    assert_eq!(device_auth.account_id, token.account_id);
    assert_eq!(device_auth.device_id, "device_4a6313cb");
    assert_eq!(device_auth.secret, "secret_4a6313cb");

    // 4. Headless login with DeviceAuth
    let refreshed_token = client
        .login_with_device_auth(&device_auth)
        .await
        .expect("device auth login failed");
    assert_eq!(refreshed_token.account_id, token.account_id);
    assert_eq!(refreshed_token.display_name.as_deref(), Some("JediMaster99"));

    // 5. Get 300s single-use exchange code
    let exchange = client
        .get_exchange_code(&refreshed_token.access_token)
        .await
        .expect("get exchange code failed");
    assert_eq!(exchange.code, "mock_exchange_code_456_xyz");
    assert_eq!(exchange.expires_in_seconds, 300);

    // 6. Delete DeviceAuth
    client
        .delete_device_auth(&device_auth.account_id, &device_auth.device_id, &refreshed_token.access_token)
        .await
        .expect("delete device auth failed");
}

// ============================================================================
// Section 6: EpicAuthStore Multi-Account & Session Persistence
// ============================================================================

#[tokio::test]
async fn test_epic_auth_store_lifecycle_and_zero_latency_switch() {
    let mock = MockEpicServer::start().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let db = Database::open_in_memory().expect("open memory db");
    db.run_migrations().await.expect("run migrations");

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client.clone());

    // 1. Authenticate Account 1
    let acc1 = store
        .login_with_code("3b2a0c4f8d914652a92db1029c73e1a0")
        .await
        .expect("login acc1 failed");
    assert_eq!(acc1.account_id, "4a6313cbbe7d432b8132b5ee67ad53fa");
    assert_eq!(acc1.display_name, "JediMaster99");
    assert!(acc1.is_active);

    // Verify DeviceAuth is in Keyring
    let saved_auth1 = keyring.get_device_auth(&acc1.account_id).unwrap().unwrap();
    assert_eq!(saved_auth1.account_id, acc1.account_id);

    // 2. Authenticate Account 2
    let acc2 = store
        .login_with_code("7f8e9d0a1b2c3d4e5f6a7b8c9d0e1f2a")
        .await
        .expect("login acc2 failed");
    assert_eq!(acc2.account_id, "9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d");
    assert_eq!(acc2.display_name, "SithLord42");
    // Account 1 was already active, so Account 2 starts inactive
    assert!(!acc2.is_active);

    // Verify both accounts in store
    let accounts = store.get_accounts().await.expect("get accounts failed");
    assert_eq!(accounts.len(), 2);

    let active_account = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(active_account.account_id, acc1.account_id);

    // 3. Zero-Latency Account Switching SLA (< 10ms requirement, target < 5ms)
    let start_time = Instant::now();
    let switched = store
        .switch_active_account(&acc2.account_id)
        .await
        .expect("switch account failed");
    let elapsed = start_time.elapsed();

    assert_eq!(switched.account_id, acc2.account_id);
    assert!(switched.is_active);
    println!("Account switch executed in: {:?}", elapsed);
    assert!(
        elapsed.as_millis() < 10,
        "Account switch SLA exceeded: {:?}",
        elapsed
    );

    // Verify active account updated
    let new_active = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(new_active.account_id, acc2.account_id);

    // 4. Session Persistence Across Backend Restart Simulation
    // Construct a brand-new EpicAuthStore simulating app relaunch
    let restarted_store = EpicAuthStore::new(db.clone(), keyring.clone(), client.clone());
    let report = restarted_store
        .restore_sessions()
        .await
        .expect("restore sessions failed");

    assert_eq!(report.total_accounts, 2);
    assert_eq!(report.restored_sessions, 2);
    assert_eq!(report.active_account_id.as_deref(), Some(acc2.account_id.as_str()));

    // Verify active session remains immediately ready
    let restored_active = restarted_store.get_active_account().await.unwrap().unwrap();
    assert_eq!(restored_active.account_id, acc2.account_id);

    // Verify valid access token is returned
    let token = restarted_store
        .get_valid_access_token(&acc2.account_id)
        .await
        .expect("get token failed");
    assert!(!token.is_empty());
}

// ============================================================================
// Section 7: Axum REST API Endpoints Integration
// ============================================================================

#[tokio::test]
async fn test_epic_rest_api_endpoints() {
    let mock = MockEpicServer::start().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let db = Database::open_in_memory().expect("open memory db");
    db.run_migrations().await.expect("run migrations");

    let epic_store = Arc::new(EpicAuthStore::new(db.clone(), keyring.clone(), client.clone()));
    let app = create_app_with_state(AppState::new(db.clone(), epic_store.clone()));

    // 1. GET /api/epic/auth/url
    let req = Request::builder()
        .uri("/api/epic/auth/url")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(body_json["url"].as_str().unwrap().contains("epicgames.com"));

    // 2. POST /api/epic/auth/login with empty code -> 400
    let req = Request::builder()
        .uri("/api/epic/auth/login")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"code": ""}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 3. POST /api/epic/auth/login with valid code -> 200
    let req = Request::builder()
        .uri("/api/epic/auth/login")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"code": "3b2a0c4f8d914652a92db1029c73e1a0"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["status"], "success");
    assert_eq!(
        body_json["account"]["account_id"],
        "4a6313cbbe7d432b8132b5ee67ad53fa"
    );

    // 4. GET /api/epic/accounts -> lists authenticated account
    let req = Request::builder()
        .uri("/api/epic/accounts")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(
        body_json["active_account_id"],
        "4a6313cbbe7d432b8132b5ee67ad53fa"
    );

    // 5. Authenticate second account via API
    let req = Request::builder()
        .uri("/api/epic/auth/login")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"code": "7f8e9d0a1b2c3d4e5f6a7b8c9d0e1f2a"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 6. POST /api/epic/switch-account -> switch to second account
    let req = Request::builder()
        .uri("/api/epic/switch-account")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"account_id": "9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["status"], "success");
    assert_eq!(
        body_json["account"]["account_id"],
        "9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d"
    );

    // 7. GET /api/epic/accounts -> active_account_id is now second account
    let req = Request::builder()
        .uri("/api/epic/accounts")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["accounts"].as_array().unwrap().len(), 2);
    assert_eq!(
        body_json["active_account_id"],
        "9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d"
    );
}

#[tokio::test]
async fn test_db_delete_epic_account_and_cascade() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let acc = Account {
        id: "acc_del_test".to_string(),
        launcher: "epic".to_string(),
        username: "test_user".to_string(),
        display_name: Some("Test User".to_string()),
        email: None,
        is_active: true,
        last_used: None,
    };
    db.upsert_epic_account(&acc).await.unwrap();

    let game = Game {
        id: "game_1".to_string(),
        launcher: "epic".to_string(),
        app_id: "App1".to_string(),
        name: "Test Game".to_string(),
        install_path: None,
        executable_path: None,
        size_bytes: 0,
        is_installed: false,
        banner_url: None,
        launch_uri: None,
        namespace: None,
    };
    db.upsert_game(&game).await.unwrap();

    let link = GameAccount {
        game_id: "game_1".to_string(),
        account_id: "acc_del_test".to_string(),
        launcher: "epic".to_string(),
    };
    db.link_game_account(&link).await.unwrap();

    // Verify presence
    assert_eq!(db.get_epic_accounts().await.unwrap().len(), 1);
    assert_eq!(db.get_game_accounts().await.unwrap().len(), 1);

    // Delete account
    let deleted = db.delete_epic_account("acc_del_test").await.unwrap();
    assert!(deleted);

    // Verify account and cascaded game_accounts are gone
    assert!(db.get_epic_accounts().await.unwrap().is_empty());
    assert!(db.get_game_accounts().await.unwrap().is_empty());

    // Deleting again returns false
    let deleted_again = db.delete_epic_account("acc_del_test").await.unwrap();
    assert!(!deleted_again);
}

#[tokio::test]
async fn test_remove_account_promotes_next_active() {
    let mock = MockEpicServer::start().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client.clone());

    // 1. Authenticate two accounts
    let acc1 = store.login_with_code("3b2a0c4f8d914652a92db1029c73e1a0").await.unwrap();
    let acc2 = store.login_with_code("7f8e9d0a1b2c3d4e5f6a7b8c9d0e1f2a").await.unwrap();
    assert!(acc1.is_active);
    assert!(!acc2.is_active);

    // 2. Remove active account acc1
    store.remove_account(&acc1.account_id).await.unwrap();

    // 3. Verify acc1 is gone from DB and Keyring
    let accounts = store.get_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_id, acc2.account_id);
    assert!(keyring.get_device_auth(&acc1.account_id).unwrap().is_none());

    // 4. Verify acc2 was promoted to active
    let active = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(active.account_id, acc2.account_id);
    assert!(active.is_active);

    // 5. Removing remaining account leaves zero accounts and None active
    store.remove_account(&acc2.account_id).await.unwrap();
    assert!(store.get_accounts().await.unwrap().is_empty());
    assert!(store.get_active_account().await.unwrap().is_none());

    // 6. Removing non-existent account returns AccountNotFound error
    let err = store.remove_account("phantom").await.unwrap_err();
    match err {
        EpicError::AccountNotFound(id) => assert_eq!(id, "phantom"),
        _ => panic!("Expected AccountNotFound, got {:?}", err),
    }
}

#[tokio::test]
async fn test_failed_switch_does_not_corrupt_in_memory_active_cache() {
    let mock = MockEpicServer::start().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client.clone());

    // Authenticate initial account: becomes active
    let acc1 = store.login_with_code("3b2a0c4f8d914652a92db1029c73e1a0").await.unwrap();
    assert_eq!(store.get_active_account().await.unwrap().unwrap().account_id, acc1.account_id);

    // Attempt to switch to nonexistent account ID
    let switch_res = store.switch_active_account("ghost_account_id_9999").await;
    assert!(switch_res.is_err(), "Switching to ghost account must fail");

    // In-memory active cache MUST remain acc1, NOT ghost_account_id_9999
    let active_after = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(
        active_after.account_id, acc1.account_id,
        "In-memory active account was poisoned by failed switch!"
    );
}

#[tokio::test]
async fn test_epic_rest_api_remove_account() {
    let mock = MockEpicServer::start().await;
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let epic_store = Arc::new(EpicAuthStore::new(db.clone(), keyring.clone(), client));
    let state = AppState::new(db, epic_store.clone());
    let app = create_app_with_state(state);

    // Login account
    let req = Request::builder()
        .uri("/api/epic/auth/login")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"code": "3b2a0c4f8d914652a92db1029c73e1a0"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Remove account via REST API
    let req = Request::builder()
        .uri("/api/epic/remove-account")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"account_id": "4a6313cbbe7d432b8132b5ee67ad53fa"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["status"], "success");

    // Account list is now empty
    let req = Request::builder()
        .uri("/api/epic/accounts")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["accounts"].as_array().unwrap().len(), 0);
}
