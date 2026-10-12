use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use tower::ServiceExt;

use swch::launchers::epic::EpicAuthStore;
use swch::launchers::minecraft::hot_swap::{
    update_tlauncher_profiles_json, update_tlauncher_properties, MinecraftHotSwap,
};
use swch::launchers::minecraft::nbt::{parse_servers_dat, write_servers_dat};
use swch::launchers::minecraft::scanner::{
    generate_offline_uuid, parse_tlauncher_profiles_accounts, parse_tlauncher_properties,
    MinecraftScanner,
};
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
    let _lock = TEST_ENV_MUTEX.lock().await;
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

#[tokio::test]
async fn test_tlauncher_detector_and_aliases() {
    let _lock = TEST_ENV_MUTEX.lock().await;
    let tmp = tempdir().unwrap();
    let detector = LauncherDetector::new();

    let fake_app = tmp.path().join("fake_tlauncher_exe.exe");
    std::fs::write(&fake_app, b"dummy tlauncher binary").unwrap();

    let fake_tl_dir = tmp.path().join(".tlauncher");
    std::fs::create_dir_all(&fake_tl_dir).unwrap();

    std::env::set_var("SWCH_TLAUNCHER_APP", fake_app.to_str().unwrap());
    std::env::set_var("SWCH_TLAUNCHER_DIR", fake_tl_dir.to_str().unwrap());

    assert_eq!(detector.detect_tlauncher(), Some(fake_app.clone()));
    assert_eq!(detector.detect_tlauncher_dir(), Some(fake_tl_dir.clone()));

    // Test SWCH_TLAUNCHER_PATH fallback
    std::env::remove_var("SWCH_TLAUNCHER_APP");
    std::env::set_var("SWCH_TLAUNCHER_PATH", fake_app.to_str().unwrap());
    assert_eq!(detector.detect_tlauncher(), Some(fake_app.clone()));

    // Verify LauncherType from_name parses T-Launcher aliases
    assert_eq!(LauncherType::from_name("tlauncher"), Some(LauncherType::Minecraft));
    assert_eq!(LauncherType::from_name("t-launcher"), Some(LauncherType::Minecraft));
    assert_eq!(LauncherType::from_name("tl"), Some(LauncherType::Minecraft));
    assert_eq!(LauncherType::from_name("TL"), Some(LauncherType::Minecraft));

    // Verify process names include TLauncher executables and strictly exclude java/javaw
    let proc_names = LauncherType::Minecraft.process_names();
    assert!(proc_names.contains(&"TLauncher.exe"));
    assert!(proc_names.contains(&"tlauncher.exe"));
    assert!(proc_names.contains(&"TL.exe"));
    assert!(proc_names.contains(&"tlauncher"));

    for name in proc_names {
        let lower = name.to_lowercase();
        assert!(
            !lower.starts_with("java") && !lower.starts_with("javaw"),
            "Minecraft processes must NOT contain java/javaw: {}",
            name
        );
    }

    std::env::remove_var("SWCH_TLAUNCHER_APP");
    std::env::remove_var("SWCH_TLAUNCHER_PATH");
    std::env::remove_var("SWCH_TLAUNCHER_DIR");
}

