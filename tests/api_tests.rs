use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use swch::server::create_app;
use swch::storage::{Account, Database, Game};

#[tokio::test]
async fn test_health_endpoint() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let app = create_app(db);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert_eq!(json["database"], "connected");
    assert!(json["version"].is_string());
}

#[tokio::test]
async fn test_games_endpoint() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let game = Game {
        id: "game_portal2".to_string(),
        launcher: "steam".to_string(),
        app_id: "620".to_string(),
        name: "Portal 2".to_string(),
        install_path: Some("/steam/portal2".to_string()),
        executable_path: Some("/steam/portal2/portal2.exe".to_string()),
        size_bytes: 12_000_000_000,
        is_installed: true,
        banner_url: Some("https://example.com/portal2.jpg".to_string()),
        launch_uri: Some("steam://run/620".to_string()),
        namespace: None,
    };
    db.upsert_game(&game).await.unwrap();

    let app = create_app(db);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/games")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let games: Vec<Game> = serde_json::from_slice(&body).unwrap();

    assert_eq!(games.len(), 1);
    assert_eq!(games[0].name, "Portal 2");
    assert_eq!(games[0].app_id, "620");
    assert!(games[0].is_installed);
}

#[tokio::test]
async fn test_accounts_endpoint() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let account = Account {
        id: "acc_epic_main".to_string(),
        launcher: "epic".to_string(),
        username: "pro_gamer".to_string(),
        display_name: Some("ProGamer2026".to_string()),
        email: None,
        is_active: true,
        last_used: Some("2026-09-13T09:30:00Z".to_string()),
    };
    db.upsert_account(&account).await.unwrap();

    let app = create_app(db);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let accounts: Vec<Account> = serde_json::from_slice(&body).unwrap();

    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].username, "pro_gamer");
    assert_eq!(accounts[0].launcher, "epic");
    assert!(accounts[0].is_active);
}

#[tokio::test]
async fn test_root_serves_index_html() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let app = create_app(db);

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html_content = String::from_utf8_lossy(&body);

    assert!(html_content.contains("swch"));
    assert!(html_content.contains("<!DOCTYPE html>"));
}

#[tokio::test]
async fn test_switch_steam_account_endpoint_empty_name() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let app = create_app(db);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/steam/switch-account")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_name": "  "}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(json["error"]
        .as_str()
        .unwrap()
        .contains("must not be empty"));
}

static TEST_ENV_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn test_switch_steam_account_endpoint_flow() {
    let _env_guard = TEST_ENV_MUTEX.lock().await;
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let steam_dir = dir.path().to_path_buf();
    let config_dir = steam_dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let loginusers = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"acc_one"
		"PersonaName"		"Player 1"
		"RememberPassword"		"1"
		"MostRecent"		"1"
	}
	"76561198000000002"
	{
		"AccountName"		"acc_two"
		"PersonaName"		"Player 2"
		"RememberPassword"		"1"
		"MostRecent"		"0"
	}
}
"#;
    fs::write(config_dir.join("loginusers.vdf"), loginusers).unwrap();

    std::env::set_var("SWCH_STEAM_DIR", &steam_dir);
    std::env::set_var("SWCH_SKIP_PROCESS_TERMINATION", "1");

    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let app = create_app(db);

    // Switch to acc_two
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/steam/switch-account")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_name": "acc_two"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["account_name"], "acc_two");
    assert_eq!(json["previous_account"], "acc_one");
    assert_eq!(json["steam_id"], "76561198000000002");

    // Clean up env
    std::env::remove_var("SWCH_STEAM_DIR");
    std::env::remove_var("SWCH_SKIP_PROCESS_TERMINATION");
}

