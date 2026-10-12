use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Mutex;
use swch::{
    launchers::hoyoverse::{
        find_game_def,
        registry::{self, RegistryEntry},
        session::HoyoverseSessionManager,
        SUPPORTED_GAMES,
    },
    modules::{LauncherDetector, ModuleId},
    storage::Database,
};
use tempfile::tempdir;
use tower::ServiceExt;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_module_id_parsing_and_detection() {
    for alias in ["hoyoverse", "HOYOVERSE", "mihoyo", "MIHOYO", "hoyo", "hoyoplay"] {
        assert_eq!(
            alias.parse::<ModuleId>().unwrap(),
            ModuleId::Hoyoverse,
            "Failed alias {alias}"
        );
    }

    assert_eq!(
        serde_json::to_string(&ModuleId::Hoyoverse).unwrap(),
        "\"hoyoverse\""
    );

    let detector = LauncherDetector::with_mock_launchers(vec!["hoyoverse".into()]);
    assert!(detector.is_installed(ModuleId::Hoyoverse));
    assert!(!detector.is_installed(ModuleId::Steam));
}

#[test]
fn test_game_definitions_and_aliases() {
    assert_eq!(SUPPORTED_GAMES.len(), 5);

    // Verify all 5 expected games are defined
    let ids: Vec<&str> = SUPPORTED_GAMES.iter().map(|g| g.id).collect();
    assert!(ids.contains(&"genshin"));
    assert!(ids.contains(&"starrail"));
    assert!(ids.contains(&"zzz"));
    assert!(ids.contains(&"honkai3rd"));
    assert!(ids.contains(&"hoyoplay"));

    // Verify alias matching
    assert_eq!(find_game_def("genshin").unwrap().id, "genshin");
    assert_eq!(find_game_def("gi").unwrap().id, "genshin");
    assert_eq!(find_game_def("starrail").unwrap().id, "starrail");
    assert_eq!(find_game_def("hsr").unwrap().id, "starrail");
    assert_eq!(find_game_def("zzz").unwrap().id, "zzz");
    assert_eq!(find_game_def("honkai3rd").unwrap().id, "honkai3rd");
    assert_eq!(find_game_def("hi3").unwrap().id, "honkai3rd");
    assert_eq!(find_game_def("hoyoplay").unwrap().id, "hoyoplay");
    assert_eq!(find_game_def("hyp").unwrap().id, "hoyoplay");
    assert_eq!(find_game_def("hoyo").unwrap().id, "hoyoplay");
    assert!(find_game_def("nonexistent").is_none());
}

#[test]
fn test_registry_mock_crud_operations() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let dir = tempdir().unwrap();
    let reg_file = dir.path().join("test_registry.json");
    std::env::set_var("SWCH_HOYOVERSE_REGISTRY_FILE", &reg_file);

    let subkey = "Software\\miHoYo\\Genshin Impact";
    let hive = "HKCU";

    // 1. Write String
    registry::write_value(
        hive,
        subkey,
        "__LastUid___h2153286551",
        &RegistryEntry::String("701234567".into()),
    )
    .unwrap();

    // 2. Write Binary
    let token_bytes = b"{\"account_id\":701234567,\"token\":\"fake_token_eu\"}".to_vec();
    registry::write_value(
        hive,
        subkey,
        "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
        &RegistryEntry::Binary(token_bytes.clone()),
    )
    .unwrap();

    // 3. Write DWord
    registry::write_value(hive, subkey, "GraphicsQuality", &RegistryEntry::DWord(3)).unwrap();

    // 4. Read back single values
    let uid_entry = registry::read_value(hive, subkey, "__LastUid___h2153286551")
        .unwrap()
        .unwrap();
    assert_eq!(uid_entry.as_str(), Some("701234567"));

    let token_entry =
        registry::read_value(hive, subkey, "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810")
            .unwrap()
            .unwrap();
    assert_eq!(token_entry.as_bytes(), Some(token_bytes.as_slice()));

    let gfx_entry = registry::read_value(hive, subkey, "GraphicsQuality")
        .unwrap()
        .unwrap();
    assert_eq!(gfx_entry.as_u64(), Some(3));

    // 5. Read all key values
    let all_vals = registry::read_key_values(hive, subkey).unwrap();
    assert_eq!(all_vals.len(), 3);

    // 6. Delete value
    registry::delete_value(hive, subkey, "GraphicsQuality").unwrap();
    assert!(registry::read_value(hive, subkey, "GraphicsQuality")
        .unwrap()
        .is_none());

    std::env::remove_var("SWCH_HOYOVERSE_REGISTRY_FILE");
}

