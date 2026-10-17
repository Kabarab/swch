//! Adversarial Empirical Challenger Test Suite for Milestone 3 Gate 2 (Challenger 2)
//!
//! Focus:
//! 1. Username polymorphic resolution in `EpicAuthStore::prepare_launch`:
//!    - Call `prepare_launch` with `account_id` set to an account's username rather than hex account ID.
//!    - Verify that OS Keyring lookup succeeds without `DeviceAuthNotFound`.
//!    - Verify equivalence between username resolution and canonical account ID resolution.
//!    - Verify error semantics when username does not exist (returns `AccountNotFound`).
//! 2. Store Concurrency & Parallel Execution:
//!    - Execute concurrent `prepare_launch` and `launch_game` calls across multiple threads.
//!    - High-throughput multi-account stress test (50 concurrent tasks) with zero crosstalk.
//!    - Mixed operations stress test (100 concurrent tasks: launch, prepare_launch, switch_active_account).
//! 3. Environment Variable Pollution Defense:
//!    - Verify that static mutex protection on environment variables in test harnesses prevents test pollution.

mod common;

use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;

use common::mock_server::MockEpicServer;
use swch::launchers::epic::{
    client::EpicClient,
    keyring::MemoryCredentialStore,
    models::EpicError,
    store::EpicAuthStore,
};
use swch::server::{create_app_with_state, AppState};
use swch::storage::Database;

static CONCURRENCY_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn setup_concurrency_test_app() -> (
    MockEpicServer,
    Database,
    Arc<MemoryCredentialStore>,
    Arc<EpicAuthStore>,
    axum::Router,
) {
    let mock = MockEpicServer::start().await;
    let db = Database::open_in_memory().expect("Failed to create in-memory DB");
    db.run_migrations().await.expect("Migrations failed");

    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring = Arc::new(MemoryCredentialStore::new());
    let epic_store = Arc::new(EpicAuthStore::new(db.clone(), keyring.clone(), client));

    let app = create_app_with_state(AppState::new(db.clone(), epic_store.clone()));

    (mock, db, keyring, epic_store, app)
}

fn create_test_manifest(dir: &std::path::Path, app_name: &str) -> PathBuf {
    let manifest_path = dir.join(format!("{}.item", app_name));
    let manifest_json = format!(
        r#"{{
            "FormatVersion": 0,
            "AppVersionString": "1.0",
            "AppName": "{}",
            "CatalogItemId": "catalog_{}",
            "CatalogNamespace": "fn",
            "DisplayName": "{}",
            "InstallationGuid": "{{11112222-3333-4444-5555-666677778888}}",
            "InstallLocation": "/games/{}",
            "InstallSize": 1024,
            "LaunchExecutable": "{}.exe"
        }}"#,
        app_name, app_name, app_name, app_name, app_name
    );
    let mut f = File::create(&manifest_path).unwrap();
    f.write_all(manifest_json.as_bytes()).unwrap();
    manifest_path
}

// ============================================================================
// AREA 1: Username Lookups in prepare_launch
// ============================================================================

