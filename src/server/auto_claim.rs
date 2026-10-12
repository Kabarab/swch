use crate::{
    auto_claim::{providers::Stores, steam_auth::SteamSession, AutoClaimer, Settings},
    storage::Database,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Api {
    service: Arc<AutoClaimer>,
    stores: Arc<Stores>,
    pending: Arc<Mutex<HashMap<String, (String, Instant)>>>,
    web_mutations: Arc<Mutex<()>>,
}
type Error = (StatusCode, Json<Value>);
fn error(code: StatusCode, message: impl Into<String>) -> Error {
    (code, Json(json!({"error": message.into()})))
}
fn internal(message: impl Into<String>) -> Error {
    error(StatusCode::INTERNAL_SERVER_ERROR, message)
}

fn local_origin(headers: &HeaderMap) -> Result<(), Error> {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1:4242");
    let host = reqwest::Url::parse(&format!("http://{host}"))
        .map_err(|_| error(StatusCode::FORBIDDEN, "Некорректный адрес SWCH."))?;
    if !matches!(host.host_str(), Some("127.0.0.1" | "localhost"))
        || headers
            .get("sec-fetch-site")
            .is_some_and(|v| v == "cross-site")
    {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Открой настройки в локальном SWCH.",
        ));
    }
    if let Some(origin) = headers.get("origin") {
        let url = origin
            .to_str()
            .ok()
            .and_then(|s| reqwest::Url::parse(s).ok());
        if !url.is_some_and(|u| {
            u.scheme() == "http"
                && matches!(u.host_str(), Some("127.0.0.1" | "localhost"))
                && u.port_or_known_default() == host.port_or_known_default()
                && u.username().is_empty()
                && u.password().is_none()
        }) {
            return Err(error(
                StatusCode::FORBIDDEN,
                "Открой настройки автоклеймера в локальном SWCH.",
            ));
        }
    }
    Ok(())
}
pub fn router(
    service: Arc<AutoClaimer>,
    epic: Arc<crate::launchers::epic::EpicAuthStore>,
) -> Router {
    let state = Api {
        service,
        stores: Arc::new(Stores::new(epic)),
        pending: Arc::new(Mutex::new(HashMap::new())),
        web_mutations: Arc::new(Mutex::new(())),
    };
    Router::new()
        .route("/api/free-games", get(status))
        .route("/api/free-games/settings", post(settings))
        .route("/api/free-games/check", post(check))
        .route("/api/free-games/cancel", post(cancel_check))
        .route("/api/free-games/catalog", get(catalog))
        .route(
            "/api/free-games/notifications",
            get(notification_status).post(notification_save),
        )
        .route(
            "/api/free-games/notifications/disconnect",
            post(notification_disconnect),
        )
        .route("/api/free-games/web/connect", post(web_connect))
        .route("/api/free-games/web/verify", post(web_verify))
        .route("/api/free-games/web/manual", post(web_manual))
        .route("/api/free-games/web/library", post(web_library))
        .route("/api/free-games/web/disconnect", post(web_disconnect))
        .route("/api/free-games/web/add", post(web_add))
        .route("/api/free-games/keys/reveal", post(key_reveal))
        .route("/api/free-games/keys/redeem", post(key_redeem))
        .route("/api/free-games/steam/start", post(steam_start))
        .route("/api/free-games/steam/automatic", post(steam_automatic))
        .route("/api/free-games/steam/complete", post(steam_complete))
        .route("/api/free-games/steam/disconnect", post(steam_disconnect))
        .route("/api/free-games/steam/extension", get(extension))
        .route("/steam/claim-connect", get(bridge))
        .with_state(state)
}
async fn status(State(api): State<Api>) -> Result<Json<Value>, Error> {
    let db: &Database = &api.service.db;
    let account_results = db
        .auto_claim_values("account-results:")
        .await
        .map_err(|_| internal("Не удалось прочитать результаты аккаунтов."))?
        .into_iter()
        .filter_map(|raw| serde_json::from_str::<Vec<crate::auto_claim::ResultRow>>(&raw).ok())
        .flatten()
        .collect::<Vec<_>>();
    let verified_accounts = db
        .auto_claim_values("account-verified:")
        .await
        .map_err(|_| internal("Не удалось прочитать подтверждения входа."))?
        .into_iter()
        .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
        .collect::<Vec<_>>();
    let accounts = api
        .service
        .accounts()
        .await
        .map_err(|_| internal("Не удалось прочитать аккаунты."))?
        .into_iter()
        .filter(|a| crate::auto_claim::SUPPORTED_PLATFORMS.contains(&a.launcher.as_str()))
        .collect::<Vec<_>>();
    let mut excluded = std::collections::HashSet::new();
    for a in &accounts {
        if a.launcher != "steam"
            && db
                .auto_claim_value(&format!("web-excluded:{}:{}", a.launcher, a.id))
                .await
                .map_err(|_| internal("Не удалось прочитать подключение."))?
                .as_deref()
                == Some("true")
        {
            excluded.insert(format!("{}:{}", a.launcher, a.id));
        }
        if a.launcher == "steam"
            && db
                .auto_claim_value(&format!("steam-excluded:{}", a.id))
                .await
                .map_err(|_| internal("Не удалось прочитать настройки Steam."))?
                .as_deref()
                == Some("true")
        {
            excluded.insert(a.id.clone());
        }
    }
    let sessions = tokio::task::spawn_blocking(move || accounts.into_iter().map(|a| {
        let session=if a.launcher=="steam" { SteamSession::load(&a.id).ok() } else {None};
        let disabled=excluded.contains(&a.id)||excluded.contains(&format!("{}:{}",a.launcher,a.id));
        let web_session=if a.launcher!="steam" {crate::auto_claim::web::Session::load(&a.launcher,&a.id).ok()} else {None};
        let connected = !disabled && (if a.launcher=="steam" {session.is_some()} else {web_session.is_some()});
        let store_verified = if a.launcher=="epic" {Some(!disabled && web_session.as_ref().is_some_and(|s| s.identity_proof == "store_page" && crate::auto_claim::providers::epic_web_session_restorable(s, chrono::Utc::now().timestamp())))} else {None};
        let automatic = a.launcher=="epic" || a.launcher=="steam" && crate::auto_claim::native_steam::available(&a);
        json!({"id":a.id,"launcher":a.launcher,"name":a.display_name.unwrap_or(a.username),"connected":connected,
            "automatic":automatic,"disabled":disabled,"store_verified":store_verified,"source":session.map(|s|s.auth_source)})
    }).collect::<Vec<_>>()).await.map_err(|_| internal("Не удалось проверить вход Steam."))?;
    Ok(Json(
        json!({"settings":api.service.settings().await.map_err(internal)?,
        "run":api.service.status().await.map_err(internal)?, "accounts":sessions,
        "platforms":crate::auto_claim::platforms::PLATFORMS,
        "account_results":account_results,"verified_accounts":verified_accounts,
        "keys":crate::auto_claim::keys::list(db).await.map_err(internal)?,
        "history":db.get_claim_history().await.map_err(|_| internal("Не удалось прочитать историю получения."))?}),
    ))
}
async fn settings(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<Settings>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    body.validate()
        .map_err(|v| error(StatusCode::BAD_REQUEST, v))?;
    api.service.save_settings(&body).await.map_err(internal)?;
    Ok(Json(json!({"settings": body})))
}
async fn catalog(State(api): State<Api>) -> Result<Json<Value>, Error> {
    let candidates = api.stores.additional_catalog().await.map_err(internal)?;
    Ok(Json(
        json!({"candidates":candidates,"source":"GamerPower","checked_at":chrono::Utc::now().to_rfc3339()}),
    ))
}
async fn notification_status() -> Result<Json<Value>, Error> {
    let config = tokio::task::spawn_blocking(crate::auto_claim::notifications::Config::load)
        .await
        .map_err(|_| internal("Не удалось прочитать уведомления."))?
        .map_err(internal)?;
    Ok(Json(json!({"channels":config.channels()})))
}
async fn notification_save(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(update): Json<crate::auto_claim::notifications::Config>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    let _guard = api.service.account_lock("notifications", "delivery").await;
    let channels = tokio::task::spawn_blocking(move || {
        let mut config = crate::auto_claim::notifications::Config::load()?;
        if update.discord.is_some() {
            config.discord = update.discord;
        }
        if update.webhook.is_some() {
            config.webhook = update.webhook;
        }
        if update.telegram.is_some() {
            config.telegram = update.telegram;
        }
        if update.email.is_some() {
            config.email = update.email;
        }
        config.save()?;
        Ok::<_, String>(config.channels())
    })
    .await
    .map_err(|_| internal("Не удалось сохранить уведомления."))?
    .map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
    api.service
        .db
        .set_auto_claim_value("notification-signature", "")
        .await
        .map_err(|_| internal("Не удалось обновить уведомления."))?;
    Ok(Json(json!({"channels":channels})))
}
async fn notification_disconnect(
    State(api): State<Api>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    let _guard = api.service.account_lock("notifications", "delivery").await;
    tokio::task::spawn_blocking(crate::auto_claim::notifications::Config::clear)
        .await
        .map_err(|_| internal("Не удалось удалить уведомления."))?
        .map_err(internal)?;
    Ok(Json(json!({"channels":[]})))
}
async fn check(
    State(api): State<Api>,
    headers: HeaderMap,
    body: Option<Json<CheckRequest>>,
) -> Result<(StatusCode, Json<Value>), Error> {
    local_origin(&headers)?;
    let body = body.map(|Json(body)| body).unwrap_or_default();
    if api.service.status().await.map_err(internal)?.running {
        return Err(error(StatusCode::CONFLICT, "Проверка уже выполняется."));
    }
    let target = match (body.platform, body.account_id) {
        (None, None) => None,
        (Some(platform), Some(id)) => {
            if !api
                .service
                .accounts()
                .await
                .map_err(internal)?
                .iter()
                .any(|a| a.launcher == platform && a.id == id)
            {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "Аккаунт не добавлен в SWCH.",
                ));
            }
            if !api
                .service
                .settings()
                .await
                .map_err(internal)?
                .platform_enabled(&platform)
            {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "Включи этот магазин в параметрах раздач.",
                ));
            }
            Some((platform, id))
        }
        _ => {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Выбери магазин и аккаунт вместе.",
            ))
        }
    };
    tokio::spawn(async move {
        let result = api.service.check_manual(target, body.visible).await;
        if let Err(e) = result {
            tracing::warn!("Manual auto claim: {}", e);
        }
    });
    Ok((StatusCode::ACCEPTED, Json(json!({"status":"started"}))))
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CheckRequest {
    platform: Option<String>,
    account_id: Option<String>,
    visible: Option<bool>,
}
async fn cancel_check(State(api): State<Api>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    Ok(Json(json!({"stopping":api.service.cancel().await})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyId {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyRedeem {
    id: String,
    platform: String,
    account_id: String,
}
async fn key_redeem(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<KeyRedeem>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    if !matches!(body.platform.as_str(), "steam" | "gog" | "microsoft") {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Для этого магазина используй ручную активацию ключа.",
        ));
    }
    let _key_guard = api.service.account_lock("key", &body.id).await;
    let mut receipt = crate::auto_claim::keys::list(&api.service.db)
        .await
        .map_err(internal)?
        .into_iter()
        .find(|r| r.id == body.id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Ключ не найден в очереди."))?;
    if receipt
        .target_platform
        .as_deref()
        .is_some_and(|p| p != body.platform)
        || receipt
            .target_account
            .as_deref()
            .is_some_and(|id| id != body.account_id)
    {
        return Err(error(
            StatusCode::CONFLICT,
            "Этот ключ относится к другому магазину или уже направлен на другой аккаунт.",
        ));
    }
    if receipt.status == "redeemed" {
        return Ok(Json(json!({"status":"redeemed"})));
    }
    let _account_guard = api
        .service
        .account_lock(&body.platform, &body.account_id)
        .await;
    let account = api
        .service
        .accounts()
        .await
        .map_err(internal)?
        .into_iter()
        .find(|a| a.launcher == body.platform && a.id == body.account_id)
        .ok_or_else(|| {
            error(
                StatusCode::BAD_REQUEST,
                "Аккаунт назначения не добавлен в SWCH.",
            )
        })?;
    if api
        .service
        .disconnected(&account.launcher, &account.id)
        .await
        .map_err(internal)?
    {
        return Err(error(
            StatusCode::CONFLICT,
            "Подключение этого аккаунта отключено.",
        ));
    }
    let id = receipt.id.clone();
    let code = tokio::task::spawn_blocking(move || crate::auto_claim::keys::reveal(&id))
        .await
        .map_err(|_| internal("Не удалось прочитать ключ."))?
        .map_err(internal)?;
    receipt.target_platform = Some(body.platform);
    receipt.target_account = Some(body.account_id);
    receipt.status = "redeeming".into();
    api.service
        .db
        .set_auto_claim_value(
            &format!("key-receipt:{}", receipt.id),
            &serde_json::to_string(&receipt)
                .map_err(|_| internal("Не удалось сохранить назначение ключа."))?,
        )
        .await
        .map_err(|_| internal("Не удалось сохранить назначение ключа."))?;
    let result = if account.launcher == "steam" {
        api.stores
            .redeem_steam_key(&account, &receipt.title, &code)
            .await
    } else if account.launcher == "microsoft" {
        let config = api.service.settings().await.map_err(internal)?;
        crate::auto_claim::microsoft::redeem_key(
            &account,
            &receipt.title,
            &code,
            config.browser_operation_timeout_seconds,
            config.browser_visible,
            &config.browser_name,
        )
        .await
    } else {
        let config = api.service.settings().await.map_err(internal)?;
        crate::auto_claim::gog_claim::redeem_key(
            &account,
            &receipt.title,
            &code,
            config.browser_operation_timeout_seconds,
            config.browser_visible,
            &config.browser_name,
        )
        .await
    };
    receipt.status = if result.is_ok() {
        "redeemed"
    } else {
        "attention"
    }
    .into();
    let raw = serde_json::to_string(&receipt)
        .map_err(|_| internal("Не удалось сохранить результат ключа."))?;
    if let Err(e) = result {
        api.service
            .db
            .set_auto_claim_value(&format!("key-receipt:{}", receipt.id), &raw)
            .await
            .map_err(|_| {
                internal("Проверь библиотеку: результат активации не удалось сохранить.")
            })?;
        return Err(error(StatusCode::CONFLICT, e));
    }
    api.service
        .db
        .confirm_key_redemption(
            &receipt.id,
            &raw,
            &crate::storage::ClaimHistory {
                id: None,
                offer_id: Some(format!("key:{}", receipt.id)),
                launcher: account.launcher,
                game_title: receipt.title,
                account_id: Some(account.id),
                claimed_at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .await
        .map_err(|_| internal("Лицензия подтверждена, но историю не удалось сохранить."))?;
    Ok(Json(json!({"status":"redeemed"})))
}
async fn key_reveal(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<KeyId>,
) -> Result<(HeaderMap, Json<Value>), Error> {
    local_origin(&headers)?;
    let found = crate::auto_claim::keys::list(&api.service.db)
        .await
        .map_err(internal)?
        .iter()
        .any(|r| r.id == body.id);
    if !found {
        return Err(error(StatusCode::NOT_FOUND, "Ключ не найден в очереди."));
    }
    let code = tokio::task::spawn_blocking(move || crate::auto_claim::keys::reveal(&body.id))
        .await
        .map_err(|_| internal("Не удалось открыть ключ."))?
        .map_err(internal)?;
    let mut headers = HeaderMap::new();
    headers.insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok((headers, Json(json!({"code":code}))))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebAccountId {
    platform: String,
    account_id: String,
    #[serde(default)]
    identity: Option<String>,
}
async fn web_account(api: &Api, body: &WebAccountId) -> Result<crate::storage::Account, Error> {
    if body.platform == "steam" {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Используй подключение Steam.",
        ));
    }
    api.service
        .accounts()
        .await
        .map_err(internal)?
        .into_iter()
        .find(|a| {
            a.launcher == body.platform
                && a.id == body.account_id
                && crate::auto_claim::platforms::platform(&a.launcher).is_some()
        })
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Этот аккаунт не добавлен в SWCH."))
}
async fn web_manual(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<WebAccountId>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    let _guard = api
        .service
        .account_lock(&body.platform, &body.account_id)
        .await;
    let account = web_account(&api, &body).await?;
    if account.launcher != "epic" {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Обычное окно восстановления доступно для Epic Games.",
        ));
    }
    let settings = api.service.settings().await.map_err(internal)?;
    let games = api
        .stores
        .epic_library_status(&account)
        .await
        .map_err(internal)?;
    let spec = crate::auto_claim::platforms::platform("epic").unwrap();
    let target = games
        .iter()
        .find(|(_, owned)| !owned)
        .map(|(o, _)| o.url.as_str())
        .unwrap_or(spec.giveaways_url);
    // Keep the owned profile's existing store session; login is a store decision.
    crate::auto_claim::browser_profile::open_manual(&account, target, &settings.browser_name)
        .await
        .map_err(|e| error(StatusCode::CONFLICT, e))?;
    Ok(Json(
        json!({"status":"opened","account_name":account.display_name.unwrap_or(account.username),
        "games":games.into_iter().map(|(o,owned)|json!({"title":o.title,"url":o.url,"owned":owned})).collect::<Vec<_>>()}),
    ))
}
async fn web_library(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<WebAccountId>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    let _guard = api
        .service
        .account_lock(&body.platform, &body.account_id)
        .await;
    let account = web_account(&api, &body).await?;
    if account.launcher != "epic" {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Проверка доступна для Epic Games.",
        ));
    }
    let games = api
        .stores
        .epic_library_status(&account)
        .await
        .map_err(internal)?;
    Ok(Json(
        json!({"account_name":account.display_name.unwrap_or(account.username),
        "games":games.into_iter().map(|(o,owned)|json!({"title":o.title,"url":o.url,"owned":owned})).collect::<Vec<_>>()}),
    ))
}
async fn connect_web_account(
    api: &Api,
    account: &crate::storage::Account,
    country: &str,
) -> Result<(), Error> {
    let settings = api.service.settings().await.map_err(internal)?;
    if account.launcher == "epic" {
        api.stores
            .automatic_epic_web_connect(account)
            .await
            .map_err(|e| error(StatusCode::UNAUTHORIZED, e))?;
    } else {
        crate::auto_claim::browser_claim::connect(
            account,
            country,
            settings.browser_login_timeout_seconds,
            &settings.browser_name,
        )
        .await
        .map_err(|e| error(StatusCode::UNAUTHORIZED, e))?;
        api.service.db.set_auto_claim_value(&format!("account-verified:{}:{}",account.launcher,account.id),
            &json!({"launcher":account.launcher,"account_id":account.id,"at":chrono::Utc::now().to_rfc3339()}).to_string()).await.map_err(|_|internal("Не удалось сохранить подтверждение входа."))?;
    }
    api.service
        .db
        .set_auto_claim_value(
            &format!("web-excluded:{}:{}", account.launcher, account.id),
            "false",
        )
        .await
        .map_err(|_| internal("Не удалось сохранить подключение."))?;
    Ok(())
}
async fn web_connect(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<WebAccountId>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    let _guard = api
        .service
        .account_lock(&body.platform, &body.account_id)
        .await;
    let mut account = web_account(&api, &body).await?;
    if let Some(identity) = body.identity.filter(|v| !v.trim().is_empty()) {
        if identity.len() > 256
            || identity.chars().any(char::is_control)
            || matches!(account.launcher.as_str(), "steam" | "epic" | "gog")
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Для этого аккаунта используются сохранённые данные магазина.",
            ));
        }
        account.email = Some(identity.trim().into());
    }
    let a = account.clone();
    let country = tokio::task::spawn_blocking(move || {
        crate::auto_claim::web::Session::load(&a.launcher, &a.id).map(|s| s.country)
    })
    .await
    .map_err(|_| internal("Не удалось прочитать регион."))?
    .unwrap_or("US".into());
    connect_web_account(&api, &account, &country).await?;
    Ok(Json(json!({"status":"connected"})))
}
async fn web_disconnect(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<WebAccountId>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    let _guard = api
        .service
        .account_lock(&body.platform, &body.account_id)
        .await;
    let account = web_account(&api, &body).await?;
    api.service
        .db
        .set_auto_claim_value(
            &format!("web-excluded:{}:{}", account.launcher, account.id),
            "true",
        )
        .await
        .map_err(|_| internal("Не удалось отключить магазин."))?;
    tokio::task::spawn_blocking(move || {
        crate::auto_claim::web::Session::delete(&account.launcher, &account.id)
    })
    .await
    .map_err(|_| internal("Не удалось удалить вход."))?
    .map_err(internal)?;
    Ok(Json(json!({"status":"disconnected"})))
}
async fn web_verify(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<WebAccountId>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    let _guard = api
        .service
        .account_lock(&body.platform, &body.account_id)
        .await;
    let account = web_account(&api, &body).await?;
    if account.launcher == "epic" {
        api.stores
            .automatic_epic_web_connect(&account)
            .await
            .map_err(internal)?;
    }
    let a = account.clone();
    let session = tokio::task::spawn_blocking(move || {
        crate::auto_claim::web::Session::load(&a.launcher, &a.id)
    })
    .await
    .map_err(|_| internal("Не удалось прочитать вход."))?
    .map_err(internal)?;
    let settings = api.service.settings().await.map_err(internal)?;
    crate::auto_claim::browser_claim::connect_with_session(
        &account,
        &session.country.clone(),
        settings.browser_login_timeout_seconds,
        Some(session),
        &settings.browser_name,
    )
    .await
    .map_err(|e| error(StatusCode::UNAUTHORIZED, e))?;
    api.service.db.set_auto_claim_value(&format!("account-verified:{}:{}",account.launcher,account.id),
        &json!({"launcher":account.launcher,"account_id":account.id,"at":chrono::Utc::now().to_rfc3339()}).to_string()).await.map_err(|_|internal("Не удалось сохранить подтверждение входа."))?;
    Ok(Json(json!({"status":"connected"})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebAdd {
    platform: String,
    identity: String,
    country: String,
}
async fn web_add(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<WebAdd>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    if !crate::auto_claim::SUPPORTED_PLATFORMS[2..].contains(&body.platform.as_str())
        || body.identity.trim().is_empty()
        || body.identity.len() > 256
        || body.identity.chars().any(char::is_control)
        || body.country.len() != 2
        || !body.country.bytes().all(|b| b.is_ascii_uppercase())
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Выбери магазин, логин или email и двухбуквенный регион.",
        ));
    }
    let _mutation = api.web_mutations.lock().await;
    let identity = body.identity.trim().to_string();
    let mut accounts: Vec<crate::storage::Account> = api
        .service
        .db
        .auto_claim_value("web_accounts")
        .await
        .map_err(|_| internal("Не удалось прочитать магазины."))?
        .map(|v| serde_json::from_str(&v))
        .transpose()
        .map_err(|_| internal("Список магазинов повреждён."))?
        .unwrap_or_default();
    if accounts
        .iter()
        .any(|a| a.launcher == body.platform && a.username == identity)
    {
        return Err(error(
            StatusCode::CONFLICT,
            "Этот аккаунт магазина уже добавлен.",
        ));
    }
    let mut random = [0u8; 16];
    getrandom::getrandom(&mut random).map_err(|_| internal("Не удалось создать подключение."))?;
    let account = crate::storage::Account {
        id: format!(
            "web_{}",
            random
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>()
        ),
        launcher: body.platform,
        username: identity.clone(),
        display_name: Some(identity),
        email: None,
        is_active: false,
        last_used: None,
    };
    connect_web_account(&api, &account, &body.country).await?;
    accounts.push(account.clone());
    let saved = api
        .service
        .db
        .set_auto_claim_value(
            "web_accounts",
            &serde_json::to_string(&accounts)
                .map_err(|_| internal("Не удалось сохранить магазин."))?,
        )
        .await;
    if saved.is_err() {
        let a = account.clone();
        let _ = tokio::task::spawn_blocking(move || {
            crate::auto_claim::web::Session::delete(&a.launcher, &a.id)
        })
        .await;
        return Err(internal("Не удалось сохранить магазин."));
    }
    Ok(Json(json!({"status":"connected","account_id":account.id})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountId {
    account_id: String,
}
async fn added_steam(api: &Api, id: &str) -> Result<(), Error> {
    if !crate::launchers::steam::library::valid_steam_id(id)
        || !api
            .service
            .accounts()
            .await
            .map_err(|_| internal("Не удалось прочитать аккаунты."))?
            .iter()
            .any(|a| a.launcher == "steam" && a.id == id)
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Этот аккаунт Steam не добавлен в SWCH.",
        ));
    }
    Ok(())
}
async fn steam_automatic(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<AccountId>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    added_steam(&api, &body.account_id).await?;
    let _guard = api.service.steam_account_lock(&body.account_id).await;
    let account = api
        .service
        .accounts()
        .await
        .map_err(|_| internal("Не удалось прочитать аккаунты."))?
        .into_iter()
        .find(|a| a.id == body.account_id && a.launcher == "steam")
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Аккаунт Steam удалён."))?;
    api.service
        .db
        .set_auto_claim_value(&format!("steam-excluded:{}", account.id), "false")
        .await
        .map_err(|_| internal("Не удалось сохранить настройку Steam."))?;
    api.stores
        .automatic_steam_connect(&account)
        .await
        .map_err(|v| error(StatusCode::UNAUTHORIZED, v))?;
    api.service.db.set_auto_claim_value(&format!("account-verified:steam:{}",account.id),
        &json!({"launcher":"steam","account_id":account.id,"at":chrono::Utc::now().to_rfc3339()}).to_string()).await.map_err(|_|internal("Не удалось сохранить подтверждение входа."))?;
    let settings = api.service.settings().await.map_err(internal)?;
    api.service
        .save_settings(&settings)
        .await
        .map_err(internal)?;
    Ok(Json(json!({"status":"connected","source":"native"})))
}
async fn steam_start(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<AccountId>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    added_steam(&api, &body.account_id).await?;
    let _guard = api.service.steam_account_lock(&body.account_id).await;
    let host = headers
        .get("host")
        .and_then(|s| s.to_str().ok())
        .unwrap_or("127.0.0.1:4242");
    let origin = reqwest::Url::parse(&format!("http://{host}"))
        .map_err(|_| error(StatusCode::BAD_REQUEST, "Некорректный адрес SWCH."))?;
    if !matches!(origin.host_str(), Some("127.0.0.1" | "localhost")) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Используй локальный адрес SWCH.",
        ));
    }
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|_| internal("Не удалось подготовить подключение Steam."))?;
    let id = bytes.iter().map(|v| format!("{v:02x}")).collect::<String>();
    let mut pending = api.pending.lock().await;
    pending.retain(|_, (_, expiry)| *expiry > Instant::now());
    if pending.len() >= 32 {
        return Err(error(
            StatusCode::TOO_MANY_REQUESTS,
            "Закрой предыдущие подключения и повтори через несколько минут.",
        ));
    }
    pending.insert(
        id.clone(),
        (
            body.account_id.clone(),
            Instant::now() + Duration::from_secs(600),
        ),
    );
    Ok(Json(
        json!({"url":format!("{}steam/claim-connect#session={id}&account={}", origin, body.account_id)}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Complete {
    session: String,
    account_id: String,
    login_secure: String,
    session_id: String,
    country: String,
    #[serde(default)]
    refresh_token: Option<String>,
}
async fn steam_complete(
    State(api): State<Api>,
    Json(body): Json<Complete>,
) -> Result<Json<Value>, Error> {
    if !crate::launchers::steam::library::valid_steam_id(&body.account_id) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Подключение истекло или выбран другой аккаунт Steam.",
        ));
    }
    let _guard = api.service.steam_account_lock(&body.account_id).await;
    // The browser extension can finish only a short-lived capability created in
    // SWCH for this exact account. It cannot enroll arbitrary browser accounts.
    let mut pending = api.pending.lock().await;
    let expected = pending
        .get(&body.session)
        .filter(|(_, t)| *t > Instant::now())
        .map(|(id, _)| id.clone());
    if expected.as_deref() != Some(&body.account_id) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Подключение истекло или выбран другой аккаунт Steam.",
        ));
    }
    let session = SteamSession {
        account_id: body.account_id,
        login_secure: body.login_secure,
        session_id: body.session_id,
        country: body.country,
        refresh_token: body.refresh_token,
        auth_source: "browser".into(),
    };
    session
        .validate()
        .map_err(|v| error(StatusCode::BAD_REQUEST, v))?;
    added_steam(&api, &session.account_id).await?;
    api.service
        .db
        .set_auto_claim_value(&format!("steam-excluded:{}", session.account_id), "false")
        .await
        .map_err(|_| internal("Не удалось сохранить настройку Steam."))?;
    // Consume before the remote check so parallel submissions cannot overwrite credentials.
    pending.remove(&body.session);
    drop(pending);
    api.stores
        .verify_steam_session(&session)
        .await
        .map_err(|v| error(StatusCode::UNAUTHORIZED, v))?;
    tokio::task::spawn_blocking(move || session.save())
        .await
        .map_err(|_| internal("Не удалось сохранить вход Steam."))?
        .map_err(internal)?;
    let settings = api.service.settings().await.map_err(internal)?;
    api.service
        .save_settings(&settings)
        .await
        .map_err(internal)?;
    Ok(Json(json!({"status":"connected"})))
}
async fn steam_disconnect(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<AccountId>,
) -> Result<Json<Value>, Error> {
    local_origin(&headers)?;
    added_steam(&api, &body.account_id).await?;
    let _guard = api.service.steam_account_lock(&body.account_id).await;
    api.pending
        .lock()
        .await
        .retain(|_, (id, _)| id != &body.account_id);
    api.service
        .db
        .set_auto_claim_value(&format!("steam-excluded:{}", body.account_id), "true")
        .await
        .map_err(|_| internal("Не удалось отключить автоподключение Steam."))?;
    tokio::task::spawn_blocking(move || SteamSession::delete(&body.account_id))
        .await
        .map_err(|_| internal("Не удалось удалить вход Steam."))?
        .map_err(internal)?;
    Ok(Json(json!({"status":"disconnected"})))
}
async fn bridge() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../../ui/steam-claim-bridge.html"))
}

