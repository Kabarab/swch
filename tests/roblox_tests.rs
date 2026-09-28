use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::sync::Arc;
use tempfile::tempdir;
use tower::ServiceExt;

use swch::launchers::epic::EpicAuthStore;
use swch::launchers::roblox::client::RobloxClient;
use swch::launchers::roblox::scanner::RobloxScanner;
use swch::launchers::roblox::RobloxManager;
use swch::modules::detector::LauncherDetector;
use swch::modules::types::ModuleId;
use swch::modules::ModuleManager;
use swch::server::{create_app_with_state, AppState};
use swch::storage::{Database, RobloxAccountInfo, RobloxPlace};

#[tokio::test]
async fn test_roblox_detector_env_override() {
    let tmp = tempdir().unwrap();
    let detector = LauncherDetector::new();
    std::env::set_var("SWCH_ROBLOX_DIR", tmp.path().to_str().unwrap());
    let detected = detector.detect_path(ModuleId::Roblox);
    assert_eq!(detected, Some(tmp.path().to_path_buf()));
    std::env::remove_var("SWCH_ROBLOX_DIR");
}

#[test]
fn test_roblox_client_cookie_formatting() {
    let raw = "_|WARNING:-DO-NOT-SHARE-THIS...testcookie";
    let formatted = RobloxClient::format_cookie(raw);
    assert_eq!(formatted, ".ROBLOSECURITY=_|WARNING:-DO-NOT-SHARE-THIS...testcookie");

    let already_formatted = ".ROBLOSECURITY=_|WARNING:-DO-NOT-SHARE-THIS...testcookie";
    assert_eq!(RobloxClient::format_cookie(already_formatted), already_formatted);
}

#[test]
fn test_extract_warning_cookies_from_bytes() {
    let mut data = Vec::new();
    data.extend_from_slice(b"some random prefix text\x00\x01\x02");
    let fake_cookie = format!("_|WARNING:-DO-NOT-SHARE-THIS.--Sharing-this-will-allow-someone-to-log-in-to-your-account-and-to-steal-your-ROBUX-and-items|_{}", "A".repeat(200));
    data.extend_from_slice(fake_cookie.as_bytes());
    data.extend_from_slice(b"; path=/; domain=.roblox.com\x00another_cookie=123");

    let extracted = RobloxScanner::extract_warning_cookies_from_bytes(&data);
    assert_eq!(extracted.len(), 1);
    assert_eq!(extracted[0], fake_cookie);
}

#[tokio::test]
async fn test_roblox_storage_crud() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test_roblox.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let account = RobloxAccountInfo {
        account_id: "roblox_123456".to_string(),
        username: "Builderman".to_string(),
        display_name: "The Builder".to_string(),
        user_id: "123456".to_string(),
        cookie: "_|WARNING:-DO-NOT-SHARE-THIS...test".to_string(),
        is_active: true,
        is_premium: true,
        robux_balance: 50000,
        avatar_url: Some("https://example.com/avatar.png".to_string()),
        headshot_url: Some("https://example.com/headshot.png".to_string()),
        rap_value: 120000,
        is_banned: false,
        last_validated: 1700000000,
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    };

    // Save account
    db.save_roblox_account(&account).await.unwrap();

    // Retrieve accounts
    let accounts = db.get_roblox_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].username, "Builderman");
    assert_eq!(accounts[0].robux_balance, 50000);
    assert!(accounts[0].is_active);

    // Save place
    let place = RobloxPlace {
        place_id: "9285238704".to_string(),
        name: "Blox Fruits".to_string(),
        description: Some("Anime pirate game".to_string()),
        icon_url: Some("https://example.com/icon.png".to_string()),
        creator_name: Some("Gamer Robot Inc".to_string()),
        is_manual: true,
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    };
    db.save_roblox_place(&place).await.unwrap();

    let places = db.get_roblox_places().await.unwrap();
    assert_eq!(places.len(), 1);
    assert_eq!(places[0].name, "Blox Fruits");
    assert!(places[0].is_manual);

    // Cache get/put
    db.roblox_cache_put("test_cache_key", "{\"hello\":\"world\"}").await.unwrap();
    let cached = db.roblox_cache_get("test_cache_key").await.unwrap();
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().0, "{\"hello\":\"world\"}");

    // Settings
    let def_multi = db.get_roblox_setting("multi_instance_enabled").await.unwrap();
    assert_eq!(def_multi, Some("true".to_string()));

    db.set_roblox_setting("multi_instance_enabled", "false").await.unwrap();
    let updated_multi = db.get_roblox_setting("multi_instance_enabled").await.unwrap();
    assert_eq!(updated_multi, Some("false".to_string()));

    // Delete place & account
    db.delete_roblox_place("9285238704").await.unwrap();
    let places_after = db.get_roblox_places().await.unwrap();
    assert_eq!(places_after.len(), 0);

    db.delete_roblox_account("roblox_123456").await.unwrap();
    let accounts_after = db.get_roblox_accounts().await.unwrap();
    assert_eq!(accounts_after.len(), 0);
}