#[tokio::test]
async fn test_switch_steam_account_endpoint_with_open_url() {
    let _env_guard = TEST_ENV_MUTEX.lock().await;
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let steam_dir = dir.path().to_path_buf();
    let config_dir = steam_dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let loginusers = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"acc_one"
		"PersonaName"		"Player 1"
		"RememberPassword"		"1"
		"MostRecent"		"1"
	}
	"76561198000000002"
	{
		"AccountName"		"acc_two"
		"PersonaName"		"Player 2"
		"RememberPassword"		"1"
		"MostRecent"		"0"
	}
}
"#;
    fs::write(config_dir.join("loginusers.vdf"), loginusers).unwrap();

    std::env::set_var("SWCH_STEAM_DIR", &steam_dir);
    std::env::set_var("SWCH_SKIP_PROCESS_TERMINATION", "1");

    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let app = create_app(db.clone());

    // Switch to acc_two with open_url for an inventory item
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/steam/switch-account")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"account_id": "76561198000000002", "open_url": "https://steamcommunity.com/profiles/76561198000000002/inventory/#730_2_123456789"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["account_name"], "acc_two");
    assert_eq!(json["steam_id"], "76561198000000002");
    assert!(json["message"].as_str().unwrap().contains("открываем страницу"));

    // An unsupported destination must fail before changing the active Steam account.
    let loginusers_before = fs::read_to_string(config_dir.join("loginusers.vdf")).unwrap();
    let invalid = create_app(db)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/steam/switch-account")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"account_id":"76561198000000001","open_url":"https://example.com/dev/apikey"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let loginusers_after = fs::read_to_string(config_dir.join("loginusers.vdf")).unwrap();
    assert_eq!(loginusers_after, loginusers_before);

    // Clean up env
    std::env::remove_var("SWCH_STEAM_DIR");
    std::env::remove_var("SWCH_SKIP_PROCESS_TERMINATION");
}

#[tokio::test]
async fn test_switch_steam_account_endpoint_by_steam_id_and_not_found() {
    let _env_guard = TEST_ENV_MUTEX.lock().await;
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let steam_dir = dir.path().to_path_buf();
    let config_dir = steam_dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let loginusers = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"acc_one"
		"PersonaName"		"Player 1"
		"RememberPassword"		"1"
		"MostRecent"		"1"
	}
	"76561198000000002"
	{
		"AccountName"		"acc_two"
		"PersonaName"		"Player 2"
		"RememberPassword"		"1"
		"MostRecent"		"0"
	}
}
"#;
    fs::write(config_dir.join("loginusers.vdf"), loginusers).unwrap();

    std::env::set_var("SWCH_STEAM_DIR", &steam_dir);
    std::env::set_var("SWCH_SKIP_PROCESS_TERMINATION", "1");

    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let app = create_app(db.clone());

    // Switch by SteamID64
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/steam/switch-account")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_name": "76561198000000002"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["account_name"], "acc_two");

    // Try switching to unknown user
    let response_not_found = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/steam/switch-account")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_name": "unknown_player"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response_not_found.status(), StatusCode::NOT_FOUND);

    std::env::remove_var("SWCH_STEAM_DIR");
    std::env::remove_var("SWCH_SKIP_PROCESS_TERMINATION");
}

