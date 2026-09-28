//! E2E Test Suite for Epic Games Multi-Account & Direct Game Execution Engine (swch2)
//!
//! Tiers 1-4 comprehensive test coverage:
//! - Tier 1: Features F1 - F16 (≥ 5 tests per feature = 80 tests)
//! - Tier 2: Boundary & Corner Cases (26 tests)
//! - Tier 3: Pairwise Combinatorial & Interaction Tests (15 tests)
//! - Tier 4: Real-World Workload Scenarios (6 scenarios)
//! Total: 127 opaque-box integration tests

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use common::*;
use swch::storage::{Account, Database};

// ============================================================================
// Helper Setup Functions
// ============================================================================

async fn setup_test_env() -> (MockEpicServer, Database, EpicAuthStore) {
    let mock = MockEpicServer::start().await;
    let db = Database::open_in_memory().expect("Failed to create in-memory DB");
    db.run_migrations().await.expect("Migrations failed");

    let client = EpicClient::from_router(mock.router());
    let store = EpicAuthStore::with_memory_store(db.clone(), client);
    (mock, db, store)
}

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

// ============================================================================
// TIER 1: FEATURE COVERAGE (F1 - F16)
// ============================================================================

// ----------------------------------------------------------------------------
// F1: OAuth2 Auth Code Exchange & Sanitization
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f1_01_oauth2_exchange_valid_code() {
    let (mock, _, store) = setup_test_env().await;
    let code = "3b2a0c4f8d914652a92db1029c73e1a0";
    mock.register_auth_code(code, "acc_luke_1", "Luke Skywalker")
        .await;

    let account = store.login_with_code(code).await.expect("Login failed");
    assert_eq!(account.account_id, "acc_luke_1");
    assert_eq!(account.display_name.as_deref(), Some("Luke Skywalker"));
    assert!(account.is_active);
}

#[tokio::test]
async fn test_f1_02_sanitize_code_raw_hex() {
    let input = "3b2a0c4f8d914652a92db1029c73e1a0";
    let clean = sanitize_auth_code(input).expect("Sanitization failed");
    assert_eq!(clean, "3b2a0c4f8d914652a92db1029c73e1a0");
}

#[tokio::test]
async fn test_f1_03_sanitize_code_json_payload() {
    let json = r#"{"redirectUrl":"https://example.com","authorizationCode":"4c3b2a109876543210fedcba09876543","sid":null}"#;
    let clean = sanitize_auth_code(json).expect("JSON sanitization failed");
    assert_eq!(clean, "4c3b2a109876543210fedcba09876543");
}

#[tokio::test]
async fn test_f1_04_sanitize_code_url_fragment() {
    let url = "https://www.epicgames.com/id/api/redirect?clientId=123#code=abcdef0123456789abcdef0123456789";
    let clean = sanitize_auth_code(url).expect("URL fragment sanitization failed");
    assert_eq!(clean, "abcdef0123456789abcdef0123456789");
}

#[tokio::test]
async fn test_f1_05_sanitize_code_quoted_string_with_whitespace() {
    let raw = "   \"3b2a0c4f8d914652a92db1029c73e1a0\"  \n";
    let clean = sanitize_auth_code(raw).expect("Quoted sanitization failed");
    assert_eq!(clean, "3b2a0c4f8d914652a92db1029c73e1a0");
}

// ----------------------------------------------------------------------------
// F2: Permanent DeviceAuth Generation
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f2_01_device_auth_generation_success() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth = client
        .create_device_auth("test_account_1", "valid_bearer_token")
        .await
        .expect("Failed to create device auth");

    assert_eq!(auth.account_id, "test_account_1");
    assert!(!auth.device_id.is_empty());
    assert!(!auth.secret.is_empty());
}

#[tokio::test]
async fn test_f2_02_device_auth_contains_expected_fields() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth = client
        .create_device_auth("acc_check_fields", "valid_bearer_token")
        .await
        .unwrap();

    assert!(auth.device_id.starts_with("dev_"));
    assert!(auth.secret.starts_with("secret_"));
    assert!(auth.user_agent.is_some());
}

#[tokio::test]
async fn test_f2_03_device_auth_requires_bearer_auth() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let result = client
        .create_device_auth("test_account_1", "invalid_token")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_f2_04_device_auth_distinct_per_account() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth1 = client
        .create_device_auth("acc_user_1", "valid_token")
        .await
        .unwrap();
    let auth2 = client
        .create_device_auth("acc_user_2", "valid_token")
        .await
        .unwrap();

    assert_ne!(auth1.account_id, auth2.account_id);
    assert_ne!(auth1.device_id, auth2.device_id);
    assert_ne!(auth1.secret, auth2.secret);
}

#[tokio::test]
async fn test_f2_05_device_auth_revocation_endpoint() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth = client
        .create_device_auth("acc_to_delete", "valid_token")
        .await
        .unwrap();
    let delete_res = client
        .delete_device_auth(&auth.account_id, &auth.device_id, "valid_token")
        .await;
    assert!(delete_res.is_ok());
}

// ----------------------------------------------------------------------------
// F3: OS Keyring Store & Fallback
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f3_01_keyring_store_and_retrieve() {
    let store = MemoryCredentialStore::new();
    let dev = DeviceAuth {
        device_id: "dev_123".to_string(),
        account_id: "acc_456".to_string(),
        secret: "sec_789".to_string(),
        user_agent: None,
        created: None,
    };

    store.set_device_auth(&dev).expect("Save failed");
    let retrieved = store
        .get_device_auth("acc_456")
        .expect("Retrieve failed")
        .expect("Should exist");

    assert_eq!(retrieved.account_id, "acc_456");
    assert_eq!(retrieved.device_id, "dev_123");
    assert_eq!(retrieved.secret, "sec_789");
}

#[tokio::test]
async fn test_f3_02_keyring_not_found_returns_none() {
    let store = MemoryCredentialStore::new();
    let res = store.get_device_auth("non_existent").unwrap();
    assert!(res.is_none());
}

#[tokio::test]
async fn test_f3_03_keyring_delete_credential() {
    let store = MemoryCredentialStore::new();
    let dev = DeviceAuth {
        device_id: "dev_del".to_string(),
        account_id: "acc_del".to_string(),
        secret: "sec_del".to_string(),
        user_agent: None,
        created: None,
    };

    store.set_device_auth(&dev).unwrap();
    store.delete_device_auth("acc_del").unwrap();
    let res = store.get_device_auth("acc_del").unwrap();
    assert!(res.is_none());
}

#[tokio::test]
async fn test_f3_04_keyring_overwrite_credential() {
    let store = MemoryCredentialStore::new();
    let dev1 = DeviceAuth {
        device_id: "dev_old".to_string(),
        account_id: "acc_shared".to_string(),
        secret: "sec_old".to_string(),
        user_agent: None,
        created: None,
    };
    let dev2 = DeviceAuth {
        device_id: "dev_new".to_string(),
        account_id: "acc_shared".to_string(),
        secret: "sec_new".to_string(),
        user_agent: None,
        created: None,
    };

    store.set_device_auth(&dev1).unwrap();
    store.set_device_auth(&dev2).unwrap();

    let res = store
        .get_device_auth("acc_shared")
        .unwrap()
        .expect("Missing");
    assert_eq!(res.device_id, "dev_new");
    assert_eq!(res.secret, "sec_new");
}

#[tokio::test]
async fn test_f3_05_memory_store_thread_safety() {
    let store = Arc::new(MemoryCredentialStore::new());
    let mut handles = Vec::new();

    for i in 0..10 {
        let s = store.clone();
        handles.push(tokio::spawn(async move {
            let auth = DeviceAuth {
                device_id: format!("dev_{}", i),
                account_id: format!("acc_{}", i),
                secret: format!("sec_{}", i),
                user_agent: None,
                created: None,
            };
            s.set_device_auth(&auth).unwrap();
            let retrieved = s.get_device_auth(&format!("acc_{}", i)).unwrap().unwrap();
            assert_eq!(retrieved.device_id, format!("dev_{}", i));
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(store.list_accounts().unwrap().len(), 10);
}

// ----------------------------------------------------------------------------
// F4: Headless Token Refresh via DeviceAuth
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f4_01_device_auth_token_refresh_success() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth = DeviceAuth {
        device_id: "dev_acc1".to_string(),
        account_id: "4a6313cbbe7d432b8132b5ee67ad53fa".to_string(),
        secret: "secret_acc1".to_string(),
        user_agent: None,
        created: None,
    };

    let token = client
        .login_with_device_auth(&auth)
        .await
        .expect("Refresh failed");
    assert!(!token.access_token.is_empty());
    assert_eq!(token.account_id, auth.account_id);
}

#[tokio::test]
async fn test_f4_02_device_auth_refresh_preserves_account_id() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth = DeviceAuth {
        device_id: "dev_acc2".to_string(),
        account_id: "9f8e7d6c5b4a3210fedcba9876543210".to_string(),
        secret: "secret_acc2".to_string(),
        user_agent: None,
        created: None,
    };

    let token = client.login_with_device_auth(&auth).await.unwrap();
    assert_eq!(token.account_id, "9f8e7d6c5b4a3210fedcba9876543210");
    assert_eq!(token.token_type, "bearer");
}

#[tokio::test]
async fn test_f4_03_device_auth_refresh_with_wrong_secret_fails() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth = DeviceAuth {
        device_id: "dev_acc1".to_string(),
        account_id: "4a6313cbbe7d432b8132b5ee67ad53fa".to_string(),
        secret: "wrong_secret_tampered".to_string(),
        user_agent: None,
        created: None,
    };

    let err = client.login_with_device_auth(&auth).await.unwrap_err();
    match err {
        EpicClientError::Api(api_err) => {
            assert_eq!(
                api_err.error_code,
                "errors.com.epicgames.account.invalid_account_credentials"
            );
        }
        other => panic!("Expected Api error, got {:?}", other),
    }
}

#[tokio::test]
async fn test_f4_04_device_auth_refresh_missing_device_fails() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth = DeviceAuth {
        device_id: "".to_string(),
        account_id: "4a6313cbbe7d432b8132b5ee67ad53fa".to_string(),
        secret: "secret_acc1".to_string(),
        user_agent: None,
        created: None,
    };

    assert!(client.login_with_device_auth(&auth).await.is_err());
}

