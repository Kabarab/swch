//! Adversarial Empirical Challenger Test Suite for Milestone 3 Gate 2
//! Focus:
//! 1. Secret Leakage Prevention in HTTP launch responses (assert -AUTH_PASSWORD is never leaked unmasked)
//!    - Byte-by-byte raw HTTP response payload inspection
//!    - Injection of counterfeit auth flags in custom arguments
//!    - Error payload inspection
//! 2. REST Status Code Invariants (400, 401, 404)
//!    - Empty JSON `{}` -> 400 Bad Request
//!    - Missing / whitespace `app_name` -> 400 Bad Request
//!    - Missing active account -> 400 Bad Request
//!    - Incomplete installation manifest -> 400 Bad Request
//!    - DLC launch attempt -> 400 Bad Request
//!    - Non-installed game without manifest in live mode -> 404 Not Found
//!    - Non-existent account -> 404 Not Found
//!    - Missing DeviceAuth in Keyring -> 404 Not Found
//!    - Invalid/expired DeviceAuth credentials -> 401 Unauthorized
//! 3. Username polymorphic resolution
//! 4. High concurrency zero-crosstalk secret isolation

mod common;

use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use tempfile::tempdir;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use common::mock_server::MockEpicServer;
use swch::launchers::epic::{
    client::EpicClient,
    keyring::{CredentialStore, MemoryCredentialStore},
    models::DeviceAuth,
    store::EpicAuthStore,
};
use swch::server::{create_app_with_state, AppState};
use swch::storage::{Account, Database};

static GATE2_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

