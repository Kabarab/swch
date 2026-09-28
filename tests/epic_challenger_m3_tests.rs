//! Adversarial Empirical Challenger Test Suite for Milestone 3 (Challenger 2)
//!
//! Rigorous stress-testing of:
//! 1. REST API Error Responses & Status Code Invariants:
//!    - POST /api/epic/launch with empty JSON ({})
//!    - POST /api/epic/launch with empty app_name: "" and whitespace app_name: "   "
//!    - POST /api/epic/launch when no account is active and no account_id provided
//!    - POST /api/epic/launch with non-existent account_id: "nonexistent_account"
//!    - POST /api/epic/launch with invalid or expired credentials (401 vs 500 status analysis)
//!    - POST /api/epic/launch with game marked as incomplete install (400 Bad Request)
//!    - POST /api/epic/launch with DLC addon (cannot launch DLC)
//! 2. Launch Payload Sanitization & Command Injection Defense:
//!    - Shell metacharacters injection via additional_args (; && | $() `` \n)
//!    - Credential leakage prevention across HTTP responses, error payloads, and display arguments
//! 3. Store Concurrency & Multi-Account Execution:
//!    - Rapid concurrent launch requests for two distinct accounts simultaneously
//!    - Account context flags verification (-epicusername, -epicuserid) with zero crosstalk
//!    - Unique single-use exchange codes per concurrent launch
//!    - Concurrent account switching vs launching race conditions

mod common;

use std::collections::HashSet;
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

async fn setup_challenger_test_app() -> (
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
// AREA 1: REST API Error Responses & Status Code Invariants
// ============================================================================

static CHALLENGER_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[tokio::test]
async fn test_challenge_launch_empty_json_body() {
    let (_mock, _db, _keyring, _store, app) = setup_challenger_test_app().await;

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

    let status = response.status();
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8_lossy(&body_bytes);

    println!("Empty JSON response status: {}, body: {}", status, body_str);

    // Hardened behavior:
    // With #[serde(default)] on app_name in LaunchGameRequest, empty JSON payload ({})
    // deserializes cleanly and triggers 400 Bad Request with standardized error body.
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "Expected 400 Bad Request for empty JSON body with missing app_name"
    );
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["error"].as_str().unwrap(), "app_name must not be empty");
}

#[tokio::test]
async fn test_challenge_launch_empty_and_whitespace_app_name() {
    let (_mock, _db, _keyring, _store, app) = setup_challenger_test_app().await;

    // Test 1: Empty string
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"app_name": ""}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(json["error"].as_str().unwrap().contains("app_name must not be empty"));

    // Test 2: Whitespace only
    let response_ws = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"app_name": "    \t\n  "}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response_ws.status(), StatusCode::BAD_REQUEST);
    let body_ws = response_ws.into_body().collect().await.unwrap().to_bytes();
    let json_ws: Value = serde_json::from_slice(&body_ws).unwrap();
    assert!(json_ws["error"].as_str().unwrap().contains("app_name must not be empty"));
}

#[tokio::test]
async fn test_challenge_launch_no_active_account_and_no_account_id() {
    let (_mock, _db, _keyring, _store, app) = setup_challenger_test_app().await;

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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("No active Epic account"),
        "Unexpected error: {:?}",
        json
    );
}

#[tokio::test]
async fn test_challenge_launch_nonexistent_account_id() {
    let (_mock, _db, _keyring, _store, app) = setup_challenger_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"account_id": "nonexistent_account_9999", "app_name": "Fortnite"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("not found"),
        "Expected error to mention not found, got: {:?}",
        json
    );
}