#[tokio::test]
async fn steam_launch_dry_run_switches_only_on_dispatch_and_rejects_non_owner() {
    let _guard = TEST_ENV_MUTEX.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::create_dir_all(root.join("steamapps")).unwrap();
    std::fs::write(root.join("config/loginusers.vdf"),r#""users" { "76561197960265729" { "AccountName" "one" "MostRecent" "1" } "76561197960265730" { "AccountName" "two" "MostRecent" "0" } }"#).unwrap();
    std::fs::write(
        root.join("steamapps/appmanifest_730.acf"),
        r#""AppState" {"StateFlags" "4" "installdir" "cs"}"#,
    )
    .unwrap();
    std::env::set_var("SWCH_STEAM_DIR", root);
    std::env::set_var("SWCH_SESSIONS_DIR", root.join("sessions"));
    std::env::set_var("SWCH_SKIP_PROCESS_TERMINATION", "1");
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let mut game =
        swch::launchers::steam::library::SteamGame::new("730", Some("76561197960265730".into()));
    game.access = "owned".into();
    game.is_free = Some(false);
    db.steam_cache_put(
        "owned:76561197960265730",
        &serde_json::to_string(&vec![game]).unwrap(),
    )
    .await
    .unwrap();
    let call = |id: &str, dry: bool, update: bool| {
        Request::builder().method("POST").uri("/api/steam/launch").header("content-type","application/json").body(Body::from(serde_json::json!({"account_id":id,"app_id":"730","dry_run":dry,"update_only":update,"args":if update {vec![]} else {vec!["literal argument","$(not-a-command)"]}}).to_string())).unwrap()
    };
    let before = std::fs::read(root.join("config/loginusers.vdf")).unwrap();
    let response = create_app(db.clone())
        .oneshot(call("76561197960265730", true, false))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["switch_required"], true);
    assert_eq!(value["command"]["args"][3], "literal argument");
    assert_eq!(value["command"]["args"][4], "$(not-a-command)");
    assert_eq!(
        std::fs::read(root.join("config/loginusers.vdf")).unwrap(),
        before
    );
    let response = create_app(db.clone())
        .oneshot(call("76561197960265729", false, false))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        std::fs::read(root.join("config/loginusers.vdf")).unwrap(),
        before
    );
    let response = create_app(db.clone())
        .oneshot(call("76561197960265730", false, false))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let library =
        swch::launchers::steam::library::scan(&swch::launchers::steam::SteamPaths::new(root))
            .unwrap();
    assert!(
        library
            .accounts
            .iter()
            .find(|a| a.id == "76561197960265730")
            .unwrap()
            .is_active
    );
    let response = create_app(db.clone())
        .oneshot(call("76561197960265730", true, true))
        .await
        .unwrap();
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        value["command"]["args"],
        serde_json::json!(["-silent", "steam://install/730"])
    );
    let mut free_game =
        swch::launchers::steam::library::SteamGame::new("730", Some("76561197960265730".into()));
    free_game.access = "owned".into();
    db.steam_cache_put(
        "owned:76561197960265730",
        &serde_json::to_string(&vec![free_game]).unwrap(),
    )
    .await
    .unwrap();
    db.steam_cache_put(
        "game-details:v2:730",
        r#"{"is_free":true,"has_inventory":true}"#,
    )
    .await
    .unwrap();
    let response = create_app(db)
        .oneshot(call("76561197960265729", true, false))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Store-confirmed free game must allow an account with no local history"
    );
    for key in [
        "SWCH_STEAM_DIR",
        "SWCH_SESSIONS_DIR",
        "SWCH_SKIP_PROCESS_TERMINATION",
    ] {
        std::env::remove_var(key);
    }
}

