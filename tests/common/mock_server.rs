#![allow(dead_code)]

use axum::{
    extract::{Path as AxPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::models::*;

#[derive(Debug, Clone)]
pub struct LoggedRequest {
    pub method: String,
    pub path: String,
    pub auth_header: Option<String>,
}

#[derive(Default)]
pub struct MockServerState {
    pub valid_auth_codes: HashMap<String, (String, String)>, // code -> (account_id, display_name)
    pub registered_device_auths: HashMap<(String, String), String>, // (account_id, device_id) -> secret
    pub issued_access_tokens: HashMap<String, String>,       // token -> account_id
    pub entitlements: HashMap<String, Vec<Entitlement>>,     // account_id -> entitlements
    pub catalog_items: HashMap<String, HashMap<String, CatalogItem>>, // namespace -> id -> item
    pub request_log: Vec<LoggedRequest>,
    pub fail_next_exchange: bool,
    pub fail_next_token_refresh: bool,
}

#[derive(Clone)]
pub struct MockEpicServer {
    base_url: String,
    state: Arc<RwLock<MockServerState>>,
    request_counter: Arc<AtomicUsize>,
    shutdown_flag: Arc<AtomicBool>,
}

impl MockEpicServer {
    pub async fn start() -> Self {
        let state = Arc::new(RwLock::new(MockServerState::default()));
        let request_counter = Arc::new(AtomicUsize::new(0));
        let shutdown_flag = Arc::new(AtomicBool::new(false));

        let app = create_mock_router(state.clone(), request_counter.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind ephemeral port for Mock Epic Server");
        let local_addr = listener
            .local_addr()
            .expect("Failed to get local address of listener");
        let base_url = format!("http://{}", local_addr);

        let shutdown_flag_clone = shutdown_flag.clone();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    while !shutdown_flag_clone.load(Ordering::Relaxed) {
                        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    }
                })
                .await
                .ok();
        });

        let server = Self {
            base_url,
            state,
            request_counter,
            shutdown_flag,
        };

        // Initialize with default standard test data
        server.seed_defaults().await;
        server
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn router(&self) -> Router {
        create_mock_router(self.state.clone(), self.request_counter.clone())
    }

    pub fn shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::Relaxed);
    }

    pub fn get_request_count(&self) -> usize {
        self.request_counter.load(Ordering::Relaxed)
    }

    pub async fn set_fail_next_exchange(&self, fail: bool) {
        let mut s = self.state.write().await;
        s.fail_next_exchange = fail;
    }

    pub async fn set_fail_next_refresh(&self, fail: bool) {
        let mut s = self.state.write().await;
        s.fail_next_token_refresh = fail;
    }

    pub async fn register_auth_code(&self, code: &str, account_id: &str, display_name: &str) {
        let mut s = self.state.write().await;
        s.valid_auth_codes.insert(
            code.to_string(),
            (account_id.to_string(), display_name.to_string()),
        );
    }

    pub async fn register_device_auth(&self, account_id: &str, device_id: &str, secret: &str) {
        let mut s = self.state.write().await;
        s.registered_device_auths.insert(
            (account_id.to_string(), device_id.to_string()),
            secret.to_string(),
        );
    }

    pub async fn set_entitlements(&self, account_id: &str, entitlements: Vec<Entitlement>) {
        let mut s = self.state.write().await;
        s.entitlements.insert(account_id.to_string(), entitlements);
    }

    pub async fn set_catalog_item(&self, namespace: &str, item: CatalogItem) {
        let mut s = self.state.write().await;
        s.catalog_items
            .entry(namespace.to_string())
            .or_default()
            .insert(item.id.clone(), item);
    }

    pub async fn has_received_path(&self, substr: &str) -> bool {
        let s = self.state.read().await;
        s.request_log.iter().any(|r| r.path.contains(substr))
    }

    pub async fn get_request_log(&self) -> Vec<LoggedRequest> {
        let s = self.state.read().await;
        s.request_log.clone()
    }

    pub async fn clear_request_log(&self) {
        let mut s = self.state.write().await;
        s.request_log.clear();
    }

    async fn seed_defaults(&self) {
        let mut s = self.state.write().await;
        // Account 1: Luke Skywalker
        let acc1 = "4a6313cbbe7d432b8132b5ee67ad53fa";
        s.valid_auth_codes.insert(
            "valid_code_acc1_1234567890abcdef".to_string(),
            (acc1.to_string(), "Luke Skywalker".to_string()),
        );
        s.registered_device_auths.insert(
            (acc1.to_string(), "dev_acc1".to_string()),
            "secret_acc1".to_string(),
        );
        s.issued_access_tokens
            .insert("eg1_token_acc1".to_string(), acc1.to_string());

        // Account 2: Darth Vader
        let acc2 = "9f8e7d6c5b4a3210fedcba9876543210";
        s.valid_auth_codes.insert(
            "valid_code_acc2_1234567890abcdef".to_string(),
            (acc2.to_string(), "Darth Vader".to_string()),
        );
        s.registered_device_auths.insert(
            (acc2.to_string(), "dev_acc2".to_string()),
            "secret_acc2".to_string(),
        );
        s.issued_access_tokens
            .insert("eg1_token_acc2".to_string(), acc2.to_string());
    }
}

