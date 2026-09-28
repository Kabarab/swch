use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

use super::launch::{build_launch_parameters, mask_launch_arguments};
use super::models::{EpicAccount, EPIC_LAUNCHER_CLIENT_ID};
use super::store::EpicAuthStore;

#[derive(Serialize)]
pub struct AuthUrlResponse {
    pub url: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub code: String,
}

#[derive(Deserialize)]
pub struct SwitchRequest {
    pub account_id: String,
}

#[derive(Serialize)]
pub struct SwitchResponse {
    pub status: &'static str,
    pub account_id: String,
    pub latency_ms: f64,
}

#[derive(Deserialize)]
pub struct SyncRequest {
    pub account_id: String,
}

#[derive(Serialize)]
pub struct SyncResponse {
    pub status: &'static str,
    pub total_entitlements: usize,
    pub base_games_count: usize,
    pub installed_count: usize,
}

#[derive(Deserialize)]
pub struct LaunchRequest {
    pub account_id: String,
    pub app_name: String,
    pub namespace: Option<String>,
}

#[derive(Serialize)]
pub struct LaunchResponse {
    pub status: &'static str,
    pub app_name: String,
    pub launch_arguments: Vec<String>,
    pub display_arguments: Vec<String>,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub fn create_epic_api_router(store: EpicAuthStore) -> Router {
    Router::new()
        .route("/api/epic/auth/url", get(get_auth_url))
        .route("/api/epic/auth/login", post(login_account))
        .route("/api/epic/accounts", get(list_accounts))
        .route("/api/epic/switch", post(switch_account))
        .route("/api/epic/library/sync", post(sync_library))
        .route("/api/epic/launch", post(launch_game))
        .with_state(store)
}

async fn get_auth_url() -> Json<AuthUrlResponse> {
    let url = format!(
        "https://www.epicgames.com/id/api/redirect?clientId={}&responseType=code",
        EPIC_LAUNCHER_CLIENT_ID
    );
    Json(AuthUrlResponse { url })
}

async fn login_account(
    State(store): State<EpicAuthStore>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<EpicAccount>, (StatusCode, Json<ErrorResponse>)> {
    match store.login_with_code(&payload.code).await {
        Ok(account) => Ok(Json(account)),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Login failed: {}", e),
            }),
        )),
    }
}

async fn list_accounts(
    State(store): State<EpicAuthStore>,
) -> Result<Json<Vec<EpicAccount>>, (StatusCode, Json<ErrorResponse>)> {
    match store.get_accounts().await {
        Ok(accounts) => Ok(Json(accounts)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to retrieve accounts: {}", e),
            }),
        )),
    }
}

async fn switch_account(
    State(store): State<EpicAuthStore>,
    Json(payload): Json<SwitchRequest>,
) -> Result<Json<SwitchResponse>, (StatusCode, Json<ErrorResponse>)> {
    match store.switch_active_account(&payload.account_id).await {
        Ok((account, duration)) => Ok(Json(SwitchResponse {
            status: "success",
            account_id: account.account_id,
            latency_ms: duration.as_secs_f64() * 1000.0,
        })),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Switch failed: {}", e),
            }),
        )),
    }
}

async fn sync_library(
    State(store): State<EpicAuthStore>,
    Json(payload): Json<SyncRequest>,
) -> Result<Json<SyncResponse>, (StatusCode, Json<ErrorResponse>)> {
    match store.sync_library(&payload.account_id, None).await {
        Ok(summary) => Ok(Json(SyncResponse {
            status: "success",
            total_entitlements: summary.total_entitlements,
            base_games_count: summary.base_games_count,
            installed_count: summary.installed_count,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Sync failed: {}", e),
            }),
        )),
    }
}

async fn launch_game(
    State(store): State<EpicAuthStore>,
    Json(payload): Json<LaunchRequest>,
) -> Result<Json<LaunchResponse>, (StatusCode, Json<ErrorResponse>)> {
    let token = match store.get_valid_access_token(&payload.account_id).await {
        Ok(t) => t,
        Err(e) => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: format!("Failed to obtain access token: {}", e),
                }),
            ))
        }
    };

    let exchange = match store.client().get_exchange_code(&token).await {
        Ok(ex) => ex,
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to get exchange code: {}", e),
                }),
            ))
        }
    };

    let accounts = store.get_accounts().await.unwrap_or_default();
    let display_name = accounts
        .iter()
        .find(|a| a.account_id == payload.account_id)
        .and_then(|a| a.display_name.clone())
        .unwrap_or_else(|| "Player".to_string());

    let namespace = payload.namespace.unwrap_or_else(|| "fn".to_string());
    let raw_args = build_launch_parameters(
        &exchange.code,
        &payload.app_name,
        &payload.account_id,
        &display_name,
        &namespace,
        Some("en"),
        None,
    );

    let display_args = mask_launch_arguments(&raw_args);

    Ok(Json(LaunchResponse {
        status: "success",
        app_name: payload.app_name,
        launch_arguments: display_args.clone(),
        display_arguments: display_args,
    }))
}