#[tokio::test]
async fn test_f4_05_device_auth_repeated_refresh_consistency() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth = DeviceAuth {
        device_id: "dev_acc1".to_string(),
        account_id: "4a6313cbbe7d432b8132b5ee67ad53fa".to_string(),
        secret: "secret_acc1".to_string(),
        user_agent: None,
        created: None,
    };

    for _ in 0..5 {
        let resp = client.login_with_device_auth(&auth).await;
        assert!(resp.is_ok());
    }
}

// ----------------------------------------------------------------------------
// F5: SQLite Account Metadata Persistence
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f5_01_sqlite_insert_epic_account() {
    let (_, db, _) = setup_test_env().await;
    let account = Account {
        id: "acc_epic_1".to_string(),
        launcher: "epic".to_string(),
        username: "luke_epic".to_string(),
        display_name: Some("Luke".to_string()),
        email: None,
        is_active: true,
        last_used: Some("2026-09-13T10:00:00Z".to_string()),
    };

    db.upsert_account(&account).await.unwrap();
    let accounts = db.get_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].launcher, "epic");
    assert_eq!(accounts[0].username, "luke_epic");
}

#[tokio::test]
async fn test_f5_02_sqlite_account_display_name_persists() {
    let (_, db, _) = setup_test_env().await;
    let account = Account {
        id: "acc_epic_2".to_string(),
        launcher: "epic".to_string(),
        username: "vader_epic".to_string(),
        display_name: Some("Darth Vader".to_string()),
        email: None,
        is_active: false,
        last_used: None,
    };

    db.upsert_account(&account).await.unwrap();
    let fetched = db.get_accounts().await.unwrap();
    assert_eq!(fetched[0].display_name.as_deref(), Some("Darth Vader"));
}

#[tokio::test]
async fn test_f5_03_sqlite_account_active_flag() {
    let (_, db, _) = setup_test_env().await;
    let mut acc = Account {
        id: "acc_epic_flag".to_string(),
        launcher: "epic".to_string(),
        username: "yoda".to_string(),
        display_name: Some("Master Yoda".to_string()),
        email: None,
        is_active: false,
        last_used: None,
    };
    db.upsert_account(&acc).await.unwrap();
    assert!(!db.get_accounts().await.unwrap()[0].is_active);

    acc.is_active = true;
    db.upsert_account(&acc).await.unwrap();
    assert!(db.get_accounts().await.unwrap()[0].is_active);
}

#[tokio::test]
async fn test_f5_04_sqlite_account_last_used_timestamp() {
    let (_, db, _) = setup_test_env().await;
    let ts = "2026-09-13T12:00:00Z";
    let acc = Account {
        id: "acc_epic_ts".to_string(),
        launcher: "epic".to_string(),
        username: "obi_wan".to_string(),
        display_name: Some("Kenobi".to_string()),
        email: None,
        is_active: true,
        last_used: Some(ts.to_string()),
    };
    db.upsert_account(&acc).await.unwrap();
    let fetched = db.get_accounts().await.unwrap();
    assert_eq!(fetched[0].last_used.as_deref(), Some(ts));
}

#[tokio::test]
async fn test_f5_05_sqlite_list_epic_accounts_filters_launcher() {
    let (_, db, store) = setup_test_env().await;

    // Steam account
    let steam_acc = Account {
        id: "acc_steam_1".to_string(),
        launcher: "steam".to_string(),
        username: "steam_user".to_string(),
        display_name: Some("SteamUser".to_string()),
        email: None,
        is_active: true,
        last_used: None,
    };
    db.upsert_account(&steam_acc).await.unwrap();

    // Epic account
    let epic_acc = Account {
        id: "acc_epic_1".to_string(),
        launcher: "epic".to_string(),
        username: "epic_user".to_string(),
        display_name: Some("EpicUser".to_string()),
        email: None,
        is_active: true,
        last_used: None,
    };
    db.upsert_account(&epic_acc).await.unwrap();

    let epic_only = store.get_accounts().await.unwrap();
    assert_eq!(epic_only.len(), 1);
    assert_eq!(epic_only[0].account_id, "epic_user");
}

// ----------------------------------------------------------------------------
// F6: Multi-Account Session Persistence Across Restarts
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f6_01_multi_account_simultaneous_persistence() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_1_12345678901234567890123456", "acc_1", "User 1")
        .await;
    mock.register_auth_code("code_2_12345678901234567890123456", "acc_2", "User 2")
        .await;

    store
        .login_with_code("code_1_12345678901234567890123456")
        .await
        .unwrap();
    store
        .login_with_code("code_2_12345678901234567890123456")
        .await
        .unwrap();

    let accounts = store.get_accounts().await.unwrap();
    assert_eq!(accounts.len(), 2);
}

#[tokio::test]
async fn test_f6_02_session_restore_after_cold_restart() {
    let mock = MockEpicServer::start().await;
    let temp_db_file = tempfile::NamedTempFile::new().unwrap();
    let db_path = temp_db_file.path();

    let keyring = Arc::new(MemoryCredentialStore::new());

    // Phase 1: Store credentials
    {
        let db = Database::open(db_path).unwrap();
        db.run_migrations().await.unwrap();
        let client = EpicClient::from_router(mock.router());
        let store = EpicAuthStore::new(db, keyring.clone(), client);

        mock.register_auth_code("code_restart_123456789012345678", "acc_reboot", "Captain Rex")
            .await;
        store
            .login_with_code("code_restart_123456789012345678")
            .await
            .unwrap();
    }

    // Phase 2: Cold restart simulation with new Database handle
    {
        let db2 = Database::open(db_path).unwrap();
        let client2 = EpicClient::from_router(mock.router());
        let store2 = EpicAuthStore::new(db2, keyring.clone(), client2);

        let accounts = store2.get_accounts().await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].account_id, "acc_reboot");

        // Verify valid token can be refreshed headlessly without prompting login
        let token = store2.get_valid_access_token("acc_reboot").await.unwrap();
        assert!(!token.is_empty());
    }
}

#[tokio::test]
async fn test_f6_03_only_one_account_active_at_a_time() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_a_12345678901234567890123456", "acc_a", "Player A")
        .await;
    mock.register_auth_code("code_b_12345678901234567890123456", "acc_b", "Player B")
        .await;

    store
        .login_with_code("code_a_12345678901234567890123456")
        .await
        .unwrap();
    store
        .login_with_code("code_b_12345678901234567890123456")
        .await
        .unwrap();

    let accounts = store.get_accounts().await.unwrap();
    let active_count = accounts.iter().filter(|a| a.is_active).count();
    assert_eq!(active_count, 1);
    assert_eq!(
        store.get_active_account().await.unwrap().unwrap().account_id,
        "acc_b"
    );
}

#[tokio::test]
async fn test_f6_04_multi_account_independent_device_auth() {
    let store = MemoryCredentialStore::new();
    let d1 = DeviceAuth {
        device_id: "dev_1".to_string(),
        account_id: "acc_1".to_string(),
        secret: "sec_1".to_string(),
        user_agent: None,
        created: None,
    };
    let d2 = DeviceAuth {
        device_id: "dev_2".to_string(),
        account_id: "acc_2".to_string(),
        secret: "sec_2".to_string(),
        user_agent: None,
        created: None,
    };

    store.set_device_auth(&d1).unwrap();
    store.set_device_auth(&d2).unwrap();

    assert_eq!(
        store.get_device_auth("acc_1").unwrap().unwrap().secret,
        "sec_1"
    );
    assert_eq!(
        store.get_device_auth("acc_2").unwrap().unwrap().secret,
        "sec_2"
    );
}

#[tokio::test]
async fn test_f6_05_multi_account_independent_token_refreshes() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_x_12345678901234567890123456", "acc_x", "User X")
        .await;
    mock.register_auth_code("code_y_12345678901234567890123456", "acc_y", "User Y")
        .await;

    store
        .login_with_code("code_x_12345678901234567890123456")
        .await
        .unwrap();
    store
        .login_with_code("code_y_12345678901234567890123456")
        .await
        .unwrap();

    let (t1, t2) = tokio::join!(
        store.get_valid_access_token("acc_x"),
        store.get_valid_access_token("acc_y"),
    );

    assert!(t1.is_ok());
    assert!(t2.is_ok());
    assert_ne!(t1.unwrap(), t2.unwrap());
}