async fn setup_gate2_test_app() -> (
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

// ============================================================================
// 1. Secret Leakage Prevention in HTTP Responses (Raw Byte-by-Byte Inspection)
// ============================================================================

#[tokio::test]
async fn test_adversarial_raw_http_body_secret_leakage_byte_by_byte() {
    let _lock = GATE2_ENV_LOCK.lock().unwrap();
    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let (mock, _db, _keyring, store, app) = setup_gate2_test_app().await;

    // Login account (32-character hex code)
    let code = "11112222333344445555666677778888";
    let acc_id = "acc_byte_leak_tester";
    mock.register_auth_code(code, acc_id, "ByteLeakTester").await;
    store.login_with_code(code).await.expect("Login failed");

    let request_body = json!({
        "account_id": acc_id,
        "app_name": "Sugar",
        "namespace": "test_namespace",
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body_bytes.to_vec()).expect("Response must be valid UTF-8 JSON");

    // EMPIRICAL ASSERTION 1: No cleartext exchange code pattern anywhere in the raw bytes!
    // MockEpicServer issues exchange codes starting with "mock_exchange_code_"
    assert!(
        !body_str.contains("mock_exchange_code_"),
        "CRITICAL LEAK: Raw HTTP response contains cleartext exchange code! Full body: {}",
        body_str
    );

    // EMPIRICAL ASSERTION 2: No unmasked -AUTH_PASSWORD= in raw bytes
    // Any occurrence of -AUTH_PASSWORD= MUST be immediately followed by ********
    let mut search_idx = 0;
    while let Some(found) = body_str[search_idx..].find("-AUTH_PASSWORD=") {
        let actual_idx = search_idx + found;
        let snippet = &body_str[actual_idx..std::cmp::min(actual_idx + 25, body_str.len())];
        assert!(
            snippet.starts_with("-AUTH_PASSWORD=********"),
            "CRITICAL LEAK: -AUTH_PASSWORD is not masked with ********! Found snippet: {}",
            snippet
        );
        search_idx = actual_idx + "-AUTH_PASSWORD=".len();
    }

    // EMPIRICAL ASSERTION 3: Parse JSON and verify fields
    let json: Value = serde_json::from_str(&body_str).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["app_name"], "Sugar");

    let launch_args = json["launch_arguments"].as_array().expect("launch_arguments must be array");
    let display_args = json["display_arguments"].as_array().expect("display_arguments must be array");

    for arg in launch_args {
        let s = arg.as_str().unwrap();
        assert!(!s.contains("mock_exchange_code"), "launch_arguments leaked secret: {}", s);
        if s.to_ascii_uppercase().starts_with("-AUTH_PASSWORD=") {
            assert_eq!(s, "-AUTH_PASSWORD=********");
        }
    }

    for arg in display_args {
        let s = arg.as_str().unwrap();
        assert!(!s.contains("mock_exchange_code"), "display_arguments leaked secret: {}", s);
        if s.to_ascii_uppercase().starts_with("-AUTH_PASSWORD=") {
            assert_eq!(s, "-AUTH_PASSWORD=********");
        }
    }
}

#[tokio::test]
async fn test_adversarial_alias_endpoint_raw_http_body_secret_leakage() {
    let _lock = GATE2_ENV_LOCK.lock().unwrap();
    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let (mock, _db, _keyring, store, app) = setup_gate2_test_app().await;

    let code = "22223333444455556666777788889999";
    let acc_id = "acc_alias_leak_tester";
    mock.register_auth_code(code, acc_id, "AliasLeakTester").await;
    store.login_with_code(code).await.expect("Login failed");

    let request_body = json!({
        "account_id": acc_id,
        "app_name": "Fortnite",
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch-game")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

    assert!(
        !body_str.contains("mock_exchange_code_"),
        "Raw HTTP response on alias endpoint leaked exchange token: {}",
        body_str
    );
    assert!(body_str.contains("-AUTH_PASSWORD=********"));
}

#[tokio::test]
async fn test_adversarial_custom_args_counterfeit_auth_flags_masked() {
    let _lock = GATE2_ENV_LOCK.lock().unwrap();
    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let (mock, _db, _keyring, store, app) = setup_gate2_test_app().await;

    let code = "33334444555566667777888899990000";
    let acc_id = "acc_custom_flag_attacker";
    mock.register_auth_code(code, acc_id, "Attacker").await;
    store.login_with_code(code).await.expect("Login failed");

    // Attempt to inject multiple case variations and tokens via additional_args
    let malicious_custom_args = vec![
        "-AUTH_PASSWORD=counterfeit_secret_123".to_string(),
        "-auth_password=lowercase_secret_456".to_string(),
        "-Auth_Password=mixed_secret_789".to_string(),
        "-EPICOVT=uppercase_ovt_token".to_string(),
        "-epicovt=lowercase_ovt_token".to_string(),
        "-EpicOvt=mixed_ovt_token".to_string(),
    ];

    let request_body = json!({
        "account_id": acc_id,
        "app_name": "Sugar",
        "additional_args": malicious_custom_args,
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

    // Verify NONE of the injected secrets appear anywhere in the HTTP response
    assert!(!body_str.contains("counterfeit_secret_123"));
    assert!(!body_str.contains("lowercase_secret_456"));
    assert!(!body_str.contains("mixed_secret_789"));
    assert!(!body_str.contains("uppercase_ovt_token"));
    assert!(!body_str.contains("lowercase_ovt_token"));
    assert!(!body_str.contains("mixed_ovt_token"));
    assert!(!body_str.contains("mock_exchange_code_"));

    let json: Value = serde_json::from_str(&body_str).unwrap();
    let launch_args = json["launch_arguments"].as_array().unwrap();
    let display_args = json["display_arguments"].as_array().unwrap();

    // Assert that each -AUTH_PASSWORD flag is masked to -AUTH_PASSWORD=********
    for args in [launch_args, display_args] {
        let pw_flags: Vec<&str> = args
            .iter()
            .map(|v| v.as_str().unwrap())
            .filter(|s| s.to_ascii_uppercase().starts_with("-AUTH_PASSWORD="))
            .collect();
        // 1 primary + 3 injected = 4 total -AUTH_PASSWORD flags
        assert_eq!(pw_flags.len(), 4);
        for flag in pw_flags {
            assert_eq!(flag, "-AUTH_PASSWORD=********");
        }

        let ovt_flags: Vec<&str> = args
            .iter()
            .map(|v| v.as_str().unwrap())
            .filter(|s| s.to_ascii_lowercase().starts_with("-epicovt="))
            .collect();
        // 3 injected -epicovt flags
        assert_eq!(ovt_flags.len(), 3);
        for flag in ovt_flags {
            assert_eq!(flag, "-epicovt=[REDACTED]");
        }
    }
}

// ============================================================================
// 2. REST Status Code Invariants (400, 401, 404)
// ============================================================================

#[tokio::test]
async fn test_adversarial_rest_status_400_empty_json() {
    let (_mock, _db, _keyring, _store, app) = setup_gate2_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Empty JSON body must return 400 Bad Request"
    );
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["error"], "app_name must not be empty");
}

#[tokio::test]
async fn test_adversarial_rest_status_400_whitespace_app_names() {
    let (_mock, _db, _keyring, _store, app) = setup_gate2_test_app().await;

    let variations = vec![
        r#"{"app_name": ""}"#,
        r#"{"app_name": " "}"#,
        r#"{"app_name": "   "}"#,
        r#"{"app_name": "\t"}"#,
        r#"{"app_name": "\n\r  \t"}"#,
    ];

    for body in variations {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/epic/launch")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "Whitespace app_name '{}' must return 400 Bad Request",
            body
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "app_name must not be empty");
    }
}

#[tokio::test]
async fn test_adversarial_rest_status_400_no_active_account() {
    let (_mock, _db, _keyring, _store, app) = setup_gate2_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"app_name": "Fortnite"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Launch without account_id and no active account must return 400 Bad Request"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["error"].as_str().unwrap().contains("No active Epic account"));
}

#[tokio::test]
async fn test_adversarial_rest_status_400_incomplete_install_manifest() {
    let _lock = GATE2_ENV_LOCK.lock().unwrap();
    let (mock, _db, _keyring, store, app) = setup_gate2_test_app().await;

    let code = "44445555666677778888999900001111";
    let acc_id = "acc_inc_user";
    mock.register_auth_code(code, acc_id, "IncompleteUser").await;
    store.login_with_code(code).await.expect("Login failed");

    let temp_manifest_dir = tempdir().unwrap();
    let manifest_file_path = temp_manifest_dir.path().join("IncompleteGame.item");
    let incomplete_json = r#"{
        "FormatVersion": 0,
        "AppName": "IncompleteGame",
        "CatalogItemId": "cat_inc_123",
        "CatalogNamespace": "fn",
        "DisplayName": "Incomplete Game Title",
        "InstallationGuid": "{F0876123-1111-2222-3333-444455556666}",
        "InstallLocation": "/games/IncompleteGame",
        "InstallSize": 1000,
        "LaunchExecutable": "game.exe",
        "bIsIncompleteInstall": true
    }"#;
    let mut f = File::create(&manifest_file_path).unwrap();
    f.write_all(incomplete_json.as_bytes()).unwrap();
    drop(f);

    std::env::set_var("SWCH_EPIC_MANIFESTS_DIR", temp_manifest_dir.path().to_str().unwrap());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_id": "acc_inc_user", "app_name": "IncompleteGame"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    std::env::remove_var("SWCH_EPIC_MANIFESTS_DIR");

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Incomplete install must return 400 Bad Request"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["error"].as_str().unwrap().contains("incomplete installation"));
}

#[tokio::test]
async fn test_adversarial_rest_status_400_dlc_launch_attempt() {
    let _lock = GATE2_ENV_LOCK.lock().unwrap();
    let (mock, _db, _keyring, store, app) = setup_gate2_test_app().await;

    let code = "55556666777788889999000011112222";
    let acc_id = "acc_dlc_tester";
    mock.register_auth_code(code, acc_id, "DlcTester").await;
    store.login_with_code(code).await.expect("Login failed");

    let temp_manifest_dir = tempdir().unwrap();
    let manifest_file_path = temp_manifest_dir.path().join("DlcGame.item");
    let dlc_json = r#"{
        "FormatVersion": 0,
        "AppName": "DlcContent",
        "CatalogItemId": "cat_dlc_999",
        "CatalogNamespace": "fn",
        "DisplayName": "Epic Soundtrack DLC",
        "InstallationGuid": "{F0876123-1111-2222-3333-444455556666}",
        "InstallLocation": "/games/MainGame",
        "InstallSize": 500,
        "LaunchExecutable": "main.exe",
        "MainGameAppName": "MainGame",
        "bIsIncompleteInstall": false
    }"#;
    let mut f = File::create(&manifest_file_path).unwrap();
    f.write_all(dlc_json.as_bytes()).unwrap();
    drop(f);

    std::env::set_var("SWCH_EPIC_MANIFESTS_DIR", temp_manifest_dir.path().to_str().unwrap());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_id": "acc_dlc_tester", "app_name": "DlcContent"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    std::env::remove_var("SWCH_EPIC_MANIFESTS_DIR");

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "DLC launch attempt must return 400 Bad Request"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["error"].as_str().unwrap().contains("Cannot directly launch DLC"));
}