fn create_mock_router(
    state: Arc<RwLock<MockServerState>>,
    counter: Arc<AtomicUsize>,
) -> Router {
    Router::new()
        .route("/launcher/api/public/assets/:platform", get(|| async {
            Json(serde_json::json!(["Sugar", "GameAlpha", "GameBeta"].map(|name|
                serde_json::json!({"appName":name,"namespace":"fn","catalogItemId":format!("catalog_{name}"),"buildVersion":"1.0"}))))
        }))
        .route("/account/api/oauth/token", post(handle_oauth_token))
        .route(
            "/account/api/public/account/:account_id/deviceAuth",
            post(handle_create_device_auth),
        )
        .route(
            "/account/api/public/account/:account_id/deviceAuth/:device_id",
            delete(handle_delete_device_auth),
        )
        .route("/account/api/oauth/exchange", get(handle_exchange_code))
        .route(
            "/entitlement/api/account/:account_id/entitlements",
            get(handle_get_entitlements),
        )
        .route(
            "/catalog/api/shared/namespace/:namespace/bulk/items",
            get(handle_get_catalog_items),
        )
        .with_state((state, counter))
}

type ServerContext = (Arc<RwLock<MockServerState>>, Arc<AtomicUsize>);

async fn log_request(headers: &HeaderMap, path: &str, method: &str, ctx: &ServerContext) {
    ctx.1.fetch_add(1, Ordering::Relaxed);
    let mut s = ctx.0.write().await;
    s.request_log.push(LoggedRequest {
        method: method.to_string(),
        path: path.to_string(),
        auth_header: headers
            .get("authorization")
            .and_then(|h| h.to_str().ok().map(|s| s.to_string())),
    });
}