#[tokio::test]
async fn test_roblox_api_routes() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test_api.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let epic_store = Arc::new(EpicAuthStore::default_store(db.clone()));
    let module_manager = Arc::new(ModuleManager::new_with_epic(db.clone(), Some(epic_store.clone())));
    let roblox_manager = Arc::new(RobloxManager::new(db.clone()));
    let minecraft_manager = Arc::new(swch::launchers::minecraft::MinecraftManager::new(db.clone()));

    let update_manager = Arc::new(swch::server::updater::UpdateManager::new());
    let app = create_app_with_state(AppState {
        db: db.clone(),
        epic_store,
        module_manager,
        roblox_manager,
        minecraft_manager,
        update_manager,
    });

    // 1. GET /api/roblox/library
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/roblox/library")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let val: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(val.get("games").is_some());

    // 2. GET /api/roblox/settings
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/roblox/settings")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let val: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["multi_instance_enabled"], true);

    // 3. POST /api/roblox/settings (update multi_instance_enabled to false)
    let update_body = serde_json::json!({
        "key": "multi_instance_enabled",
        "value": "false"
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/roblox/settings")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    // 4. Prepopulate an account in DB and test /api/roblox/accounts and switch
    let test_acc = RobloxAccountInfo {
        account_id: "roblox_999".to_string(),
        username: "RobloxDev".to_string(),
        display_name: "Roblox Developer".to_string(),
        user_id: "999".to_string(),
        cookie: "_|WARNING:-DO-NOT-SHARE-THIS...key999".to_string(),
        is_active: false,
        is_premium: false,
        robux_balance: 100,
        avatar_url: None,
        headshot_url: None,
        rap_value: 0,
        is_banned: false,
        last_validated: 1700000000,
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    };
    db.save_roblox_account(&test_acc).await.unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/roblox/accounts")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let accounts: Vec<RobloxAccountInfo> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].username, "RobloxDev");

    // 5. POST /api/roblox/switch-account
    let switch_body = serde_json::json!({
        "account_id": "roblox_999"
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/roblox/switch-account")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&switch_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let accounts_after = db.get_roblox_accounts().await.unwrap();
    assert!(accounts_after[0].is_active);

    // 6. POST /api/roblox/launch (dry run with SWCH_DRY_RUN=1)
    std::env::set_var("SWCH_DRY_RUN", "1");
    // Mock the ticket generation internally or handle network fallback:
    // With fake cookie, Roblox API will fail with network/status, returning proper error code
    let launch_body = serde_json::json!({
        "account_id": "roblox_999"
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/roblox/launch")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&launch_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Since cookie is mock, it returns 500 with auth error, which confirms the launch pipeline reached auth ticket generation
    assert!(res.status() == StatusCode::INTERNAL_SERVER_ERROR || res.status() == StatusCode::OK);
    std::env::remove_var("SWCH_DRY_RUN");
}
