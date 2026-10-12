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
    if let Some(product) = request.app_id.as_deref() {
        let client = riot::local::LocalClient::ensure_riot_ready().await.map_err(|message| {
            error(StatusCode::CONFLICT, "riot_login_required", &message)
        })?;
        client.launch_product(product).await.map_err(|message| {
            error(StatusCode::CONFLICT, "product_launch_failed", &message)
        })?;
        return Ok(Json(json!({"status":"started", "message":format!("{} запускается...", riot::product(product).unwrap_or("Игра"))})));
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
    let message = match request.app_id.as_deref() {
        Some("valorant") => "VALORANT запускается...",
        Some("league_of_legends") => "League of Legends запускается...",
        Some("bacon") => "Legends of Runeterra запускается...",
        _ => "Riot Client запущен.",
    };
    Ok(Json(json!({"status":"started","message": message})))
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

#[derive(Deserialize)]
pub struct SwitchAccountRequest {
    pub account_id: Option<String>,
    pub account_name: Option<String>,
    pub display_name: Option<String>,
    #[serde(default)]
    pub launch: Option<bool>,
}

pub async fn switch_account(
    Json(request): Json<SwitchAccountRequest>,
) -> Result<Json<Value>, Error> {
    let _guard = super::handlers::SWITCH_API.try_lock().map_err(|_| {
        error(
            StatusCode::CONFLICT,
            "switch_busy",
            "Переключение аккаунта уже выполняется.",
        )
    })?;
    let target = request
        .account_id
        .as_deref()
        .or(request.account_name.as_deref())
        .ok_or_else(|| {
            error(
                StatusCode::BAD_REQUEST,
                "missing_account",
                "Укажи аккаунт для переключения.",
            )
        })?;
    let mgr = riot::session::RiotSessionManager::new();
    let result = mgr
        .switch_account(target, request.display_name.as_deref())
        .await
        .map_err(|e| {
            if matches!(e, riot::session::RiotError::LoginRequired) {
                return error(StatusCode::CONFLICT, "riot_login_required", &e.to_string());
            }
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "switch_failed",
                &format!("Не удалось переключить аккаунт Riot: {}", e),
            )
        })?;

    if result.no_saved_session {
        return Err(error(
            StatusCode::CONFLICT,
            "no_saved_session",
            result
                .message
                .as_deref()
                .unwrap_or("Сессия Riot ещё не сохранена."),
        ));
    }

    // Launch Riot Client with the switched profile if requested (default: true) and session exists
    if request.launch.unwrap_or(true) && !result.no_saved_session {
        if let Some(executable) = riot::client() {
            let skip_proc = std::env::var("SWCH_SKIP_PROCESS_TERMINATION")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            if !skip_proc {
                let expected_account = mgr.read_current_account().account_id;
                let mut child = tokio::process::Command::new(executable)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map_err(|_| {
                        error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "launch_failed",
                            "Сессия восстановлена, но Riot Client не удалось запустить.",
                        )
                    })?;
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
                // A copied cookie jar is not proof of a successful login: Riot
                // can revoke saved sessions. Confirm through the local client.
                let authenticated = async {
                    loop {
                        if let Ok(client) =
                            riot::local::LocalClient::discover(riot::local::riot_paths())
                        {
                            if let Ok(tokens) = client.get("/entitlements/v1/token").await {
                                if tokens["accessToken"]
                                    .as_str()
                                    .is_some_and(|s| !s.is_empty())
                                    && expected_account.as_deref().is_some_and(|expected| {
                                        tokens["subject"].as_str() == Some(expected)
                                    })
                                {
                                    return;
                                }
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                };
                tokio::time::timeout(std::time::Duration::from_secs(25), authenticated)
                    .await.map_err(|_| error(StatusCode::CONFLICT, "riot_login_required",
                        "Riot Client не подтвердил вход с сохранённой сессией. Войди в нужный аккаунт с включённым «Stay signed in» и нажми «Сохранить текущую сессию Riot»."))?;
                mgr.refresh_saved_session().map_err(|_| error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "backup_failed",
                    "Вход Riot выполнен, но обновлённую сессию не удалось сохранить. Нажми «Сохранить текущую сессию Riot».",
                ))?;
            }
        } else {
            return Err(error(
                StatusCode::NOT_FOUND,
                "client_not_found",
                "Riot Client не найден.",
            ));
        }
    }

    Ok(Json(json!(result)))
}

#[derive(Deserialize, Default)]
pub struct SaveSessionRequest {
    pub account_id: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
}

fn live_save_error(auth: &Value) -> Option<(&'static str, &'static str)> {
    if auth["type"].as_str() != Some("authenticated") {
        return None;
    }
    if auth["persistLogin"].as_bool() == Some(false) {
        Some(("riot_remember_disabled",
            "В Riot Client «Stay signed in» выключено для текущего входа. Автоматически применить его к этому входу не удалось. Выйди из аккаунта через меню Riot, включи галочку на экране входа, войди снова и сохрани сессию в SWCH."))
    } else if auth["persistLogin"].as_bool() == Some(true) {
        Some(("riot_session_not_written",
            "Riot Client вошёл в аккаунт, но данные для повторного входа ещё не записаны на диск. Полностью закрой Riot Client без выхода из аккаунта и повтори сохранение."))
    } else {
        None
    }
}

pub async fn save_session(
    axum::extract::State(db): axum::extract::State<crate::storage::Database>,
    Json(request): Json<SaveSessionRequest>,
) -> Result<Json<Value>, Error> {
    let _guard = super::handlers::SWITCH_API.try_lock().map_err(|_| {
        error(
            StatusCode::CONFLICT,
            "switch_busy",
            "Переключение аккаунта уже выполняется.",
        )
    })?;
    let mgr = riot::session::RiotSessionManager::new();
    let skip_local = std::env::var("SWCH_SKIP_PROCESS_TERMINATION")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    let mut live_auth = None;
    if !skip_local {
        if let Ok(client) = riot::local::LocalClient::discover(riot::local::riot_paths()) {
            // Always request remembered login when saving, but do not confuse
            // lifecycle configuration with a reusable authentication session.
            let _ = client.enable_remember_login().await;
            if let Ok(auth) = client.get("/rso-auth/v1/session").await {
                // This legacy endpoint can report persistLogin=false while
                // the modern SDK has persisted a reusable refresh token.
                // Use it for diagnostics only if no usable disk session exists.
                live_auth = Some(auth);
            }
        }
    }
    let mut current = mgr.read_current_account();
    // Riot can write its cookie jar shortly after accepting the setting.
    if !current.has_session && live_auth.as_ref().is_some_and(|auth| auth["persistLogin"] == true) {
        for _ in 0..4 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            current = mgr.read_current_account();
            if current.has_session { break; }
        }
    }
    if !current.has_session {
        if let Some((code, message)) = live_auth.as_ref().and_then(live_save_error) {
            return Err(error(StatusCode::BAD_REQUEST, code, message));
        }
        return Err(error(
            StatusCode::BAD_REQUEST,
            "no_session",
            "Сессия Riot для повторного входа не найдена. Войди в аккаунт в Riot Client с включённым «Stay signed in» и повтори сохранение.",
        ));
    }
    if !skip_local {
        if let Ok(client) = riot::local::LocalClient::discover(riot::local::riot_paths()) {
            if let Ok(tokens) = client.get("/entitlements/v1/token").await {
                if let (Some(live), Some(saved)) = (tokens["subject"].as_str(), current.account_id.as_deref()) {
                    if !live.is_empty() && live != saved {
                        return Err(error(StatusCode::CONFLICT, "account_mismatch",
                            "Сохранённые данные Riot относятся к другому аккаунту. Дождись завершения входа и повтори сохранение."));
                    }
                }
            }
        }
    }
    if let (Some(requested), Some(actual)) = (&request.account_id, &current.account_id) {
        if !requested
            .strip_prefix("riot_")
            .unwrap_or(requested)
            .eq_ignore_ascii_case(actual)
        {
            return Err(error(StatusCode::CONFLICT, "account_mismatch",
                "В Riot Client открыт другой аккаунт. Сохрани текущий аккаунт или сначала войди в нужный."));
        }
    }
    let target_id = current
        .account_id
        .or(request.account_id)
        .or_else(|| current.username.clone())
        .ok_or_else(|| {
            error(
                StatusCode::BAD_REQUEST,
                "missing_identifier",
                "Не удалось определить идентификатор аккаунта Riot.",
            )
        })?;
    let username = request.username.or(current.username);
    let display_name = request
        .display_name
        .or(current.display_name)
        .or_else(|| username.clone());

    mgr.backup_session(&target_id, username.as_deref(), display_name.as_deref())
        .map_err(|e| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "backup_failed",
                &format!("Не удалось сохранить сессию Riot: {}", e),
            )
        })?;

    // Also persist in SQLite accounts table
    let account_record = crate::storage::Account {
        id: format!("riot_{}", target_id),
        launcher: "riot".to_string(),
        username: username.clone().unwrap_or_else(|| target_id.clone()),
        display_name: display_name.clone(),
        email: None,
        is_active: true,
        last_used: Some(chrono::Utc::now().to_rfc3339()),
    };
    db.upsert_account(&account_record).await.map_err(|_| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
            "Сессия сохранена на диске, но не удалось добавить аккаунт в список.",
        )
    })?;

    Ok(Json(json!({
        "status": "success",
        "message": format!("Сессия Riot для '{}' успешно сохранена в swch!", display_name.as_deref().unwrap_or(&target_id)),
        "account": account_record,
    })))
}

