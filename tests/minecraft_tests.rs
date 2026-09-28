use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::sync::Arc;
use tempfile::tempdir;
use tower::ServiceExt;

use swch::launchers::epic::EpicAuthStore;
use swch::launchers::minecraft::hot_swap::MinecraftHotSwap;
use swch::launchers::minecraft::nbt::{parse_servers_dat, write_servers_dat};
use swch::launchers::minecraft::scanner::MinecraftScanner;
use swch::launchers::minecraft::MinecraftManager;
use swch::launchers::roblox::RobloxManager;
use swch::modules::detector::LauncherDetector;
use swch::modules::types::ModuleId;
use swch::modules::ModuleManager;
use swch::server::{create_app_with_state, AppState};
use swch::storage::models::{MinecraftAccount, MinecraftProfile, MinecraftServer, MinecraftWorld};
use swch::storage::Database;
use swch::sys::process::LauncherType;

static TEST_ENV_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn test_minecraft_detector_env_override() {
    let _lock = TEST_ENV_MUTEX.lock().await;
    let tmp = tempdir().unwrap();
    let detector = LauncherDetector::new();

    let fake_app = tmp.path().join("fake_minecraft_launcher");
    std::fs::write(&fake_app, b"dummy launcher").unwrap();

    let fake_data = tmp.path().join(".minecraft");
    std::fs::create_dir_all(&fake_data).unwrap();

    std::env::set_var("SWCH_MINECRAFT_APP", fake_app.to_str().unwrap());
    std::env::set_var("SWCH_MINECRAFT_DIR", fake_data.to_str().unwrap());

    assert_eq!(detector.detect_minecraft(), Some(fake_app.clone()));
    assert_eq!(detector.detect_minecraft_data_dir(), Some(fake_data.clone()));
    assert_eq!(detector.detect_path(ModuleId::Minecraft), Some(fake_app));

    std::env::remove_var("SWCH_MINECRAFT_APP");
    std::env::remove_var("SWCH_MINECRAFT_DIR");
}

#[test]
fn test_minecraft_nbt_servers_roundtrip_and_edge_cases() {
    let servers = vec![
        MinecraftServer {
            id: "srv_hypixel".to_string(),
            name: "Hypixel Network".to_string(),
            ip: "mc.hypixel.net:25565".to_string(),
            icon: Some("data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==".to_string()),
            is_favorite: true,
            is_imported: true,
            last_played: None,
            updated_at: "2026-09-25T12:00:00Z".to_string(),
        },
        MinecraftServer {
            id: "srv_mineplex".to_string(),
            name: "Mineplex".to_string(),
            ip: "us.mineplex.com".to_string(),
            icon: None,
            is_favorite: false,
            is_imported: true,
            last_played: None,
            updated_at: "2026-09-25T12:00:00Z".to_string(),
        },
    ];

    let bytes = write_servers_dat(&servers);
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], 10); // TAG_Compound root

    let parsed = parse_servers_dat(&bytes);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].name, "Hypixel Network");
    assert_eq!(parsed[0].ip, "mc.hypixel.net:25565");
    assert!(parsed[0].icon.as_ref().unwrap().starts_with("data:image/png;base64,"));
    assert_eq!(parsed[1].name, "Mineplex");
    assert_eq!(parsed[1].ip, "us.mineplex.com");
    assert!(parsed[1].icon.is_none());

    // Corrupt / empty data checks
    assert!(parse_servers_dat(&[]).is_empty());
    assert!(parse_servers_dat(&[10, 0, 0, 0]).is_empty());
    assert!(parse_servers_dat(&[1, 2, 3]).is_empty());
}

