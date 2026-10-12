use crate::{
    launchers::ubisoft::{self, session::SessionManager, UbisoftError},
    storage::Database,
};
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};

mod automatic;
pub use automatic::{cancel as automatic_cancel, status as automatic_status};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
static SESSION_API: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn error(e: UbisoftError) -> (StatusCode, Json<Value>) {
    let status = match &e {
        UbisoftError::Invalid(_) => StatusCode::BAD_REQUEST,
        UbisoftError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({"error": e.to_string()})))
}
fn db_error(e: crate::storage::DbError) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error":e.to_string()})),
    )
}

async fn sync_accounts(db: &Database, manager: &SessionManager) -> ApiResult {
    manager.refresh_names().map_err(error)?;
    let accounts = manager.accounts().map_err(error)?;
    for account in &accounts {
        db.upsert_account(account).await.map_err(db_error)?;
    }
    Ok(Json(json!({"accounts": accounts})))
}

pub async fn library(State(db): State<Database>) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    let manager = SessionManager::default();
    let games = ubisoft::scan_games().map_err(error)?;
    db.replace_ubisoft_games(&games).await.map_err(db_error)?;
    let accounts = sync_accounts(&db, &manager).await?.0["accounts"].clone();
    Ok(Json(
        json!({"games":games,"accounts":accounts,"client_installed":manager.launcher_dir.is_some(),
        "supported":cfg!(windows), "message": if cfg!(windows) {
            "Установленные игры Ubisoft Connect. Вход и проверка доступа выполняются официальным клиентом."
        } else { "Ubisoft Connect поддерживается на Windows." }}),
    ))
}

pub async fn accounts(State(db): State<Database>) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    sync_accounts(&db, &SessionManager::default()).await
}

/// Read-only preview: expose the account identity and name, never cached credentials.
pub async fn current_account() -> ApiResult {
    let _guard = SESSION_API.lock().await;
    let manager = SessionManager::default();
    let id = manager.current_id();
    let name = id
        .as_deref()
        .and_then(|id| ubisoft::profile::cached_name(&manager.data_dir, id))
        .or_else(|| {
            manager
                .accounts()
                .ok()?
                .into_iter()
                .find(|a| Some(&a.id) == id.as_ref())?
                .display_name
        });
    Ok(Json(json!({"account_id":id,"display_name":name})))
}

#[derive(Deserialize)]
pub struct SaveRequest {
    pub display_name: Option<String>,
}

pub async fn save_session(
    State(db): State<Database>,
    Json(request): Json<SaveRequest>,
) -> ApiResult {
    if request.display_name.as_ref().is_some_and(|s| s.len() > 200) {
        return Err(error(UbisoftError::Invalid(
            "Название аккаунта слишком длинное".into(),
        )));
    }
    let _guard = SESSION_API.lock().await;
    automatic::cancel_pending().await;
    let manager = SessionManager::default();
    if manager.authenticated_id().is_none() {
        return Err(error(UbisoftError::Invalid(
            "Ubisoft Connect сейчас ожидает вход. Войди с включённым «Запомнить меня» и дождись главной страницы, затем сохрани сессию.".into(),
        )));
    }
    manager.close_client().await.map_err(error)?;
    let account = manager
        .save_current(request.display_name.as_deref())
        .map_err(error)?;
    let _ = sync_accounts(&db, &manager).await?;
    Ok(Json(
        json!({"account":account,"message":"Сессия Ubisoft сохранена. Клиент закрыт для сохранения данных входа."}),
    ))
}

#[derive(Deserialize)]
pub struct LaunchRequest {
    pub account_id: Option<String>,
    pub app_id: Option<String>,
    pub launch: Option<bool>,
}

#[derive(Deserialize)]
pub struct PrepareRequest {
    pub launch: Option<bool>,
    pub automatic: Option<bool>,
}