/// A small store-only ZIP makes the extension available in packaged builds,
/// without an external archive binary or a generated file to keep in sync.
async fn extension() -> ([(&'static str, &'static str); 2], Vec<u8>) {
    let files = [
        (
            "manifest.json",
            include_str!("../../browser-extension/steam-claims/manifest.json"),
        ),
        (
            "background.js",
            include_str!("../../browser-extension/steam-claims/background.js"),
        ),
        (
            "bridge.js",
            include_str!("../../browser-extension/steam-claims/bridge.js"),
        ),
        (
            "README.txt",
            include_str!("../../browser-extension/steam-claims/README.txt"),
        ),
    ];
    (
        [
            ("Content-Type", "application/zip"),
            (
                "Content-Disposition",
                "attachment; filename=swch-steam-claims.zip",
            ),
        ],
        zip(&files),
    )
}
fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    fn u16le(b: &mut Vec<u8>, v: u16) {
        b.extend(v.to_le_bytes());
    }
    fn u32le(b: &mut Vec<u8>, v: u32) {
        b.extend(v.to_le_bytes());
    }
    let mut out = vec![];
    let mut central = vec![];
    for (name, text) in files {
        let offset = out.len() as u32;
        let size = text.len() as u32;
        let mut crc = !0u32;
        for b in text.bytes() {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
            }
        }
        crc = !crc;
        u32le(&mut out, 0x04034b50);
        for v in [20, 0, 0, 0, 0] {
            u16le(&mut out, v);
        }
        u32le(&mut out, crc);
        u32le(&mut out, size);
        u32le(&mut out, size);
        u16le(&mut out, name.len() as u16);
        u16le(&mut out, 0);
        out.extend(name.as_bytes());
        out.extend(text.as_bytes());
        u32le(&mut central, 0x02014b50);
        for v in [20, 20, 0, 0, 0, 0] {
            u16le(&mut central, v);
        }
        u32le(&mut central, crc);
        u32le(&mut central, size);
        u32le(&mut central, size);
        for v in [name.len() as u16, 0, 0, 0, 0] {
            u16le(&mut central, v);
        }
        u32le(&mut central, 0);
        u32le(&mut central, offset);
        central.extend(name.as_bytes());
    }
    let offset = out.len() as u32;
    let length = central.len() as u32;
    out.extend(central);
    u32le(&mut out, 0x06054b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, files.len() as u16);
    u16le(&mut out, files.len() as u16);
    u32le(&mut out, length);
    u32le(&mut out, offset);
    u16le(&mut out, 0);
    out
}
