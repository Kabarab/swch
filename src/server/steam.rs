use super::handlers::ErrorResponse;
use crate::{
    launchers::steam::{
        library::{self, SteamGame, SteamLibrary},
        remote::{InventoryPage, SteamClient},
        SteamPaths,
    },
    storage::Database,
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
type ApiError = (StatusCode, Json<ErrorResponse>);
fn error(status: StatusCode, message: impl Into<String>) -> ApiError {
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
        }),
    )
}
fn internal(_: impl std::fmt::Display) -> ApiError {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Не удалось сохранить или прочитать данные Steam.",
    )
}
fn invalid() -> ApiError {
    error(
        StatusCode::BAD_REQUEST,
        "Некорректный SteamID, AppID или номер страницы.",
    )
}

async fn snapshot(db: &Database) -> Result<SteamLibrary, ApiError> {
    let scan = tokio::task::spawn_blocking(|| SteamPaths::detect().and_then(|p| library::scan(&p)))
        .await
        .map_err(internal)?;
    let mut result = match scan {
        Ok(data) => {
            db.steam_cache_put("local", &serde_json::to_string(&data).map_err(internal)?)
                .await
                .map_err(internal)?;
            data
        }
        Err(_) => {
            let cached = db.steam_cache_get("local").await.map_err(internal)?;
            let mut data: SteamLibrary = cached
                .and_then(|(s, _)| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            data.warnings.push("Локальные данные Steam недоступны. Войди в Steam на этом компьютере и нажми «Обновить». Сохранённые данные могут устареть.".into());
            // Offline snapshots do not prove the game is still installed.
            for g in &mut data.games {
                g.game.is_installed = false;
                g.update = None;
            }
            data
        }
    };
    for account in result.accounts.clone() {
        for source in ["family", "owned", "giveaways"] {
            if let Some((json, _)) = db
                .steam_cache_get(&format!("{source}:{}", account.id))
                .await
                .map_err(internal)?
            {
                let games: Vec<SteamGame> = serde_json::from_str(&json).map_err(internal)?;
                result
                    .synced_sources
                    .entry(account.id.clone())
                    .or_default()
                    .push(source.into());
                library::merge_remote(&mut result, games);
                if source == "owned" {
                    for game in &mut result.games {
                        if game.account_id.as_deref() == Some(&account.id) && game.access == "local"
                        {
                            game.access = "history".into();
                        }
                    }
                }
            }
        }
    }
    for account in &result.accounts {
        if result
            .games
            .iter()
            .any(|g| g.account_id.as_deref() == Some(&account.id) && g.access == "family")
        {
            let entry = result.synced_sources.entry(account.id.clone()).or_default();
            if !entry.contains(&"family".to_string()) {
                entry.push("family".to_string());
            }
        }
    }
    result.api_key_configured =
        std::env::var("STEAM_WEB_API_KEY").is_ok_and(|s| !s.trim().is_empty());
    if let Ok(cached_names) = db.steam_cache_prefix("app-name:").await {
        let name_map: std::collections::HashMap<String, String> = cached_names
            .into_iter()
            .filter_map(|(k, v, _)| {
                let appid = k.strip_prefix("app-name:")?.to_string();
                Some((appid, v))
            })
            .collect();
        for g in &mut result.games {
            if g.game.name.starts_with("Steam #") {
                if let Some(name) = name_map.get(&g.game.app_id) {
                    if !name.is_empty() {
                        g.game.name = name.clone();
                    }
                }
            }
        }
    }
    result.games.retain(|g| !library::is_ignored_steam_app(&g.game.app_id, &g.game.name));
    Ok(result)
}
pub async fn get_library(State(db): State<Database>) -> Result<Json<SteamLibrary>, ApiError> {
    Ok(Json(snapshot(&db).await?))
}

// Secrets are accepted only in a JSON POST body and never stored or returned.
#[derive(Deserialize)]
pub struct SyncRequest {
    pub account_id: String,
    pub api_key: Option<String>,
    pub access_token: Option<String>,
}
pub async fn sync_library(
    State(db): State<Database>,
    Json(payload): Json<SyncRequest>,
) -> Result<Json<SteamLibrary>, ApiError> {
    static SYNC: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = SYNC
        .try_lock()
        .map_err(|_| error(StatusCode::CONFLICT, "Обновление Steam уже выполняется."))?;
    if !library::valid_steam_id(&payload.account_id) {
        return Err(invalid());
    }
    let mut data = snapshot(&db).await?;
    if !data.accounts.iter().any(|a| a.id == payload.account_id) {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Сначала войди в этот аккаунт через Steam на компьютере.",
        ));
    }
    let key = payload
        .api_key
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("STEAM_WEB_API_KEY").ok());
    let token = payload.access_token.filter(|s| !s.trim().is_empty());
    if key.is_none() && token.is_none() {
        return Err(error(StatusCode::BAD_REQUEST, "Для онлайн-библиотеки нужен Steam Web API key, для семейной — токен веб-сессии Steam. Локальные игры и часы уже доступны."));
    }
    let client = SteamClient::default();
    let mut warnings = Vec::new();
    let mut succeeded = false;
    for (source, credential) in [("family", token), ("owned", key)] {
        let Some(credential) = credential else {
            continue;
        };
        let response = if source == "owned" {
            client.owned(&payload.account_id, credential.trim()).await
        } else {
            client.family(&payload.account_id, credential.trim()).await
        };
        match response {
            Ok(games) => {
                db.steam_cache_put(
                    &format!("{source}:{}", payload.account_id),
                    &serde_json::to_string(&games).map_err(internal)?,
                )
                .await
                .map_err(internal)?;
                succeeded = true;
            }
            Err(message) => warnings.push(format!(
                "{}: {message}",
                if source == "owned" {
                    "Игры"
                } else {
                    "Семейный доступ"
                }
            )),
        }
    }
    if !succeeded {
        return Err(error(StatusCode::BAD_GATEWAY, warnings.join(" ")));
    }
    data = snapshot(&db).await?;
    data.warnings.extend(warnings);
    Ok(Json(data))
}
#[derive(Deserialize)]
pub struct InventoryQuery {
    pub account_id: String,
    pub app_id: String,
    pub context_id: Option<String>,
    pub cursor: Option<String>,
    #[serde(default)]
    pub refresh: bool,
}
pub async fn inventory(
    State(db): State<Database>,
    Query(query): Query<InventoryQuery>,
) -> Result<Json<InventoryPage>, ApiError> {
    let context = query.context_id.as_deref().unwrap_or("2");
    let numeric = |s: &str| {
        !s.is_empty()
            && s.len() <= 20
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u64>().is_ok_and(|n| n > 0)
    };
    if !library::valid_steam_id(&query.account_id)
        || !library::valid_app_id(&query.app_id)
        || !numeric(context)
        || query.cursor.as_deref().is_some_and(|s| !numeric(s))
    {
        return Err(invalid());
    }
    let key = format!(
        "inventory:{}:{}:{}:{}",
        query.account_id,
        query.app_id,
        context,
        query.cursor.as_deref().unwrap_or("first")
    );
    let cached = db.steam_cache_get(&key).await.map_err(internal)?;
    let cached_page = cached
        .as_ref()
        .and_then(|(s, _)| serde_json::from_str::<InventoryPage>(s).ok());
    if !query.refresh
        && cached
            .as_ref()
            .is_some_and(|(_, t)| chrono::Utc::now().timestamp() - t < 300)
    {
        if let Some(mut page) = cached_page.clone() {
            page.cached = true;
            return Ok(Json(page));
        }
    }
    match SteamClient::default()
        .inventory(
            &query.account_id,
            &query.app_id,
            context,
            query.cursor.as_deref(),
        )
        .await
    {
        Ok(page) => {
            if page
                .next_cursor
                .as_deref()
                .is_some_and(|s| Some(s) == query.cursor.as_deref())
            {
                return Err(error(
                    StatusCode::BAD_GATEWAY,
                    "Steam повторил страницу инвентаря. Попробуй обновить его.",
                ));
            }
            db.steam_cache_put(&key, &serde_json::to_string(&page).map_err(internal)?)
                .await
                .map_err(internal)?;
            Ok(Json(page))
        }
        Err(message) => {
            if let Some(mut page) = cached_page {
                page.cached = true;
                page.warning = Some(format!(
                    "{message} Показана сохранённая копия, она может быть устаревшей."
                ));
                Ok(Json(page))
            } else {
                Err(error(StatusCode::BAD_GATEWAY, message))
            }
        }
    }
}