#[tokio::test]
async fn test_open_launcher_endpoints() {
    let _env_guard = TEST_ENV_MUTEX.lock().await;
    let temp_dir = tempfile::tempdir().unwrap();
    let steam_dir = temp_dir.path().join("Steam");
    let sessions_dir = temp_dir.path().join("sessions");
    let config_dir = steam_dir.join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&sessions_dir).unwrap();

    let loginusers = r#""users"
{
    "76561198000000001"
    {
        "AccountName" "user_alpha"
        "PersonaName" "Alpha"
        "MostRecent" "1"
    }
    "76561198000000002"
    {
        "AccountName" "user_beta"
        "PersonaName" "Beta"
        "MostRecent" "0"
    }
}
"#;
    std::fs::write(config_dir.join("loginusers.vdf"), loginusers).unwrap();

    std::env::set_var("SWCH_STEAM_DIR", &steam_dir);
    std::env::set_var("SWCH_SESSIONS_DIR", &sessions_dir);
    std::env::set_var("SWCH_SKIP_PROCESS_TERMINATION", "1");

    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let app = create_app(db.clone());

    // 1. Test POST /api/steam/open-launcher with account_name
    let req = Request::builder()
        .method("POST")
        .uri("/api/steam/open-launcher")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"account_name":"user_beta"}"#))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["account_name"], "user_beta");

    // 2. Test POST /api/steam/open-launcher with account_id alias
    let req2 = Request::builder()
        .method("POST")
        .uri("/api/steam/open-launcher")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"account_id":"76561198000000001"}"#))
        .unwrap();

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body_bytes2 = resp2.into_body().collect().await.unwrap().to_bytes();
    let json2: Value = serde_json::from_slice(&body_bytes2).unwrap();
    assert_eq!(json2["status"], "success");
    assert_eq!(json2["account_name"], "user_alpha");

    let epic_dir = temp_dir.path().join("epic");
    std::fs::create_dir_all(&epic_dir).unwrap();
    let epic_kvs = epic_dir.join("KeyValueStore.ini");
    let epic_gus = epic_dir.join("GameUserSettings.ini");
    let epic_webcache = epic_dir.join("webcache");
    std::fs::create_dir_all(&epic_webcache).unwrap();
    std::env::set_var("SWCH_EPIC_KEY_VALUE_STORE", &epic_kvs);
    std::env::set_var("SWCH_EPIC_GAME_USER_SETTINGS", &epic_gus);
    std::env::set_var("SWCH_EPIC_WEBCACHE_DIR", &epic_webcache);
    std::env::set_var("SWCH_EPIC_SESSIONS_DIR", &sessions_dir);

    // 3. Test POST /api/epic/open-launcher with existing account
    let epic_acc = Account {
        id: "epic_test_1".into(),
        launcher: "epic".into(),
        username: "epictester".into(),
        display_name: Some("Epic Tester".into()),
        email: None,
        is_active: false,
        last_used: None,
    };
    db.upsert_account(&epic_acc).await.unwrap();

    let req_epic = Request::builder()
        .method("POST")
        .uri("/api/epic/open-launcher")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"account_id":"epic_test_1"}"#))
        .unwrap();

    let resp_epic = app.clone().oneshot(req_epic).await.unwrap();
    assert_eq!(resp_epic.status(), StatusCode::OK);
    let body_bytes_epic = resp_epic.into_body().collect().await.unwrap().to_bytes();
    let json_epic: Value = serde_json::from_slice(&body_bytes_epic).unwrap();
    assert_eq!(json_epic["status"], "success");
    assert!(json_epic["message"].as_str().unwrap().contains("Epic"));

    // 4. Test POST /api/epic/open-launcher with nonexistent account -> 404
    let req_epic_404 = Request::builder()
        .method("POST")
        .uri("/api/epic/open-launcher")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"account_id":"nonexistent_epic"}"#))
        .unwrap();

    let resp_epic_404 = app.clone().oneshot(req_epic_404).await.unwrap();
    assert_eq!(resp_epic_404.status(), StatusCode::NOT_FOUND);

    // Clean up
    std::env::remove_var("SWCH_STEAM_DIR");
    std::env::remove_var("SWCH_SESSIONS_DIR");
    std::env::remove_var("SWCH_SKIP_PROCESS_TERMINATION");
    std::env::remove_var("SWCH_EPIC_KEY_VALUE_STORE");
    std::env::remove_var("SWCH_EPIC_GAME_USER_SETTINGS");
    std::env::remove_var("SWCH_EPIC_WEBCACHE_DIR");
    std::env::remove_var("SWCH_EPIC_SESSIONS_DIR");
}

