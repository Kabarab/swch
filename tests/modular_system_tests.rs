use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::Arc;
use tempfile::tempdir;
use tower::ServiceExt;

use swch::modules::{ModuleId, ModuleManager};
use swch::server::{create_app_with_state, AppState};
use swch::storage::{Database, Game};

static TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn test_modules_initial_detection_and_zero_ram_for_missing() {
    let _lock = TEST_MUTEX.lock().await;
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let modules_dir = tmp.path().join("modules");
    std::env::set_var("SWCH_MODULES_DIR", modules_dir.to_str().unwrap());

    let module_manager = Arc::new(ModuleManager::new(db.clone()));
    // Mock that NO external launchers are installed on this machine
    module_manager.set_mock_launchers(vec![]).await;
    module_manager.init().await.unwrap();

    let status = module_manager.get_status_all().await;
    assert_eq!(status.len(), ModuleId::all().len());

    for m in &status {
        assert_eq!(m.status, "not_detected");
        assert_eq!(m.ram_bytes, 0);
        assert!(!m.active);
    }

    // Ensure EpicAuthStore was NOT instantiated
    assert!(module_manager.epic_store().await.is_none());
}

#[tokio::test]
async fn test_auto_discovery_and_dynamic_hot_plugging_when_launcher_installed_later() {
    let _lock = TEST_MUTEX.lock().await;
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let modules_dir = tmp.path().join("modules");
    std::env::set_var("SWCH_MODULES_DIR", modules_dir.to_str().unwrap());

    let module_manager = Arc::new(ModuleManager::new(db.clone()));
    // Initially: No launchers installed
    module_manager.set_mock_launchers(vec![]).await;
    module_manager.init().await.unwrap();

    assert!(!module_manager.is_module_active(ModuleId::Epic).await);
    assert!(module_manager.epic_store().await.is_none());

    // USER SCENARIO: User downloads & installs Epic Games Launcher on computer!
    module_manager.set_mock_launchers(vec!["epic".to_string()]).await;

    // Background watcher executes scan_and_sync()
    let report = module_manager.scan_and_sync().await.unwrap();

    // Verify auto-discovery hot-plugged Epic module!
    assert!(report.newly_installed.contains(&"epic".to_string()));
    assert!(report.active_modules.contains(&"epic".to_string()));
    assert!(module_manager.is_module_active(ModuleId::Epic).await);

    // Verify EpicAuthStore is now active and allocated
    assert!(module_manager.epic_store().await.is_some());

    // USER SCENARIO: User uninstalls Epic Games Launcher
    module_manager.set_mock_launchers(vec![]).await;
    let uninstall_report = module_manager.scan_and_sync().await.unwrap();

    assert!(uninstall_report.deactivated.contains(&"epic".to_string()));
    assert!(!module_manager.is_module_active(ModuleId::Epic).await);
    // Verify memory was freed: Epic store dropped!
    assert!(module_manager.epic_store().await.is_none());
}

#[tokio::test]
async fn test_api_modules_endpoints() {
    let _lock = TEST_MUTEX.lock().await;
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let modules_dir = tmp.path().join("modules");
    std::env::set_var("SWCH_MODULES_DIR", modules_dir.to_str().unwrap());

    let module_manager = Arc::new(ModuleManager::new(db.clone()));
    module_manager.set_mock_launchers(vec!["steam".to_string()]).await;
    module_manager.init().await.unwrap();

    let epic_store = Arc::new(swch::launchers::epic::EpicAuthStore::default_store(db.clone()));
    let roblox_manager = Arc::new(swch::launchers::roblox::RobloxManager::new(db.clone()));
    let minecraft_manager = Arc::new(swch::launchers::minecraft::MinecraftManager::new(db.clone()));
    let update_manager = Arc::new(swch::server::updater::UpdateManager::new());
    let app = create_app_with_state(AppState {
        db: db.clone(),
        epic_store,
        module_manager: module_manager.clone(),
        roblox_manager,
        minecraft_manager,
        update_manager,
    });

    // 1. GET /api/modules
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/modules")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["modules"].is_array());
    let modules = json["modules"].as_array().unwrap();
    assert_eq!(modules.len(), ModuleId::all().len());

    // 2. POST /api/modules/scan
    let res_scan = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/modules/scan")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_scan.status(), StatusCode::OK);

    // 3. POST /api/modules/steam/toggle (disable)
    let res_toggle = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/modules/steam/toggle")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"active": false}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_toggle.status(), StatusCode::OK);
    assert!(!module_manager.is_module_active(ModuleId::Steam).await);

    // 4. POST /api/modules/steam/toggle (re-enable)
    let res_enable = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/modules/steam/toggle")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"active": true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_enable.status(), StatusCode::OK);
    assert!(module_manager.is_module_active(ModuleId::Steam).await);

    // 5. POST /api/modules/steam/purge
    let res_purge = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/modules/steam/purge")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_purge.status(), StatusCode::OK);
    assert!(!module_manager.is_module_active(ModuleId::Steam).await);
}

#[tokio::test]
async fn test_games_and_accounts_filtering_by_active_modules() {
    let _lock = TEST_MUTEX.lock().await;
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let modules_dir = tmp.path().join("modules");
    std::env::set_var("SWCH_MODULES_DIR", modules_dir.to_str().unwrap());

    // Populate database with games from steam and epic
    let steam_game = Game {
        id: "game_steam_1".to_string(),
        launcher: "steam".to_string(),
        app_id: "10".to_string(),
        name: "Counter-Strike".to_string(),
        install_path: None,
        executable_path: None,
        size_bytes: 1_000_000,
        is_installed: true,
        banner_url: None,
        launch_uri: None,
        namespace: None,
    };
    let epic_game = Game {
        id: "game_epic_1".to_string(),
        launcher: "epic".to_string(),
        app_id: "Fortnite".to_string(),
        name: "Fortnite".to_string(),
        install_path: None,
        executable_path: None,
        size_bytes: 20_000_000,
        is_installed: true,
        banner_url: None,
        launch_uri: None,
        namespace: None,
    };
    db.upsert_game(&steam_game).await.unwrap();
    db.upsert_game(&epic_game).await.unwrap();

    let module_manager = Arc::new(ModuleManager::new(db.clone()));
    // Only Steam is active initially
    module_manager.set_mock_launchers(vec!["steam".to_string()]).await;
    module_manager.init().await.unwrap();

    let epic_store = Arc::new(swch::launchers::epic::EpicAuthStore::default_store(db.clone()));
    let roblox_manager = Arc::new(swch::launchers::roblox::RobloxManager::new(db.clone()));
    let minecraft_manager = Arc::new(swch::launchers::minecraft::MinecraftManager::new(db.clone()));
    let update_manager = Arc::new(swch::server::updater::UpdateManager::new());
    let app = create_app_with_state(AppState {
        db: db.clone(),
        epic_store,
        module_manager: module_manager.clone(),
        roblox_manager,
        minecraft_manager,
        update_manager,
    });

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/games")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let games: Vec<Game> = serde_json::from_slice(&bytes).unwrap();

    // Epic is NOT active, so only the Steam game should be returned!
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].id, "game_steam_1");

    // Now activate Epic
    module_manager.install_module(ModuleId::Epic).await.unwrap();

    let res2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/games")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let bytes2 = res2.into_body().collect().await.unwrap().to_bytes();
    let games2: Vec<Game> = serde_json::from_slice(&bytes2).unwrap();

    // Both Steam and Epic games are returned now!
    assert_eq!(games2.len(), 2);
}