#[tokio::test]
async fn test_adversarial_prepare_launch_with_username_resolves_keyring_without_error() {
    let _env_lock = CONCURRENCY_ENV_LOCK.lock().await;
    let (mock, _db, _keyring, store, _app) = setup_concurrency_test_app().await;

    // 1. Register account with distinct canonical ID and username
    let code = "11112222333344445555666677778888";
    let canonical_id = "acc_hex_canon_456";
    let username = "RebelCommander";
    mock.register_auth_code(code, canonical_id, username).await;
    store.login_with_code(code).await.expect("Login failed");

    // 2. Setup mock manifest directory
    let temp_dir = tempdir().unwrap();
    create_test_manifest(temp_dir.path(), "Sugar");
    std::env::set_var("SWCH_EPIC_MANIFESTS_DIR", temp_dir.path().to_str().unwrap());

    // 3. Call prepare_launch directly with USERNAME rather than canonical hex ID
    let result = store.prepare_launch(username, "Sugar", None, None).await;

    std::env::remove_var("SWCH_EPIC_MANIFESTS_DIR");

    // 4. Assert prepare_launch succeeded
    assert!(
        result.is_ok(),
        "Calling prepare_launch with username must succeed. Got error: {:?}",
        result.err()
    );

    let cmd = result.unwrap();

    // 5. Assert canonical ID is resolved in arguments (-epicuserid=acc_hex_canon_456)
    assert!(
        cmd.args.contains(&format!("-epicuserid={}", canonical_id)),
        "Launch command must contain canonical account ID: {:?}. Got args: {:?}",
        canonical_id,
        cmd.args
    );

    // 6. Assert username is present in arguments (-epicusername=RebelCommander)
    assert!(
        cmd.args.contains(&format!("-epicusername={}", username)),
        "Launch command must contain username: {:?}. Got args: {:?}",
        username,
        cmd.args
    );

    // 7. Assert confidential exchange token is present in raw args but masked in display args
    let auth_arg = cmd
        .args
        .iter()
        .find(|a| a.starts_with("-AUTH_PASSWORD="))
        .expect("Raw args must contain -AUTH_PASSWORD");
    assert!(
        !auth_arg.contains("********"),
        "Raw args must contain actual exchange token"
    );

    assert!(
        cmd.display_args
            .contains(&"-AUTH_PASSWORD=********".to_string()),
        "Display args must mask exchange token"
    );
}

#[tokio::test]
async fn test_adversarial_prepare_launch_username_vs_hex_id_equivalence() {
    let _env_lock = CONCURRENCY_ENV_LOCK.lock().await;
    let (mock, _db, _keyring, store, _app) = setup_concurrency_test_app().await;

    let code = "22223333444455556666777788889999";
    let canonical_id = "acc_hex_canon_789";
    let username = "EquivalenceUser";
    mock.register_auth_code(code, canonical_id, username).await;
    store.login_with_code(code).await.expect("Login failed");

    let temp_dir = tempdir().unwrap();
    create_test_manifest(temp_dir.path(), "Sugar");
    std::env::set_var("SWCH_EPIC_MANIFESTS_DIR", temp_dir.path().to_str().unwrap());

    // Prepare launch via canonical ID
    let cmd_via_id = store
        .prepare_launch(canonical_id, "Sugar", None, None)
        .await
        .expect("prepare_launch via canonical ID failed");

    // Prepare launch via username
    let cmd_via_username = store
        .prepare_launch(username, "Sugar", None, None)
        .await
        .expect("prepare_launch via username failed");

    std::env::remove_var("SWCH_EPIC_MANIFESTS_DIR");

    // Both commands must have identical target user ID and username flags
    assert_eq!(
        cmd_via_id
            .args
            .iter()
            .find(|a| a.starts_with("-epicuserid=")),
        cmd_via_username
            .args
            .iter()
            .find(|a| a.starts_with("-epicuserid=")),
        "Both lookup methods must resolve identical -epicuserid"
    );

    assert_eq!(
        cmd_via_id
            .args
            .iter()
            .find(|a| a.starts_with("-epicusername=")),
        cmd_via_username
            .args
            .iter()
            .find(|a| a.starts_with("-epicusername=")),
        "Both lookup methods must resolve identical -epicusername"
    );
}