#[test]
fn test_genshin_session_save_and_restore_cycle() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let dir = tempdir().unwrap();
    let reg_file = dir.path().join("reg_genshin.json");
    let sessions_dir = dir.path().join("sessions");
    std::env::set_var("SWCH_HOYOVERSE_REGISTRY_FILE", &reg_file);
    std::env::set_var("SWCH_HOYOVERSE_SESSIONS_DIR", &sessions_dir);

    let manager = HoyoverseSessionManager::new(sessions_dir.clone());
    let subkey = "Software\\miHoYo\\Genshin Impact";

    // Setup Account 1 in registry (Main Account EU)
    registry::write_value(
        "HKCU",
        subkey,
        "__LastUid___h2153286551",
        &RegistryEntry::String("701111111".into()),
    )
    .unwrap();
    registry::write_value(
        "HKCU",
        subkey,
        "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
        &RegistryEntry::Binary(b"{\"account_id\":701111111,\"token\":\"token_acc_1\"}".to_vec()),
    )
    .unwrap();

    let acc1 = manager
        .save_current("genshin", Some("Genshin Main"))
        .unwrap();
    assert_eq!(acc1.id, "hoyoverse_genshin_701111111");
    assert_eq!(acc1.username, "701111111");
    assert_eq!(acc1.display_name.as_deref(), Some("Genshin Main"));
    assert!(acc1.is_active);

    // Setup Account 2 in registry (Alt Account NA)
    registry::write_value(
        "HKCU",
        subkey,
        "__LastUid___h2153286551",
        &RegistryEntry::String("602222222".into()),
    )
    .unwrap();
    registry::write_value(
        "HKCU",
        subkey,
        "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
        &RegistryEntry::Binary(b"{\"account_id\":602222222,\"token\":\"token_acc_2\"}".to_vec()),
    )
    .unwrap();

    let acc2 = manager.save_current("genshin", Some("Genshin Alt")).unwrap();
    assert_eq!(acc2.id, "hoyoverse_genshin_602222222");
    assert!(acc2.is_active);

    // Verify stored accounts state
    let list = manager.accounts(Some("genshin")).unwrap();
    assert_eq!(list.len(), 2);
    let stored_acc1 = list.iter().find(|a| a.id == acc1.id).unwrap();
    let stored_acc2 = list.iter().find(|a| a.id == acc2.id).unwrap();
    assert!(!stored_acc1.is_active);
    assert!(stored_acc2.is_active);

    // Switch/Restore back to Account 1
    let restored1 = manager.restore(&acc1.id).unwrap();
    assert!(restored1.is_active);
    assert_eq!(restored1.id, acc1.id);

    // Verify registry now has Account 1 values
    let current_uid = registry::read_value("HKCU", subkey, "__LastUid___h2153286551")
        .unwrap()
        .unwrap();
    assert_eq!(current_uid.as_str(), Some("701111111"));

    let list_after = manager.accounts(Some("genshin")).unwrap();
    let reloaded_acc1 = list_after.iter().find(|a| a.id == acc1.id).unwrap();
    let reloaded_acc2 = list_after.iter().find(|a| a.id == acc2.id).unwrap();
    assert!(reloaded_acc1.is_active);
    assert!(!reloaded_acc2.is_active);

    std::env::remove_var("SWCH_HOYOVERSE_REGISTRY_FILE");
    std::env::remove_var("SWCH_HOYOVERSE_SESSIONS_DIR");
}

