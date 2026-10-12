//! Server REST API handlers for HoYoverse / miHoYo games.
use crate::{
    launchers::hoyoverse::{self, session::HoyoverseSessionManager, HoyoverseError},
    storage::Database,
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
static SESSION_API: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn error(e: HoyoverseError) -> (StatusCode, Json<Value>) {
    let status = match &e {
        HoyoverseError::Invalid(_) => StatusCode::BAD_REQUEST,
        HoyoverseError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({"error": e.to_string()})))
}

fn db_error(e: crate::storage::DbError) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": e.to_string()})),
    )
}

#[derive(Deserialize, Default)]
pub struct AccountsQuery {
    pub game: Option<String>,
}

#[derive(Deserialize)]
pub struct CurrentAccountQuery {
    pub game: Option<String>,
}

#[derive(Deserialize)]
pub struct SaveSessionRequest {
    pub game: String,
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct SwitchAccountRequest {
    pub account_id: String,
}

#[derive(Deserialize)]
pub struct PrepareRequest {
    pub game: String,
}

#[derive(Deserialize)]
pub struct RemoveAccountRequest {
    pub account_id: String,
}

#[derive(Deserialize)]
pub struct LaunchRequest {
    pub game: String,
}

async fn sync_accounts(
    db: &Database,
    manager: &HoyoverseSessionManager,
    game: Option<&str>,
) -> ApiResult {
    let accounts = manager.accounts(game).map_err(error)?;
    for account in &accounts {
        db.upsert_account(account).await.map_err(db_error)?;
    }
    Ok(Json(json!({ "accounts": accounts })))
}

/// GET /api/hoyoverse/games
/// Scans local games, registers them in DB, and returns status.
pub async fn library(State(db): State<Database>) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    let manager = HoyoverseSessionManager::default();
    let games = hoyoverse::scan_games().map_err(error)?;
    db.replace_hoyoverse_games(&games).await.map_err(db_error)?;

    let accounts = sync_accounts(&db, &manager, None).await?.0["accounts"].clone();
    let launcher_dir = hoyoverse::launcher_dir();

    Ok(Json(json!({
        "games": games,
        "accounts": accounts,
        "client_installed": launcher_dir.is_some(),
        "supported": cfg!(windows),
        "supported_games": hoyoverse::all_games(),
        "message": if cfg!(windows) {
            "Поддерживаются Genshin Impact, Honkai: Star Rail, Zenless Zone Zero, Honkai Impact 3rd и HoYoPlay."
        } else {
            "Быстрое переключение реестра HoYoverse оптимизировано для Windows."
        }
    })))
}

/// GET /api/hoyoverse/accounts
/// Returns saved accounts, optionally filtered by `?game=...`.
pub async fn accounts(
    State(db): State<Database>,
    Query(query): Query<AccountsQuery>,
) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    let manager = HoyoverseSessionManager::default();
    sync_accounts(&db, &manager, query.game.as_deref()).await
}

/// GET /api/hoyoverse/current-account
/// Inspects current logged-in identity from local registry.
pub async fn current_account(
    State(db): State<Database>,
    Query(query): Query<CurrentAccountQuery>,
) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    let manager = HoyoverseSessionManager::default();
    let game = query.game.as_deref().unwrap_or("genshin");
    let current_id = manager.current_id(game);

    let display_name = if let Some(ref id) = current_id {
        let db_name = db.get_accounts().await.ok().and_then(|list| {
            list.into_iter().find(|a| a.id == *id).and_then(|a| a.display_name)
        });
        db_name.or_else(|| {
            manager
                .accounts(Some(game))
                .ok()
                .and_then(|list| list.into_iter().find(|a| a.id == *id))
                .and_then(|a| a.display_name)
        })
    } else {
        None
    };

    Ok(Json(json!({
        "game": game,
        "account_id": current_id,
        "display_name": display_name
    })))
}

/// POST /api/hoyoverse/sessions/save
/// Captures active login credentials and writes local snapshot.
pub async fn save_session(
    State(db): State<Database>,
    Json(request): Json<SaveSessionRequest>,
) -> ApiResult {
    if request.display_name.as_ref().is_some_and(|s| s.len() > 200) {
        return Err(error(HoyoverseError::Invalid(
            "Название аккаунта слишком длинное".into(),
        )));
    }

    let _guard = SESSION_API.lock().await;
    let manager = HoyoverseSessionManager::default();
    let account = manager
        .save_current(&request.game, request.display_name.as_deref())
        .map_err(error)?;

    db.upsert_account(&account).await.map_err(db_error)?;
    let _ = sync_accounts(&db, &manager, Some(&request.game)).await?;

    Ok(Json(json!({
        "account": account,
        "message": format!("Сессия для '{}' успешно сохранена в SWCH.", account.username)
    })))
}

/// POST /api/hoyoverse/switch-account
/// Restores a previously saved HoYoverse account.
pub async fn switch_account(
    State(db): State<Database>,
    Json(request): Json<SwitchAccountRequest>,
) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    let manager = HoyoverseSessionManager::default();

    let account = manager.restore(&request.account_id).map_err(error)?;
    db.upsert_account(&account).await.map_err(db_error)?;
    let _ = sync_accounts(&db, &manager, None).await?;

    Ok(Json(json!({
        "status": "success",
        "account": account,
        "message": format!("Аккаунт '{}' активирован. Можно запускать игру.", account.username)
    })))
}

/// POST /api/hoyoverse/accounts/prepare
/// Clears credentials to allow fresh in-game authentication.
pub async fn prepare_account(
    State(db): State<Database>,
    Json(request): Json<PrepareRequest>,
) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    let manager = HoyoverseSessionManager::default();

    manager.prepare_login(&request.game).map_err(error)?;
    let _ = sync_accounts(&db, &manager, Some(&request.game)).await?;

    Ok(Json(json!({
        "status": "prepared",
        "game": request.game,
        "message": "Данные входа очищены. Запустите игру для входа в новый аккаунт, затем сохраните его в SWCH."
    })))
}

/// POST /api/hoyoverse/accounts/remove
/// Removes a saved account snapshot.
pub async fn remove_account(
    State(db): State<Database>,
    Json(request): Json<RemoveAccountRequest>,
) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    let manager = HoyoverseSessionManager::default();

    manager.remove(&request.account_id).map_err(error)?;
    let _ = db.delete_account(&request.account_id, "hoyoverse").await;

    Ok(Json(json!({
        "status": "removed",
        "account_id": request.account_id
    })))
}

/// POST /api/hoyoverse/launch
/// Starts the selected HoYoverse game.
pub async fn launch_game(Json(request): Json<LaunchRequest>) -> ApiResult {
    hoyoverse::launch_game(&request.game).map_err(error)?;
    Ok(Json(json!({
        "status": "launched",
        "game": request.game
    })))
}

/// POST /api/hoyoverse/open-launcher
/// Opens the official HoYoPlay launcher.
pub async fn open_launcher() -> ApiResult {
    hoyoverse::open_launcher().map_err(error)?;
    Ok(Json(json!({
        "status": "opened"
    })))
}