#[tokio::test]
async fn test_minecraft_storage_crud_and_sync() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test_minecraft.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    // 1. Accounts CRUD & sync to base `accounts` table
    let acc1 = MinecraftAccount {
        account_id: "mc_uuid_1".to_string(),
        username: "Alex".to_string(),
        account_type: "msa".to_string(),
        uuid: "mc_uuid_1".to_string(),
        avatar_url: Some("https://mc-heads.net/avatar/Alex/64".to_string()),
        skin_url: None,
        is_active: false,
        access_token: None,
        raw_json: None,
        last_used: Some("2026-09-25T12:00:00Z".to_string()),
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    };
    let acc2 = MinecraftAccount {
        account_id: "mc_uuid_2".to_string(),
        username: "Steve".to_string(),
        account_type: "mojang".to_string(),
        uuid: "mc_uuid_2".to_string(),
        avatar_url: Some("https://mc-heads.net/avatar/Steve/64".to_string()),
        skin_url: None,
        is_active: true,
        access_token: None,
        raw_json: None,
        last_used: Some("2026-09-25T12:00:00Z".to_string()),
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    };

    db.save_minecraft_account(&acc1).await.unwrap();
    db.save_minecraft_account(&acc2).await.unwrap();

    let all_accs = db.get_minecraft_accounts().await.unwrap();
    assert_eq!(all_accs.len(), 2);

    // Verify sync in base `accounts` table
    let base_accounts = db.get_accounts().await.unwrap();
    let mc_base: Vec<_> = base_accounts.iter().filter(|a| a.launcher == "minecraft").collect();
    assert_eq!(mc_base.len(), 2);

    // Set active
    db.set_active_minecraft_account("mc_uuid_1").await.unwrap();
    let updated1 = db.get_minecraft_account("mc_uuid_1").await.unwrap().unwrap();
    let updated2 = db.get_minecraft_account("mc_uuid_2").await.unwrap().unwrap();
    assert!(updated1.is_active);
    assert!(!updated2.is_active);

    // Delete account
    db.delete_minecraft_account("mc_uuid_2").await.unwrap();
    assert_eq!(db.get_minecraft_accounts().await.unwrap().len(), 1);

    // 2. Profiles CRUD
    let profiles = vec![
        MinecraftProfile {
            id: "fabric-1.21".to_string(),
            name: "Fabric 1.21.1".to_string(),
            profile_type: "custom".to_string(),
            version_id: "fabric-loader-0.16.5-1.21.1".to_string(),
            icon: None,
            game_dir: None,
            java_dir: None,
            java_args: None,
            last_used: Some("2026-09-25T12:00:00Z".to_string()),
            last_version_id: Some("1.21.1".to_string()),
            created: Some("2026-09-25T10:00:00Z".to_string()),
        },
        MinecraftProfile {
            id: "release".to_string(),
            name: "Latest Release".to_string(),
            profile_type: "latest-release".to_string(),
            version_id: "1.21.1".to_string(),
            icon: None,
            game_dir: None,
            java_dir: None,
            java_args: None,
            last_used: None,
            last_version_id: None,
            created: None,
        },
    ];
    db.save_minecraft_profiles(&profiles).await.unwrap();
    let loaded_profiles = db.get_minecraft_profiles().await.unwrap();
    assert_eq!(loaded_profiles.len(), 2);

    // 3. Worlds CRUD
    let worlds = vec![MinecraftWorld {
        folder_name: "Survival1".to_string(),
        level_name: "My Survival World".to_string(),
        last_played: 1700000000,
        game_mode: "Survival".to_string(),
        hardcore: false,
        version_name: Some("1.21.1".to_string()),
        icon_base64: Some("data:image/png;base64,mockicon".to_string()),
        size_bytes: 1048576,
    }];
    db.save_minecraft_worlds(&worlds).await.unwrap();
    let loaded_worlds = db.get_minecraft_worlds().await.unwrap();
    assert_eq!(loaded_worlds.len(), 1);
    assert_eq!(loaded_worlds[0].level_name, "My Survival World");

    // 4. Servers CRUD & sync to `games` table
    let server = MinecraftServer {
        id: "hypixel_id".to_string(),
        name: "Hypixel".to_string(),
        ip: "mc.hypixel.net".to_string(),
        icon: Some("data:image/png;base64,servericon".to_string()),
        is_favorite: true,
        is_imported: false,
        last_played: None,
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    };
    db.save_minecraft_server(&server).await.unwrap();
    let loaded_servers = db.get_minecraft_servers().await.unwrap();
    assert_eq!(loaded_servers.len(), 1);
    assert_eq!(loaded_servers[0].ip, "mc.hypixel.net");

    // Verify sync to `games` table
    let games = db.get_games().await.unwrap();
    let hypixel_game = games.iter().find(|g| g.id == "minecraft_server_hypixel_id");
    assert!(hypixel_game.is_some());
    let game = hypixel_game.unwrap();
    assert_eq!(game.name, "Hypixel");
    assert_eq!(game.launcher, "minecraft");

    // Delete server
    db.delete_minecraft_server("hypixel_id").await.unwrap();
    assert!(db.get_minecraft_servers().await.unwrap().is_empty());
    let games_after = db.get_games().await.unwrap();
    assert!(!games_after.iter().any(|g| g.id == "minecraft_server_hypixel_id"));
}