// POST /account/api/oauth/token
async fn handle_oauth_token(
    State(ctx): State<ServerContext>,
    headers: HeaderMap,
    body: String,
) -> Response {
    log_request(&headers, "/account/api/oauth/token", "POST", &ctx).await;

    // Verify basic auth header if present
    if let Some(auth) = headers.get("authorization").and_then(|h| h.to_str().ok()) {
        if !auth.starts_with("Basic ") {
            return (
                StatusCode::UNAUTHORIZED,
                Json(EpicApiError {
                    error_code: "errors.com.epicgames.common.oauth.invalid_client".to_string(),
                    error_message: "Invalid client credentials".to_string(),
                    message_vars: None,
                    numeric_error_code: Some(1011),
                    originating_service: Some("account".to_string()),
                    intent: None,
                }),
            )
                .into_response();
        }
    }

    // Parse form parameters
    let mut params: HashMap<String, String> = HashMap::new();
    for pair in body.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let key = k.trim().to_string();
            // Basic percent decoding
            let val = v.replace('+', " ");
            params.insert(key, val);
        }
    }

    let grant_type = params.get("grant_type").map(|s| s.as_str()).unwrap_or("");

    match grant_type {
        "authorization_code" => {
            let code = params.get("code").map(|s| s.as_str()).unwrap_or("");
            let mut state = ctx.0.write().await;

            // Check if code is registered or recognizable
            let (acc_id, display_name) = if let Some((acc, name)) = state.valid_auth_codes.remove(code) {
                (acc, name)
            } else if (code.len() == 32 && code.chars().all(|c| c.is_ascii_hexdigit()))
                || code.starts_with("valid_code")
            {
                (
                    "4a6313cbbe7d432b8132b5ee67ad53fa".to_string(),
                    "JediMaster".to_string(),
                )
            } else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(EpicApiError {
                        error_code:
                            "errors.com.epicgames.account.oauth.authorization_code_not_found"
                                .to_string(),
                        error_message: "Sorry the code you provided is not valid, or has expired"
                            .to_string(),
                        message_vars: Some(vec![code.to_string()]),
                        numeric_error_code: Some(18031),
                        originating_service: Some("account".to_string()),
                        intent: Some("prod10".to_string()),
                    }),
                )
                    .into_response();
            };

            let access_token = format!("eg1~{}", uuid_simple());
            state
                .issued_access_tokens
                .insert(access_token.clone(), acc_id.clone());

            let resp = OAuthTokenResponse {
                access_token,
                expires_in: 28800,
                expires_at: "2026-09-13T20:00:00.000Z".to_string(),
                token_type: "bearer".to_string(),
                refresh_token: Some(format!("eg1~refresh_{}", uuid_simple())),
                refresh_expires: Some(864000),
                refresh_expires_at: Some("2026-09-23T12:00:00.000Z".to_string()),
                account_id: acc_id,
                client_id: EPIC_LAUNCHER_CLIENT_ID.to_string(),
                internal_client: Some(true),
                client_service: Some("prod-fn".to_string()),
                display_name: Some(display_name),
                app: Some("launcher".to_string()),
                in_app_id: None,
                device_id: None,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        "device_auth" => {
            let mut state = ctx.0.write().await;
            if state.fail_next_token_refresh {
                state.fail_next_token_refresh = false;
                return (
                    StatusCode::BAD_REQUEST,
                    Json(EpicApiError {
                        error_code: "errors.com.epicgames.account.invalid_account_credentials"
                            .to_string(),
                        error_message: "Simulated refresh failure".to_string(),
                        message_vars: None,
                        numeric_error_code: Some(18031),
                        originating_service: Some("account".to_string()),
                        intent: None,
                    }),
                )
                    .into_response();
            }

            let account_id = params.get("account_id").map(|s| s.as_str()).unwrap_or("");
            let device_id = params.get("device_id").map(|s| s.as_str()).unwrap_or("");
            let secret = params.get("secret").map(|s| s.as_str()).unwrap_or("");

            let expected_secret = state
                .registered_device_auths
                .get(&(account_id.to_string(), device_id.to_string()));

            // Allow if matches registered or matches standard fallback pattern
            let is_valid = expected_secret.map_or(false, |s| s == secret)
                || (!account_id.is_empty()
                    && !device_id.is_empty()
                    && secret.starts_with("secret_"));

            if !is_valid {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(EpicApiError {
                        error_code: "errors.com.epicgames.account.invalid_account_credentials"
                            .to_string(),
                        error_message: "Sorry the credentials you provided are not valid."
                            .to_string(),
                        message_vars: None,
                        numeric_error_code: Some(18031),
                        originating_service: Some("account".to_string()),
                        intent: Some("prod10".to_string()),
                    }),
                )
                    .into_response();
            }

            let access_token = format!("eg1~{}", uuid_simple());
            state
                .issued_access_tokens
                .insert(access_token.clone(), account_id.to_string());

            let resp = OAuthTokenResponse {
                access_token,
                expires_in: 28800,
                expires_at: "2026-09-13T20:00:00.000Z".to_string(),
                token_type: "bearer".to_string(),
                refresh_token: None,
                refresh_expires: None,
                refresh_expires_at: None,
                account_id: account_id.to_string(),
                client_id: EPIC_LAUNCHER_CLIENT_ID.to_string(),
                internal_client: Some(true),
                client_service: Some("prod-fn".to_string()),
                display_name: Some("ReAuthenticatedPlayer".to_string()),
                app: Some("launcher".to_string()),
                in_app_id: None,
                device_id: Some(device_id.to_string()),
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(EpicApiError {
                error_code: "errors.com.epicgames.common.oauth.unsupported_grant_type".to_string(),
                error_message: format!("Unsupported grant type: {}", grant_type),
                message_vars: None,
                numeric_error_code: Some(1013),
                originating_service: Some("account".to_string()),
                intent: None,
            }),
        )
            .into_response(),
    }
}