#[tokio::test]
async fn test_challenge_launch_missing_device_credentials_returns_404() {
    let (_mock, db, _keyring, _store, app) = setup_challenger_test_app().await;

    // Account exists in DB, but no credentials in Keyring
    let account = Account {
        id: "acc_without_keys".to_string(),
        launcher: "epic".to_string(),
        username: "user_without_keys".to_string(),
        display_name: Some("GhostUser".to_string()),
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
                .body(Body::from(
                    r#"{"account_id": "acc_without_keys", "app_name": "Fortnite"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("credentials not found")
            || json["error"].as_str().unwrap().contains("not found"),
        "Unexpected error: {:?}",
        json
    );
}

#[tokio::test]
async fn test_challenge_launch_invalid_device_credentials_status_analysis() {
    let (_mock, db, keyring, _store, app) = setup_challenger_test_app().await;

    let acc_id = "acc_invalid_secret_user";
    let account = Account {
        id: acc_id.to_string(),
        launcher: "epic".to_string(),
        username: "invalid_secret_user".to_string(),
        display_name: Some("InvalidSecretUser".to_string()),
        email: None,
        is_active: true,
        last_used: None,
    };
    db.upsert_account(&account).await.unwrap();

    // Store invalid DeviceAuth in keyring
    let bad_device_auth = DeviceAuth {
        account_id: acc_id.to_string(),
        device_id: "dev_bad_123".to_string(),
        secret: "bad_secret_not_valid".to_string(),
        user_agent: None,
        created: None,
        ..Default::default()
    };
    keyring.set_device_auth(&bad_device_auth).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"account_id": "acc_invalid_secret_user", "app_name": "Fortnite"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    println!(
        "Invalid DeviceAuth launch response: status={}, body={:?}",
        status, json
    );

    // Hardened behavior:
    // When Epic's OAuth token endpoint rejects invalid DeviceAuth credentials, it returns HTTP 400
    // with errorCode `invalid_account_credentials`. In `launch_epic_game` (src/server/handlers.rs),
    // this is properly mapped to 401 Unauthorized.
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "Expected 401 Unauthorized when credentials are invalid"
    );
    assert!(
        json["error"].as_str().unwrap().contains("authorization error")
            || json["error"].as_str().unwrap().contains("invalid_account_credentials")
    );
}

#[tokio::test]
async fn test_challenge_launch_incomplete_install_returns_400() {
    let _env_lock = CHALLENGER_ENV_LOCK.lock().unwrap();
    let (mock, _db, _keyring, store, app) = setup_challenger_test_app().await;

    // Login account (32-char hex code)
    let code = "33333333333333333333333333333333";
    let acc_id = "acc_incomplete_user";
    mock.register_auth_code(code, acc_id, "IncUser").await;
    store.login_with_code(code).await.expect("Login failed");

    // Create a temporary manifests directory containing an incomplete install manifest
    let temp_manifest_dir = tempdir().unwrap();
    let manifest_file_path = temp_manifest_dir.path().join("Fortnite_inc.item");
    let incomplete_json = r#"{
        "FormatVersion": 0,
        "AppName": "Fortnite",
        "CatalogItemId": "4fe75bbc5a674f4f9b356b5c90567da5",
        "CatalogNamespace": "fn",
        "DisplayName": "Fortnite Incomplete",
        "InstallationGuid": "{F0876123-1111-2222-3333-444455556666}",
        "InstallLocation": "/games/Fortnite",
        "InstallSize": 45000000000,
        "LaunchExecutable": "FortniteClient.exe",
        "bIsIncompleteInstall": true
    }"#;
    let mut f = File::create(&manifest_file_path).unwrap();
    f.write_all(incomplete_json.as_bytes()).unwrap();
    drop(f);

    // Point SWCH_EPIC_MANIFESTS_DIR to this directory
    std::env::set_var("SWCH_EPIC_MANIFESTS_DIR", temp_manifest_dir.path().to_str().unwrap());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"account_id": "acc_incomplete_user", "app_name": "Fortnite"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Clean up env
    std::env::remove_var("SWCH_EPIC_MANIFESTS_DIR");

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Expected 400 Bad Request for incomplete installation"
    );
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("incomplete installation"),
        "Expected error message to mention incomplete installation, got: {:?}",
        json
    );
}