#[derive(Deserialize)]
pub struct AvatarQuery {
    pub account_id: String,
}
pub async fn avatar(
    State(db): State<Database>,
    Query(query): Query<AvatarQuery>,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::{IntoResponse, Redirect};
    if !library::valid_steam_id(&query.account_id) {
        return Err(invalid());
    }
    if let Ok(paths) = SteamPaths::detect() {
        if let Ok(bytes) = tokio::fs::read(
            paths
                .config_dir()
                .join("avatarcache")
                .join(format!("{}.png", query.account_id)),
        )
        .await
        {
            return Ok((
                [
                    (axum::http::header::CONTENT_TYPE, "image/png"),
                    (axum::http::header::CACHE_CONTROL, "private, max-age=3600"),
                ],
                bytes,
            )
                .into_response());
        }
    }
    let key = format!("avatar:{}", query.account_id);
    if let Some((url, time)) = db.steam_cache_get(&key).await.map_err(internal)? {
        if chrono::Utc::now().timestamp() - time < 86400 {
            return Ok(Redirect::temporary(&url).into_response());
        }
    }
    let url = SteamClient::default()
        .avatar(&query.account_id)
        .await
        .map_err(|e| error(StatusCode::NOT_FOUND, e))?;
    db.steam_cache_put(&key, &url).await.map_err(internal)?;
    Ok(Redirect::temporary(&url).into_response())
}
#[derive(Deserialize)]
pub struct PriceQuery {
    pub app_id: String,
    pub market_hash_name: String,
}
pub async fn price(
    State(db): State<Database>,
    Query(query): Query<PriceQuery>,
) -> Result<Json<crate::launchers::steam::remote::PriceQuote>, ApiError> {
    use crate::launchers::steam::remote::PriceQuote;
    if !library::valid_app_id(&query.app_id)
        || query.market_hash_name.is_empty()
        || query.market_hash_name.len() > 512
        || query.market_hash_name.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    let key = format!("price:USD:{}:{}", query.app_id, query.market_hash_name);
    let read_cached = || async {
        db.steam_cache_get(&key).await.map_err(internal).map(|v| {
            v.and_then(|(s, t)| serde_json::from_str::<PriceQuote>(&s).ok().map(|p| (p, t)))
        })
    };
    if let Some((mut quote, time)) = read_cached().await? {
        if chrono::Utc::now().timestamp() - time < 3600 {
            quote.cached = true;
            return Ok(Json(quote));
        }
    }
    // Share pacing between windows and reuse quotes across accounts/stacks.
    static PRICE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = PRICE_LOCK.lock().await;
    let cached = read_cached().await?;
    if let Some((mut quote, time)) = cached.clone() {
        if chrono::Utc::now().timestamp() - time < 3600 {
            quote.cached = true;
            return Ok(Json(quote));
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(3100)).await;
    match SteamClient::default()
        .price(&query.app_id, &query.market_hash_name)
        .await
    {
        Ok(quote) => {
            db.steam_cache_put(&key, &serde_json::to_string(&quote).map_err(internal)?)
                .await
                .map_err(internal)?;
            Ok(Json(quote))
        }
        Err(message) => {
            if let Some((mut quote, _)) = cached {
                quote.cached = true;
                quote.warning = Some(message);
                Ok(Json(quote))
            } else {
                Err(error(StatusCode::BAD_GATEWAY, message))
            }
        }
    }
}
#[derive(Deserialize)]
pub struct GameQuery {
    pub app_id: String,
}
#[derive(Deserialize)]
pub struct ArtworkQuery {
    pub app_id: String,
    pub kind: String,
}
pub async fn artwork(State(db): State<Database>, Query(query): Query<ArtworkQuery>) -> Result<axum::response::Response, ApiError> {
    use axum::response::{IntoResponse, Redirect};
    use crate::launchers::steam::remote::SteamArtwork;
    if !library::valid_app_id(&query.app_id) || !matches!(query.kind.as_str(), "poster" | "hero") { return Err(invalid()); }
    let key = format!("artwork:v1:{}", query.app_id);
    let cached = db.steam_cache_get(&key).await.map_err(internal)?;
    let read = |cached: &Option<(String, i64)>| cached.as_ref()
        .and_then(|(json, _)| serde_json::from_str::<SteamArtwork>(json).ok());
    let mut art = cached.as_ref().filter(|(_, time)| chrono::Utc::now().timestamp() - time < 86400 * 7)
        .and_then(|_| read(&cached));
    if art.is_none() {
        // Coalesce simultaneous poster/hero cache misses and pace public Store requests.
        static ARTWORK_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _guard = ARTWORK_LOCK.lock().await;
        let newer = db.steam_cache_get(&key).await.map_err(internal)?;
        art = newer.as_ref().filter(|(_, time)| chrono::Utc::now().timestamp() - time < 86400 * 7)
            .and_then(|_| read(&newer));
        if art.is_none() {
            if let Ok(fresh) = SteamClient::default().artwork(&query.app_id).await {
                db.steam_cache_put(&key, &serde_json::to_string(&fresh).map_err(internal)?).await.map_err(internal)?;
                art = Some(fresh);
            } else { art = read(&cached); }
        }
    }
    let url = art.and_then(|art| if query.kind == "hero" { art.hero.or(art.poster) } else { art.poster.or(art.hero) })
        .unwrap_or_else(|| format!("https://cdn.akamai.steamstatic.com/steam/apps/{}/{}.jpg", query.app_id,
            if query.kind == "hero" { "header" } else { "library_600x900" }));
    let mut response = Redirect::temporary(&url).into_response();
    response.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("public, max-age=3600"));
    Ok(response)
}
pub async fn game_details(
    State(db): State<Database>,
    Query(query): Query<GameQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !library::valid_app_id(&query.app_id) || library::is_ignored_steam_app(&query.app_id, "") {
        return Err(invalid());
    }
    let key = format!("game-details:v2:{}", query.app_id);
    let name_key = format!("app-name:{}", query.app_id);
    if let Some((json, time)) = db.steam_cache_get(&key).await.map_err(internal)? {
        if chrono::Utc::now().timestamp() - time < 86400 * 7 {
            let mut val: serde_json::Value = serde_json::from_str(&json).map_err(internal)?;
            if val.get("name").is_none() || val["name"].is_null() {
                if let Ok(Some((cached_name, _))) = db.steam_cache_get(&name_key).await {
                    val["name"] = cached_name.into();
                }
            }
            if val.get("name").is_some() && !val["name"].is_null() {
                return Ok(Json(val));
            }
        }
    }
    let client = SteamClient::default();
    let details = client.app_details(&query.app_id).await.ok();
    let name = details.as_ref().and_then(|d| d.name.clone());
    let is_free = details.as_ref().and_then(|d| d.is_free);
    let cached = db.steam_cache_get("market-apps").await.map_err(internal)?;
    let mut apps = cached
        .as_ref()
        .filter(|(_, t)| chrono::Utc::now().timestamp() - t < 86400)
        .and_then(|(s, _)| serde_json::from_str::<Vec<String>>(s).ok());
    if apps.is_none() {
        if let Ok(fresh) = client.inventory_apps().await {
            db.steam_cache_put(
                "market-apps",
                &serde_json::to_string(&fresh).map_err(internal)?,
            )
            .await
            .map_err(internal)?;
            apps = Some(fresh);
        }
    }
    let has_inventory = apps.map(|list| list.contains(&query.app_id));
    if is_free.is_none() && has_inventory.is_none() && name.is_none() {
        return Err(error(
            StatusCode::BAD_GATEWAY,
            "Steam не предоставил сведения об игре. Повтори обновление позже.",
        ));
    }
    if let Some(ref title) = name {
        if !title.is_empty() {
            let _ = db.steam_cache_put(&name_key, title).await;
        }
    }
    let value = serde_json::json!({
        "name": name,
        "is_free": is_free,
        "has_inventory": has_inventory
    });
    db.steam_cache_put(&key, &value.to_string())
        .await
        .map_err(internal)?;
    Ok(Json(value))
}