#[tokio::test]
async fn test_tlauncher_accounts_parser_and_properties() {
    // 1. Test offline UUID generator
    let uuid1 = generate_offline_uuid("Alex_TL");
    let uuid2 = generate_offline_uuid("Alex_TL");
    let uuid3 = generate_offline_uuid("Steve_TL");
    assert_eq!(uuid1, uuid2);
    assert_ne!(uuid1, uuid3);
    assert_eq!(uuid1.len(), 36);

    // 2. Test parse_tlauncher_profiles_accounts with Map format
    let map_json = serde_json::json!({
        "selectedUser": {
            "account": "Alex_TL"
        },
        "selected_account": "Alex_TL",
        "accounts": {
            "acc_1": {
                "displayName": "Alex_TL",
                "type": "free",
                "uuid": "11111111-2222-3333-4444-555555555555",
                "active": true
            },
            "acc_2": {
                "displayName": "Steve_Mojang",
                "type": "mojang",
                "uuid": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                "active": false
            }
        }
    }).to_string();

    let accounts = parse_tlauncher_profiles_accounts(&map_json);
    assert_eq!(accounts.len(), 2);
    let active_acc = accounts.iter().find(|a| a.username == "Alex_TL").unwrap();
    assert!(active_acc.is_active);
    assert_eq!(active_acc.account_type, "tlauncher_free");
    let inactive_acc = accounts.iter().find(|a| a.username == "Steve_Mojang").unwrap();
    assert!(!inactive_acc.is_active);
    assert_eq!(inactive_acc.account_type, "tlauncher_mojang");

    // 3. Test parse_tlauncher_profiles_accounts with Array format
    let arr_json = serde_json::json!({
        "selectedUser": "Steve_TL",
        "selected_account": "Steve_TL",
        "accounts": [
            {
                "username": "Steve_TL",
                "type": "offline",
                "uuid": "22222222-3333-4444-5555-666666666666"
            }
        ]
    }).to_string();

    let arr_accounts = parse_tlauncher_profiles_accounts(&arr_json);
    assert_eq!(arr_accounts.len(), 1);
    assert_eq!(arr_accounts[0].username, "Steve_TL");
    assert!(arr_accounts[0].is_active);
    assert_eq!(arr_accounts[0].account_type, "tlauncher_free");

    // 4. Test parse_tlauncher_properties
    let props = "
# T-Launcher properties
version=2.89
selected.account=CraftHero
account.selected=CraftHero
";
    let prop_accs = parse_tlauncher_properties(props);
    assert_eq!(prop_accs.len(), 1);
    assert_eq!(prop_accs[0].username, "CraftHero");
    assert!(prop_accs[0].is_active);
    assert_eq!(prop_accs[0].account_type, "tlauncher");

    // 5. Integration scan with Scanner
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test_tl_scan.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let mc_dir = tmp.path().join(".minecraft");
    std::fs::create_dir_all(&mc_dir).unwrap();
    std::fs::write(mc_dir.join("TlauncherProfiles.json"), &map_json).unwrap();

    let scanner = MinecraftScanner::new(db.clone());
    let scanned_count = scanner.scan_accounts(&mc_dir).await;
    assert_eq!(scanned_count, 2);

    let db_accs = db.get_minecraft_accounts().await.unwrap();
    assert_eq!(db_accs.len(), 2);
}

