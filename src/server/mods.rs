use crate::mods::{types::*, ModManager};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path, Query},
    http::{header, Method, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, path::PathBuf, sync::Arc};
use tower::ServiceExt;
use tower_http::services::ServeFile;

type Response = std::result::Result<Json<Value>, (StatusCode, Json<Value>)>;
fn respond(value: Result<Value>) -> Response {
    value
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({"error":error}))))
}
fn done(value: Result<()>) -> Response {
    respond(value.map(|_| json!({"status":"ok"})))
}

pub fn router() -> Router<super::AppState> {
    Router::new()
        .route("/api/mods", get(list))
        .route("/api/mods/catalog", get(catalog))
        .route("/api/mods/events", get(events))
        .route("/api/mods/install", post(install))
        .route("/api/mods/install-file", post(install_file))
        .route("/api/mods/development", post(development))
        .route("/api/mods/theme", post(theme))
        .route("/api/mods/:id/toggle", post(toggle))
        .route("/api/mods/:id/games", post(bind_games))
        .route("/api/mods/:id/restart", post(restart))
        .route("/api/mods/:id/remove", post(remove))
        .route("/api/mods/:id/clear-data", post(clear_data))
        .route("/api/mods/:id/logs", get(logs))
        .route("/api/mods/:id/ui", get(ui))
        .route("/api/mods/:id/assets/*path", get(asset))
        .route("/api/mods/:id/settings", get(settings).post(save_settings))
        .route("/api/mods/:id/sdk", post(sdk))
        .route("/api/platforms", get(platforms))
        .route("/api/platforms/library", get(library))
        .route("/api/platforms/:id/:operation", post(platform_operation))
        .layer(DefaultBodyLimit::disable())
        .layer(axum::middleware::from_fn(local_origin))
}
pub(crate) async fn local_origin(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let host = request
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let local = host
        .split(':')
        .next()
        .is_some_and(|h| matches!(h, "127.0.0.1" | "localhost"));
    // Sandboxed mod frames have an opaque origin. Only package resource reads
    // are public to that origin; management and SDK routes retain their guard.
    let asset_read = matches!(*request.method(), Method::GET | Method::HEAD)
        && matches!(request.uri().path().split('/').collect::<Vec<_>>().as_slice(), ["", "api", "mods", id, "assets", rest @ ..] if !id.is_empty() && !rest.is_empty())
        && request.headers().get(header::ORIGIN).map_or(true, |origin| origin == "null");
    let origin_ok = asset_read || request.headers().get("origin").map_or(true, |o| {
        o.to_str()
            .ok()
            .is_some_and(|o| o == format!("http://{host}"))
    });
    let site_ok = asset_read || request
        .headers()
        .get("sec-fetch-site")
        .map_or(true, |s| s != "cross-site");
    if !local || !origin_ok || !site_ok {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"Mod API accepts only local SWCH requests"})),
        )
            .into_response();
    }
    next.run(request).await
}
pub async fn notify_changes(
    Extension(manager): Extension<Arc<ModManager>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mutation = request.method() != axum::http::Method::GET
        && request.method() != axum::http::Method::HEAD
        && request.uri().path().starts_with("/api/")
        && !request.uri().path().starts_with("/api/mods")
        && !request.uri().path().starts_with("/api/platforms")
        && !request.uri().path().starts_with("/api/i18n");
    let response = next.run(request).await;
    if mutation && response.status().is_success() {
        let _ = manager.events.send(json!({"event":"library.changed"}));
        let _ = manager.events.send(json!({"event":"accounts.changed"}));
    }
    response
}
async fn list(Extension(manager): Extension<Arc<ModManager>>) -> Json<Value> {
    Json(
        json!({"mods":manager.list().await,"theme":manager.active_theme().await,"api":API_VERSION}),
    )
}
#[derive(Deserialize)]
struct CatalogQuery {
    #[serde(default)]
    refresh: bool,
}
async fn catalog(
    Extension(m): Extension<Arc<ModManager>>,
    Query(q): Query<CatalogQuery>,
) -> Response {
    respond(m.catalog(q.refresh).await)
}
#[derive(Deserialize)]
struct Install {
    id: String,
    #[serde(default)]
    trusted: bool,
}
async fn install(Extension(m): Extension<Arc<ModManager>>, Json(input): Json<Install>) -> Response {
    if !input.trusted {
        return respond(Err("Confirm that you trust the mod author".into()));
    }
    respond(
        m.install_catalog(&input.id)
            .await
            .map(|id| json!({"id":id})),
    )
}
#[derive(Deserialize)]
struct LocalQuery {
    sha256: Option<String>,
    #[serde(default)]
    trusted: bool,
}
async fn install_file(
    Extension(m): Extension<Arc<ModManager>>,
    Query(q): Query<LocalQuery>,
    body: Body,
) -> Response {
    if !q.trusted {
        return respond(Err("Confirm that you trust the mod author".into()));
    }
    respond(async {
        let (file, hash) = crate::mods::package::spool(body.into_data_stream(), &m.root.join("downloads")).await?;
        let id = m.install_file(file.path(), q.sha256.as_deref().unwrap_or(&hash), None).await?;
        Ok(json!({"id":id}))
    }.await)
}
#[derive(Deserialize)]
struct Development {
    path: PathBuf,
    #[serde(default)]
    trusted: bool,
}
async fn development(
    Extension(m): Extension<Arc<ModManager>>,
    Json(q): Json<Development>,
) -> Response {
    if !q.trusted {
        return respond(Err("Confirm that you trust the development mod".into()));
    }
    respond(m.development(q.path).await.map(|id| json!({"id":id})))
}
async fn toggle(
    Extension(m): Extension<Arc<ModManager>>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    done(
        m.toggle(&id, input["enabled"].as_bool().unwrap_or(false))
            .await,
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GameBindings {
    games: Option<Vec<GameTarget>>,
}
async fn bind_games(
    Extension(m): Extension<Arc<ModManager>>,
    Path(id): Path<String>,
    Json(input): Json<GameBindings>,
) -> Response {
    done(m.bind_games(&id, input.games).await)
}
async fn restart(Extension(m): Extension<Arc<ModManager>>, Path(id): Path<String>) -> Response {
    done(m.reload(&id).await)
}
async fn remove(Extension(m): Extension<Arc<ModManager>>, Path(id): Path<String>) -> Response {
    done(m.uninstall(&id).await)
}
async fn clear_data(Extension(m): Extension<Arc<ModManager>>, Path(id): Path<String>) -> Response {
    done(m.clear_data(&id).await)
}
async fn logs(Extension(m): Extension<Arc<ModManager>>, Path(id): Path<String>) -> Response {
    respond(m.logs(&id).await.map(|logs| json!({"logs":logs})))
}
async fn ui(Extension(m): Extension<Arc<ModManager>>, Path(id): Path<String>) -> Response {
    respond(m.ui(&id).await.map(|html| json!({"html":html})))
}
async fn asset(
    Extension(m): Extension<Arc<ModManager>>,
    Path((id, path)): Path<(String, String)>,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let path = match m.asset(&id, &path).await {
        Ok(path) => path,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let mut response = match ServeFile::new(path).oneshot(request).await {
        Ok(response) => response.map(Body::new),
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    };
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, header::HeaderValue::from_static("null"));
    headers.insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-cache"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, header::HeaderValue::from_static("nosniff"));
    response
}
async fn settings(Extension(m): Extension<Arc<ModManager>>, Path(id): Path<String>) -> Response {
    respond(m.sdk_call(&id, "settings.get".into(), json!({})).await)
}
async fn save_settings(
    Extension(m): Extension<Arc<ModManager>>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    respond(
        m.sdk_call(&id, "settings.set".into(), json!({"value":input}))
            .await,
    )
}
async fn sdk(
    Extension(m): Extension<Arc<ModManager>>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    respond(
        m.sdk_call(
            &id,
            input["method"].as_str().unwrap_or("").into(),
            input["params"].clone(),
        )
        .await,
    )
}
async fn platforms(Extension(m): Extension<Arc<ModManager>>) -> Json<Value> {
    Json(json!({"platforms":m.platforms().await}))
}
async fn library(Extension(m): Extension<Arc<ModManager>>) -> Response {
    respond(m.library().await)
}
async fn platform_operation(
    Extension(m): Extension<Arc<ModManager>>,
    Path((id, operation)): Path<(String, String)>,
    Json(input): Json<Value>,
) -> Response {
    respond(m.platform_operation(&id, &operation, input).await)
}
async fn theme(Extension(m): Extension<Arc<ModManager>>, Json(input): Json<Value>) -> Response {
    respond(m.theme(input["id"].as_str()).await)
}
async fn events(
    Extension(m): Extension<Arc<ModManager>>,
) -> Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>> {
    let stream = futures_util::stream::unfold(m.events.subscribe(), |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(value) => return Some((Ok(Event::default().data(value.to_string())), rx)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