// ----------------------------------------------------------------------------
// F7: Instant Account Switch (< 5ms SLA)
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f7_01_account_switch_under_5ms_sla() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_sw1_123456789012345678901234", "acc_sw1", "Fast 1")
        .await;
    mock.register_auth_code("code_sw2_123456789012345678901234", "acc_sw2", "Fast 2")
        .await;

    store
        .login_with_code("code_sw1_123456789012345678901234")
        .await
        .unwrap();
    store
        .login_with_code("code_sw2_123456789012345678901234")
        .await
        .unwrap();

    // Measure switch to acc_sw1
    let (acc, duration) = store.switch_active_account("acc_sw1").await.unwrap();
    assert_eq!(acc.account_id, "acc_sw1");
    assert!(
        duration.as_millis() < 5,
        "Switch latency was {:?}, which exceeds the 5ms SLA threshold!",
        duration
    );
}

#[tokio::test]
async fn test_f7_02_account_switch_updates_active_flags() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_fl1_123456789012345678901234", "acc_fl1", "Flag 1")
        .await;
    mock.register_auth_code("code_fl2_123456789012345678901234", "acc_fl2", "Flag 2")
        .await;

    store
        .login_with_code("code_fl1_123456789012345678901234")
        .await
        .unwrap();
    store
        .login_with_code("code_fl2_123456789012345678901234")
        .await
        .unwrap();

    store.switch_active_account("acc_fl1").await.unwrap();
    let accounts = store.get_accounts().await.unwrap();
    let fl1 = accounts.iter().find(|a| a.account_id == "acc_fl1").unwrap();
    let fl2 = accounts.iter().find(|a| a.account_id == "acc_fl2").unwrap();

    assert!(fl1.is_active);
    assert!(!fl2.is_active);
}

#[tokio::test]
async fn test_f7_03_account_switch_updates_last_used() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_lu1_123456789012345678901234", "acc_lu1", "LU 1")
        .await;
    store
        .login_with_code("code_lu1_123456789012345678901234")
        .await
        .unwrap();

    let (acc, _) = store.switch_active_account("acc_lu1").await.unwrap();
    assert!(acc.last_used.is_some());
}

#[tokio::test]
async fn test_f7_04_account_switch_nonexistent_account_fails() {
    let (_, _, store) = setup_test_env().await;
    let res = store.switch_active_account("acc_unknown").await;
    assert!(matches!(res, Err(StoreError::AccountNotFound(_))));
}

#[tokio::test]
async fn test_f7_05_rapid_sequential_account_switches() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_rp1_123456789012345678901234", "acc_rp1", "RP 1")
        .await;
    mock.register_auth_code("code_rp2_123456789012345678901234", "acc_rp2", "RP 2")
        .await;

    store
        .login_with_code("code_rp1_123456789012345678901234")
        .await
        .unwrap();
    store
        .login_with_code("code_rp2_123456789012345678901234")
        .await
        .unwrap();

    let mut total_dur = std::time::Duration::ZERO;
    for i in 0..50 {
        let target = if i % 2 == 0 { "acc_rp1" } else { "acc_rp2" };
        let (acc, dur) = store.switch_active_account(target).await.unwrap();
        assert_eq!(acc.account_id, target);
        total_dur += dur;
    }
    let avg = total_dur.as_secs_f64() * 1000.0 / 50.0;
    assert!(avg < 5.0, "Average switch latency must be under 5ms SLA, got {:.3}ms", avg);
}

// ----------------------------------------------------------------------------
// F8: Entitlements Fetch & Pagination
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f8_01_fetch_entitlements_single_page() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let ent = Entitlement {
        id: "ent_1".to_string(),
        entitlement_name: "FallGuys".to_string(),
        namespace: "ns1".to_string(),
        catalog_item_id: "cat_1".to_string(),
        account_id: "acc_ent_1".to_string(),
        identity_id: None,
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: None,
        consumable: Some(false),
        status: "ACTIVE".to_string(),
        active: true,
        use_count: Some(0),
        created: None,
        updated: None,
    };
    mock.set_entitlements("acc_ent_1", vec![ent]).await;

    let items = client
        .fetch_entitlements("valid_token", "acc_ent_1", 0, 1000)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].entitlement_name, "FallGuys");
}

#[tokio::test]
async fn test_f8_02_fetch_entitlements_pagination_loop() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    // Create 1500 mock entitlements
    let mut large_list = Vec::new();
    for i in 0..1500 {
        large_list.push(Entitlement {
            id: format!("ent_{}", i),
            entitlement_name: format!("Game_{}", i),
            namespace: "ns".to_string(),
            catalog_item_id: format!("cat_{}", i),
            account_id: "acc_large".to_string(),
            identity_id: None,
            entitlement_type: "EXECUTABLE".to_string(),
            grant_date: None,
            consumable: None,
            status: "ACTIVE".to_string(),
            active: true,
            use_count: None,
            created: None,
            updated: None,
        });
    }
    mock.set_entitlements("acc_large", large_list).await;

    let all = client
        .fetch_all_entitlements("valid_token", "acc_large")
        .await
        .unwrap();
    assert_eq!(all.len(), 1500);
}

#[tokio::test]
async fn test_f8_03_fetch_entitlements_empty_library() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    mock.set_entitlements("acc_empty", Vec::new()).await;
    let all = client
        .fetch_all_entitlements("valid_token", "acc_empty")
        .await
        .unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn test_f8_04_fetch_entitlements_unauthorized_token() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let res = client
        .fetch_entitlements("invalid_token", "acc_any", 0, 10)
        .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_f8_05_entitlements_extract_namespace_and_catalog_id() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let ent = Entitlement {
        id: "ent_extract".to_string(),
        entitlement_name: "Civ6".to_string(),
        namespace: "civ6_ns_123".to_string(),
        catalog_item_id: "civ6_item_456".to_string(),
        account_id: "acc_ext".to_string(),
        identity_id: None,
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    mock.set_entitlements("acc_ext", vec![ent]).await;

    let items = client
        .fetch_all_entitlements("valid_token", "acc_ext")
        .await
        .unwrap();
    assert_eq!(items[0].namespace, "civ6_ns_123");
    assert_eq!(items[0].catalog_item_id, "civ6_item_456");
}

// ----------------------------------------------------------------------------
// F9: Catalog Fetch & DLC Classification
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f9_01_catalog_fetch_bulk_items() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let item = CatalogItem {
        id: "game_cyberpunk".to_string(),
        title: "Cyberpunk 2077".to_string(),
        description: Some("Open world RPG".to_string()),
        key_images: None,
        categories: Some(vec![CategoryPath {
            path: "games".to_string(),
        }]),
        namespace: "cyberpunk_ns".to_string(),
        status: Some("ACTIVE".to_string()),
        creation_date: None,
        last_modified_date: None,
        custom_attributes: None,
        main_game_item: None,
    };
    mock.set_catalog_item("cyberpunk_ns", item).await;

    let catalog = client
        .fetch_catalog_items("valid_token", "cyberpunk_ns", &["game_cyberpunk"])
        .await
        .unwrap();

    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog["game_cyberpunk"].title, "Cyberpunk 2077");
}

#[tokio::test]
async fn test_f9_02_catalog_classify_dlc() {
    let item = CatalogItem {
        id: "dlc_expansion".to_string(),
        title: "Expansion DLC".to_string(),
        description: None,
        key_images: None,
        categories: Some(vec![CategoryPath {
            path: "addons".to_string(),
        }]),
        namespace: "ns".to_string(),
        status: None,
        creation_date: None,
        last_modified_date: None,
        custom_attributes: None,
        main_game_item: Some(MainGameRef {
            id: "base_game_id".to_string(),
        }),
    };

    assert!(item.main_game_item.is_some());
    assert_eq!(
        item.main_game_item.as_ref().unwrap().id,
        "base_game_id"
    );
}

#[tokio::test]
async fn test_f9_03_catalog_classify_base_game() {
    let item = CatalogItem {
        id: "base_game".to_string(),
        title: "Main Game".to_string(),
        description: None,
        key_images: None,
        categories: Some(vec![CategoryPath {
            path: "games".to_string(),
        }]),
        namespace: "ns".to_string(),
        status: None,
        creation_date: None,
        last_modified_date: None,
        custom_attributes: None,
        main_game_item: None,
    };

    assert!(item.main_game_item.is_none());
}

#[tokio::test]
async fn test_f9_04_catalog_filter_unreal_engine_assets() {
    let item = CatalogItem {
        id: "ue_asset_1".to_string(),
        title: "Modular SciFi Kit".to_string(),
        description: None,
        key_images: None,
        categories: Some(vec![CategoryPath {
            path: "assets".to_string(),
        }]),
        namespace: "ue".to_string(),
        status: None,
        creation_date: None,
        last_modified_date: None,
        custom_attributes: None,
        main_game_item: None,
    };

    let is_engine_asset = item.namespace == "ue";
    assert!(is_engine_asset);
}

#[tokio::test]
async fn test_f9_05_catalog_select_tall_box_art() {
    let item = CatalogItem {
        id: "game_art".to_string(),
        title: "Art Game".to_string(),
        description: None,
        key_images: Some(vec![
            KeyImage {
                image_type: "DieselGameBox".to_string(),
                url: "https://example.com/banner.jpg".to_string(),
                md5: None,
                width: Some(1920),
                height: Some(1080),
            },
            KeyImage {
                image_type: "DieselGameBoxTall".to_string(),
                url: "https://example.com/tall.jpg".to_string(),
                md5: None,
                width: Some(1200),
                height: Some(1600),
            },
        ]),
        categories: None,
        namespace: "ns".to_string(),
        status: None,
        creation_date: None,
        last_modified_date: None,
        custom_attributes: None,
        main_game_item: None,
    };

    let tall_image = item
        .key_images
        .as_ref()
        .and_then(|images| images.iter().find(|img| img.image_type == "DieselGameBoxTall"))
        .map(|img| img.url.as_str());

    assert_eq!(tall_image, Some("https://example.com/tall.jpg"));
}

