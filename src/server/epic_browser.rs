//! Local bridge used by the optional SWCH browser extension.
use std::sync::Arc;
use axum::{extract::State, http::{HeaderMap, StatusCode}, response::Html, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::launchers::epic::{EpicAuthStore, EpicClient, models::sanitize_auth_code};
use crate::launchers::epic::browser::{BrowserLoginStatus, LOGIN_TTL};
use super::handlers::ErrorResponse;

type ApiError = (StatusCode, Json<ErrorResponse>);
fn error(status: StatusCode, text: &str) -> ApiError {
    (status, Json(ErrorResponse { error: text.into() }))
}

fn local_origin(headers: &HeaderMap) -> Result<String, ApiError> {
    let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or("");
    let url = reqwest::Url::parse(&format!("http://{host}"))
        .map_err(|_| error(StatusCode::BAD_REQUEST, "Неверный адрес SWCH."))?;
    if !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
        || !url.username().is_empty() || url.password().is_some()
        || url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(error(StatusCode::FORBIDDEN, "Вход доступен только в локальном SWCH."));
    }
    Ok(url.origin().ascii_serialization())
}

pub async fn start(
    State(store): State<Arc<EpicAuthStore>>, headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let origin = local_origin(&headers)?;
    if let Some(request_origin) = headers.get("origin") {
        if request_origin.to_str().ok() != Some(origin.as_str()) {
            return Err(error(StatusCode::FORBIDDEN, "Начни вход из приложения SWCH."));
        }
    }
    if headers.get("sec-fetch-site").and_then(|h| h.to_str().ok()) == Some("cross-site") {
        return Err(error(StatusCode::FORBIDDEN, "Начни вход из приложения SWCH."));
    }
    let id = store.browser_auth.start().await
        .map_err(|message| error(StatusCode::TOO_MANY_REQUESTS, message))?;
    let url = format!("{origin}/epic/connect#session={id}");
    Ok(Json(json!({ "session_id": id, "browser_url": url, "expires_in": LOGIN_TTL.as_secs() })))
}

#[derive(Deserialize)]
pub struct SessionRequest { pub session_id: String }

async fn current(store: &EpicAuthStore, id: &str) -> Result<BrowserLoginStatus, ApiError> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(error(StatusCode::NOT_FOUND, "Сессия входа не найдена."));
    }
    store.browser_auth.status(id).await
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Сессия входа не найдена. Начни вход заново."))
}

pub async fn status(
    State(store): State<Arc<EpicAuthStore>>, Json(body): Json<SessionRequest>,
) -> Result<Json<BrowserLoginStatus>, ApiError> {
    Ok(Json(current(&store, &body.session_id).await?))
}

pub async fn prepare(
    State(store): State<Arc<EpicAuthStore>>, Json(body): Json<SessionRequest>,
) -> Result<Json<Value>, ApiError> {
    let state = current(&store, &body.session_id).await?;
    if state.status != "pending" {
        return Err(error(StatusCode::CONFLICT, "Сессия входа завершена. Начни вход заново."));
    }
    Ok(Json(json!({"auth_url": EpicClient::auth_url(), "expires_in": LOGIN_TTL.as_secs()})))
}

/// Opening a URL is separate from creating the session, so browser-only clients
/// can also use the returned browser_url. The capability stays in the fragment.
#[derive(Deserialize)]
pub struct OpenRequest { pub session_id: String }
pub async fn open_browser(
    State(store): State<Arc<EpicAuthStore>>, headers: HeaderMap, Json(body): Json<OpenRequest>,
) -> Result<Json<Value>, ApiError> {
    let origin = local_origin(&headers)?;
    let state = current(&store, &body.session_id).await?;
    if state.status != "pending" {
        return Err(error(StatusCode::CONFLICT, "Начни вход заново."));
    }
    let url = format!("{origin}/epic/connect#session={}", body.session_id);
    tokio::task::spawn_blocking(move || open::that(url)).await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Не удалось открыть браузер."))?
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Не удалось открыть браузер."))?;
    Ok(Json(json!({"status": "opened"})))
}

#[derive(Deserialize)]
pub struct CompleteRequest { pub session_id: String, pub code: String }

pub async fn complete(
    State(store): State<Arc<EpicAuthStore>>, Json(body): Json<CompleteRequest>,
) -> Result<Json<BrowserLoginStatus>, ApiError> {
    current(&store, &body.session_id).await?;
    let code = sanitize_auth_code(&body.code)
        .map_err(|_| error(StatusCode::BAD_REQUEST, "Неверный код авторизации Epic."))?;
    if !store.browser_auth.claim(&body.session_id).await {
        return Err(error(StatusCode::CONFLICT, "Сессия уже использована или завершена. Начни вход заново."));
    }
    match store.login_with_code(&code).await {
        Ok(account) => {
            store.browser_auth.connected(&body.session_id, account.clone()).await;
            let result = current(&store, &body.session_id).await?;
            tokio::spawn(async move {
                let success = store.sync_library(&account.account_id, None).await.is_ok();
                store.browser_auth.library_finished(&body.session_id, success).await;
            });
            Ok(Json(result))
        }
        Err(_) => {
            store.browser_auth.failed(&body.session_id).await;
            Err(error(StatusCode::BAD_GATEWAY, "Не удалось подключить аккаунт Epic. Начни вход заново или используй код."))
        }
    }
}

pub async fn cancel(
    State(store): State<Arc<EpicAuthStore>>, Json(body): Json<SessionRequest>,
) -> Result<Json<BrowserLoginStatus>, ApiError> {
    current(&store, &body.session_id).await?;
    Ok(Json(store.browser_auth.cancel(&body.session_id).await
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Сессия входа не найдена."))?))
}

pub async fn bridge() -> Html<&'static str> {
    Html(include_str!("../../ui/epic-browser-bridge.html"))
}