#[derive(Deserialize)]
pub struct LaunchRequest {
    pub account_id: String,
    pub app_id: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub update_only: bool,
}
pub async fn launch(
    State(db): State<Database>,
    Json(query): Json<LaunchRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !library::valid_steam_id(&query.account_id) || !library::valid_app_id(&query.app_id) {
        return Err(invalid());
    }
    let _guard = super::handlers::SWITCH_API.try_lock().map_err(|_| {
        error(
            StatusCode::CONFLICT,
            "Переключение или запуск Steam уже выполняется.",
        )
    })?;
    let data = snapshot(&db).await?;
    let account = data
        .accounts
        .iter()
        .find(|a| a.id == query.account_id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Аккаунт не найден в Steam."))?;
    let mut free = data
        .games
        .iter()
        .filter(|g| g.game.app_id == query.app_id)
        .find_map(|g| g.is_free);
    // The UI may have resolved missing appinfo via the Store. Use the same fresh
    // normalized metadata here so free games work for accounts with no history.
    if free.is_none() {
        if let Some((payload, time)) = db
            .steam_cache_get(&format!("game-details:v2:{}", query.app_id))
            .await
            .map_err(internal)?
        {
            if chrono::Utc::now().timestamp() - time < 86400 {
                free = serde_json::from_str::<serde_json::Value>(&payload)
                    .ok()
                    .and_then(|v| v["is_free"].as_bool());
            }
        }
    }
    let game = data
        .games
        .iter()
        .find(|g| {
            g.game.app_id == query.app_id
                && (free == Some(true)
                    || (g.account_id.as_deref() == Some(&query.account_id)
                        && (["owned", "family"].contains(&g.access.as_str()) || g.playtime_minutes.is_some_and(|minutes| minutes > 0))))
        })
        .ok_or_else(|| {
            error(
                StatusCode::FORBIDDEN,
                "Доступ этого аккаунта к игре не подтверждён. Обнови библиотеку Steam.",
            )
        })?;
    if !query.update_only && !game.game.is_installed && game.update.is_none() {
        return Err(error(StatusCode::CONFLICT, "Сначала установи игру."));
    }
    let paths = SteamPaths::detect().map_err(internal)?;
    let command = crate::launchers::steam::client_launch::command(
        &paths,
        &query.app_id,
        query.update_only,
        &query.args,
    )
    .map_err(|m| error(StatusCode::BAD_REQUEST, m))?;
    if query.dry_run {
        return Ok(Json(
            serde_json::json!({"status":"dry_run","account_id":account.id,"switch_required":!account.is_active,"command":command,"update":game.update}),
        ));
    }
    // MostRecent identifies the last selected profile, not a logged-in client.
    // Prepare it even when the library marks it active; the manager can reuse
    // an authenticated running session without restarting Steam.
    crate::launchers::steam::SteamSessionManager::new()
        .switch_account(&account.id, Some(&db))
        .await
        .map_err(|e| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Не удалось переключить Steam: {e}"),
            )
        })?;
    let skip = std::env::var("SWCH_SKIP_PROCESS_TERMINATION")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    if !skip {
        command
            .dispatch()
            .await
            .map_err(|m| error(StatusCode::BAD_GATEWAY, m))?;
    }
    Ok(Json(
        serde_json::json!({"status":"requested","account_id":account.id,"update_only":query.update_only,
        "message":if query.update_only {"Запрос установки / обновления передан Steam. Прогресс появится после начала загрузки. Steam может запросить подтверждение."} else {"Запрос на запуск передан Steam для выбранного аккаунта. Steam выполнит обновление перед запуском, если оно требуется. Если Steam запросит вход, подтверди его."}}),
    ))
}