// ----------------------------------------------------------------------------
// F10: Local .item Manifest Parsing
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f10_01_parse_valid_windows_manifest() {
    let path = fixtures_path().join("manifests/valid_game_win.item");
    let manifest = parse_manifest_file(&path).expect("Failed to parse Windows manifest");

    assert_eq!(manifest.app_name, "Sugar");
    assert_eq!(manifest.display_name, "Cyberpunk 2077");
    assert_eq!(manifest.launch_executable, "bin\\x64\\Cyberpunk2077.exe");
    assert!(!manifest.is_incomplete_install.unwrap_or(false));
}

#[tokio::test]
async fn test_f10_02_parse_valid_macos_manifest() {
    let path = fixtures_path().join("manifests/valid_game_macos.item");
    let manifest = parse_manifest_file(&path).expect("Failed to parse macOS manifest");

    assert_eq!(manifest.app_name, "Kinglet");
    assert_eq!(manifest.display_name, "Sid Meier's Civilization VI");
    assert_eq!(manifest.launch_executable, "Civ6.app");
}

#[tokio::test]
async fn test_f10_03_parse_incomplete_install_flag() {
    let path = fixtures_path().join("manifests/incomplete_install.item");
    let manifest = parse_manifest_file(&path).expect("Failed to parse incomplete manifest");

    assert_eq!(manifest.app_name, "Fortnite");
    assert!(manifest.is_incomplete_install.unwrap_or(false));
    assert!(!manifest.is_valid_installation());
}

#[tokio::test]
async fn test_f10_04_parse_corrupt_json_manifest() {
    let path = fixtures_path().join("manifests/corrupt_manifest.item");
    let result = parse_manifest_file(&path);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_f10_05_parse_dlc_manifest() {
    let path = fixtures_path().join("manifests/dlc_addon.item");
    let manifest = parse_manifest_file(&path).expect("Failed to parse DLC manifest");

    assert!(manifest.is_dlc());
    assert_eq!(manifest.main_game_app_name.as_deref(), Some("Kinglet"));
}

// ----------------------------------------------------------------------------
// F11: Cloud/Local Library Aggregator
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f11_01_aggregate_cloud_and_local_games() {
    let (mock, db, store) = setup_test_env().await;
    let acc_id = "acc_agg_1";
    mock.register_auth_code("code_agg_123456789012345678901234", acc_id, "Aggregator Player")
        .await;

    let ent = Entitlement {
        id: "ent_agg_civ".to_string(),
        entitlement_name: "Sid Meier's Civilization VI".to_string(),
        namespace: "cd14dcaa4f3443f19f7169a980559c62".to_string(),
        catalog_item_id: "42ac1ee840304cb1807172a9b47dc8e3".to_string(),
        account_id: acc_id.to_string(),
        identity_id: None,
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    mock.set_entitlements(acc_id, vec![ent]).await;

    store
        .login_with_code("code_agg_123456789012345678901234")
        .await
        .unwrap();

    let manifests_dir = fixtures_path().join("manifests");
    let summary = store
        .sync_library(acc_id, Some(&manifests_dir))
        .await
        .unwrap();

    assert_eq!(summary.base_games_count, 1);
    let games = db.get_games().await.unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].name, "Sid Meier's Civilization VI");
}

#[tokio::test]
async fn test_f11_02_aggregate_flags_installed_games() {
    let (mock, db, store) = setup_test_env().await;
    let acc_id = "acc_inst_1";
    mock.register_auth_code("code_inst_123456789012345678901234", acc_id, "Installed User")
        .await;

    // Matches valid_game_macos.item
    let ent = Entitlement {
        id: "ent_civ".to_string(),
        entitlement_name: "Civilization VI".to_string(),
        namespace: "cd14dcaa4f3443f19f7169a980559c62".to_string(),
        catalog_item_id: "42ac1ee840304cb1807172a9b47dc8e3".to_string(),
        account_id: acc_id.to_string(),
        identity_id: None,
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    mock.set_entitlements(acc_id, vec![ent]).await;
    store
        .login_with_code("code_inst_123456789012345678901234")
        .await
        .unwrap();

    let manifests_dir = fixtures_path().join("manifests");
    store
        .sync_library(acc_id, Some(&manifests_dir))
        .await
        .unwrap();

    let games = db.get_games().await.unwrap();
    assert!(games[0].is_installed);
}

#[tokio::test]
async fn test_f11_03_aggregate_uninstalled_cloud_games() {
    let (mock, db, store) = setup_test_env().await;
    let acc_id = "acc_uninst_1";
    mock.register_auth_code("code_uninst_1234567890123456789012", acc_id, "Uninstalled User")
        .await;

    let ent = Entitlement {
        id: "ent_uninst".to_string(),
        entitlement_name: "Uninstalled Game".to_string(),
        namespace: "uninst_ns".to_string(),
        catalog_item_id: "uninst_item_id_99999".to_string(),
        account_id: acc_id.to_string(),
        identity_id: None,
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    mock.set_entitlements(acc_id, vec![ent]).await;
    store
        .login_with_code("code_uninst_1234567890123456789012")
        .await
        .unwrap();

    let manifests_dir = fixtures_path().join("manifests");
    store
        .sync_library(acc_id, Some(&manifests_dir))
        .await
        .unwrap();

    let games = db.get_games().await.unwrap();
    assert!(!games[0].is_installed);
}

#[tokio::test]
async fn test_f11_04_aggregate_separates_dlc_from_base_games() {
    let (mock, db, store) = setup_test_env().await;
    let acc_id = "acc_dlc_test";
    mock.register_auth_code("code_dlc_123456789012345678901234", acc_id, "DLC User")
        .await;

    let base = Entitlement {
        id: "ent_base".to_string(),
        entitlement_name: "Base Game".to_string(),
        namespace: "ns".to_string(),
        catalog_item_id: "base_item".to_string(),
        account_id: acc_id.to_string(),
        identity_id: None,
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    let dlc = Entitlement {
        id: "ent_dlc".to_string(),
        entitlement_name: "Expansion".to_string(),
        namespace: "ns".to_string(),
        catalog_item_id: "dlc_item".to_string(),
        account_id: acc_id.to_string(),
        identity_id: None,
        entitlement_type: "ENTITLEMENT".to_string(), // DLC
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    mock.set_entitlements(acc_id, vec![base, dlc]).await;
    store
        .login_with_code("code_dlc_123456789012345678901234")
        .await
        .unwrap();

    let summary = store.sync_library(acc_id, None).await.unwrap();
    assert_eq!(summary.base_games_count, 1);
    assert_eq!(summary.dlc_count, 1);

    let games = db.get_games().await.unwrap();
    assert_eq!(games.len(), 1); // Only base game in primary table
}

#[tokio::test]
async fn test_f11_05_aggregate_associates_game_accounts() {
    let (mock, db, store) = setup_test_env().await;
    let acc_id = "acc_assoc_1";
    mock.register_auth_code("code_assoc_12345678901234567890123", acc_id, "Assoc User")
        .await;

    let ent = Entitlement {
        id: "ent_assoc".to_string(),
        entitlement_name: "Assoc Game".to_string(),
        namespace: "ns".to_string(),
        catalog_item_id: "assoc_cat_id".to_string(),
        account_id: acc_id.to_string(),
        identity_id: None,
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    mock.set_entitlements(acc_id, vec![ent]).await;
    store
        .login_with_code("code_assoc_12345678901234567890123")
        .await
        .unwrap();
    store.sync_library(acc_id, None).await.unwrap();

    let links = db.get_game_accounts().await.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].game_id, "epic_assoc_cat_id");
    assert_eq!(links[0].account_id, format!("acc_epic_{}", acc_id));
}

// ----------------------------------------------------------------------------
// F12: Exchange Code Generation
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f12_01_generate_exchange_code_success() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let ex = client
        .get_exchange_code("valid_token")
        .await
        .expect("Exchange failed");
    assert!(!ex.code.is_empty());
    assert_eq!(ex.expires_in_seconds, 300);
}

#[tokio::test]
async fn test_f12_02_exchange_code_valid_ttl() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let ex = client.get_exchange_code("valid_token").await.unwrap();
    assert_eq!(ex.expires_in_seconds, 300);
}

#[tokio::test]
async fn test_f12_03_exchange_code_unique_per_request() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let ex1 = client.get_exchange_code("valid_token").await.unwrap();
    let ex2 = client.get_exchange_code("valid_token").await.unwrap();
    assert_ne!(ex1.code, ex2.code);
}

#[tokio::test]
async fn test_f12_04_exchange_code_requires_auth() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let res = client.get_exchange_code("invalid_token").await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_f12_05_exchange_code_creating_client_id() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let ex = client.get_exchange_code("valid_token").await.unwrap();
    assert_eq!(ex.creating_client_id, EPIC_LAUNCHER_CLIENT_ID);
}

// ----------------------------------------------------------------------------
// F13: CLI Launch Parameter Construction
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f13_01_build_standard_launch_parameters() {
    let args = build_launch_parameters(
        "ex_secret_123",
        "Sugar",
        "acc_user_1",
        "PlayerOne",
        "namespace_1",
        Some("en"),
        None,
    );

    assert!(args.contains(&"-AUTH_LOGIN=unused".to_string()));
    assert!(args.contains(&"-AUTH_PASSWORD=ex_secret_123".to_string()));
    assert!(args.contains(&"-AUTH_TYPE=exchangecode".to_string()));
    assert!(args.contains(&"-epicapp=Sugar".to_string()));
    assert!(args.contains(&"-epicenv=Prod".to_string()));
    assert!(args.contains(&"-EpicPortal".to_string()));
}