#[tokio::test]
async fn test_restrictions_endpoints() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let app = create_app(db.clone());

    // 1. GET /api/accounts/restrictions -> empty list
    let req_get = Request::builder()
        .method("GET")
        .uri("/api/accounts/restrictions")
        .body(Body::empty())
        .unwrap();

    let resp_get = app.clone().oneshot(req_get).await.unwrap();
    assert_eq!(resp_get.status(), StatusCode::OK);
    let body_bytes = resp_get.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["restrictions"].as_array().unwrap().len(), 0);

    // 2. POST /api/accounts/restrictions -> save restrictions
    let restrictions_payload = serde_json::json!({
        "account_id": "76561198809112858",
        "launcher": "steam",
        "community_banned": false,
        "trade_banned": true,
        "market_banned": true,
        "vac_banned": true,
        "vac_bans_count": 1,
        "game_bans_count": 0,
        "economy_ban": "banned",
        "is_limited": true,
        "game_bans": [
            {
                "app_id": "730",
                "name": "Counter-Strike 2",
                "ban_type": "VAC"
            }
        ],
        "details": "Test ban",
        "updated_at": "2026-09-17T10:00:00Z"
    });

    let req_post = Request::builder()
        .method("POST")
        .uri("/api/accounts/restrictions")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&restrictions_payload).unwrap()))
        .unwrap();

    let resp_post = app.clone().oneshot(req_post).await.unwrap();
    assert_eq!(resp_post.status(), StatusCode::OK);
    let post_body = resp_post.into_body().collect().await.unwrap().to_bytes();
    let saved_json: Value = serde_json::from_slice(&post_body).unwrap();
    assert_eq!(saved_json["account_id"], "76561198809112858");
    assert_eq!(saved_json["trade_banned"], true);
    assert_eq!(saved_json["market_banned"], true);
    assert_eq!(saved_json["vac_banned"], true);
    assert_eq!(saved_json["game_bans"][0]["name"], "Counter-Strike 2");

    // 3. Verify GET /api/accounts/restrictions now returns 1 restriction
    let req_get2 = Request::builder()
        .method("GET")
        .uri("/api/accounts/restrictions")
        .body(Body::empty())
        .unwrap();

    let resp_get2 = app.clone().oneshot(req_get2).await.unwrap();
    assert_eq!(resp_get2.status(), StatusCode::OK);
    let body_bytes2 = resp_get2.into_body().collect().await.unwrap().to_bytes();
    let json2: Value = serde_json::from_slice(&body_bytes2).unwrap();
    assert_eq!(json2["restrictions"].as_array().unwrap().len(), 1);
    assert_eq!(json2["restrictions"][0]["account_id"], "76561198809112858");
}

#[test]
fn test_sanitize_steam_url() {
    use swch::server::handlers::sanitize_steam_url;

    assert_eq!(
        sanitize_steam_url("https://steamcommunity.com/profiles/76561198000000002/inventory/#730_2_12345"),
        Some("steam://openurl/https://steamcommunity.com/profiles/76561198000000002/inventory/#730_2_12345".to_string())
    );
    assert_eq!(
        sanitize_steam_url("steam://openurl/https://steamcommunity.com/profiles/76561198000000002/inventory/#730_2_12345"),
        Some("steam://openurl/https://steamcommunity.com/profiles/76561198000000002/inventory/#730_2_12345".to_string())
    );
    assert_eq!(
        sanitize_steam_url("steam://open/main"),
        Some("steam://open/main".to_string())
    );
    // Malicious or arbitrary protocols should be rejected
    assert_eq!(sanitize_steam_url("http://evil.com"), None);
    assert_eq!(sanitize_steam_url("file:///etc/passwd"), None);
    assert_eq!(sanitize_steam_url(""), None);
    assert_eq!(
        sanitize_steam_url("https://help.steampowered.com/en/wizard/VacBans"),
        Some("steam://openurl/https://help.steampowered.com/en/wizard/VacBans".to_string())
    );
    assert_eq!(
        sanitize_steam_url("https://help.steampowered.com/en/wizard/VacBans.evil"),
        None
    );
}

#[tokio::test]
async fn test_system_shutdown_endpoint() {
    let db = Database::open_in_memory().unwrap();
    let app = create_app(db);

    let notify = swch::server::handlers::get_shutdown_notify();
    let notified = notify.notified();

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/system/shutdown")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    tokio::time::timeout(std::time::Duration::from_millis(500), notified)
        .await
        .expect("timed out waiting for shutdown notification");
}