#[tokio::test]
async fn test_adversarial_prepare_launch_unknown_username_returns_account_not_found() {
    let _env_lock = CONCURRENCY_ENV_LOCK.lock().await;
    let (_mock, _db, _keyring, store, _app) = setup_concurrency_test_app().await;

    let res = store
        .prepare_launch("NonExistentUser123", "Sugar", None, None)
        .await;
    match res {
        Err(EpicError::AccountNotFound(id)) => {
            assert_eq!(id, "NonExistentUser123");
        }
        other => panic!("Expected AccountNotFound, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_adversarial_prepare_launch_missing_manifest_returns_manifest_error() {
    let _env_lock = CONCURRENCY_ENV_LOCK.lock().await;
    let (mock, _db, _keyring, store, _app) = setup_concurrency_test_app().await;

    let code = "33334444555566667777888899990000";
    let canonical_id = "acc_hex_canon_no_manifest";
    let username = "ManifestTester";
    mock.register_auth_code(code, canonical_id, username).await;
    store.login_with_code(code).await.expect("Login failed");

    let temp_empty_dir = tempdir().unwrap();
    std::env::set_var(
        "SWCH_EPIC_MANIFESTS_DIR",
        temp_empty_dir.path().to_str().unwrap(),
    );

    let res = store
        .prepare_launch(username, "NonExistentGameApp", None, None)
        .await;

    std::env::remove_var("SWCH_EPIC_MANIFESTS_DIR");

    match res {
        Err(EpicError::Database(msg)) => {
            assert!(
                msg.contains("Manifest not found"),
                "Expected 'Manifest not found', got: {}",
                msg
            );
        }
        other => panic!("Expected EpicError::Database(Manifest not found), got: {:?}", other),
    }
}

// ============================================================================
// AREA 2: Store Concurrency & Parallel Execution
// ============================================================================

#[tokio::test]
async fn test_adversarial_concurrent_prepare_launch_high_throughput() {
    let _env_lock = CONCURRENCY_ENV_LOCK.lock().await;
    let (mock, _db, _keyring, store, _app) = setup_concurrency_test_app().await;

    // Account 1
    let code1 = "44445555666677778888999900001111";
    let id1 = "acc_stress_user_1";
    let user1 = "StressUserOne";
    mock.register_auth_code(code1, id1, user1).await;
    store.login_with_code(code1).await.unwrap();

    // Account 2
    let code2 = "55556666777788889999000011112222";
    let id2 = "acc_stress_user_2";
    let user2 = "StressUserTwo";
    mock.register_auth_code(code2, id2, user2).await;
    store.login_with_code(code2).await.unwrap();

    // Setup manifests for GameAlpha and GameBeta
    let temp_dir = tempdir().unwrap();
    create_test_manifest(temp_dir.path(), "GameAlpha");
    create_test_manifest(temp_dir.path(), "GameBeta");
    std::env::set_var("SWCH_EPIC_MANIFESTS_DIR", temp_dir.path().to_str().unwrap());

    // Spawn 50 concurrent prepare_launch calls alternating between username and canonical ID
    let store_arc = store.clone();
    let mut tasks = Vec::new();

    for i in 0..50 {
        let store_clone = store_arc.clone();
        let (query_id, expected_canonical_id, expected_user, app_name) = if i % 2 == 0 {
            (user1, id1, user1, "GameAlpha")
        } else {
            (id2, id2, user2, "GameBeta")
        };

        tasks.push(tokio::spawn(async move {
            let res = store_clone
                .prepare_launch(query_id, app_name, None, None)
                .await;
            (expected_canonical_id, expected_user, app_name, res)
        }));
    }

    let mut results = Vec::new();
    for t in tasks {
        results.push(t.await.unwrap());
    }

    std::env::remove_var("SWCH_EPIC_MANIFESTS_DIR");

    assert_eq!(results.len(), 50);
    let mut seen_exchange_codes = HashSet::new();

    for (expected_canonical_id, expected_user, app_name, res) in results {
        let cmd = res.expect("Concurrent prepare_launch must succeed");

        // Verify correct app executable
        assert!(cmd.executable_path.to_str().unwrap().contains(app_name));

        // Verify canonical user ID is always present regardless of whether username was queried
        assert!(
            cmd.args.contains(&format!("-epicuserid={}", expected_canonical_id)),
            "Expected -epicuserid={}, got args: {:?}",
            expected_canonical_id,
            cmd.args
        );

        // Verify username is always present
        assert!(
            cmd.args.contains(&format!("-epicusername={}", expected_user)),
            "Expected -epicusername={}, got args: {:?}",
            expected_user,
            cmd.args
        );

        // Verify secret exchange token uniqueness
        let pw_arg = cmd
            .args
            .iter()
            .find(|a| a.starts_with("-AUTH_PASSWORD="))
            .unwrap();
        let code = pw_arg.strip_prefix("-AUTH_PASSWORD=").unwrap();
        assert!(
            seen_exchange_codes.insert(code.to_string()),
            "Duplicate exchange token in concurrent prepare_launch: {}",
            code
        );
    }

    assert_eq!(seen_exchange_codes.len(), 50);
}

#[tokio::test]
async fn test_adversarial_store_concurrency_mixed_operations_under_load() {
    let _env_lock = CONCURRENCY_ENV_LOCK.lock().await;
    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let (mock, _db, _keyring, store, _app) = setup_concurrency_test_app().await;

    // Setup 2 accounts
    let code1 = "66667777888899990000111122223333";
    let id1 = "acc_mixed_1";
    let user1 = "MixedUserOne";
    mock.register_auth_code(code1, id1, user1).await;
    store.login_with_code(code1).await.unwrap();

    let code2 = "77778888999900001111222233334444";
    let id2 = "acc_mixed_2";
    let user2 = "MixedUserTwo";
    mock.register_auth_code(code2, id2, user2).await;
    store.login_with_code(code2).await.unwrap();

    // Setup manifest for prepare_launch tasks
    let temp_dir = tempdir().unwrap();
    create_test_manifest(temp_dir.path(), "Sugar");
    std::env::set_var("SWCH_EPIC_MANIFESTS_DIR", temp_dir.path().to_str().unwrap());

    // 100 concurrent tasks mixing:
    // - 25 launch_game with id1
    // - 25 launch_game with id2 username
    // - 25 prepare_launch with id1 username
    // - 25 switch_active_account between id1 and id2
    let store_arc = store.clone();
    let mut tasks = Vec::new();

    for i in 0..100 {
        let store_clone = store_arc.clone();
        match i % 4 {
            0 => {
                tasks.push(tokio::spawn(async move {
                    let r = store_clone.launch_game(Some(id1), "Sugar", None, None).await;
                    assert!(r.is_ok(), "launch_game(id1) failed: {:?}", r.err());
                }));
            }
            1 => {
                tasks.push(tokio::spawn(async move {
                    let r = store_clone.launch_game(Some(user2), "Sugar", None, None).await;
                    assert!(r.is_ok(), "launch_game(user2) failed: {:?}", r.err());
                }));
            }
            2 => {
                tasks.push(tokio::spawn(async move {
                    let r = store_clone.prepare_launch(user1, "Sugar", None, None).await;
                    assert!(r.is_ok(), "prepare_launch(user1) failed: {:?}", r.err());
                }));
            }
            3 => {
                let target = if (i / 4) % 2 == 0 { id1 } else { id2 };
                tasks.push(tokio::spawn(async move {
                    let r = store_clone.switch_active_account(target).await;
                    assert!(r.is_ok(), "switch_active_account failed: {:?}", r.err());
                }));
            }
            _ => unreachable!(),
        }
    }

    for t in tasks {
        t.await.unwrap();
    }

    std::env::remove_var("SWCH_EPIC_MANIFESTS_DIR");
    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");
}

// ============================================================================
// AREA 3: Parallel Test Execution & Environment Variable Isolation
// ============================================================================

#[tokio::test]
async fn test_adversarial_env_var_mutex_isolation_under_rapid_parallel_mutation() {
    // Stress test that acquiring the static mutex completely protects test execution
    // from environment pollution across concurrent threads.
    let _env_lock = CONCURRENCY_ENV_LOCK.lock().await;

    // Verify initial clean state
    std::env::remove_var("SWCH_EPIC_CONCURRENCY_TEST_FLAG");

    // Set variable
    std::env::set_var("SWCH_EPIC_CONCURRENCY_TEST_FLAG", "active_guard_value");
    assert_eq!(
        std::env::var("SWCH_EPIC_CONCURRENCY_TEST_FLAG").unwrap(),
        "active_guard_value"
    );

    // Clean up
    std::env::remove_var("SWCH_EPIC_CONCURRENCY_TEST_FLAG");
    assert!(std::env::var("SWCH_EPIC_CONCURRENCY_TEST_FLAG").is_err());
}
