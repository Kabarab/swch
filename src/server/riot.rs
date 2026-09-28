use crate::launchers::riot;
use axum::{http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};

type Error = (StatusCode, Json<Value>);
fn error(status: StatusCode, code: &str, message: &str) -> Error {
    (status, Json(json!({"code":code,"error":message})))
}

pub async fn library() -> Json<riot::Library> {
    Json(riot::library())
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchRequest {
    pub app_id: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

async fn run(request: LaunchRequest) -> Result<Json<Value>, Error> {
    let args = riot::launch_args(request.app_id.as_deref()).map_err(|code| {
        error(
            StatusCode::BAD_REQUEST,
            code,
            "Игра неизвестна или не поддерживается на этой ОС.",
        )
    })?;
    if !cfg!(any(target_os = "macos", target_os = "windows")) {
        return Err(error(
            StatusCode::NOT_IMPLEMENTED,
            "unsupported_platform",
            "Riot Client не поддерживается на этой ОС.",
        ));
    }
    let executable = riot::client().ok_or_else(|| {
        error(
            StatusCode::NOT_FOUND,
            "client_not_found",
            "Riot Client не найден. Установи клиент или укажи SWCH_RIOT_CLIENT.",
        )
    })?;
    if request.dry_run {
        return Ok(Json(
            json!({"status":"dry_run","executable":executable,"args":args}),
        ));
    }
    let mut child = tokio::process::Command::new(executable)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "launch_failed",
                "Не удалось запустить Riot Client.",
            )
        })?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(Json(
        json!({"status":"started","message":"Запрос передан Riot Client. Вход и выбор аккаунта выполняются в клиенте."}),
    ))
}

pub async fn launch(Json(request): Json<LaunchRequest>) -> Result<Json<Value>, Error> {
    if request.app_id.is_none() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "missing_product",
            "Укажи игру для запуска.",
        ));
    }
    run(request).await
}
pub async fn open_launcher(Json(request): Json<LaunchRequest>) -> Result<Json<Value>, Error> {
    run(request).await
}

pub async fn unsupported() -> Error {
    error(
        StatusCode::NOT_IMPLEMENTED,
        "unsupported_operation",
        "Вход и переключение аккаунтов выполняются в Riot Client.",
    )
}

pub async fn profiles(
    axum::extract::State(db): axum::extract::State<crate::storage::Database>,
) -> Result<Json<Value>, Error> {
    let profiles = db.riot_profiles().await.map_err(|_| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
            "Не удалось прочитать профили Riot.",
        )
    })?;
    Ok(Json(json!({"profiles":profiles})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncRequest {
    pub app_id: String,
}
static PROFILE_SYNC: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
pub async fn sync_profile(
    axum::extract::State(db): axum::extract::State<crate::storage::Database>,
    Json(request): Json<SyncRequest>,
) -> Result<Json<Value>, Error> {
    let _guard = PROFILE_SYNC.try_lock().map_err(|_| {
        error(
            StatusCode::CONFLICT,
            "sync_busy",
            "Синхронизация Riot уже выполняется.",
        )
    })?;
    let result = match request.app_id.as_str() {
        "league_of_legends" => riot::local::sync_league().await,
        "valorant" => riot::valorant::sync().await,
        _ => {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "unsupported_product",
                "Коллекция этой игры пока недоступна.",
            ))
        }
    };
    let profile = result.map_err(|message| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "profile_unavailable",
            &message,
        )
    })?;
    db.save_riot_profile(profile).await.map_err(|_| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
            "Не удалось сохранить профиль Riot.",
        )
    })?;
    profiles(axum::extract::State(db)).await
}