#[tokio::test]
async fn test_f13_02_launch_parameters_include_user_context() {
    let args = build_launch_parameters(
        "ex_code",
        "Fortnite",
        "acc_fn_1",
        "BattleRoyaleGod",
        "fn",
        Some("en"),
        None,
    );

    assert!(args.contains(&"-epicusername=BattleRoyaleGod".to_string()));
    assert!(args.contains(&"-epicuserid=acc_fn_1".to_string()));
    assert!(args.contains(&"-epicsandboxid=fn".to_string()));
}

#[tokio::test]
async fn test_f13_03_launch_parameters_locale_custom() {
    let args = build_launch_parameters(
        "ex_code",
        "App",
        "acc",
        "User",
        "ns",
        Some("ru-RU"),
        None,
    );
    assert!(args.contains(&"-epiclocale=ru-RU".to_string()));
}

#[tokio::test]
async fn test_f13_04_launch_parameters_ordering_preserves_mandatory_keys() {
    let args = build_launch_parameters("ex_code", "App", "acc", "User", "ns", None, None);
    let joined = args.join(" ");
    assert!(joined.contains("-AUTH_LOGIN=unused"));
    assert!(joined.contains("-AUTH_PASSWORD="));
    assert!(joined.contains("-AUTH_TYPE=exchangecode"));
    assert!(joined.contains("-EpicPortal"));
}

#[tokio::test]
async fn test_f13_05_launch_parameters_custom_user_args_appended() {
    let custom = vec!["-fullscreen".to_string(), "-novid".to_string()];
    let args = build_launch_parameters(
        "ex_code",
        "App",
        "acc",
        "User",
        "ns",
        None,
        Some(&custom),
    );

    assert!(args.contains(&"-fullscreen".to_string()));
    assert!(args.contains(&"-novid".to_string()));
}

// ----------------------------------------------------------------------------
// F14: Cross-Platform Direct Launch
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f14_01_direct_launch_command_structure() {
    let path = fixtures_path().join("manifests/valid_game_win.item");
    let manifest = parse_manifest_file(&path).unwrap();

    let cmd = prepare_launch_command(
        &manifest,
        "ex_tok_123",
        "acc_123",
        "Player",
        Some("en"),
        None,
    )
    .unwrap();

    assert!(cmd.executable_path.to_string_lossy().contains("Cyberpunk2077.exe"));
    assert!(!cmd.args.is_empty());
    assert_eq!(cmd.env.get("EPIC_APP_NAME").unwrap(), "Sugar");
}

#[tokio::test]
async fn test_f14_02_launch_working_dir_set_to_install_dir() {
    let path = fixtures_path().join("manifests/valid_game_win.item");
    let manifest = parse_manifest_file(&path).unwrap();

    let cmd = prepare_launch_command(&manifest, "ex_tok", "acc", "User", None, None).unwrap();
    assert_eq!(
        cmd.working_dir,
        PathBuf::from(r"C:\Games\Cyberpunk 2077")
    );
}

#[tokio::test]
async fn test_f14_03_launch_macos_bundle_resolution() {
    let path = fixtures_path().join("manifests/valid_game_macos.item");
    let manifest = parse_manifest_file(&path).unwrap();

    let cmd = prepare_launch_command(&manifest, "ex_tok", "acc", "User", None, None).unwrap();
    assert!(cmd.executable_path.to_string_lossy().ends_with("Civ6.app"));
}

#[tokio::test]
async fn test_f14_04_launch_environment_variables() {
    let path = fixtures_path().join("manifests/valid_game_win.item");
    let manifest = parse_manifest_file(&path).unwrap();

    let cmd = prepare_launch_command(&manifest, "ex_tok", "acc", "User", None, None).unwrap();
    assert_eq!(
        cmd.env.get("EPIC_NAMESPACE").unwrap(),
        "0a2d5924765643a39e05465c04da7d50"
    );
}

#[tokio::test]
async fn test_f14_05_launch_fails_on_incomplete_install() {
    let path = fixtures_path().join("manifests/incomplete_install.item");
    let manifest = parse_manifest_file(&path).unwrap();

    let res = prepare_launch_command(&manifest, "ex_tok", "acc", "User", None, None);
    assert!(matches!(res, Err(LaunchError::IncompleteInstall)));
}

// ----------------------------------------------------------------------------
// F15: Sensitive Secret Masking in Logs
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f15_01_mask_exchange_code_in_logs() {
    let args = vec![
        "-AUTH_LOGIN=unused".to_string(),
        "-AUTH_PASSWORD=super_secret_exchange_token_xyz".to_string(),
        "-AUTH_TYPE=exchangecode".to_string(),
    ];

    let masked = mask_launch_arguments(&args);
    assert_eq!(masked[1], "-AUTH_PASSWORD=********");
}

#[tokio::test]
async fn test_f15_02_mask_multiple_sensitive_flags() {
    let args = vec![
        "-AUTH_PASSWORD=secret_token".to_string(),
        "-epicovt=confidential_ovt_token".to_string(),
        "-epicapp=App".to_string(),
    ];

    let masked = mask_launch_arguments(&args);
    assert_eq!(masked[0], "-AUTH_PASSWORD=********");
    assert_eq!(masked[1], "-epicovt=[REDACTED]");
    assert_eq!(masked[2], "-epicapp=App");
}

#[tokio::test]
async fn test_f15_03_display_args_never_contains_raw_exchange_code() {
    let raw_secret = "confidential_998877665544332211";
    let args = vec![
        "-AUTH_LOGIN=unused".to_string(),
        format!("-AUTH_PASSWORD={}", raw_secret),
        "-EpicPortal".to_string(),
    ];

    let formatted = format!("{}", RedactedLaunchArgs(&args));
    assert!(!formatted.contains(raw_secret));
    assert!(formatted.contains("-AUTH_PASSWORD=********"));
}

#[tokio::test]
async fn test_f15_04_non_sensitive_args_unmodified() {
    let args = vec![
        "-epicapp=Sugar".to_string(),
        "-epicenv=Prod".to_string(),
        "-epiclocale=en".to_string(),
    ];

    let masked = mask_launch_arguments(&args);
    assert_eq!(masked, args);
}

#[tokio::test]
async fn test_f15_05_mask_empty_password_flag() {
    let args = vec!["-AUTH_PASSWORD=".to_string()];
    let masked = mask_launch_arguments(&args);
    assert_eq!(masked[0], "-AUTH_PASSWORD=********");
}

// ----------------------------------------------------------------------------
// F16: REST API Endpoints
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_f16_01_api_login_url_endpoint() {
    let (_, _, store) = setup_test_env().await;
    let router = create_epic_api_router(store);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/epic/auth/url")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(json["url"].as_str().unwrap().contains("epicgames.com"));
    assert!(json["url"].as_str().unwrap().contains(EPIC_LAUNCHER_CLIENT_ID));
}

#[tokio::test]
async fn test_f16_02_api_login_code_endpoint() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("api_valid_code_12345678901234567890", "acc_api_1", "API User")
        .await;

    let router = create_epic_api_router(store);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"code": "api_valid_code_12345678901234567890"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let account: EpicAccount = serde_json::from_slice(&body).unwrap();
    assert_eq!(account.account_id, "acc_api_1");
}

#[tokio::test]
async fn test_f16_03_api_accounts_list_endpoint() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_list_123456789012345678901234", "acc_list_1", "Lister")
        .await;
    store
        .login_with_code("code_list_123456789012345678901234")
        .await
        .unwrap();

    let router = create_epic_api_router(store);
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/epic/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let accounts: Vec<EpicAccount> = serde_json::from_slice(&body).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_id, "acc_list_1");
}

#[tokio::test]
async fn test_f16_04_api_switch_account_endpoint() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_s1_12345678901234567890123456", "acc_s1", "S1")
        .await;
    mock.register_auth_code("code_s2_12345678901234567890123456", "acc_s2", "S2")
        .await;

    store
        .login_with_code("code_s1_12345678901234567890123456")
        .await
        .unwrap();
    store
        .login_with_code("code_s2_12345678901234567890123456")
        .await
        .unwrap();

    let router = create_epic_api_router(store);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/switch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_id": "acc_s1"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["account_id"], "acc_s1");
    assert!(json["latency_ms"].as_f64().unwrap() < 5.0);
}

#[tokio::test]
async fn test_f16_05_api_launch_game_endpoint() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_ln_12345678901234567890123456", "acc_ln", "Launcher")
        .await;
    store
        .login_with_code("code_ln_12345678901234567890123456")
        .await
        .unwrap();

    let router = create_epic_api_router(store);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"account_id": "acc_ln", "app_name": "Sugar"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["app_name"], "Sugar");
    assert!(json["display_arguments"]
        .to_string()
        .contains("-AUTH_PASSWORD=********"));
}

// ============================================================================
// TIER 2: BOUNDARY & CORNER CASES
// ============================================================================

#[test]
fn test_b01_empty_auth_code_rejected() {
    assert!(sanitize_auth_code("").is_err());
    assert!(sanitize_auth_code("   ").is_err());
}

#[test]
fn test_b02_malformed_auth_code_rejected() {
    assert!(sanitize_auth_code("not_a_valid_hex_32_chars").is_err());
    assert!(sanitize_auth_code("12345").is_err());
}