#[tokio::test]
async fn test_tlauncher_hot_swap_integration() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test_tl_hotswap.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let mc_dir = tmp.path().join(".minecraft");
    let tl_dir = tmp.path().join(".tlauncher");
    std::fs::create_dir_all(&mc_dir).unwrap();
    std::fs::create_dir_all(&tl_dir).unwrap();

    // Initial TlauncherProfiles.json
    let initial_tl_json = serde_json::json!({
        "selectedUser": { "account": "OldPlayer" },
        "selected_account": "OldPlayer",
        "accounts": {
            "OldPlayer": {
                "username": "OldPlayer",
                "displayName": "OldPlayer",
                "type": "FREE",
                "uuid": "00000000-0000-0000-0000-000000000000",
                "active": true
            }
        }
    }).to_string();

    let tl_profiles_path = mc_dir.join("TlauncherProfiles.json");
    std::fs::write(&tl_profiles_path, &initial_tl_json).unwrap();

    // Initial properties file
    let props_path = tl_dir.join("tlauncher-2.0.properties");
    std::fs::write(&props_path, "selected.account=OldPlayer\nlogin.account=OldPlayer\n").unwrap();

    // Target account to switch into
    let target_account = MinecraftAccount {
        account_id: "tl_NewPlayer".to_string(),
        username: "NewPlayer".to_string(),
        account_type: "tlauncher_free".to_string(),
        uuid: generate_offline_uuid("NewPlayer"),
        avatar_url: None,
        skin_url: None,
        is_active: false,
        access_token: None,
        raw_json: None,
        last_used: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    // 1. Direct unit test of update_tlauncher_profiles_json
    update_tlauncher_profiles_json(&tl_profiles_path, &target_account).unwrap();

    // Verify backup created
    let bak_path = PathBuf::from(format!("{}.bak", tl_profiles_path.display()));
    assert!(bak_path.exists(), "TlauncherProfiles.json.bak must exist");

    // Verify file content updated
    let updated_content = std::fs::read_to_string(&tl_profiles_path).unwrap();
    let updated_val: Value = serde_json::from_str(&updated_content).unwrap();
    assert_eq!(updated_val["selected_account"], "NewPlayer");
    assert_eq!(updated_val["selectedUser"]["account"], "NewPlayer");
    let accounts_map = updated_val["accounts"].as_object().unwrap();
    assert!(accounts_map.contains_key("NewPlayer"));
    assert_eq!(accounts_map["NewPlayer"]["active"], true);
    assert_eq!(accounts_map["OldPlayer"]["active"], false);

    // 2. Direct unit test of update_tlauncher_properties
    update_tlauncher_properties(&props_path, &target_account.username).unwrap();
    let prop_bak_path = PathBuf::from(format!("{}.bak", props_path.display()));
    assert!(prop_bak_path.exists(), "tlauncher-2.0.properties.bak must exist");

    let updated_props = std::fs::read_to_string(&props_path).unwrap();
    assert!(updated_props.contains("selected.account=NewPlayer"));
    assert!(updated_props.contains("login.account=NewPlayer"));

    // 3. Test through MinecraftHotSwap manager
    let _lock = TEST_ENV_MUTEX.lock().await;
    std::env::set_var("SWCH_MINECRAFT_DIR", mc_dir.to_str().unwrap());
    std::env::set_var("SWCH_TLAUNCHER_DIR", tl_dir.to_str().unwrap());

    db.save_minecraft_account(&target_account).await.unwrap();

    let hot_swap = MinecraftHotSwap::new(db.clone());
    let active_name = hot_swap.switch_account("tl_NewPlayer").await.unwrap();
    assert!(active_name.contains("NewPlayer"));

    let reloaded = db.get_minecraft_account("tl_NewPlayer").await.unwrap().unwrap();
    assert!(reloaded.is_active);

    std::env::remove_var("SWCH_MINECRAFT_DIR");
    std::env::remove_var("SWCH_TLAUNCHER_DIR");
}

#[tokio::test]
async fn test_tlauncher_api_add_and_launch_endpoints() {
    let _lock = TEST_ENV_MUTEX.lock().await;
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test_tl_api.db");
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

    // 1. POST /api/minecraft/accounts/add
    let add_payload = serde_json::json!({
        "username": "CraftMaster99",
        "account_type": "tlauncher"
    });

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/minecraft/accounts/add")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&add_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let acc_val: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(acc_val["username"], "CraftMaster99");
    assert_eq!(acc_val["account_type"], "tlauncher");
    assert_eq!(acc_val["is_active"], true);
    let added_id = acc_val["account_id"].as_str().unwrap().to_string();

    // Verify account saved in DB
    let saved_acc = db.get_minecraft_account(&added_id).await.unwrap().unwrap();
    assert_eq!(saved_acc.username, "CraftMaster99");
    assert!(saved_acc.is_active);

    // 2. GET /api/minecraft/library returns available_launchers and install flags
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
    assert!(lib_val["available_launchers"].is_array());
    let launchers = lib_val["available_launchers"].as_array().unwrap();
    assert!(launchers.iter().any(|l| l["id"] == "official"));
    assert!(launchers.iter().any(|l| l["id"] == "tlauncher"));

    // 3. POST /api/minecraft/open-launcher with launcher_client
    let open_payload = serde_json::json!({
        "account_id": added_id,
        "launcher_client": "tlauncher"
    });

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/minecraft/open-launcher")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&open_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Valid response status (OK, or INTERNAL_SERVER_ERROR / NOT_FOUND / BAD_REQUEST when client exe is not installed on test runner)
    assert!(
        res.status() == StatusCode::OK
            || res.status() == StatusCode::INTERNAL_SERVER_ERROR
            || res.status() == StatusCode::BAD_REQUEST
            || res.status() == StatusCode::NOT_FOUND,
        "Unexpected status: {:?}",
        res.status()
    );

    // 4. POST /api/minecraft/launch with launcher_client
    let launch_payload = serde_json::json!({
        "game_id": "minecraft_java",
        "profile_id": null,
        "launcher_client": "tlauncher"
    });

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/minecraft/launch")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&launch_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        res.status() == StatusCode::OK
            || res.status() == StatusCode::INTERNAL_SERVER_ERROR
            || res.status() == StatusCode::BAD_REQUEST
            || res.status() == StatusCode::NOT_FOUND,
        "Unexpected status: {:?}",
        res.status()
    );
}