#[cfg(test)]
mod save_diagnostics_tests {
    use super::*;

    #[test]
    fn logged_in_with_remember_disabled_has_specific_guidance() {
        let (code, message) =
            live_save_error(&json!({"type":"authenticated", "persistLogin":false})).unwrap();
        assert_eq!(code, "riot_remember_disabled");
        assert!(message.contains("выключено"));
        assert!(message.contains("Выйди из аккаунта"));
        let (code, _) =
            live_save_error(&json!({"type":"authenticated", "persistLogin":true})).unwrap();
        assert_eq!(code, "riot_session_not_written");
        assert!(live_save_error(
            &json!({"type":"unknown_authentication_response", "persistLogin":false})
        )
        .is_none());
        assert!(live_save_error(&json!({"type":"authenticated"})).is_none());
    }
}

pub async fn current_session() -> Json<Value> {
    let mgr = riot::session::RiotSessionManager::new();
    let current = mgr.read_current_account();
    let is_running = mgr.is_launcher_running();
    let has_saved = if let Some(ref id) = current.account_id {
        mgr.has_session(id)
    } else {
        false
    };
    Json(json!({
        "current_account": current,
        "is_running": is_running,
        "is_session_saved": has_saved,
    }))
}

pub async fn list_sessions() -> Json<Value> {
    let mgr = riot::session::RiotSessionManager::new();
    Json(json!({
        "sessions": mgr.list_sessions(),
    }))
}