#[tokio::test]
async fn test_adversarial_rest_status_404_nonexistent_account() {
    let (_mock, _db, _keyring, _store, app) = setup_gate2_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_id": "phantom_account_999", "app_name": "Sugar"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "Nonexistent account must return 404 Not Found"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_adversarial_rest_status_404_missing_device_auth_in_keyring() {
    let (_mock, db, _keyring, _store, app) = setup_gate2_test_app().await;

    let account = Account {
        id: "acc_keyring_empty".to_string(),
        launcher: "epic".to_string(),
        username: "KeylessUser".to_string(),
        display_name: Some("KeylessUser".to_string()),
        email: None,
        is_active: true,
        last_used: None,
    };
    db.upsert_account(&account).await.unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_id": "acc_keyring_empty", "app_name": "Sugar"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "Missing DeviceAuth in Keyring must return 404 Not Found"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["error"].as_str().unwrap().contains("credentials not found"));
}

#[tokio::test]
async fn test_adversarial_rest_status_404_non_installed_game_live_mode() {
    let _lock = GATE2_ENV_LOCK.lock().unwrap();
    // Ensure simulation mode is strictly OFF
    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

    let (mock, _db, _keyring, store, app) = setup_gate2_test_app().await;

    let code = "66667777888899990000111122223333";
    let acc_id = "acc_live_404_tester";
    mock.register_auth_code(code, acc_id, "Live404Tester").await;
    store.login_with_code(code).await.expect("Login failed");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_id": "acc_live_404_tester", "app_name": "UninstalledGameXYZ"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "Uninstalled game without manifest in live mode must return 404 Not Found"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("Manifest or installation not found"),
        "Error message should mention manifest or installation not found: {:?}",
        json
    );
}