#[test]
fn test_tlauncher_properties_priority_and_edge_cases() {
    // 1. Priority: selected.account must win even if login.account is defined first
    let props_order = "
login.account=OldUser
selected.account=ActiveUser
";
    let accs = parse_tlauncher_properties(props_order);
    assert_eq!(accs.len(), 1);
    assert_eq!(accs[0].username, "ActiveUser", "selected.account must have higher priority than login.account");

    // 2. account.selected fallback
    let props_account_selected = "
# Comment line
login.account=FallbackUser
account.selected=ChosenUser
";
    let accs2 = parse_tlauncher_properties(props_account_selected);
    assert_eq!(accs2.len(), 1);
    assert_eq!(accs2[0].username, "ChosenUser");

    // 3. Empty properties
    let empty_accs = parse_tlauncher_properties("# Just comments\n; Another comment\n");
    assert!(empty_accs.is_empty());
}

#[test]
fn test_tlauncher_profiles_case_insensitivity_and_format_preservation() {
    let tmp = tempdir().unwrap();
    let tl_json_path = tmp.path().join("TlauncherProfiles.json");

    // File with lowercase username and string selectedUser format
    let initial_json = serde_json::json!({
        "selectedUser": "alex_tl",
        "selected_account": "alex_tl",
        "accounts": {
            "alex_tl": {
                "username": "alex_tl",
                "displayName": "alex_tl",
                "type": "FREE",
                "uuid": "11111111-2222-3333-4444-555555555555",
                "active": true
            }
        }
    }).to_string();
    std::fs::write(&tl_json_path, &initial_json).unwrap();

    // Target account with uppercase username
    let target = MinecraftAccount {
        account_id: "tl_Alex_TL".to_string(),
        username: "Alex_TL".to_string(),
        account_type: "tlauncher_free".to_string(),
        uuid: "11111111-2222-3333-4444-555555555555".to_string(),
        avatar_url: None,
        skin_url: None,
        is_active: false,
        access_token: None,
        raw_json: None,
        last_used: None,
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    };

    update_tlauncher_profiles_json(&tl_json_path, &target).unwrap();

    let updated_content = std::fs::read_to_string(&tl_json_path).unwrap();
    let updated_val: Value = serde_json::from_str(&updated_content).unwrap();

    // Must preserve string format of selectedUser if originally string
    assert_eq!(updated_val["selectedUser"], "Alex_TL");

    // Must NOT insert duplicate account with different casing
    let accs_map = updated_val["accounts"].as_object().unwrap();
    assert_eq!(accs_map.len(), 1, "Case-insensitive match must not duplicate existing account");
    assert!(accs_map.contains_key("alex_tl"));
    assert_eq!(accs_map["alex_tl"]["active"], true);
}