#[tokio::test]
async fn test_challenge_launch_dlc_addon_status_analysis() {
    let _env_lock = CHALLENGER_ENV_LOCK.lock().unwrap();
    let (mock, _db, _keyring, store, app) = setup_challenger_test_app().await;

    // Login account (32-char hex code)
    let code = "44444444444444444444444444444444";
    let acc_id = "acc_dlc_user";
    mock.register_auth_code(code, acc_id, "DlcUser").await;
    store.login_with_code(code).await.expect("Login failed");

    // Create a temporary manifests directory containing a DLC manifest
    let temp_manifest_dir = tempdir().unwrap();
    let manifest_file_path = temp_manifest_dir.path().join("DlcAddon.item");
    let dlc_json = r#"{
        "FormatVersion": 0,
        "AppName": "DlcApp",
        "CatalogItemId": "cat_dlc",
        "CatalogNamespace": "fn",
        "DisplayName": "Expansion Pack",
        "InstallationGuid": "{F0876123-1111-2222-3333-444455556666}",
        "InstallLocation": "/games/BaseGame",
        "InstallSize": 1000,
        "LaunchExecutable": "base.exe",
        "MainGameAppName": "BaseGame",
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
                .body(Body::from(
                    r#"{"account_id": "acc_dlc_user", "app_name": "DlcApp"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    std::env::remove_var("SWCH_EPIC_MANIFESTS_DIR");

    let status = response.status();
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    println!("DLC launch status: {}, body: {:?}", status, json);

    // Hardened behavior:
    // When a client attempts to launch a DLC directly, `store.rs` returns
    // `EpicError::Database("Cannot directly launch DLC...")`.
    // In `handlers.rs`, this is properly mapped to 400 Bad Request.
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "Expected 400 Bad Request when attempting to directly launch DLC"
    );
    assert!(json["error"].as_str().unwrap().contains("Cannot directly launch DLC"));
}

// ============================================================================
// AREA 2: Launch Payload Sanitization & Command Injection
// ============================================================================

#[tokio::test]
async fn test_challenge_command_injection_sanitization() {
    let _env_lock = CHALLENGER_ENV_LOCK.lock().unwrap();
    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let (mock, _db, _keyring, store, app) = setup_challenger_test_app().await;

    let code = "55555555555555555555555555555555";
    let acc_id = "acc_injection_test";
    mock.register_auth_code(code, acc_id, "SecOps").await;
    store.login_with_code(code).await.expect("Login failed");

    // Attack payload containing dangerous shell metacharacters
    let malicious_args = vec![
        "; echo PWNED ;".to_string(),
        "&& rm -rf /".to_string(),
        "| cat /etc/passwd".to_string(),
        "$(whoami)".to_string(),
        "`id`".to_string(),
        "arg with spaces and \"quotes\"".to_string(),
        "--custom-flag=test\nnewline".to_string(),
        "-AUTH_PASSWORD=attempted_override".to_string(),
        "-epicovt=attempted_token_override".to_string(),
    ];

    let request_body = json!({
        "account_id": acc_id,
        "app_name": "Sugar",
        "additional_args": malicious_args,
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

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["status"], "success");

    let launch_args: Vec<String> = json["launch_arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    let display_args: Vec<String> = json["display_arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    // Verify tokenized arguments preserved as discrete argv elements (no subshell interpolation)
    assert!(launch_args.iter().any(|a| a == "; echo PWNED ;"));
    assert!(launch_args.iter().any(|a| a == "&& rm -rf /"));
    assert!(launch_args.iter().any(|a| a == "| cat /etc/passwd"));
    assert!(launch_args.iter().any(|a| a == "$(whoami)"));
    assert!(launch_args.iter().any(|a| a == "`id`"));

    // Verify display_arguments masks ALL sensitive tokens
    for arg in &display_args {
        if arg.to_ascii_uppercase().starts_with("-AUTH_PASSWORD=") {
            assert_eq!(arg, "-AUTH_PASSWORD=********");
        }
        if arg.to_ascii_lowercase().starts_with("-epicovt=") {
            assert_eq!(arg, "-epicovt=[REDACTED]");
        }
    }
    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");
}

#[tokio::test]
async fn test_challenge_no_secret_leakage_in_error_responses() {
    let (mock, _db, _keyring, store, app) = setup_challenger_test_app().await;

    let code = "66666666666666666666666666666666";
    let acc_id = "acc_leak_test";
    mock.register_auth_code(code, acc_id, "LeakTester").await;
    store.login_with_code(code).await.expect("Login failed");

    // Force exchange code generation failure
    mock.set_fail_next_exchange(true).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_id": "acc_leak_test", "app_name": "Sugar"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8_lossy(&body_bytes);

    // Verify raw secret or token fragments never appear in the error output
    assert!(!body_str.contains("secret_"));
    assert!(!body_str.contains("device_"));
    assert!(!body_str.contains("mock_exchange_code"));
}

// ============================================================================
// AREA 3: Store Concurrency & Multi-Account Execution
// ============================================================================

#[tokio::test]
async fn test_challenge_concurrent_launches_multi_account_zero_crosstalk() {
    let _env_lock = CHALLENGER_ENV_LOCK.lock().unwrap();
    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let (mock, _db, _keyring, store, _app) = setup_challenger_test_app().await;

    // Account 1: Alice (32-char hex code)
    let code_alice = "77777777777777777777777777777777";
    let id_alice = "acc_alice_777";
    let name_alice = "Alice_Wonderland";
    mock.register_auth_code(code_alice, id_alice, name_alice).await;
    store.login_with_code(code_alice).await.unwrap();

    // Account 2: Bob (32-char hex code)
    let code_bob = "88888888888888888888888888888888";
    let id_bob = "acc_bob_888";
    let name_bob = "Bob_The_Builder";
    mock.register_auth_code(code_bob, id_bob, name_bob).await;
    store.login_with_code(code_bob).await.unwrap();

    // Fire 60 concurrent launch requests: 30 for Alice, 30 for Bob
    let store_arc = store.clone();
    let mut tasks = Vec::new();

    for i in 0..60 {
        let store_clone = store_arc.clone();
        let (target_id, expected_name, app_name) = if i % 2 == 0 {
            (id_alice, name_alice, "Sugar")
        } else {
            (id_bob, name_bob, "Fortnite")
        };

        tasks.push(tokio::spawn(async move {
            let res = store_clone
                .launch_game(Some(target_id), app_name, None, None)
                .await;
            (target_id, expected_name, app_name, res)
        }));
    }

    let mut results = Vec::new();
    for t in tasks {
        results.push(t.await.unwrap());
    }
    assert_eq!(results.len(), 60);

    let mut seen_exchange_codes = HashSet::new();

    for (target_id, expected_name, app_name, launch_res) in results {
        let launch = launch_res.expect("Concurrent launch must succeed");
        assert_eq!(launch.app_name, app_name);

        let raw_args = &launch.launch_arguments;

        // Check 1: -epicuserid must match target_id exactly
        let expected_user_id_arg = format!("-epicuserid={}", target_id);
        assert!(
            raw_args.contains(&expected_user_id_arg),
            "Expected {}, got args: {:?}",
            expected_user_id_arg,
            raw_args
        );

        // Check 2: -epicusername must match expected_name exactly
        let expected_user_name_arg = format!("-epicusername={}", expected_name);
        assert!(
            raw_args.contains(&expected_user_name_arg),
            "Expected {}, got args: {:?}",
            expected_user_name_arg,
            raw_args
        );

        // Check 3: ZERO crosstalk!
        if target_id == id_alice {
            assert!(!raw_args.iter().any(|a| a.contains(id_bob)));
            assert!(!raw_args.iter().any(|a| a.contains(name_bob)));
        } else {
            assert!(!raw_args.iter().any(|a| a.contains(id_alice)));
            assert!(!raw_args.iter().any(|a| a.contains(name_alice)));
        }

        // Check 4: Unique exchange code extraction
        let pw_arg = raw_args
            .iter()
            .find(|a| a.starts_with("-AUTH_PASSWORD="))
            .expect("Must have -AUTH_PASSWORD");
        let exchange_code = pw_arg.strip_prefix("-AUTH_PASSWORD=").unwrap();
        assert!(
            seen_exchange_codes.insert(exchange_code.to_string()),
            "Duplicate exchange code detected: {}",
            exchange_code
        );

        // Check 5: Display args are masked
        assert!(launch
            .display_arguments
            .contains(&"-AUTH_PASSWORD=********".to_string()));
    }

    assert_eq!(
        seen_exchange_codes.len(),
        60,
        "All 60 launches must receive unique exchange tokens"
    );
    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");
}

#[tokio::test]
async fn test_challenge_concurrent_account_switching_and_launching() {
    let _env_lock = CHALLENGER_ENV_LOCK.lock().unwrap();
    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let (mock, _db, _keyring, store, _app) = setup_challenger_test_app().await;

    let code_alice = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let id_alice = "acc_alice_sw";
    let name_alice = "Alice_Sw";
    mock.register_auth_code(code_alice, id_alice, name_alice).await;
    store.login_with_code(code_alice).await.unwrap();

    let code_bob = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let id_bob = "acc_bob_sw";
    let name_bob = "Bob_Sw";
    mock.register_auth_code(code_bob, id_bob, name_bob).await;
    store.login_with_code(code_bob).await.unwrap();

    let store_arc = store.clone();
    let mut switch_tasks = Vec::new();

    // 20 concurrent switch tasks
    for i in 0..20 {
        let store_clone = store_arc.clone();
        let target = if i % 2 == 0 { id_alice } else { id_bob };
        switch_tasks.push(tokio::spawn(async move {
            store_clone.switch_active_account(target).await
        }));
    }

    // 20 concurrent launch tasks relying on the active account (account_id = None)
    let mut launch_tasks = Vec::new();
    for _ in 0..20 {
        let store_clone = store_arc.clone();
        launch_tasks.push(tokio::spawn(async move {
            store_clone.launch_game(None, "Sugar", None, None).await
        }));
    }

    let mut switch_res = Vec::new();
    for t in switch_tasks {
        switch_res.push(t.await.unwrap());
    }

    let mut launch_res = Vec::new();
    for t in launch_tasks {
        launch_res.push(t.await.unwrap());
    }

    for r in switch_res {
        assert!(r.is_ok(), "Switch task failed");
    }

    for r in launch_res {
        let launch = r.expect("Active account launch failed");
        let raw_args = &launch.launch_arguments;

        // Every launch must have atomic consistency:
        // EITHER (acc_alice AND Alice_Sw) OR (acc_bob AND Bob_Sw).
        // NEVER a mismatched pair!
        let has_alice_id = raw_args.iter().any(|a| a == &format!("-epicuserid={}", id_alice));
        let has_alice_name = raw_args.iter().any(|a| a == &format!("-epicusername={}", name_alice));
        let has_bob_id = raw_args.iter().any(|a| a == &format!("-epicuserid={}", id_bob));
        let has_bob_name = raw_args.iter().any(|a| a == &format!("-epicusername={}", name_bob));

        if has_alice_id {
            assert!(has_alice_name, "Alice ID must pair with Alice name");
            assert!(!has_bob_id, "Must not contain Bob ID");
            assert!(!has_bob_name, "Must not contain Bob name");
        } else if has_bob_id {
            assert!(has_bob_name, "Bob ID must pair with Bob name");
            assert!(!has_alice_id, "Must not contain Alice ID");
            assert!(!has_alice_name, "Must not contain Alice name");
        } else {
            panic!("Launch arguments missing valid account identity: {:?}", raw_args);
        }
    }
    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");
}

#[tokio::test]
async fn test_challenge_launch_game_alias_endpoint() {
    let _env_lock = CHALLENGER_ENV_LOCK.lock().unwrap();
    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let (mock, _db, _keyring, store, app) = setup_challenger_test_app().await;

    let code = "cccccccccccccccccccccccccccccccc";
    let acc_id = "acc_alias_user";
    mock.register_auth_code(code, acc_id, "AliasUser").await;
    store.login_with_code(code).await.expect("Login failed");

    // Test POST /api/epic/launch-game
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/epic/launch-game")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"account_id": "acc_alias_user", "app_name": "Sugar"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["app_name"], "Sugar");
    assert_eq!(json["pid"], 42424);
}