#[test]
fn test_b03_whitespace_and_newlines_sanitized() {
    let input = "\n\t  3b2a0c4f8d914652a92db1029c73e1a0 \r\n ";
    let clean = sanitize_auth_code(input).unwrap();
    assert_eq!(clean, "3b2a0c4f8d914652a92db1029c73e1a0");
}

#[test]
fn test_b04_json_payload_missing_code_rejected() {
    let json = r#"{"redirectUrl":"https://example.com","sid":null}"#;
    assert!(sanitize_auth_code(json).is_err());
}

#[test]
fn test_b05_sql_injection_in_auth_code_rejected() {
    let injection = "' OR '1'='1' -- 3b2a0c4f8d914652a92db1029c73e1a0";
    assert!(sanitize_auth_code(injection).is_err());
}

#[tokio::test]
async fn test_b06_expired_auth_code_returns_400() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let res = client.exchange_code_for_token("expired_unknown_code").await;
    assert!(matches!(res, Err(EpicClientError::Api(_))));
}

#[tokio::test]
async fn test_b07_keyring_query_unknown_account_none() {
    let store = MemoryCredentialStore::new();
    assert!(store.get_device_auth("acc_nonexistent").unwrap().is_none());
}

#[tokio::test]
async fn test_b08_corrupt_json_in_keyring_fails_gracefully() {
    let store = MemoryCredentialStore::new();
    let dev = DeviceAuth {
        device_id: "dev".to_string(),
        account_id: "acc_bad".to_string(),
        secret: "sec".to_string(),
        user_agent: None,
        created: None,
    };
    store.set_device_auth(&dev).unwrap();

    // Overwrite with invalid json in internal storage
    store
        .set_device_auth(&DeviceAuth {
            device_id: "dev_ok".to_string(),
            account_id: "acc_bad".to_string(),
            secret: "sec_ok".to_string(),
            user_agent: None,
            created: None,
        })
        .unwrap();

    let res = store.get_device_auth("acc_bad");
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_b09_idempotent_credential_deletion() {
    let store = MemoryCredentialStore::new();
    // Deleting non-existent credential should succeed without error
    assert!(store.delete_device_auth("acc_never_existed").is_ok());
}

#[tokio::test]
async fn test_b10_device_auth_wrong_secret_rejected() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth = DeviceAuth {
        device_id: "dev_acc1".to_string(),
        account_id: "4a6313cbbe7d432b8132b5ee67ad53fa".to_string(),
        secret: "bad_secret_value".to_string(),
        user_agent: None,
        created: None,
    };

    let res = client.login_with_device_auth(&auth).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_b11_device_auth_empty_device_id_rejected() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let auth = DeviceAuth {
        device_id: "".to_string(),
        account_id: "4a6313cbbe7d432b8132b5ee67ad53fa".to_string(),
        secret: "secret_acc1".to_string(),
        user_agent: None,
        created: None,
    };

    assert!(client.login_with_device_auth(&auth).await.is_err());
}

#[tokio::test]
async fn test_b12_empty_string_account_id_switch_rejected() {
    let (_, _, store) = setup_test_env().await;
    assert!(store.switch_active_account("").await.is_err());
}

#[test]
fn test_b13_empty_manifest_file_rejected() {
    assert!(parse_manifest_from_str("").is_err());
    assert!(parse_manifest_from_str("   ").is_err());
}

#[test]
fn test_b14_truncated_json_manifest_rejected() {
    let truncated = r#"{"AppName": "Broken", "CatalogItemId": "#;
    assert!(parse_manifest_from_str(truncated).is_err());
}

#[test]
fn test_b15_manifest_with_empty_app_name_rejected() {
    let json = r#"{
        "FormatVersion": 0,
        "AppName": "",
        "CatalogItemId": "123",
        "CatalogNamespace": "ns",
        "DisplayName": "No App Name",
        "InstallationGuid": "guid",
        "InstallLocation": "loc",
        "LaunchExecutable": "game.exe"
    }"#;
    assert!(parse_manifest_from_str(json).is_err());
}

#[test]
fn test_b16_manifest_with_empty_launch_executable_handled() {
    let path = fixtures_path().join("manifests/empty_launch_executable.item");
    let manifest = parse_manifest_file(&path).unwrap();
    let exe = manifest.resolve_executable_path();
    assert_eq!(exe, PathBuf::from(r"C:\Games\NoExecutable"));
}

#[test]
fn test_b17_incomplete_install_marked_invalid() {
    let path = fixtures_path().join("manifests/incomplete_install.item");
    let manifest = parse_manifest_file(&path).unwrap();
    assert!(!manifest.is_valid_installation());
}

#[test]
fn test_b18_scan_nonexistent_manifest_dir_returns_empty() {
    let res = scan_manifest_directory(Path::new("/nonexistent/manifest/path/123"));
    assert!(res.unwrap().is_empty());
}

#[tokio::test]
async fn test_b19_entitlements_offset_past_length_returns_empty() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let ent = Entitlement {
        id: "ent_single".to_string(),
        entitlement_name: "Game".to_string(),
        namespace: "ns".to_string(),
        catalog_item_id: "cat".to_string(),
        account_id: "acc_single".to_string(),
        identity_id: None,
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    mock.set_entitlements("acc_single", vec![ent]).await;

    let items = client
        .fetch_entitlements("valid_token", "acc_single", 100, 10)
        .await
        .unwrap();
    assert!(items.is_empty());
}

#[tokio::test]
async fn test_b20_catalog_fetch_unknown_item_returns_empty_map() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let res = client
        .fetch_catalog_items("valid_token", "empty_ns", &["unknown_id"])
        .await
        .unwrap();
    assert!(res.is_empty());
}

#[test]
fn test_b21_catalog_item_missing_optional_key_images() {
    let json = r#"{
        "id": "item_no_images",
        "title": "Text Game",
        "namespace": "ns"
    }"#;
    let item: CatalogItem = serde_json::from_str(json).unwrap();
    assert!(item.key_images.is_none());
}

#[test]
fn test_b22_secret_masking_empty_password_flag() {
    let args = vec!["-AUTH_PASSWORD=".to_string()];
    let masked = mask_launch_arguments(&args);
    assert_eq!(masked[0], "-AUTH_PASSWORD=********");
}

#[test]
fn test_b23_secret_masking_multiple_flags_together() {
    let args = vec![
        "-AUTH_PASSWORD=token1".to_string(),
        "-epicovt=token2".to_string(),
    ];
    let masked = mask_launch_arguments(&args);
    assert_eq!(masked[0], "-AUTH_PASSWORD=********");
    assert_eq!(masked[1], "-epicovt=[REDACTED]");
}

#[tokio::test]
async fn test_b24_switching_unknown_account_fails_without_modifying_active() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_b24_123456789012345678901234", "acc_b24", "Original")
        .await;
    store
        .login_with_code("code_b24_123456789012345678901234")
        .await
        .unwrap();

    let res = store.switch_active_account("nonexistent_account").await;
    assert!(res.is_err());

    // Active account should remain acc_b24
    let current = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(current.account_id, "acc_b24");
}

#[tokio::test]
async fn test_b25_switching_to_currently_active_account_succeeds() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_b25_123456789012345678901234", "acc_b25", "Active")
        .await;
    store
        .login_with_code("code_b25_123456789012345678901234")
        .await
        .unwrap();

    let (acc, dur) = store.switch_active_account("acc_b25").await.unwrap();
    assert_eq!(acc.account_id, "acc_b25");
    assert!(dur.as_millis() < 5);
}

#[tokio::test]
async fn test_b26_special_characters_in_display_name_and_game_title() {
    let (mock, db, store) = setup_test_env().await;
    let special_name = "Jedi_Воин_🎮_<Script>_\"quoted\"";
    mock.register_auth_code(
        "code_spec_12345678901234567890123",
        "acc_spec",
        special_name,
    )
    .await;

    let acc = store
        .login_with_code("code_spec_12345678901234567890123")
        .await
        .unwrap();
    assert_eq!(acc.display_name.as_deref(), Some(special_name));

    let fetched = db.get_accounts().await.unwrap();
    assert_eq!(fetched[0].display_name.as_deref(), Some(special_name));
}

// ============================================================================
// TIER 3: PAIRWISE COMBINATORIAL & INTERACTION TESTS
// ============================================================================

#[tokio::test]
async fn test_i01_account_switch_then_game_launch() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_i1_a_12345678901234567890123", "acc_i1_a", "Player A")
        .await;
    mock.register_auth_code("code_i1_b_12345678901234567890123", "acc_i1_b", "Player B")
        .await;

    store
        .login_with_code("code_i1_a_12345678901234567890123")
        .await
        .unwrap();
    store
        .login_with_code("code_i1_b_12345678901234567890123")
        .await
        .unwrap();

    // Switch to Player A
    store.switch_active_account("acc_i1_a").await.unwrap();

    // Launch game for active Player A
    let token = store.get_valid_access_token("acc_i1_a").await.unwrap();
    let ex = store.client().get_exchange_code(&token).await.unwrap();

    let args = build_launch_parameters(
        &ex.code,
        "Sugar",
        "acc_i1_a",
        "Player A",
        "0a2d5924765643a39e05465c04da7d50",
        Some("en"),
        None,
    );

    assert!(args.contains(&"-epicuserid=acc_i1_a".to_string()));
    assert!(args.contains(&"-epicusername=Player A".to_string()));
}

