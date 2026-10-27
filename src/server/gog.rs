use crate::{
    launchers::gog::{self, auth::GogStore, local, GogError},
    storage::{Account, Database},
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
fn error(e: GogError) -> (StatusCode, Json<Value>) {
    let status = match &e {
        GogError::Invalid(_) => StatusCode::BAD_REQUEST,
        GogError::Unauthorized => StatusCode::UNAUTHORIZED,
        GogError::Network | GogError::Http(_) | GogError::Response => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({"error":e.to_string()})))
}
async fn connected(db: &Database) -> Result<Vec<Account>, GogError> {
    Ok(db
        .get_accounts()
        .await?
        .into_iter()
        .filter(|a| a.launcher == "gog")
        .collect())
}
async fn installations() -> Result<Vec<crate::storage::Game>, GogError> {
    tokio::task::spawn_blocking(local::scan_games)
        .await
        .map_err(|_| GogError::Response)?
}

pub async fn auth_url() -> Json<Value> {
    Json(json!({"url":gog::auth::auth_url()}))
}
pub async fn open_auth() -> ApiResult {
    tokio::task::spawn_blocking(|| open::that(gog::auth::auth_url()))
        .await
        .map_err(|_| error(GogError::Response))?
        .map_err(|e| error(e.into()))?;
    Ok(Json(
        json!({"message":"Войди в GOG в браузере и скопируй адрес страницы после входа."}),
    ))
}
#[derive(Deserialize)]
pub struct LoginRequest {
    pub code: String,
}
pub async fn login(
    State(db): State<Database>,
    Extension(store): Extension<Arc<GogStore>>,
    Json(request): Json<LoginRequest>,
) -> ApiResult {
    let account = store.login(&db, &request.code).await.map_err(error)?;
    let mut warnings = Vec::new();
    match store.sync(&db, &account.id).await {
        Ok((_, notices)) => warnings.extend(notices),
        Err(e) => warnings.push(format!(
            "Аккаунт подключён; синхронизация не завершена: {e}"
        )),
    }
    Ok(Json(
        json!({"account":account,"warnings":warnings,"message":"Аккаунт GOG подключён."}),
    ))
}
pub async fn accounts(State(db): State<Database>) -> ApiResult {
    Ok(Json(
        json!({"accounts":connected(&db).await.map_err(error)?}),
    ))
}
#[derive(Default, Deserialize)]
pub struct LibraryQuery {
    pub account_id: Option<String>,
}

pub async fn library(
    State(db): State<Database>,
    Extension(store): Extension<Arc<GogStore>>,
    Query(query): Query<LibraryQuery>,
) -> ApiResult {
    let accounts = connected(&db).await.map_err(error)?;
    if query
        .account_id
        .as_ref()
        .is_some_and(|id| !accounts.iter().any(|a| &a.id == id))
    {
        return Err(error(GogError::Invalid("Аккаунт GOG не найден.".into())));
    }
    let links = db.get_game_accounts().await.map_err(|e| error(e.into()))?;
    let local = installations().await.map_err(error)?;
    let installed: BTreeMap<_, _> = local.into_iter().map(|g| (g.app_id.clone(), g)).collect();
    let mut games = BTreeMap::new();
    for mut game in db
        .get_games()
        .await
        .map_err(|e| error(e.into()))?
        .into_iter()
        .filter(|g| g.launcher == "gog")
    {
        let owners: Vec<_> = links
            .iter()
            .filter(|l| l.launcher == "gog" && l.game_id == game.id)
            .map(|l| l.account_id.clone())
            .collect();
        if query
            .account_id
            .as_ref()
            .is_some_and(|id| !owners.contains(id))
        {
            continue;
        }
        // Installation is machine-wide, ownership is account-specific.
        game.is_installed = false;
        game.install_path = None;
        game.executable_path = None;
        if let Some(local) = installed.get(&game.app_id) {
            game.is_installed = true;
            game.install_path = local.install_path.clone();
        }
        let mut value = serde_json::to_value(&game).map_err(|_| error(GogError::Response))?;
        value["owner_ids"] = json!(owners);
        games.insert(game.app_id, value);
    }
    if query.account_id.is_none() {
        for (id, game) in installed {
            if games.contains_key(&id) {
                continue;
            }
            let mut value = serde_json::to_value(game).map_err(|_| error(GogError::Response))?;
            value["owner_ids"] = json!([]);
            value["local_only"] = json!(true);
            games.insert(id, value);
        }
    }
    let client_installed = tokio::task::spawn_blocking(local::launcher_dir)
        .await
        .ok()
        .flatten()
        .is_some();
    let launcher_account_id = store.launcher_account_id();
    let accounts: Vec<_> = accounts
        .into_iter()
        .map(|a| {
            let active = launcher_account_id.as_deref() == Some(&a.id);
            let mut value = serde_json::to_value(a).unwrap_or(Value::Null);
            value["launcher_active"] = json!(active);
            value
        })
        .collect();
    Ok(Json(
        json!({"games":games.into_values().collect::<Vec<_>>(),"accounts":accounts,
        "client_installed":client_installed,"launch_supported":cfg!(windows),"launcher_account_id":launcher_account_id,
        "message":"Библиотека GOG. SWCH переключает аккаунт в GALAXY перед запуском игры."}),
    ))
}