/// Keep the saved copy of the active login in step with Riot's own token
/// rotation. This neither signs in dormant accounts nor extends MFA trust.
pub fn start_session_refresh_loop() {
    tokio::spawn(async {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let Ok(_guard) = super::handlers::SWITCH_API.try_lock() else { continue; };
            let mgr = riot::session::RiotSessionManager::new();
            if let Err(e) = mgr.refresh_saved_session() {
                tracing::warn!("Could not refresh saved Riot session: {}", e);
            }
        }
    });
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveAccountRequest {
    pub account_id: String,
}

pub async fn remove_account(
    axum::extract::State(db): axum::extract::State<crate::storage::Database>,
    Json(request): Json<RemoveAccountRequest>,
) -> Result<Json<Value>, Error> {
    let id = request.account_id.trim();
    let valid = id.strip_prefix("riot_").is_some_and(|raw| {
        !raw.is_empty() && raw != "." && raw != ".." && !raw.contains(['/', '\\'])
    });
    if !valid {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            "Укажи аккаунт Riot для удаления.",
        ));
    }
    let _guard = super::handlers::SWITCH_API.try_lock().map_err(|_| {
        error(
            StatusCode::CONFLICT,
            "switch_busy",
            "Дождись завершения переключения аккаунта.",
        )
    })?;
    let _sync = PROFILE_SYNC.try_lock().map_err(|_| {
        error(
            StatusCode::CONFLICT,
            "sync_busy",
            "Дождись завершения синхронизации Riot.",
        )
    })?;
    riot::session::RiotSessionManager::new()
        .remove_session(id)
        .map_err(|e| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "remove_failed",
                &format!("Не удалось удалить сессию Riot: {e}"),
            )
        })?;
    db.delete_riot_account(id).await.map_err(|_| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
            "Не удалось удалить профиль Riot из списка.",
        )
    })?;
    Ok(Json(
        json!({"status":"success", "account_id":id, "message":"Аккаунт Riot удалён из SWCH."}),
    ))
}