// POST /account/api/public/account/:account_id/deviceAuth
async fn handle_create_device_auth(
    State(ctx): State<ServerContext>,
    AxPath(account_id): AxPath<String>,
    headers: HeaderMap,
) -> Response {
    log_request(
        &headers,
        &format!("/account/api/public/account/{}/deviceAuth", account_id),
        "POST",
        &ctx,
    )
    .await;

    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !auth_header.starts_with("Bearer ") || auth_header.contains("invalid_token") {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let device_id = format!("dev_{}", uuid_simple());
    let secret = format!("secret_{}", uuid_simple());

    {
        let mut state = ctx.0.write().await;
        state.registered_device_auths.insert(
            (account_id.clone(), device_id.clone()),
            secret.clone(),
        );
    }

    let resp = DeviceAuth {
        device_id,
        account_id,
        secret,
        user_agent: Some(
            "UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit"
                .to_string(),
        ),
        created: Some(DeviceAuthCreated {
            location: Some("Earth".to_string()),
            ip_address: Some("192.0.2.1".to_string()),
            date_time: Some("2026-09-13T12:00:00.000Z".to_string()),
        }),
    };

    (StatusCode::OK, Json(resp)).into_response()
}

// DELETE /account/api/public/account/:account_id/deviceAuth/:device_id
async fn handle_delete_device_auth(
    State(ctx): State<ServerContext>,
    AxPath((account_id, device_id)): AxPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    log_request(
        &headers,
        &format!(
            "/account/api/public/account/{}/deviceAuth/{}",
            account_id, device_id
        ),
        "DELETE",
        &ctx,
    )
    .await;

    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !auth_header.starts_with("Bearer ") {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let mut state = ctx.0.write().await;
    state.registered_device_auths.remove(&(account_id, device_id));
    StatusCode::NO_CONTENT.into_response()
}

// GET /account/api/oauth/exchange
async fn handle_exchange_code(
    State(ctx): State<ServerContext>,
    headers: HeaderMap,
) -> Response {
    log_request(&headers, "/account/api/oauth/exchange", "GET", &ctx).await;

    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !auth_header.starts_with("Bearer ") || auth_header.contains("invalid_token") {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let mut state = ctx.0.write().await;
    if state.fail_next_exchange {
        state.fail_next_exchange = false;
        return (StatusCode::INTERNAL_SERVER_ERROR, "Simulated exchange failure").into_response();
    }

    let code = format!("ex_{}", uuid_simple());
    let resp = ExchangeCodeResponse {
        expires_in_seconds: 300,
        code,
        creating_client_id: EPIC_LAUNCHER_CLIENT_ID.to_string(),
    };
    (StatusCode::OK, Json(resp)).into_response()
}

#[derive(Deserialize)]
struct EntitlementsQuery {
    #[serde(default)]
    start: usize,
    #[serde(default = "default_count")]
    count: usize,
}

fn default_count() -> usize {
    1000
}

// GET /entitlement/api/account/:account_id/entitlements
async fn handle_get_entitlements(
    State(ctx): State<ServerContext>,
    AxPath(account_id): AxPath<String>,
    Query(query): Query<EntitlementsQuery>,
    headers: HeaderMap,
) -> Response {
    log_request(
        &headers,
        &format!("/entitlement/api/account/{}/entitlements", account_id),
        "GET",
        &ctx,
    )
    .await;

    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !auth_header.starts_with("Bearer ") || auth_header.contains("invalid_token") {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let state = ctx.0.read().await;
    let empty_vec = Vec::new();
    let all = state.entitlements.get(&account_id).unwrap_or(&empty_vec);

    let start = query.start;
    let count = query.count;

    let slice = if start >= all.len() {
        Vec::new()
    } else {
        let end = (start + count).min(all.len());
        all[start..end].to_vec()
    };

    (StatusCode::OK, Json(slice)).into_response()
}

// GET /catalog/api/shared/namespace/:namespace/bulk/items
async fn handle_get_catalog_items(
    State(ctx): State<ServerContext>,
    AxPath(namespace): AxPath<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    log_request(
        &headers,
        &format!("/catalog/api/shared/namespace/{}/bulk/items", namespace),
        "GET",
        &ctx,
    )
    .await;

    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !auth_header.starts_with("Bearer ") || auth_header.contains("invalid_token") {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let requested_ids: Vec<&str> = params
        .iter()
        .filter_map(|(k, v)| if k == "id" { Some(v.as_str()) } else { None })
        .collect();

    let state = ctx.0.read().await;
    let mut result: HashMap<String, CatalogItem> = HashMap::new();

    if let Some(items) = state.catalog_items.get(&namespace) {
        if requested_ids.is_empty() {
            // Return all if no filter
            result = items.clone();
        } else {
            for id in requested_ids {
                if let Some(item) = items.get(id) {
                    result.insert(id.to_string(), item.clone());
                }
            }
        }
    }

    (StatusCode::OK, Json(result)).into_response()
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:032x}", now)
}
