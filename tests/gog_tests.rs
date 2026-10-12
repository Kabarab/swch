use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, Request, StatusCode},
    routing::{get, post},
    Extension, Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use swch::launchers::gog::galaxy::{GalaxyClient, LoginState};
use swch::{
    launchers::gog::{
        self,
        auth::{authorization_code, CredentialStore, Credentials, Endpoints, GogStore},
        local, GogError,
    },
    storage::{Account, Database},
};
use tower::ServiceExt;

struct NativeState {
    login: LoginState,
    running: bool,
    closes: usize,
    writes: usize,
    backups: usize,
    fail_confirmation: bool,
    game_running: bool,
}
struct MockGalaxy(Mutex<NativeState>);
impl MockGalaxy {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(NativeState {
            login: LoginState {
                user_id: Some("1".into()),
                username: Some("Previous".into()),
                refresh_token: Some("refresh-1-native".into()),
            },
            running: true,
            closes: 0,
            writes: 0,
            backups: 0,
            fail_confirmation: false,
            game_running: false,
        })))
    }
}
impl GalaxyClient for MockGalaxy {
    fn available(&self) -> bool {
        true
    }
    fn state(&self) -> Result<LoginState, GogError> {
        Ok(self.0.lock().unwrap().login.clone())
    }
    fn running(&self) -> bool {
        self.0.lock().unwrap().running
    }
    fn close(&self) -> Result<(), GogError> {
        let mut s = self.0.lock().unwrap();
        if s.game_running {
            return Err(GogError::Invalid("Заверши игру".into()));
        }
        s.closes += 1;
        s.running = false;
        Ok(())
    }
    fn write(&self, login: &LoginState) -> Result<(), GogError> {
        let mut s = self.0.lock().unwrap();
        assert!(!s.running);
        s.writes += 1;
        s.login = login.clone();
        Ok(())
    }
    fn open(&self) -> Result<(), GogError> {
        let mut s = self.0.lock().unwrap();
        s.running = true;
        let id = s.login.user_id.clone().unwrap_or_else(|| {
            s.login
                .refresh_token
                .as_ref()
                .unwrap()
                .split('-')
                .nth(1)
                .unwrap()
                .into()
        });
        let id = if s.fail_confirmation {
            s.fail_confirmation = false;
            "3".to_string()
        } else {
            id
        };
        s.login.user_id = Some(id.clone());
        s.login.refresh_token = Some(format!("refresh-{id}-native"));
        Ok(())
    }
    fn backup(&self, _: &LoginState) -> Result<(), GogError> {
        self.0.lock().unwrap().backups += 1;
        Ok(())
    }
    fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(20)
    }
}

#[derive(Default)]
struct MemoryCredentials(Mutex<HashMap<String, Credentials>>);
impl CredentialStore for MemoryCredentials {
    fn get(&self, id: &str) -> Result<Option<Credentials>, GogError> {
        Ok(self.0.lock().unwrap().get(id).cloned())
    }
    fn set(&self, id: &str, value: &Credentials) -> Result<(), GogError> {
        self.0.lock().unwrap().insert(id.into(), value.clone());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), GogError> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}