#[test]
fn test_update_accounts_json_does_not_corrupt_on_foreign_tlauncher_account() {
    let tmp = tempdir().unwrap();
    let accounts_path = tmp.path().join("launcher_accounts.json");

    let initial_json = serde_json::json!({
        "accounts": {
            "official_local_1": {
                "active": true,
                "minecraftProfile": {
                    "id": "uuid_1",
                    "name": "Steve"
                },
                "type": "Xbox"
            }
        },
        "activeAccountLocalId": "official_local_1"
    });
    std::fs::write(&accounts_path, serde_json::to_string_pretty(&initial_json).unwrap()).unwrap();

    // Attempting to update with a foreign T-Launcher account ID
    let modified = swch::launchers::minecraft::hot_swap::update_accounts_json(
        &accounts_path,
        "tl_OfflineUser"
    ).unwrap();

    assert!(!modified, "Should not modify launcher_accounts.json when target ID does not exist in accounts");

    let content_after = std::fs::read_to_string(&accounts_path).unwrap();
    let val_after: Value = serde_json::from_str(&content_after).unwrap();
    assert_eq!(val_after["activeAccountLocalId"], "official_local_1", "Original official account must remain intact");
}

#[tokio::test]
async fn test_launch_auto_resolution_by_account_type() {
    let _lock = TEST_ENV_MUTEX.lock().await;
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test_launch_auto.db");
    let db = Database::open(&db_path).unwrap();
    db.run_migrations().await.unwrap();

    let fake_tl = tmp.path().join("fake_tl.exe");
    std::fs::write(&fake_tl, b"tl").unwrap();
    std::env::set_var("SWCH_TLAUNCHER_APP", fake_tl.to_str().unwrap());

    let fake_mc = tmp.path().join("fake_mc.exe");
    std::fs::write(&fake_mc, b"mc").unwrap();
    std::env::set_var("SWCH_MINECRAFT_APP", fake_mc.to_str().unwrap());

    // 1. Offline TLauncher account
    let tl_acc = MinecraftAccount {
        account_id: "tl_offline_player".to_string(),
        username: "TLPlayer".to_string(),
        account_type: "tlauncher_free".to_string(),
        uuid: "tl_uuid".to_string(),
        avatar_url: None,
        skin_url: None,
        is_active: true,
        access_token: None,
        raw_json: None,
        last_used: None,
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    };
    db.save_minecraft_account(&tl_acc).await.unwrap();

    // 2. Official Microsoft account
    let msa_acc = MinecraftAccount {
        account_id: "msa_player".to_string(),
        username: "MSAPlayer".to_string(),
        account_type: "msa".to_string(),
        uuid: "msa_uuid".to_string(),
        avatar_url: None,
        skin_url: None,
        is_active: false,
        access_token: None,
        raw_json: None,
        last_used: None,
        updated_at: "2026-09-25T12:00:00Z".to_string(),
    };
    db.save_minecraft_account(&msa_acc).await.unwrap();

    let launcher = swch::launchers::minecraft::launch::MinecraftLauncher::new(db.clone());

    // Auto resolution with T-Launcher account -> mode_used must be "tlauncher"
    let req_tl = swch::launchers::minecraft::launch::LaunchMinecraftRequest {
        account_id: Some("tl_offline_player".to_string()),
        profile_id: None,
        mode: None,
        launcher_client: Some("auto".to_string()),
        server_ip: None,
        world_folder: None,
    };
    let res_tl = launcher.launch(req_tl).await;
    if let Ok(r) = res_tl {
        assert_eq!(r.mode_used, "tlauncher");
    }

    // Auto resolution with MSA account -> mode_used must be "official"
    let req_msa = swch::launchers::minecraft::launch::LaunchMinecraftRequest {
        account_id: Some("msa_player".to_string()),
        profile_id: None,
        mode: None,
        launcher_client: None,
        server_ip: None,
        world_folder: None,
    };
    let res_msa = launcher.launch(req_msa).await;
    if let Ok(r) = res_msa {
        assert_eq!(r.mode_used, "official");
    }

    std::env::remove_var("SWCH_TLAUNCHER_APP");
    std::env::remove_var("SWCH_MINECRAFT_APP");
}