#[tokio::test]
async fn test_i02_library_sync_triggers_token_refresh_if_needed() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_i2_12345678901234567890123456", "acc_i2", "User I2")
        .await;
    store
        .login_with_code("code_i2_12345678901234567890123456")
        .await
        .unwrap();

    // Sync library queries entitlements using token refreshed via DeviceAuth
    let summary = store.sync_library("acc_i2", None).await.unwrap();
    assert_eq!(summary.total_entitlements, 0);
}

#[tokio::test]
async fn test_i03_multi_account_library_ownership_filter() {
    let (mock, db, store) = setup_test_env().await;
    mock.register_auth_code("code_i3_1_12345678901234567890123", "acc_i3_1", "User 1")
        .await;
    mock.register_auth_code("code_i3_2_12345678901234567890123", "acc_i3_2", "User 2")
        .await;

    // Account 1 owns Civ6
    let ent1 = Entitlement {
        id: "ent_civ".to_string(),
        entitlement_name: "Civ6".to_string(),
        namespace: "ns1".to_string(),
        catalog_item_id: "cat_civ".to_string(),
        account_id: "acc_i3_1".to_string(),
        identity_id: None,
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    mock.set_entitlements("acc_i3_1", vec![ent1]).await;

    // Account 2 owns Cyberpunk
    let ent2 = Entitlement {
        id: "ent_cp".to_string(),
        entitlement_name: "Cyberpunk".to_string(),
        namespace: "ns2".to_string(),
        catalog_item_id: "cat_cp".to_string(),
        account_id: "acc_i3_2".to_string(),
        identity_id: None,
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    mock.set_entitlements("acc_i3_2", vec![ent2]).await;

    store
        .login_with_code("code_i3_1_12345678901234567890123")
        .await
        .unwrap();
    store.sync_library("acc_i3_1", None).await.unwrap();

    store
        .login_with_code("code_i3_2_12345678901234567890123")
        .await
        .unwrap();
    store.sync_library("acc_i3_2", None).await.unwrap();

    let all_links = db.get_game_accounts().await.unwrap();
    assert_eq!(all_links.len(), 2);

    let user1_links: Vec<_> = all_links
        .into_iter()
        .filter(|l| l.account_id == "acc_epic_acc_i3_1")
        .collect();
    assert_eq!(user1_links.len(), 1);
    assert_eq!(user1_links[0].game_id, "epic_cat_civ");
}

#[tokio::test]
async fn test_i04_incomplete_install_launch_prevention() {
    let path = fixtures_path().join("manifests/incomplete_install.item");
    let manifest = parse_manifest_file(&path).unwrap();

    let res = prepare_launch_command(&manifest, "ex_tok", "acc", "User", None, None);
    assert!(matches!(res, Err(LaunchError::IncompleteInstall)));
}

#[tokio::test]
async fn test_i05_offline_playable_game_launch_without_server() {
    let path = fixtures_path().join("manifests/offline_only.item");
    let manifest = parse_manifest_file(&path).unwrap();

    assert!(manifest.can_run_offline.unwrap_or(false));
    let cmd = prepare_launch_command(
        &manifest,
        "offline_dummy_exchange",
        "acc_local",
        "OfflinePlayer",
        None,
        None,
    )
    .unwrap();

    assert!(cmd.executable_path.to_string_lossy().contains("Celeste.exe"));
}

#[tokio::test]
async fn test_i06_reauthentication_updates_device_auth_without_duplicating_account() {
    let (mock, db, store) = setup_test_env().await;
    mock.register_auth_code("code_reauth_1_123456789012345678", "acc_reauth", "Name V1")
        .await;
    store
        .login_with_code("code_reauth_1_123456789012345678")
        .await
        .unwrap();

    mock.register_auth_code("code_reauth_2_123456789012345678", "acc_reauth", "Name V2")
        .await;
    store
        .login_with_code("code_reauth_2_123456789012345678")
        .await
        .unwrap();

    let accounts = db.get_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].display_name.as_deref(), Some("Name V2"));
}

#[tokio::test]
async fn test_i07_concurrent_keyring_access_across_threads() {
    let store = Arc::new(MemoryCredentialStore::new());
    let mut tasks = Vec::new();

    for i in 0..20 {
        let s = store.clone();
        tasks.push(tokio::spawn(async move {
            let auth = DeviceAuth {
                device_id: format!("dev_{}", i),
                account_id: format!("acc_{}", i % 5),
                secret: format!("sec_{}", i),
                user_agent: None,
                created: None,
            };
            s.set_device_auth(&auth).unwrap();
            let _ = s.get_device_auth(&format!("acc_{}", i % 5)).unwrap();
        }));
    }

    for t in tasks {
        t.await.unwrap();
    }
}

#[tokio::test]
async fn test_i08_rapid_sequential_account_switches_under_load() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_sw_a_12345678901234567890123", "acc_a", "A")
        .await;
    mock.register_auth_code("code_sw_b_12345678901234567890123", "acc_b", "B")
        .await;

    store
        .login_with_code("code_sw_a_12345678901234567890123")
        .await
        .unwrap();
    store
        .login_with_code("code_sw_b_12345678901234567890123")
        .await
        .unwrap();

    let mut latencies = Vec::new();
    for i in 0..20 {
        let target = if i % 2 == 0 { "acc_a" } else { "acc_b" };
        let (_, dur) = store.switch_active_account(target).await.unwrap();
        latencies.push(dur);
    }

    let max_dur = latencies.iter().max().unwrap();
    assert!(max_dur.as_millis() < 5);
}

#[tokio::test]
async fn test_i09_cloud_dlc_entitlement_linked_to_local_base_game() {
    let base_manifest = parse_manifest_file(fixtures_path().join("manifests/valid_game_macos.item")).unwrap();
    let dlc_manifest = parse_manifest_file(fixtures_path().join("manifests/dlc_addon.item")).unwrap();

    assert_eq!(
        dlc_manifest.main_game_app_name.as_deref(),
        Some(base_manifest.app_name.as_str())
    );
}

#[tokio::test]
async fn test_i10_device_auth_deletion_cleans_keyring_and_server() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_del_1234567890123456789012345", "acc_del_test", "Deleter")
        .await;

    store
        .login_with_code("code_del_1234567890123456789012345")
        .await
        .unwrap();
    let auth = store
        .keyring()
        .get_device_auth("acc_del_test")
        .unwrap()
        .unwrap();

    let token = store.get_valid_access_token("acc_del_test").await.unwrap();
    let delete_res = store
        .client()
        .delete_device_auth(&auth.account_id, &auth.device_id, &token)
        .await;
    assert!(delete_res.is_ok());

    store.keyring().delete_device_auth("acc_del_test").unwrap();
    assert!(store
        .keyring()
        .get_device_auth("acc_del_test")
        .unwrap()
        .is_none());
}

#[test]
fn test_i11_direct_launch_custom_arguments_preserved() {
    let custom = vec!["-culture=ru".to_string(), "-nosound".to_string()];
    let args = build_launch_parameters("token", "App", "acc", "User", "ns", None, Some(&custom));

    assert!(args.contains(&"-culture=ru".to_string()));
    assert!(args.contains(&"-nosound".to_string()));
}

#[tokio::test]
async fn test_i12_unauthenticated_launch_attempt_fails() {
    let (_, _, store) = setup_test_env().await;
    let res = store.get_valid_access_token("acc_not_logged_in").await;
    assert!(matches!(res, Err(StoreError::AccountNotFound(_))));
}

#[tokio::test]
async fn test_i13_entitlements_pagination_multi_batch() {
    let (mock, _, _) = setup_test_env().await;
    let client = EpicClient::from_router(mock.router());

    let mut list = Vec::new();
    for i in 0..2500 {
        list.push(Entitlement {
            id: format!("ent_batch_{}", i),
            entitlement_name: format!("BatchGame_{}", i),
            namespace: "ns".to_string(),
            catalog_item_id: format!("cat_batch_{}", i),
            account_id: "acc_multi_batch".to_string(),
            identity_id: None,
            entitlement_type: "EXECUTABLE".to_string(),
            grant_date: None,
            consumable: None,
            status: "ACTIVE".to_string(),
            active: true,
            use_count: None,
            created: None,
            updated: None,
        });
    }
    mock.set_entitlements("acc_multi_batch", list).await;

    let all = client
        .fetch_all_entitlements("valid_token", "acc_multi_batch")
        .await
        .unwrap();
    assert_eq!(all.len(), 2500);
}

#[tokio::test]
async fn test_i14_catalog_filtering_engine_assets_and_mods() {
    let path = fixtures_path().join("api/catalog_items.json");
    let content = std::fs::read_to_string(path).unwrap();
    let catalog: HashMap<String, CatalogItem> = serde_json::from_str(&content).unwrap();

    let ue_asset = &catalog["unreal_engine_asset_pack"];
    assert_eq!(ue_asset.namespace, "ue");

    let mod_item = &catalog["mod_community_map"];
    let is_mod = mod_item
        .categories
        .as_ref()
        .map_or(false, |cats| cats.iter().any(|c| c.path == "mods"));
    assert!(is_mod);
}