pub async fn prepare_account(
    State(db): State<Database>,
    Json(request): Json<PrepareRequest>,
) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    automatic::cancel_pending().await;
    let manager = SessionManager::default();
    let open = request.launch.unwrap_or(true);
    let automatic_id = request
        .automatic
        .unwrap_or(false)
        .then(automatic::new_id)
        .transpose()
        .map_err(error)?;
    if open && (!cfg!(windows) || manager.launcher_dir.is_none()) {
        return Err(error(UbisoftError::Invalid(
            "Установи Ubisoft Connect на Windows.".into(),
        )));
    }
    let current = manager.authenticated_id();
    manager.close_client().await.map_err(error)?;
    let saved = if current
        .filter(|id| manager.authenticated_id().as_ref() == Some(id))
        .is_some()
    {
        Some(manager.save_current(None).map_err(error)?.id)
    } else {
        None
    };
    manager.prepare_login().map_err(error)?;
    if open {
        if let Err(e) = ubisoft::open_client(None) {
            if let Some(id) = saved.as_deref() {
                manager.restore(id).map_err(error)?;
            }
            return Err(error(e));
        }
    }
    let _ = sync_accounts(&db, &manager).await?;
    if let Some(id) = automatic_id.as_ref() {
        automatic::begin(id.clone(), db, manager, open, saved.clone()).await;
    }
    Ok(Json(
        json!({"status":"success", "saved_account_id":saved, "automatic_session_id":automatic_id,
        "message": if automatic_id.is_some() { "Войди в Ubisoft с «Запомнить меня». SWCH добавит аккаунт автоматически." } else { "Войди в Ubisoft с «Запомнить меня» и сохрани текущую сессию." }}),
    ))
}

async fn activate(db: &Database, request: LaunchRequest, open: bool) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    automatic::cancel_pending().await;
    let manager = SessionManager::default();
    if open && (!cfg!(windows) || manager.launcher_dir.is_none()) {
        return Err(error(UbisoftError::Invalid(
            "Установи Ubisoft Connect на Windows, затем повтори запуск.".into(),
        )));
    }
    if let Some(app) = &request.app_id {
        ubisoft::launch_uri(app).map_err(error)?;
        if !ubisoft::scan_games()
            .map_err(error)?
            .iter()
            .any(|g| &g.app_id == app)
        {
            return Err(error(UbisoftError::Invalid(
                "Игра Ubisoft не установлена. Обнови библиотеку.".into(),
            )));
        }
    }
    if let Some(id) = &request.account_id {
        manager.validate_saved(id).map_err(error)?;
        let authenticated = manager.authenticated_id();
        manager.close_client().await.map_err(error)?;
        if let Some(current) =
            authenticated.filter(|id| manager.authenticated_id().as_ref() == Some(id))
        {
            // Preserve refreshed credentials only for accounts the user has connected.
            if manager.validate_saved(&current).is_ok() {
                manager.save_current(None).map_err(error)?;
            }
        }
        manager.restore(id).map_err(error)?;
        let _ = sync_accounts(db, &manager).await?;
    }
    if open {
        ubisoft::open_client(request.app_id.as_deref()).map_err(error)?;
    }
    Ok(Json(
        json!({"status":"success", "message":if open { "Запрос передан Ubisoft Connect. Клиент проверит вход и доступ к игре." } else { "Сохранённая сессия Ubisoft восстановлена." }}),
    ))
}

pub async fn switch_account(
    State(db): State<Database>,
    Json(request): Json<LaunchRequest>,
) -> ApiResult {
    if request.account_id.is_none() {
        return Err(error(UbisoftError::Invalid("Укажи аккаунт Ubisoft".into())));
    }
    let open = request.launch.unwrap_or(true);
    activate(&db, request, open).await
}
pub async fn open_launcher(
    State(db): State<Database>,
    Json(mut request): Json<LaunchRequest>,
) -> ApiResult {
    request.app_id = None;
    activate(&db, request, true).await
}
pub async fn launch(State(db): State<Database>, Json(request): Json<LaunchRequest>) -> ApiResult {
    if request.app_id.is_none() {
        return Err(error(UbisoftError::Invalid("Укажи игру Ubisoft".into())));
    }
    activate(&db, request, true).await
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    pub account_id: String,
}
pub async fn remove_account(
    State(db): State<Database>,
    Json(request): Json<RemoveRequest>,
) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    automatic::cancel_pending().await;
    let manager = SessionManager::default();
    let uuid = ubisoft::session::account_uuid(&request.account_id).map_err(error)?;
    manager.remove(&uuid).map_err(error)?;
    db.delete_account(&format!("ubisoft_{uuid}"), "ubisoft")
        .await
        .map_err(db_error)?;
    Ok(Json(
        json!({"status":"success", "message":"Аккаунт удалён из SWCH. Вход в Ubisoft Connect сохранён."}),
    ))
}