#[derive(Default)]
struct MockState {
    calls: Vec<String>,
    fail_second: bool,
    empty: bool,
    repeat_page: bool,
    fail_auth: bool,
    wrong_refresh_user: bool,
    reject_initial: bool,
    fail_metadata: bool,
}
type Mock = Arc<Mutex<MockState>>;
struct Server {
    url: String,
    state: Mock,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn new() -> Self {
        async fn token(
            State(state): State<Mock>,
            Query(q): Query<HashMap<String, String>>,
        ) -> (StatusCode, Json<Value>) {
            let mut state = state.lock().unwrap();
            state.calls.push(q["grant_type"].clone());
            if state.fail_auth {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"request includes private-code"})),
                );
            }
            let user = if q["grant_type"] == "refresh_token" && state.wrong_refresh_user {
                "2"
            } else if q.get("code").is_some_and(|s| s == "second")
                || q.get("refresh_token")
                    .is_some_and(|s| s.starts_with("refresh-2-"))
            {
                "2"
            } else {
                "1"
            };
            let suffix = if q["grant_type"] == "refresh_token" {
                "rotated"
            } else {
                "initial"
            };
            (
                StatusCode::OK,
                Json(
                    json!({"user_id":user,"access_token":format!("access-{user}-{suffix}"),"refresh_token":format!("refresh-{user}-{suffix}"),"expires_in":3600}),
                ),
            )
        }
        async fn user(Path(id): Path<String>, headers: HeaderMap) -> Json<Value> {
            assert!(headers["authorization"]
                .to_str()
                .unwrap()
                .starts_with("Bearer access-"));
            Json(json!({"id":id,"username":format!("Player {id}")}))
        }
        async fn releases(
            State(state): State<Mock>,
            Path(id): Path<String>,
            Query(q): Query<HashMap<String, String>>,
            headers: HeaderMap,
        ) -> (StatusCode, Json<Value>) {
            assert!(headers["authorization"]
                .to_str()
                .unwrap()
                .starts_with("Bearer access-"));
            let mut state = state.lock().unwrap();
            if state.reject_initial
                && headers["authorization"]
                    .to_str()
                    .unwrap()
                    .ends_with("-initial")
            {
                return (StatusCode::UNAUTHORIZED, Json(json!({"error":"expired"})));
            }
            state.calls.push(format!(
                "library/{id}/{}",
                q.get("page_token").map(String::as_str).unwrap_or("first")
            ));
            if state.empty {
                return (StatusCode::OK, Json(json!({"items":[]})));
            }
            if q.contains_key("page_token") {
                if state.fail_second {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":"offline"})),
                    );
                }
                return (
                    StatusCode::OK,
                    Json(
                        json!({"items":[{"platform_id":"gog","external_id":"102"}],"next_page_token":if state.repeat_page {Some("second")}else{None}}),
                    ),
                );
            }
            (
                StatusCode::OK,
                Json(
                    json!({"items":[{"platform_id":"gog","external_id":101},{"platform_id":"gog","external_id":"101"},{"platform_id":"steam","external_id":"730"}],"next_page_token":"second"}),
                ),
            )
        }
        async fn product(
            State(state): State<Mock>,
            Path(id): Path<String>,
        ) -> (StatusCode, Json<Value>) {
            if state.lock().unwrap().fail_metadata {
                return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({})));
            }
            (
                StatusCode::OK,
                Json(
                    json!({"id":id.parse::<u64>().unwrap(),"title":format!("Game {id}"),"game_type":"game","images":{"background":"//images.gog.com/test.jpg"}}),
                ),
            )
        }
        let state = Mock::default();
        let app = Router::new()
            .route("/token", get(token))
            .route("/users/:id", get(user))
            .route("/users/:id/releases", get(releases))
            .route("/products/:id", get(product))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { url, state, task }
    }
    fn store(&self, credentials: Arc<MemoryCredentials>) -> Arc<GogStore> {
        Arc::new(GogStore::new(
            Endpoints {
                auth: self.url.clone(),
                users: self.url.clone(),
                library: self.url.clone(),
                products: self.url.clone(),
            },
            credentials,
        ))
    }
}
async fn database() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    db
}
fn account(id: &str) -> Account {
    Account {
        id: format!("gog_{id}"),
        launcher: "gog".into(),
        username: id.into(),
        display_name: None,
        email: None,
        is_active: false,
        last_used: None,
    }
}