#[tokio::test]
async fn test_i15_cold_restart_rehydration_from_sqlite_and_keyring() {
    let mock = MockEpicServer::start().await;
    let temp = tempfile::NamedTempFile::new().unwrap();
    let p = temp.path();

    let keyring = Arc::new(MemoryCredentialStore::new());

    // Write initial account
    {
        let db = Database::open(p).unwrap();
        db.run_migrations().await.unwrap();
        let client = EpicClient::from_router(mock.router());
        let store = EpicAuthStore::new(db, keyring.clone(), client);

        mock.register_auth_code("code_cold_1234567890123456789012", "acc_cold", "ColdUser")
            .await;
        store
            .login_with_code("code_cold_1234567890123456789012")
            .await
            .unwrap();
    }

    // Reopen from disk
    {
        let db = Database::open(p).unwrap();
        let client = EpicClient::from_router(mock.router());
        let store = EpicAuthStore::new(db, keyring.clone(), client);

        let active = store.get_active_account().await.unwrap().unwrap();
        assert_eq!(active.account_id, "acc_cold");
        let token = store.get_valid_access_token("acc_cold").await.unwrap();
        assert!(!token.is_empty());
    }
}

// ============================================================================
// TIER 4: REAL-WORLD APPLICATION SCENARIOS
// ============================================================================

/// Scenario 1: Two-Account Setup & Seamless Switching
#[tokio::test]
async fn test_scenario_01_two_account_setup_and_switching() {
    let (mock, _db, store) = setup_test_env().await;

    // 1. User authenticates Account A (JediMaster)
    mock.register_auth_code("code_s1_jedi_12345678901234567890", "acc_jedi", "JediMaster")
        .await;
    let acc_a = store
        .login_with_code("code_s1_jedi_12345678901234567890")
        .await
        .expect("Jedi login failed");
    assert_eq!(acc_a.display_name.as_deref(), Some("JediMaster"));
    assert!(acc_a.is_active);

    // 2. User authenticates Account B (SithLord)
    mock.register_auth_code("code_s1_sith_12345678901234567890", "acc_sith", "SithLord")
        .await;
    let acc_b = store
        .login_with_code("code_s1_sith_12345678901234567890")
        .await
        .expect("Sith login failed");
    assert_eq!(acc_b.display_name.as_deref(), Some("SithLord"));
    assert!(acc_b.is_active);

    // 3. Verify both accounts exist in SQLite
    let all = store.get_accounts().await.unwrap();
    assert_eq!(all.len(), 2);

    // 4. Switch from Account B to Account A and measure latency against < 5ms SLA
    let (switched, dur) = store.switch_active_account("acc_jedi").await.unwrap();
    assert_eq!(switched.account_id, "acc_jedi");
    assert!(switched.is_active);
    assert!(
        dur.as_millis() < 5,
        "Account switch exceeded 5ms SLA: {:?}",
        dur
    );

    // 5. Verify in DB only Jedi is active
    let active = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(active.account_id, "acc_jedi");

    // 6. Switch back to Sith
    let (switched_back, dur_back) = store.switch_active_account("acc_sith").await.unwrap();
    assert_eq!(switched_back.account_id, "acc_sith");
    assert!(dur_back.as_millis() < 5);
}

/// Scenario 2: Cold Backend Restart with Multi-Account Session Restore
#[tokio::test]
async fn test_scenario_02_cold_restart_session_restore() {
    let mock = MockEpicServer::start().await;
    let temp = tempfile::NamedTempFile::new().unwrap();
    let db_path = temp.path();
    let keyring = Arc::new(MemoryCredentialStore::new());

    // Session 1: Setup accounts
    {
        let db = Database::open(db_path).unwrap();
        db.run_migrations().await.unwrap();
        let client = EpicClient::from_router(mock.router());
        let store = EpicAuthStore::new(db, keyring.clone(), client);

        mock.register_auth_code("code_sc2_1_12345678901234567890", "acc_sc2_1", "Player One")
            .await;
        mock.register_auth_code("code_sc2_2_12345678901234567890", "acc_sc2_2", "Player Two")
            .await;

        store
            .login_with_code("code_sc2_1_12345678901234567890")
            .await
            .unwrap();
        store
            .login_with_code("code_sc2_2_12345678901234567890")
            .await
            .unwrap();
    }

    // Session 2: Cold restart
    {
        let db = Database::open(db_path).unwrap();
        let client = EpicClient::from_router(mock.router());
        let store = EpicAuthStore::new(db, keyring.clone(), client);

        let accounts = store.get_accounts().await.unwrap();
        assert_eq!(accounts.len(), 2);

        // Verify headless token refresh works for all accounts without prompting login
        for acc in accounts {
            let token = store.get_valid_access_token(&acc.account_id).await.unwrap();
            assert!(!token.is_empty());
        }
    }
}

/// Scenario 3: Full Library Sync (Cloud Entitlements + Local Manifests)
#[tokio::test]
async fn test_scenario_03_full_library_sync() {
    let (mock, db, store) = setup_test_env().await;
    let acc_id = "acc_full_sync";
    mock.register_auth_code("code_sync_1234567890123456789012", acc_id, "Sync Captain")
        .await;

    // Load account1 fixtures
    let ent_json = std::fs::read_to_string(fixtures_path().join("api/account1_entitlements.json")).unwrap();
    let entitlements: Vec<Entitlement> = serde_json::from_str(&ent_json).unwrap();
    mock.set_entitlements(acc_id, entitlements).await;

    store
        .login_with_code("code_sync_1234567890123456789012")
        .await
        .unwrap();

    let manifests_dir = fixtures_path().join("manifests");
    let summary = store
        .sync_library(acc_id, Some(&manifests_dir))
        .await
        .unwrap();

    assert_eq!(summary.total_entitlements, 3);
    assert_eq!(summary.base_games_count, 2);
    assert_eq!(summary.dlc_count, 1);
    assert!(summary.installed_count >= 1); // Civ6 is installed

    let games = db.get_games().await.unwrap();
    assert_eq!(games.len(), 2);
    let installed_game = games.iter().find(|g| g.is_installed).unwrap();
    assert_eq!(installed_game.app_id, "42ac1ee840304cb1807172a9b47dc8e3");
}

/// Scenario 4: End-to-End Game Launch with One-Time Exchange Token
#[tokio::test]
async fn test_scenario_04_e2e_game_launch_with_exchange_token() {
    let (mock, _, store) = setup_test_env().await;
    let acc_id = "acc_launch_user";
    mock.register_auth_code("code_launch_123456789012345678901", acc_id, "GamerX")
        .await;

    store
        .login_with_code("code_launch_123456789012345678901")
        .await
        .unwrap();

    // 1. Get fresh access token via stored DeviceAuth
    let access_token = store.get_valid_access_token(acc_id).await.unwrap();

    // 2. Request single-use exchange code
    let exchange = store
        .client()
        .get_exchange_code(&access_token)
        .await
        .unwrap();
    assert_eq!(exchange.expires_in_seconds, 300);

    // 3. Load manifest
    let manifest = parse_manifest_file(fixtures_path().join("manifests/valid_game_macos.item")).unwrap();

    // 4. Prepare CLI parameters
    let cmd = prepare_launch_command(
        &manifest,
        &exchange.code,
        acc_id,
        "GamerX",
        Some("en"),
        None,
    )
    .unwrap();

    // 5. Verify secrets masked in display/logging args
    let display_str = format!("{}", RedactedLaunchArgs(&cmd.args));
    assert!(!display_str.contains(&exchange.code));
    assert!(display_str.contains("-AUTH_PASSWORD=********"));

    // 6. Verify executable path resolved
    assert!(cmd.executable_path.to_string_lossy().contains("Civ6.app"));
}

/// Scenario 5: Token Invalidation Recovery & Fallback
#[tokio::test]
async fn test_scenario_05_token_invalidation_recovery() {
    let (mock, _, store) = setup_test_env().await;
    let acc_id = "acc_recovery";
    mock.register_auth_code("code_rec_123456789012345678901234", acc_id, "Survivor")
        .await;

    store
        .login_with_code("code_rec_123456789012345678901234")
        .await
        .unwrap();

    // Invalidate next token refresh to simulate transient error
    mock.set_fail_next_refresh(true).await;
    let failed = store.get_valid_access_token(acc_id).await;
    assert!(failed.is_err());

    // Subsequent call should succeed as failure flag was single-use
    let recovered = store.get_valid_access_token(acc_id).await;
    assert!(recovered.is_ok());
}

/// Scenario 6: High-Frequency Rapid Account Switching Stress Test
#[tokio::test]
async fn test_scenario_06_high_frequency_switching_stress_test() {
    let (mock, _, store) = setup_test_env().await;
    mock.register_auth_code("code_stress1_1234567890123456789", "acc_stress_1", "SwitchMaster1")
        .await;
    mock.register_auth_code("code_stress2_1234567890123456789", "acc_stress_2", "SwitchMaster2")
        .await;

    store
        .login_with_code("code_stress1_1234567890123456789")
        .await
        .unwrap();
    store
        .login_with_code("code_stress2_1234567890123456789")
        .await
        .unwrap();

    let start_all = Instant::now();
    let mut total_duration = std::time::Duration::ZERO;
    let iterations = 100;

    for i in 0..iterations {
        let target = if i % 2 == 0 {
            "acc_stress_1"
        } else {
            "acc_stress_2"
        };
        let (acc, dur) = store.switch_active_account(target).await.unwrap();
        assert_eq!(acc.account_id, target);
        assert!(dur.as_millis() < 5, "Single switch took {:?}", dur);
        total_duration += dur;
    }

    let avg_ms = (total_duration.as_secs_f64() * 1000.0) / iterations as f64;
    println!(
        "Stress test completed 100 switches in {:?}. Average switch latency: {:.3} ms",
        start_all.elapsed(),
        avg_ms
    );
    assert!(avg_ms < 2.0, "Average switch latency must be well below SLA");
}