pub async fn downloads() -> Result<Json<serde_json::Value>, ApiError> {
    let states = tokio::task::spawn_blocking(|| SteamPaths::detect().map(|p| crate::launchers::steam::downloads::snapshot(&p)))
        .await
        .map_err(internal)?
        .map_err(internal)?;
    Ok(Json(
        serde_json::json!({"games":states,"updated_at":chrono::Utc::now().to_rfc3339()}),
    ))
}

#[derive(Deserialize)]
pub struct ContextQuery {
    pub account_id: String,
    #[serde(default)]
    pub refresh: bool,
}
pub async fn inventory_contexts(
    State(db): State<Database>,
    Query(q): Query<ContextQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !library::valid_steam_id(&q.account_id) {
        return Err(invalid());
    }
    let key = format!("contexts:{}", q.account_id);
    let cached = db.steam_cache_get(&key).await.map_err(internal)?;
    if !q.refresh {
        if let Some((s, t)) = &cached {
            if chrono::Utc::now().timestamp() - t < 300 {
                return Ok(Json(serde_json::from_str(s).map_err(internal)?));
            }
        }
    }
    match SteamClient::default()
        .inventory_contexts(&q.account_id)
        .await
    {
        Ok(contexts) => {
            let value = serde_json::json!({"contexts":contexts,"updated_at":chrono::Utc::now().to_rfc3339(),"warning":null});
            db.steam_cache_put(&key, &value.to_string())
                .await
                .map_err(internal)?;
            Ok(Json(value))
        }
        Err(message) => {
            if let Some((s, _)) = cached {
                let mut value: serde_json::Value = serde_json::from_str(&s).map_err(internal)?;
                value["warning"] = message.into();
                Ok(Json(value))
            } else {
                Err(error(StatusCode::BAD_GATEWAY, message))
            }
        }
    }
}
/// Return only normalized public inventory/price caches. Follow each current page chain;
/// never join a new first page to older pages left over from a previous snapshot.
pub async fn inventory_cache(
    State(db): State<Database>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = db
        .steam_cache_prefix("inventory:")
        .await
        .map_err(internal)?;
    let map: std::collections::HashMap<_, _> =
        rows.into_iter().map(|(k, s, t)| (k, (s, t))).collect();
    let mut pages = vec![];
    for (key, (text, stamp)) in &map {
        if !key.ends_with(":first") {
            continue;
        }
        let parts: Vec<_> = key.split(':').collect();
        if parts.len() != 5 {
            continue;
        }
        let Ok(mut page) = serde_json::from_str::<InventoryPage>(text) else {
            continue;
        };
        let mut seen = std::collections::HashSet::new();
        while let Some(cursor) = page.next_cursor.clone() {
            if !seen.insert(cursor.clone()) {
                break;
            }
            let next_key = format!("inventory:{}:{}:{}:{cursor}", parts[1], parts[2], parts[3]);
            let Some((text, t)) = map.get(&next_key) else {
                break;
            };
            if t < stamp {
                break;
            }
            let Ok(next) = serde_json::from_str::<InventoryPage>(text) else {
                break;
            };
            if next.total_count != page.total_count || next.updated_at < page.updated_at {
                break;
            }
            let ids: std::collections::HashSet<_> =
                page.items.iter().map(|i| i.asset_id.clone()).collect();
            page.items.extend(
                next.items
                    .into_iter()
                    .filter(|i| !ids.contains(&i.asset_id)),
            );
            page.next_cursor = next.next_cursor;
        }
        page.cached = true;
        pages.push(serde_json::json!({"account_id":parts[1],"app_id":parts[2],"context_id":parts[3],"page":page}));
    }
    let mut quotes = vec![];
    for (k, s, _) in db
        .steam_cache_prefix("price:USD:")
        .await
        .map_err(internal)?
    {
        let parts: Vec<_> = k.splitn(4, ':').collect();
        if parts.len() == 4 {
            if let Ok(quote) = serde_json::from_str::<serde_json::Value>(&s) {
                quotes.push(serde_json::json!({"app_id":parts[2],"name":parts[3],"quote":quote}));
            }
        }
    }
    let mut contexts = vec![];
    for (key, payload, _) in db.steam_cache_prefix("contexts:").await.map_err(internal)? {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) {
            contexts.push(
                serde_json::json!({"account_id":key.trim_start_matches("contexts:"),"data":value}),
            );
        }
    }
    Ok(Json(
        serde_json::json!({"pages":pages,"quotes":quotes,"contexts":contexts}),
    ))
}