#[test]
fn test_multi_game_isolation_hsr_and_zzz() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let dir = tempdir().unwrap();
    let reg_file = dir.path().join("reg_multi.json");
    let sessions_dir = dir.path().join("sessions_multi");
    std::env::set_var("SWCH_HOYOVERSE_REGISTRY_FILE", &reg_file);
    std::env::set_var("SWCH_HOYOVERSE_SESSIONS_DIR", &sessions_dir);

    let manager = HoyoverseSessionManager::new(sessions_dir);

    // 1. Honkai: Star Rail session (App_LastUserID_h2841727341 as DWORD)
    let hsr_key = "Software\\Cognosphere\\Star Rail";
    registry::write_value(
        "HKCU",
        hsr_key,
        "App_LastUserID_h2841727341",
        &RegistryEntry::DWord(609876543),
    )
    .unwrap();
    registry::write_value(
        "HKCU",
        hsr_key,
        "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
        &RegistryEntry::Binary(b"{\"account_id\":609876543,\"token\":\"hsr_tok\"}".to_vec()),
    )
    .unwrap();

    let hsr_acc = manager.save_current("starrail", Some("HSR Main")).unwrap();
    assert_eq!(hsr_acc.id, "hoyoverse_starrail_609876543");
    assert_eq!(hsr_acc.username, "609876543");

    // 2. Zenless Zone Zero session (extracted from SDK JSON token)
    let zzz_key = "Software\\miHoYo\\ZenlessZoneZero";
    registry::write_value(
        "HKCU",
        zzz_key,
        "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
        &RegistryEntry::Binary(b"{\"account_id\":1337420,\"token\":\"zzz_tok\"}".to_vec()),
    )
    .unwrap();

    let zzz_acc = manager.save_current("zzz", Some("ZZZ Main")).unwrap();
    assert_eq!(zzz_acc.id, "hoyoverse_zzz_1337420");
    assert_eq!(zzz_acc.username, "1337420");

    // Check game filtering
    let hsr_list = manager.accounts(Some("starrail")).unwrap();
    assert_eq!(hsr_list.len(), 1);
    assert_eq!(hsr_list[0].id, hsr_acc.id);

    let zzz_list = manager.accounts(Some("zzz")).unwrap();
    assert_eq!(zzz_list.len(), 1);
    assert_eq!(zzz_list[0].id, zzz_acc.id);

    let all_list = manager.accounts(None).unwrap();
    assert_eq!(all_list.len(), 2);

    std::env::remove_var("SWCH_HOYOVERSE_REGISTRY_FILE");
    std::env::remove_var("SWCH_HOYOVERSE_SESSIONS_DIR");
}

#[test]
fn test_prepare_login_clears_tokens() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let dir = tempdir().unwrap();
    let reg_file = dir.path().join("reg_prep.json");
    let sessions_dir = dir.path().join("sessions_prep");
    std::env::set_var("SWCH_HOYOVERSE_REGISTRY_FILE", &reg_file);
    std::env::set_var("SWCH_HOYOVERSE_SESSIONS_DIR", &sessions_dir);

    let manager = HoyoverseSessionManager::new(sessions_dir);
    let subkey = "Software\\miHoYo\\Genshin Impact";

    registry::write_value(
        "HKCU",
        subkey,
        "__LastUid___h2153286551",
        &RegistryEntry::String("700000001".into()),
    )
    .unwrap();
    registry::write_value(
        "HKCU",
        subkey,
        "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
        &RegistryEntry::Binary(b"{\"account_id\":700000001}".to_vec()),
    )
    .unwrap();

    let acc = manager.save_current("genshin", None).unwrap();
    assert!(acc.is_active);

    // Call prepare_login
    manager.prepare_login("genshin").unwrap();

    // Verify login keys are cleared
    assert!(registry::read_value("HKCU", subkey, "__LastUid___h2153286551")
        .unwrap()
        .is_none());
    assert!(
        registry::read_value("HKCU", subkey, "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810")
            .unwrap()
            .is_none()
    );

    // Saved account is marked inactive
    let list = manager.accounts(Some("genshin")).unwrap();
    assert_eq!(list.len(), 1);
    assert!(!list[0].is_active);

    std::env::remove_var("SWCH_HOYOVERSE_REGISTRY_FILE");
    std::env::remove_var("SWCH_HOYOVERSE_SESSIONS_DIR");
}