#[test]
fn redirect_and_raw_code_validation() {
    assert_eq!(authorization_code(" abc-12_~. ").unwrap(), "abc-12_~.");
    assert_eq!(
        authorization_code("https://embed.gog.com/on_login_success?origin=client&code=abc%2D12")
            .unwrap(),
        "abc-12"
    );
    for bad in [
        "",
        "https://attacker.test/on_login_success?code=a",
        "https://embed.gog.com@attacker.test/on_login_success?code=a",
        "http://embed.gog.com/on_login_success?code=a",
        "https://embed.gog.com/other?code=a",
        "https://embed.gog.com/on_login_success",
        "a&refresh_token=secret",
        "a\nsecret",
    ] {
        assert!(authorization_code(bad).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn login_pagination_refresh_and_restart_keep_account_ownership() {
    let server = Server::new().await;
    let db = database().await;
    let credentials = Arc::new(MemoryCredentials::default());
    let store = server.store(credentials.clone());
    let a = store.login(&db, "first").await.unwrap();
    assert_eq!(a.id, "gog_1");
    assert!(a.is_active);
    assert_eq!(store.sync(&db, &a.id).await.unwrap().0, 2);
    let b = store.login(&db, "second").await.unwrap();
    store.sync(&db, &b.id).await.unwrap();
    assert_eq!(db.get_games().await.unwrap().len(), 2);
    assert_eq!(db.get_game_accounts().await.unwrap().len(), 4);
    assert!(
        db.get_accounts()
            .await
            .unwrap()
            .iter()
            .find(|a| a.id == "gog_2")
            .unwrap()
            .is_active
    );
    let mut expired = credentials.get("gog_1").unwrap().unwrap();
    expired.expires_at = 0;
    credentials.set("gog_1", &expired).unwrap();
    drop(store);
    let restarted = server.store(credentials.clone());
    restarted.sync(&db, "gog_1").await.unwrap();
    assert_eq!(
        credentials.get("gog_1").unwrap().unwrap().refresh_token,
        "refresh-1-rotated"
    );
    assert_eq!(
        credentials.get("gog_2").unwrap().unwrap().refresh_token,
        "refresh-2-initial"
    );
    let payload = serde_json::to_string(&db.get_accounts().await.unwrap()).unwrap();
    assert!(!payload.contains("refresh-"));
    assert!(!payload.contains("access-"));
    server.state.lock().unwrap().empty = true;
    restarted.sync(&db, "gog_1").await.unwrap();
    assert_eq!(db.get_game_accounts().await.unwrap().len(), 2);
    assert_eq!(db.get_games().await.unwrap().len(), 2);
    restarted.remove(&db, "gog_2").await.unwrap();
    assert!(credentials.get("gog_2").unwrap().is_none());
    assert!(db.get_games().await.unwrap().is_empty());
}

#[tokio::test]
async fn failed_or_repeating_page_keeps_cached_library() {
    let server = Server::new().await;
    let db = database().await;
    let store = server.store(Arc::new(MemoryCredentials::default()));
    store.login(&db, "first").await.unwrap();
    store.sync(&db, "gog_1").await.unwrap();
    let old = db.get_game_accounts().await.unwrap();
    server.state.lock().unwrap().fail_second = true;
    assert!(store.sync(&db, "gog_1").await.is_err());
    assert_eq!(db.get_game_accounts().await.unwrap(), old);
    {
        let mut state = server.state.lock().unwrap();
        state.fail_second = false;
        state.repeat_page = true;
    }
    assert!(store.sync(&db, "gog_1").await.is_err());
    assert_eq!(db.get_game_accounts().await.unwrap(), old);
}

#[tokio::test]
async fn refreshed_tokens_cannot_change_account_identity() {
    let server = Server::new().await;
    let db = database().await;
    let credentials = Arc::new(MemoryCredentials::default());
    let store = server.store(credentials.clone());
    store.login(&db, "first").await.unwrap();
    let mut token = credentials.get("gog_1").unwrap().unwrap();
    token.expires_at = 0;
    credentials.set("gog_1", &token).unwrap();
    server.state.lock().unwrap().wrong_refresh_user = true;
    assert!(matches!(
        store.sync(&db, "gog_1").await,
        Err(GogError::Unauthorized)
    ));
    assert_eq!(credentials.get("gog_1").unwrap().unwrap().user_id, "1");
}

#[tokio::test]
async fn database_updates_rollback_and_never_affect_other_launchers() {
    let db = database().await;
    let a = account("1");
    db.upsert_account(&a).await.unwrap();
    let mut steam = account("2");
    steam.id = "steam_account".into();
    steam.launcher = "steam".into();
    steam.is_active = true;
    db.upsert_account(&steam).await.unwrap();
    db.set_active_gog_account("gog_1").await.unwrap();
    assert!(db.set_active_gog_account("missing").await.is_err());
    let game = gog::game("101", "Game".into(), None);
    db.replace_gog_library("gog_1", std::slice::from_ref(&game))
        .await
        .unwrap();
    assert!(db
        .replace_gog_library("gog_1", &[game.clone(), game])
        .await
        .is_err());
    assert_eq!(db.get_game_accounts().await.unwrap().len(), 1);
    assert!(
        db.get_accounts()
            .await
            .unwrap()
            .iter()
            .find(|a| a.id == "steam_account")
            .unwrap()
            .is_active
    );
    assert!(
        db.get_accounts()
            .await
            .unwrap()
            .iter()
            .find(|a| a.id == "gog_1")
            .unwrap()
            .is_active
    );
}

#[test]
fn local_manifest_scan_deduplicates_and_excludes_dlc_and_corruption() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Game with spaces");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("goggame-101.info"),json!({"gameId":"101","rootGameId":"101","name":"Local Game","playTasks":[{"isPrimary":true}]}).to_string()).unwrap();
    std::fs::write(
        dir.join("goggame-102.info"),
        json!({"gameId":"102","rootGameId":"101","name":"DLC","playTasks":[{"isPrimary":true}]})
            .to_string(),
    )
    .unwrap();
    std::fs::write(dir.join("goggame-103.info"), "corrupt").unwrap();
    let games = local::games_from_dirs(&[tmp.path().into(), tmp.path().into()]);
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].name, "Local Game");
    let args = local::launch_arguments(&games[0]).unwrap();
    assert_eq!(args[2], "/gameId=101");
    assert_eq!(args[3], format!("/path={}", dir.display()));
    let mut bad = games[0].clone();
    bad.app_id = "101 /command=other".into();
    assert!(local::launch_arguments(&bad).is_err());
    let registry=format!("HKEY_LOCAL_MACHINE\\SOFTWARE\\GOG.com\\Games\\101\n    path    REG_SZ    {}\n    gameName    REG_SZ    Registry Title\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\999_is1\n    InstallLocation    REG_SZ    {}\n    Publisher    REG_SZ    Other Publisher",dir.display(),dir.display());
    assert_eq!(local::games_from_registry(&registry).len(), 1);
}