#[derive(Deserialize)]
pub struct GamePriceQuery {
    pub app_id: String,
    pub cc: Option<String>,
    pub currency: Option<String>,
}

pub async fn game_price(
    State(db): State<Database>,
    Query(query): Query<GamePriceQuery>,
) -> Result<Json<crate::launchers::steam::remote::GamePrice>, ApiError> {
    use crate::launchers::steam::remote::GamePrice;
    if !library::valid_app_id(&query.app_id) || library::is_ignored_steam_app(&query.app_id, "") {
        return Err(invalid());
    }
    let cc = query.cc.as_deref().filter(|s| s.len() == 2 && s.chars().all(|c| c.is_ascii_alphabetic())).unwrap_or("US").to_uppercase();
    let currency = query.currency.as_deref().filter(|s| s.len() == 3 && s.chars().all(|c| c.is_ascii_alphabetic())).map(|s| s.to_uppercase()).unwrap_or_else(|| {
        match cc.as_str() {
            "BR" => "BRL".into(),
            "RU" => "RUB".into(),
            "KZ" => "KZT".into(),
            "UA" => "UAH".into(),
            "DE" | "FR" | "ES" | "IT" => "EUR".into(),
            "GB" => "GBP".into(),
            "PL" => "PLN".into(),
            "CN" => "CNY".into(),
            "JP" => "JPY".into(),
            _ => "USD".into(),
        }
    });

    let key = format!("game-price:{}:{}:{}", cc, currency, query.app_id);
    let read = || async {
        db.steam_cache_get(&key).await.map_err(internal).map(|v| {
            v.and_then(|(s, t)| serde_json::from_str::<GamePrice>(&s).ok().map(|q| (q, t)))
        })
    };
    let fresh = |(mut price, _time): (GamePrice, i64)| {
        price.cached = true;
        Some(price)
    };
    if let Some(price) = read().await?.and_then(fresh) {
        return Ok(Json(price));
    }
    static STORE_PRICES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = STORE_PRICES.lock().await;
    let cached = read().await?;
    if let Some(price) = cached.clone().and_then(fresh) {
        return Ok(Json(price));
    }
    tokio::time::sleep(std::time::Duration::from_millis(1600)).await;
    match SteamClient::default().game_price(&query.app_id, Some(&cc)).await {
        Ok(price) => {
            let save_key = format!("game-price:{}:{}:{}", price.region, price.currency, query.app_id);
            db.steam_cache_put(&save_key, &serde_json::to_string(&price).map_err(internal)?)
                .await
                .map_err(internal)?;
            if save_key != key {
                let _ = db.steam_cache_put(&key, &serde_json::to_string(&price).map_err(internal)?).await;
            }
            Ok(Json(price))
        }
        Err(message) => {
            if let Some((mut price, _)) = cached {
                price.cached = true;
                price.warning = Some(format!("{message} Показана сохранённая цена."));
                Ok(Json(price))
            } else {
                Err(error(StatusCode::BAD_GATEWAY, message))
            }
        }
    }
}