#[tokio::test]
async fn test_axum_server_endpoints() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let dir = tempdir().unwrap();
    let reg_file = dir.path().join("reg_api.json");
    let sessions_dir = dir.path().join("sessions_api");
    std::env::set_var("SWCH_HOYOVERSE_REGISTRY_FILE", &reg_file);
    std::env::set_var("SWCH_HOYOVERSE_SESSIONS_DIR", &sessions_dir);

    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    // Populate registry with initial session
    let subkey = "Software\\miHoYo\\Genshin Impact";
    registry::write_value(
        "HKCU",
        subkey,
        "__LastUid___h2153286551",
        &RegistryEntry::String("705555555".into()),
    )
    .unwrap();
    registry::write_value(
        "HKCU",
        subkey,
        "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
        &RegistryEntry::Binary(b"{\"account_id\":705555555}".to_vec()),
    )
    .unwrap();

    let app = axum::Router::new()
        .route("/api/hoyoverse/games", axum::routing::get(swch::server::hoyoverse::library))
        .route("/api/hoyoverse/accounts", axum::routing::get(swch::server::hoyoverse::accounts))
        .route("/api/hoyoverse/current-account", axum::routing::get(swch::server::hoyoverse::current_account))
        .route("/api/hoyoverse/sessions/save", axum::routing::post(swch::server::hoyoverse::save_session))
        .route("/api/hoyoverse/switch-account", axum::routing::post(swch::server::hoyoverse::switch_account))
        .route("/api/hoyoverse/accounts/prepare", axum::routing::post(swch::server::hoyoverse::prepare_account))
        .route("/api/hoyoverse/accounts/remove", axum::routing::post(swch::server::hoyoverse::remove_account))
        .with_state(db.clone());

    // 1. GET /api/hoyoverse/games
    let req = Request::builder()
        .uri("/api/hoyoverse/games")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert!(v["games"].is_array());
    assert!(v["supported_games"].is_array());

    // 2. POST /api/hoyoverse/sessions/save
    let save_payload = json!({
        "game": "genshin",
        "display_name": "API Test Main"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/hoyoverse/sessions/save")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&save_payload).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&body).unwrap();
    let account_id = v["account"]["id"].as_str().unwrap().to_string();
    assert_eq!(account_id, "hoyoverse_genshin_705555555");

    // 3. GET /api/hoyoverse/accounts
    let req = Request::builder()
        .uri("/api/hoyoverse/accounts?game=genshin")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["accounts"].as_array().unwrap().len(), 1);

    // 4. POST /api/hoyoverse/switch-account
    let switch_payload = json!({
        "account_id": account_id
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/hoyoverse/switch-account")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&switch_payload).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 5. POST /api/hoyoverse/accounts/prepare
    let prep_payload = json!({
        "game": "genshin"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/hoyoverse/accounts/prepare")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&prep_payload).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 6. POST /api/hoyoverse/accounts/remove
    let rm_payload = json!({
        "account_id": account_id
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/hoyoverse/accounts/remove")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&rm_payload).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    std::env::remove_var("SWCH_HOYOVERSE_REGISTRY_FILE");
    std::env::remove_var("SWCH_HOYOVERSE_SESSIONS_DIR");
}
