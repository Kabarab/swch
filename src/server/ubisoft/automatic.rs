//! Short-lived automatic capture started by an explicit add-account action.
use super::{db_error, error, sync_accounts, ApiResult, SESSION_API};
use crate::{
    launchers::ubisoft::{self, session::SessionManager, UbisoftError},
    storage::{Account, Database},
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Clone, Serialize)]
struct Login {
    session_id: String,
    status: &'static str,
    account: Option<Account>,
    error: Option<String>,
    #[serde(skip)]
    deadline: Instant,
    #[serde(skip)]
    previous_account_id: Option<String>,
    #[serde(skip)]
    reopen: bool,
}
static LOGIN: tokio::sync::Mutex<Option<Login>> = tokio::sync::Mutex::const_new(None);

pub(super) fn new_id() -> Result<String, UbisoftError> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| {
        UbisoftError::Invalid("Не удалось начать автоматическое добавление аккаунта.".into())
    })?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub(super) async fn cancel_pending() {
    if let Some(login) = LOGIN.lock().await.as_mut() {
        if login.status == "waiting" {
            login.status = "cancelled";
        }
    }
}

pub(super) async fn begin(
    id: String,
    db: Database,
    manager: SessionManager,
    reopen: bool,
    previous_account_id: Option<String>,
) {
    *LOGIN.lock().await = Some(Login {
        session_id: id.clone(),
        status: "waiting",
        account: None,
        error: None,
        deadline: Instant::now() + Duration::from_secs(600),
        previous_account_id,
        reopen,
    });
    tokio::spawn(async move {
        let mut authenticated_since: Option<(String, Instant)> = None;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            {
                let mut state = LOGIN.lock().await;
                let Some(login) = state
                    .as_mut()
                    .filter(|l| l.session_id == id && l.status == "waiting")
                else {
                    return;
                };
                if Instant::now() >= login.deadline {
                    login.status = "expired";
                    return;
                }
            }
            let Some(account_id) = manager.authenticated_id() else {
                authenticated_since = None;
                continue;
            };
            if authenticated_since
                .as_ref()
                .is_none_or(|(seen, _)| seen != &account_id)
            {
                authenticated_since = Some((account_id.clone(), Instant::now()));
            }
            // Login is reported before the home/profile response finishes loading.
            // Keep the native client running until its public name arrives, with a
            // bounded fallback for offline or restricted accounts. Later syncs repair it.
            if ubisoft::profile::cached_name(&manager.data_dir, &account_id).is_none()
                && authenticated_since
                    .as_ref()
                    .is_some_and(|(_, since)| since.elapsed() < Duration::from_secs(20))
            {
                continue;
            }
            // Let the official client finish writing its remembered login/profile.
            tokio::time::sleep(Duration::from_secs(2)).await;
            let _guard = SESSION_API.lock().await;
            {
                let mut state = LOGIN.lock().await;
                let Some(login) = state
                    .as_mut()
                    .filter(|l| l.session_id == id && l.status == "waiting")
                else {
                    return;
                };
                if manager.authenticated_id().as_ref() != Some(&account_id) {
                    continue;
                }
                login.status = "saving";
            }
            let result = async {
                manager.close_client().await.map_err(error)?;
                if manager.authenticated_id().as_ref() != Some(&account_id) {
                    return Err(error(UbisoftError::Invalid(
                        "Аккаунт изменился во время сохранения. Начни добавление снова.".into(),
                    )));
                }
                let account = manager.save_current(None).map_err(error)?;
                db.upsert_account(&account).await.map_err(db_error)?;
                let _ = sync_accounts(&db, &manager).await?;
                Ok(account)
            }
            .await;
            // Leave the official client available after capturing its stable files.
            let reopen_error = if reopen {
                ubisoft::open_client(None).err().map(|e| e.to_string())
            } else {
                None
            };
            let mut state = LOGIN.lock().await;
            if let Some(login) = state.as_mut().filter(|l| l.session_id == id) {
                match result {
                    Ok(account) => {
                        login.status = "connected";
                        login.account = Some(account);
                        login.error = reopen_error;
                    }
                    Err((_, details)) => {
                        login.status = "failed";
                        login.error = Some(
                            details.0["error"]
                                .as_str()
                                .unwrap_or("Не удалось сохранить аккаунт Ubisoft.")
                                .into(),
                        );
                    }
                }
            }
            return;
        }
    });
}

#[derive(Deserialize)]
pub struct SessionRequest {
    pub session_id: String,
}

pub async fn status(Query(request): Query<SessionRequest>) -> ApiResult {
    let state = LOGIN.lock().await;
    let login = state
        .as_ref()
        .filter(|l| l.session_id == request.session_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"Попытка добавления Ubisoft не найдена. Начни вход снова."})),
            )
        })?;
    Ok(Json(
        serde_json::to_value(login).map_err(|e| error(e.into()))?,
    ))
}

pub async fn cancel(State(db): State<Database>, Json(request): Json<SessionRequest>) -> ApiResult {
    let _guard = SESSION_API.lock().await;
    let mut state = LOGIN.lock().await;
    let login = state
        .as_mut()
        .filter(|l| l.session_id == request.session_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"Попытка добавления Ubisoft не найдена."})),
            )
        })?;
    let restore = if login.status == "waiting" {
        login.status = "cancelled";
        login
            .previous_account_id
            .clone()
            .map(|id| (id, login.reopen))
    } else {
        None
    };
    let response = serde_json::to_value(login).map_err(|e| error(e.into()))?;
    drop(state);
    if let Some((id, reopen)) = restore {
        let manager = SessionManager::default();
        manager.close_client().await.map_err(error)?;
        manager.restore(&id).map_err(error)?;
        if reopen {
            ubisoft::open_client(None).map_err(error)?;
        }
        let _ = sync_accounts(&db, &manager).await?;
    }
    Ok(Json(response))
}