#[derive(Deserialize, Default)]
pub struct GamePricesQuery {
    pub cc: Option<String>,
}

pub async fn game_prices(
    State(db): State<Database>,
    Query(query): Query<GamePricesQuery>,
) -> Result<Json<Vec<crate::launchers::steam::remote::GamePrice>>, ApiError> {
    let mut result = vec![];
    let prefix = if let Some(ref cc) = query.cc {
        format!("game-price:{}:", cc.to_uppercase())
    } else {
        "game-price:".to_string()
    };
    for (_, payload, _) in db
        .steam_cache_prefix(&prefix)
        .await
        .map_err(internal)?
    {
        if let Ok(mut price) =
            serde_json::from_str::<crate::launchers::steam::remote::GamePrice>(&payload)
        {
            price.cached = true;
            if !result.iter().any(|p: &crate::launchers::steam::remote::GamePrice| p.app_id == price.app_id && p.currency == price.currency) {
                result.push(price);
            }
        }
    }
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct GameStatsQuery {
    pub account_id: String,
    pub app_id: String,
    #[serde(default)]
    pub refresh: bool,
}
#[derive(Deserialize)]
pub struct GameStatsKey {
    pub api_key: String,
}
pub async fn game_stats(
    State(db): State<Database>,
    Query(q): Query<GameStatsQuery>,
) -> Result<Json<crate::launchers::steam::game_stats::GameStats>, ApiError> {
    load_game_stats(db, q, None).await
}
pub async fn game_stats_with_key(
    State(db): State<Database>,
    Query(q): Query<GameStatsQuery>,
    Json(body): Json<GameStatsKey>,
) -> Result<Json<crate::launchers::steam::game_stats::GameStats>, ApiError> {
    if body.api_key.len() > 256 || body.api_key.trim().is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "Введи ключ Steam Web API."));
    }
    if !library::valid_steam_id(&q.account_id) || !crate::launchers::steam::game_stats::supported(&q.app_id) {
        return Err(invalid());
    }
    let (restrictions, checked) = super::handlers::check_and_store_steam_restrictions(
        &db, &q.account_id, Some(body.api_key.trim()),
    ).await;
    let mut result = load_game_stats(db, q, Some(body.api_key)).await?;
    if !checked {
        result.0.notes.push("Не удалось проверить блокировки Steam. Отсутствие результата не означает отсутствие бана.".into());
    } else if restrictions.vac_banned || restrictions.game_bans_count > 0 {
        result.0.notes.push(format!("На аккаунте обнаружены блокировки: VAC — {}, игровые — {}. Игры показаны в разделе аккаунта, когда Steam сообщает AppID или они указаны по данным поддержки.", restrictions.vac_bans_count, restrictions.game_bans_count));
    }
    Ok(result)
}
async fn load_game_stats(
    db: Database,
    q: GameStatsQuery,
    key: Option<String>,
) -> Result<Json<crate::launchers::steam::game_stats::GameStats>, ApiError> {
    use crate::launchers::steam::game_stats::{self, GameStats, StatsClient};
    if !library::valid_steam_id(&q.account_id) || !game_stats::supported(&q.app_id) {
        return Err(invalid());
    }
    let version = if q.app_id == "1422450" { "v3" } else { "v1" };
    let cache_key = format!("game-stats:{version}:{}:{}", q.account_id, q.app_id);
    // Serialize fetches and recheck the cache so repeated renders do not amplify requests.
    static STATS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = STATS.lock().await;
    let cached = db
        .steam_cache_get(&cache_key)
        .await
        .map_err(internal)?
        .and_then(|(s, t)| serde_json::from_str::<GameStats>(&s).ok().map(|v| (v, t)));
    if let Some((mut value, time)) = cached.clone() {
        let ttl = if q.refresh || value.status == "unavailable" {
            60
        } else {
            600
        };
        // A supplied key may recover a failed authentication immediately.
        if chrono::Utc::now().timestamp() - time < ttl
            && key.is_none()
        {
            value.cached = true;
            return Ok(Json(value));
        }
    }
    let key = key.or_else(|| std::env::var("STEAM_WEB_API_KEY").ok());
    let mut value = StatsClient::default()
        .fetch(&q.account_id, &q.app_id, key.as_deref().map(str::trim))
        .await;
    if matches!(value.status.as_str(), "unavailable" | "needs_key") {
        if let Some((mut previous, _)) =
            cached.filter(|(v, _)| matches!(v.status.as_str(), "available" | "partial"))
        {
            previous.cached = true;
            previous.stale = true;
            previous.notes.push(
                "Обновить не удалось. Показана сохранённая статистика; она могла устареть.".into(),
            );
            previous.notes.extend(value.notes.into_iter().skip(1));
            return Ok(Json(previous));
        }
    }
    value.cached = false;
    if value.status != "needs_key" {
        db.steam_cache_put(
            &cache_key,
            &serde_json::to_string(&value).map_err(internal)?,
        )
        .await
        .map_err(internal)?;
    }
    Ok(Json(value))
}