#[tokio::test]
async fn api_login_errors_hide_codes_and_tokens() {
    let server = Server::new().await;
    server.state.lock().unwrap().fail_auth = true;
    let db = database().await;
    let app = Router::new()
        .route("/login", post(swch::server::gog::login))
        .layer(Extension(
            server.store(Arc::new(MemoryCredentials::default())),
        ))
        .with_state(db);
    let response = app
        .oneshot(
            Request::post("/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"code":"private-code"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(!body.contains("private-code"));
    assert!(!body.contains("client_secret"));
    assert!(!body.contains(&server.url));
}

#[tokio::test]
async fn api_switch_missing_account_does_not_clear_active_selection() {
    let db = database().await;
    db.upsert_account(&account("1")).await.unwrap();
    db.set_active_gog_account("gog_1").await.unwrap();
    let app = swch::server::create_app(db.clone());
    let response = app
        .oneshot(
            Request::post("/api/gog/switch-account")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_id":"missing"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(db.get_accounts().await.unwrap()[0].is_active);
}

#[tokio::test]
async fn online_module_remains_active_without_galaxy_client() {
    let tmp = tempfile::tempdir().unwrap();
    let previous = std::env::var_os("SWCH_MODULES_DIR");
    std::env::set_var("SWCH_MODULES_DIR", tmp.path().join("modules"));
    let db = database().await;
    db.upsert_account(&account("1")).await.unwrap();
    let manager = swch::modules::ModuleManager::new(db.clone());
    manager.set_mock_launchers(vec![]).await;
    manager.init().await.unwrap();
    assert!(manager.is_module_active(swch::modules::ModuleId::Gog).await);
    manager.scan_and_sync().await.unwrap();
    assert!(manager.is_module_active(swch::modules::ModuleId::Gog).await);
    db.delete_gog_account("gog_1").await.unwrap();
    manager.scan_and_sync().await.unwrap();
    assert!(!manager.is_module_active(swch::modules::ModuleId::Gog).await);
    match previous {
        Some(value) => std::env::set_var("SWCH_MODULES_DIR", value),
        None => std::env::remove_var("SWCH_MODULES_DIR"),
    }
}

#[tokio::test]
async fn unauthorized_library_refreshes_once_and_rotates_the_correct_account() {
    let server = Server::new().await;
    let db = database().await;
    let credentials = Arc::new(MemoryCredentials::default());
    let store = server.store(credentials.clone());
    store.login(&db, "second").await.unwrap();
    server.state.lock().unwrap().reject_initial = true;
    assert_eq!(store.sync(&db, "gog_2").await.unwrap().0, 2);
    assert_eq!(
        credentials.get("gog_2").unwrap().unwrap().refresh_token,
        "refresh-2-rotated"
    );
    assert!(credentials.get("gog_1").unwrap().is_none());
    assert_eq!(
        server
            .state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|s| *s == "refresh_token")
            .count(),
        1
    );
}

#[tokio::test]
async fn metadata_outage_preserves_titles_and_ownership_with_warnings() {
    let server = Server::new().await;
    let db = database().await;
    let store = server.store(Arc::new(MemoryCredentials::default()));
    store.login(&db, "first").await.unwrap();
    store.sync(&db, "gog_1").await.unwrap();
    let old = db.get_games().await.unwrap();
    server.state.lock().unwrap().fail_metadata = true;
    let result = store.sync(&db, "gog_1").await.unwrap();
    assert_eq!(result.0, 2);
    assert_eq!(result.1.len(), 2);
    assert_eq!(db.get_games().await.unwrap(), old);
    assert_eq!(db.get_game_accounts().await.unwrap().len(), 2);
}

#[tokio::test]
async fn native_switch_closes_client_passes_token_and_waits_for_actual_login() {
    let server = Server::new().await;
    let db = database().await;
    let creds = Arc::new(MemoryCredentials::default());
    let native = MockGalaxy::new();
    let store = GogStore::new(
        Endpoints {
            auth: server.url.clone(),
            users: server.url.clone(),
            library: server.url.clone(),
            products: server.url.clone(),
        },
        creds.clone(),
    )
    .with_galaxy(native.clone());
    store.login(&db, "second").await.unwrap();
    db.set_active_gog_account("gog_2").await.unwrap();
    assert_eq!(store.launcher_account_id().as_deref(), Some("gog_1"));
    store.activate_launcher(&db, "gog_2").await.unwrap();
    assert_eq!(store.launcher_account_id().as_deref(), Some("gog_2"));
    assert_eq!(native.0.lock().unwrap().closes, 1);
    assert_eq!(native.0.lock().unwrap().backups, 1);
    assert_eq!(
        creds.get("gog_2").unwrap().unwrap().refresh_token,
        "refresh-2-native"
    );
    store.activate_launcher(&db, "gog_2").await.unwrap();
    assert_eq!(native.0.lock().unwrap().closes, 1);
}

#[tokio::test]
async fn unconfirmed_native_switch_restores_previous_login_and_database_selection() {
    let server = Server::new().await;
    let db = database().await;
    let native = MockGalaxy::new();
    let store = GogStore::new(
        Endpoints {
            auth: server.url.clone(),
            users: server.url.clone(),
            library: server.url.clone(),
            products: server.url.clone(),
        },
        Arc::new(MemoryCredentials::default()),
    )
    .with_galaxy(native.clone());
    store.login(&db, "second").await.unwrap();
    db.upsert_account(&account("1")).await.unwrap();
    db.set_active_gog_account("gog_1").await.unwrap();
    native.0.lock().unwrap().fail_confirmation = true;
    assert!(store.activate_launcher(&db, "gog_2").await.is_err());
    assert_eq!(store.launcher_account_id().as_deref(), Some("gog_1"));
    assert!(native.running());
    assert!(
        db.get_accounts()
            .await
            .unwrap()
            .iter()
            .find(|a| a.id == "gog_1")
            .unwrap()
            .is_active
    );
}

#[tokio::test]
async fn bad_credentials_and_running_game_leave_native_login_untouched() {
    let server = Server::new().await;
    let db = database().await;
    let native = MockGalaxy::new();
    let store = GogStore::new(
        Endpoints {
            auth: server.url.clone(),
            users: server.url.clone(),
            library: server.url.clone(),
            products: server.url.clone(),
        },
        Arc::new(MemoryCredentials::default()),
    )
    .with_galaxy(native.clone());
    store.login(&db, "second").await.unwrap();
    server.state.lock().unwrap().fail_auth = true;
    assert!(store.activate_launcher(&db, "gog_2").await.is_err());
    assert_eq!(native.0.lock().unwrap().closes, 0);
    server.state.lock().unwrap().fail_auth = false;
    native.0.lock().unwrap().game_running = true;
    assert!(store.activate_launcher(&db, "gog_2").await.is_err());
    assert_eq!(native.0.lock().unwrap().writes, 0);
    assert_eq!(store.launcher_account_id().as_deref(), Some("gog_1"));
}