#[tokio::test]
async fn test_minecraft_hot_swap_atomic_backup() {
    let _lock = TEST_ENV_MUTEX.lock().await;
    let tmp = tempdir().unwrap();
    let mc_dir = tmp.path().join(".minecraft");
    std::fs::create_dir_all(&mc_dir).unwrap();

    let accounts_json = serde_json::json!({
        "accounts": {
            "account_a": {
                "active": true,
                "minecraftProfile": {
                    "id": "uuid_a",
                    "name": "Steve"
                },
                "type": "Xbox"
            },
            "account_b": {
                "active": false,
                "minecraftProfile": {
                    "id": "uuid_b",
                    "name": "Alex"
                },
                "type": "Xbox"
            }
        },
        "activeAccountLocalId": "account_a",
        "formatVersion": 1
    });

    let accounts_file = mc_dir.join("launcher_accounts.json");
    std::fs::write(&accounts_file, serde_json::to_string_pretty(&accounts_json).unwrap()).unwrap();

    let db_path = tmp.path().join("test_hotswap.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    // Pre-insert account_b into DB
    let acc_b = MinecraftAccount {
        account_id: "account_b".to_string(),
        username: "Alex".to_string(),
        account_type: "msa".to_string(),
        uuid: "uuid_b".to_string(),
        avatar_url: None,
        skin_url: None,
        is_active: false,
        access_token: None,
        raw_json: None,
        last_used: None,
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    };
    db.save_minecraft_account(&acc_b).await.unwrap();

    std::env::set_var("SWCH_MINECRAFT_DIR", mc_dir.to_str().unwrap());

    let hotswap = MinecraftHotSwap::new(db.clone());
    let switch_res = hotswap.switch_account("account_b").await;
    assert!(switch_res.is_ok(), "Hot-swap failed: {:?}", switch_res);

    // Verify backup created
    let backup_file = mc_dir.join("launcher_accounts.json.bak");
    assert!(backup_file.exists(), ".bak file must exist after hot-swap");
    let backup_content: Value = serde_json::from_str(&std::fs::read_to_string(&backup_file).unwrap()).unwrap();
    assert_eq!(backup_content["activeAccountLocalId"], "account_a");

    // Verify target updated
    let updated_content: Value = serde_json::from_str(&std::fs::read_to_string(&accounts_file).unwrap()).unwrap();
    assert_eq!(updated_content["activeAccountLocalId"], "account_b");
    assert_eq!(updated_content["accounts"]["account_b"]["active"], true);
    assert_eq!(updated_content["accounts"]["account_a"]["active"], false);

    std::env::remove_var("SWCH_MINECRAFT_DIR");
}

#[tokio::test]
async fn test_minecraft_scanner_end_to_end() {
    let _lock = TEST_ENV_MUTEX.lock().await;
    let tmp = tempdir().unwrap();
    let mc_dir = tmp.path().join(".minecraft");
    std::fs::create_dir_all(&mc_dir).unwrap();

    // 1. Write mock launcher_accounts.json
    let accounts_json = serde_json::json!({
        "accounts": {
            "local_acc_1": {
                "active": true,
                "minecraftProfile": {
                    "id": "uuid_1",
                    "name": "Notch"
                },
                "type": "Xbox"
            }
        },
        "activeAccountLocalId": "local_acc_1",
        "formatVersion": 1
    });
    std::fs::write(mc_dir.join("launcher_accounts.json"), serde_json::to_string_pretty(&accounts_json).unwrap()).unwrap();

    // 2. Write mock launcher_profiles.json
    let profiles_json = serde_json::json!({
        "profiles": {
            "fabric_profile": {
                "name": "Fabric 1.21.1",
                "type": "custom",
                "lastVersionId": "fabric-loader-0.16.5-1.21.1",
                "lastUsed": "2026-09-25T15:00:00.000Z"
            }
        }
    });
    std::fs::write(mc_dir.join("launcher_profiles.json"), serde_json::to_string_pretty(&profiles_json).unwrap()).unwrap();

    // 3. Write mock servers.dat via write_servers_dat
    let test_servers = vec![MinecraftServer {
        id: "srv_2b2t".to_string(),
        name: "2b2t".to_string(),
        ip: "2b2t.org".to_string(),
        icon: None,
        is_favorite: false,
        is_imported: true,
        last_played: None,
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    }];
    let srv_bytes = write_servers_dat(&test_servers);
    std::fs::write(mc_dir.join("servers.dat"), srv_bytes).unwrap();

    // 4. Write mock saves/World1
    let world_dir = mc_dir.join("saves").join("TestWorld");
    std::fs::create_dir_all(&world_dir).unwrap();
    std::fs::write(world_dir.join("level.dat"), b"mock level data").unwrap();
    // 1x1 transparent PNG bytes
    let dummy_png = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
        0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
        0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(world_dir.join("icon.png"), dummy_png).unwrap();

    std::env::set_var("SWCH_MINECRAFT_DIR", mc_dir.to_str().unwrap());

    let db_path = tmp.path().join("test_scanner.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let scanner = MinecraftScanner::new(db.clone());
    let (acc_cnt, prof_cnt, srv_cnt, world_cnt) = scanner.scan_and_import().await;

    assert_eq!(acc_cnt, 1);
    assert_eq!(prof_cnt, 1);
    assert_eq!(srv_cnt, 1);
    assert_eq!(world_cnt, 1);

    let saved_worlds = db.get_minecraft_worlds().await.unwrap();
    assert_eq!(saved_worlds.len(), 1);
    assert!(saved_worlds[0].icon_base64.as_ref().unwrap().starts_with("data:image/png;base64,"));

    std::env::remove_var("SWCH_MINECRAFT_DIR");
}

#[test]
fn test_minecraft_process_names_strictly_exclude_java() {
    let launcher_type = LauncherType::Minecraft;
    let proc_names = launcher_type.process_names();

    assert!(!proc_names.is_empty());
    for name in proc_names {
        let lower = name.to_lowercase();
        assert!(
            !lower.starts_with("java") && !lower.starts_with("javaw"),
            "Strict requirement violation: '{}' must NOT contain java/javaw",
            name
        );
    }
}

#[tokio::test]
async fn test_minecraft_api_endpoints() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test_api.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let epic_store = Arc::new(EpicAuthStore::default_store(db.clone()));
    let module_manager = Arc::new(ModuleManager::new_with_epic(db.clone(), Some(epic_store.clone())));
    let roblox_manager = Arc::new(RobloxManager::new(db.clone()));
    let minecraft_manager = Arc::new(MinecraftManager::new(db.clone()));

    let update_manager = Arc::new(swch::server::updater::UpdateManager::new());
    let app = create_app_with_state(AppState {
        db: db.clone(),
        epic_store,
        module_manager,
        roblox_manager,
        minecraft_manager,
        update_manager,
    });

    // 1. GET /api/minecraft/library
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/minecraft/library")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let lib_val: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(lib_val["games"].is_array());
    assert_eq!(lib_val["games"][0]["id"], "minecraft_java");

    // 2. POST /api/minecraft/servers/add
    let add_srv = serde_json::json!({
        "name": "Hypixel Network",
        "ip": "mc.hypixel.net",
        "is_favorite": true
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/minecraft/servers/add")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&add_srv).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    // 3. GET /api/minecraft/servers
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/minecraft/servers")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let srv_list: Vec<MinecraftServer> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(srv_list.len(), 1);
    assert_eq!(srv_list[0].name, "Hypixel Network");
    assert_eq!(srv_list[0].ip, "mc.hypixel.net");

    // 4. DELETE /api/minecraft/servers/:id
    let srv_id = &srv_list[0].id;
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/minecraft/servers/{}", srv_id))
                .method("DELETE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    // Verify deleted
    assert!(db.get_minecraft_servers().await.unwrap().is_empty());

    // 5. POST /api/minecraft/accounts/scan
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/minecraft/accounts/scan")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let scan_val: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(scan_val["status"], "success");
}