/// Opens only the fixed official key page, without switching accounts or reading keys.
pub async fn open_api_key() -> Result<Json<serde_json::Value>, ApiError> {
    let opened = tokio::task::spawn_blocking(|| {
        open::that("steam://openurl/https://steamcommunity.com/dev/apikey")
    }).await;
    if !matches!(opened, Ok(Ok(()))) {
        return Err(error(StatusCode::INTERNAL_SERVER_ERROR,
            "Не удалось открыть Steam. Используй ссылку «Открыть страницу в браузере»."));
    }
    Ok(Json(serde_json::json!({"status": "opened"})))
}

/// User-triggered interactive login; the helper key stays server-side and is used once.
pub async fn automatic_key_stats(
    State(db): State<Database>, #[allow(unused_mut)] Query(mut q): Query<GameStatsQuery>,
) -> Result<Json<crate::launchers::steam::game_stats::GameStats>, ApiError> {
    if q.app_id != "730" || !library::valid_steam_id(&q.account_id) { return Err(invalid()); }
    #[cfg(not(target_os = "macos"))]
    { let _ = (db, q); return Err(error(StatusCode::NOT_IMPLEMENTED, "Автоматическое получение ключа пока доступно только на macOS.")); }
    #[cfg(target_os = "macos")]
    {
        static KEY_WINDOW: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _guard = KEY_WINDOW.try_lock().map_err(|_| error(StatusCode::CONFLICT, "Окно получения ключа уже открыто."))?;
        let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("swch_steam_key");
        let mut command = tokio::process::Command::new(helper);
        command.arg(&q.account_id).kill_on_drop(true).stderr(std::process::Stdio::null());
        let output = tokio::time::timeout(std::time::Duration::from_secs(310), command.output())
            .await.map_err(|_| error(StatusCode::REQUEST_TIMEOUT, "Время ожидания входа истекло."))?
            .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "Не удалось открыть окно Steam. Собери помощник scripts/build-steam-key-helper.sh или введи ключ вручную."))?;
        let key = String::from_utf8(output.stdout).unwrap_or_default();
        if !output.status.success() || key.len() != 32 || !key.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(error(StatusCode::BAD_REQUEST, "Ключ не получен: окно закрыто или время ожидания истекло. Можно повторить или ввести ключ вручную."));
        }
        q.refresh = true;
        load_game_stats(db, q, Some(key)).await
    }
}