#[tokio::test]
async fn test_adversarial_rest_status_401_invalid_device_auth_credentials() {
    let (_mock, db, keyring, _store, app) = setup_gate2_test_app().await;

    let acc_id = "acc_bad_creds_tester";
    let account = Account {
        id: acc_id.to_string(),
        launcher: "epic".to_string(),
        username: "BadCredsUser".to_string(),
        display_name: Some("BadCredsUser".to_string()),
        email: None,
        is_active: true,
        last_used: None,
    };
    db.upsert_account(&account).await.unwrap();

    // Store invalid secret in keyring
    let bad_auth = DeviceAuth {
        account_id: acc_id.to_string(),
        device_id: "dev_bad_id".to_string(),
        secret: "invalid_secret_rejected_by_epic".to_string(),
        user_agent: None,
        created: None,
        ..Default::default()
    };
    keyring.set_device_auth(&bad_auth).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_id": "acc_bad_creds_tester", "app_name": "Sugar"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Invalid DeviceAuth credentials rejected by Epic must return 401 Unauthorized"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("authorization error")
            || json["error"].as_str().unwrap().contains("invalid_account_credentials")
    );
}

// ============================================================================
// 3. Account ID & Username Polymorphic Canonicalization
// ============================================================================

#[tokio::test]
async fn test_adversarial_username_polymorphic_resolution() {
    let _lock = GATE2_ENV_LOCK.lock().unwrap();
    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let (mock, _db, _keyring, store, app) = setup_gate2_test_app().await;

    let code = "77778888999900001111222233334444";
    let acc_id = "acc_shepard_canonical_id";
    let username = "CommanderShepard";
    mock.register_auth_code(code, acc_id, username).await;
    store.login_with_code(code).await.expect("Login failed");

    // Launch using USERNAME instead of account ID
    let request_body = json!({
        "account_id": username,
        "app_name": "Sugar",
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Launch with username parameter must resolve canonical account and succeed"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "success");

    let display_args: Vec<String> = json["display_arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    // Verify canonical account ID used in flags
    assert!(display_args.contains(&format!("-epicuserid={}", acc_id)));
    assert!(display_args.contains(&format!("-epicusername={}", username)));
    assert!(display_args.contains(&"-AUTH_PASSWORD=********".to_string()));
}

// ============================================================================
// 4. Concurrent Requests & Secret Isolation
// ============================================================================

#[tokio::test]
async fn test_adversarial_concurrent_requests_secret_isolation() {
    let _lock = GATE2_ENV_LOCK.lock().unwrap();
    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let (mock, _db, _keyring, store, app) = setup_gate2_test_app().await;

    // Account 1
    let code1 = "88889999000011112222333344445555";
    let id1 = "acc_iso_user_1";
    let name1 = "IsoUserOne";
    mock.register_auth_code(code1, id1, name1).await;
    store.login_with_code(code1).await.unwrap();

    // Account 2
    let code2 = "99990000111122223333444455556666";
    let id2 = "acc_iso_user_2";
    let name2 = "IsoUserTwo";
    mock.register_auth_code(code2, id2, name2).await;
    store.login_with_code(code2).await.unwrap();

    // 40 concurrent HTTP requests through Axum router
    let mut tasks = Vec::new();
    for i in 0..40 {
        let app_clone = app.clone();
        let target_id = if i % 2 == 0 { id1 } else { id2 };
        let app_name = if i % 2 == 0 { "GameA" } else { "GameB" };

        tasks.push(tokio::spawn(async move {
            let req_body = json!({
                "account_id": target_id,
                "app_name": app_name,
            });
            let response = app_clone
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/epic/launch")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
            (target_id, status, body_bytes)
        }));
    }

    let mut responses = Vec::new();
    for t in tasks {
        responses.push(t.await.unwrap());
    }
    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

    assert_eq!(responses.len(), 40);
    for (target_id, status, body_bytes) in responses {
        assert_eq!(status, StatusCode::OK);
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        // 1. NEVER contains cleartext exchange code
        assert!(!body_str.contains("mock_exchange_code"));

        // 2. ALWAYS contains masked -AUTH_PASSWORD=********
        assert!(body_str.contains("-AUTH_PASSWORD=********"));

        // 3. ZERO account crosstalk
        if target_id == id1 {
            assert!(body_str.contains(id1));
            assert!(!body_str.contains(id2));
        } else {
            assert!(body_str.contains(id2));
            assert!(!body_str.contains(id1));
        }
    }
}