pub async fn profiles(
    axum::extract::State(db): axum::extract::State<crate::storage::Database>,
) -> Result<Json<Value>, Error> {
    let mut profiles = db.riot_profiles().await.unwrap_or_default();

    let mgr = riot::session::RiotSessionManager::new();
    profiles.retain(|profile| !mgr.is_removed(&profile.account_id));
    let current = mgr.read_current_account();
    let sessions = mgr.list_sessions();
    let db_accounts = db.get_accounts().await.unwrap_or_default();

    let mut unified_accounts: Vec<crate::storage::Account> = Vec::new();

    // 1. From SQLite accounts table
    for a in db_accounts.into_iter().filter(|a| a.launcher == "riot") {
        if mgr.is_removed(&a.id) {
            continue;
        }
        let is_active = if let Some(ref cur_id) = current.account_id {
            a.id.eq_ignore_ascii_case(&format!("riot_{}", cur_id))
                || a.username.eq_ignore_ascii_case(cur_id)
        } else {
            false
        };
        let dname = a.display_name.clone().or_else(|| Some(a.username.clone()));
        let uname = if a.username.is_empty() || a.username.contains('-') {
            dname
                .as_ref()
                .map(|d| d.split('#').next().unwrap_or(d).to_string())
                .unwrap_or(a.username)
        } else {
            a.username
        };
        unified_accounts.push(crate::storage::Account {
            username: uname,
            display_name: dname,
            is_active,
            ..a
        });
    }

    // 2. From saved session snapshots on disk
    for s in sessions {
        let id = format!("riot_{}", s.account_id);
        if !unified_accounts.iter().any(|a| {
            a.id.eq_ignore_ascii_case(&id) || a.username.eq_ignore_ascii_case(&s.account_id)
        }) {
            let is_active = current.account_id.as_deref() == Some(&s.account_id);
            let uname = s
                .username
                .or_else(|| {
                    s.display_name
                        .as_ref()
                        .map(|d| d.split('#').next().unwrap_or(d).to_string())
                })
                .unwrap_or_else(|| s.account_id.clone());
            let dname = s.display_name.clone().or_else(|| Some(uname.clone()));
            unified_accounts.push(crate::storage::Account {
                id,
                launcher: "riot".to_string(),
                username: uname,
                display_name: dname,
                email: None,
                is_active,
                last_used: Some(s.backed_up_at),
            });
        }
    }

    // 3. Current active account if not already in list
    if current.has_session
        && !current
            .account_id
            .as_deref()
            .is_some_and(|id| mgr.is_removed(id))
    {
        if let Some(ref cur_id) = current.account_id {
            let id = format!("riot_{}", cur_id);
            if !unified_accounts
                .iter()
                .any(|a| a.id.eq_ignore_ascii_case(&id) || a.username.eq_ignore_ascii_case(cur_id))
            {
                let uname = current
                    .username
                    .or_else(|| {
                        current
                            .display_name
                            .as_ref()
                            .map(|d| d.split('#').next().unwrap_or(d).to_string())
                    })
                    .unwrap_or_else(|| cur_id.clone());
                let dname = current.display_name.clone().or_else(|| Some(uname.clone()));
                unified_accounts.push(crate::storage::Account {
                    id,
                    launcher: "riot".to_string(),
                    username: uname,
                    display_name: dname,
                    email: None,
                    is_active: true,
                    last_used: Some(chrono::Utc::now().to_rfc3339()),
                });
            }
        }
    }

    // SQLite and session snapshots retain names from the time of connection.
    // A synced Riot profile is a newer identity source for all of its games.
    for account in &mut unified_accounts {
        if let Some(name) = riot::profile::latest_display_name(&profiles, &account.id).map(str::to_owned) {
            account.display_name = Some(name.clone());
            for profile in profiles.iter_mut().filter(|p| p.account_id.eq_ignore_ascii_case(&account.id)) {
                profile.display_name = name.clone();
            }
        }
    }
    if let Ok(client) = riot::local::LocalClient::discover(riot::local::riot_paths()) {
        if let Ok(Ok((id, name))) = tokio::time::timeout(
            std::time::Duration::from_secs(3), client.current_identity()
        ).await {
            if let Some(account) = unified_accounts.iter_mut().find(|a| a.id.eq_ignore_ascii_case(&id)) {
                if account.display_name.as_deref() != Some(&name) || profiles.iter().any(|p| p.account_id.eq_ignore_ascii_case(&id) && p.display_name != name) {
                    let _ = db.update_riot_display_name(&account.id, &name).await;
                }
                account.display_name = Some(name.clone());
                for profile in profiles.iter_mut().filter(|p| p.account_id.eq_ignore_ascii_case(&id)) {
                    profile.display_name = name.clone();
                }
            }
        }
    }

    Ok(Json(json!({
        "profiles": profiles,
        "accounts": unified_accounts,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncRequest {
    pub app_id: String,
    pub account_id: Option<String>,
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
        "league_of_legends" => riot::local::sync_league(request.account_id.as_deref()).await,
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
    if request.account_id.as_deref().is_some_and(|id| id != profile.account_id) {
        return Err(error(StatusCode::CONFLICT, "account_mismatch",
            "В клиенте игры открыт другой аккаунт. Войди в выбранный аккаунт и повтори обновление."));
    }
    db.save_riot_profile(profile).await.map_err(|_| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
            "Не удалось сохранить профиль Riot.",
        )
    })?;
    profiles(axum::extract::State(db)).await
}