#[derive(Default, Deserialize)]
pub struct AccountRequest {
    pub account_id: Option<String>,
}
pub async fn sync(
    State(db): State<Database>,
    Extension(store): Extension<Arc<GogStore>>,
    Json(request): Json<AccountRequest>,
) -> ApiResult {
    let accounts = connected(&db).await.map_err(error)?;
    if request
        .account_id
        .as_ref()
        .is_some_and(|id| !accounts.iter().any(|a| &a.id == id))
    {
        return Err(error(GogError::Invalid("Аккаунт GOG не найден.".into())));
    }
    let mut warnings = Vec::new();
    let mut synced = 0;
    for account in accounts
        .iter()
        .filter(|a| request.account_id.as_ref().is_none_or(|id| &a.id == id))
    {
        match store.sync(&db, &account.id).await {
            Ok((_, notices)) => {
                synced += 1;
                warnings.extend(notices);
            }
            Err(e) if request.account_id.is_some() => return Err(error(e)),
            Err(e) => warnings.push(format!("{}: {e}", account.username)),
        }
    }
    Ok(Json(
        json!({"synced_accounts":synced,"warnings":warnings,"message":"Синхронизация GOG завершена."}),
    ))
}
pub async fn switch_account(
    State(db): State<Database>,
    Extension(store): Extension<Arc<GogStore>>,
    Json(request): Json<AccountRequest>,
) -> ApiResult {
    let id = request
        .account_id
        .ok_or_else(|| error(GogError::Invalid("Укажи аккаунт GOG.".into())))?;
    if !connected(&db)
        .await
        .map_err(error)?
        .iter()
        .any(|a| a.id == id)
    {
        return Err(error(GogError::Invalid("Аккаунт GOG не найден.".into())));
    }
    store.activate_launcher(&db, &id).await.map_err(error)?;
    Ok(Json(
        json!({"message":"GOG GALAXY вошёл в выбранный аккаунт."}),
    ))
}
pub async fn remove(
    State(db): State<Database>,
    Extension(store): Extension<Arc<GogStore>>,
    claimer:Option<Extension<Arc<crate::auto_claim::AutoClaimer>>>,
    Json(request): Json<AccountRequest>,
) -> ApiResult {
    let id = request
        .account_id
        .ok_or_else(|| error(GogError::Invalid("Укажи аккаунт GOG.".into())))?;
    let _guard=if let Some(Extension(service))=&claimer {Some(service.account_lock("gog",&id).await)} else {None};
    store.remove(&db, &id).await.map_err(error)?;
    db.set_auto_claim_value(&format!("web-excluded:gog:{id}"),"true").await.map_err(|e|error(GogError::Database(e)))?;
    tokio::task::spawn_blocking(move||crate::auto_claim::web::Session::delete("gog",&id)).await.map_err(|_|error(GogError::Response))?.map_err(|e|error(GogError::Invalid(e)))?;
    Ok(Json(json!({"message":"Аккаунт GOG удалён из SWCH."})))
}
pub async fn open_launcher(
    State(db): State<Database>,
    Extension(store): Extension<Arc<GogStore>>,
    Json(request): Json<AccountRequest>,
) -> ApiResult {
    if let Some(id) = request.account_id {
        store.activate_launcher(&db, &id).await.map_err(error)?;
    }
    tokio::task::spawn_blocking(|| local::open_client(None))
        .await
        .map_err(|_| error(GogError::Response))?
        .map_err(error)?;
    Ok(Json(json!({"message":"GOG GALAXY открыт."})))
}
#[derive(Deserialize)]
pub struct LaunchRequest {
    pub app_id: String,
    pub account_id: Option<String>,
}
pub async fn launch(
    State(db): State<Database>,
    Extension(store): Extension<Arc<GogStore>>,
    Json(request): Json<LaunchRequest>,
) -> ApiResult {
    if !gog::valid_id(&request.app_id) {
        return Err(error(GogError::Invalid("Некорректный ID игры GOG.".into())));
    }
    if let Some(id) = &request.account_id {
        if !connected(&db)
            .await
            .map_err(error)?
            .iter()
            .any(|a| &a.id == id)
            || !db
                .get_game_accounts()
                .await
                .map_err(|e| error(e.into()))?
                .iter()
                .any(|l| {
                    &l.account_id == id
                        && l.launcher == "gog"
                        && l.game_id == format!("gog_{}", request.app_id)
                })
        {
            return Err(error(GogError::Invalid(
                "Игра отсутствует в библиотеке выбранного аккаунта GOG.".into(),
            )));
        }
    }
    let installed = installations()
        .await
        .map_err(error)?
        .into_iter()
        .find(|g| g.app_id == request.app_id);
    if installed.is_none()
        && request.account_id.is_none()
        && !db
            .get_games()
            .await
            .map_err(|e| error(e.into()))?
            .iter()
            .any(|g| g.launcher == "gog" && g.app_id == request.app_id)
    {
        return Err(error(GogError::Invalid(
            "Игра GOG не установлена. Обнови библиотеку.".into(),
        )));
    }
    if let Some(id) = request.account_id.as_deref() {
        store.activate_launcher(&db, id).await.map_err(error)?;
    }
    tokio::task::spawn_blocking(move || local::open_client(installed.as_ref()))
        .await
        .map_err(|_| error(GogError::Response))?
        .map_err(error)?;
    Ok(Json(
        json!({"message":"Запрос передан GOG GALAXY с выбранным аккаунтом."}),
    ))
}
