//! Adversarial Challenge Test Suite for Milestone 1 (M1)
//! Tests auth code sanitization, high-concurrency switching, parallel tokens, and memory credential store race conditions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{Form, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::json;

use swch::launchers::epic::{
    sanitize_auth_code, CredentialStore, DeviceAuth, EpicAuthStore, EpicClient,
    MemoryCredentialStore, EPIC_BASIC_AUTH,
};
use swch::storage::{Account, Database};

// ============================================================================
// Section 1: Mock Epic Server for Concurrency Testing
// ============================================================================

#[derive(Clone, Default)]
struct MockServerState {
    pub login_count: Arc<AtomicUsize>,
    pub device_auth_count: Arc<AtomicUsize>,
    pub exchange_count: Arc<AtomicUsize>,
}

fn create_mock_epic_router(state: MockServerState) -> Router {
    Router::new()
        .route("/account/api/oauth/token", post(mock_token_handler))
        .route(
            "/account/api/public/account/:account_id/deviceAuth",
            post(mock_create_device_auth_handler),
        )
        .route(
            "/account/api/public/account/:account_id/deviceAuth/:device_id",
            delete(mock_delete_device_auth_handler),
        )
        .route("/account/api/oauth/exchange", get(mock_exchange_handler))
        .with_state(state)
}

async fn mock_token_handler(
    State(state): State<MockServerState>,
    headers: HeaderMap,
    Form(params): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let auth = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok());
    if auth != Some(EPIC_BASIC_AUTH) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "errorCode": "errors.com.epicgames.common.oauth.invalid_client",
                "errorMessage": "Invalid client authentication credentials"
            })),
        );
    }

    state.login_count.fetch_add(1, Ordering::Relaxed);
    let grant_type = params.get("grant_type").map(|s| s.as_str()).unwrap_or("");

    match grant_type {
        "authorization_code" => {
            let code = params.get("code").map(|s| s.as_str()).unwrap_or("");
            let account_id = format!("acc_for_code_{}", &code[..code.len().min(8)]);
            (
                StatusCode::OK,
                Json(json!({
                    "access_token": format!("token_{}", code),
                    "token_type": "bearer",
                    "expires_in": 28800,
                    "account_id": account_id,
                    "client_id": "34a02cf8f4414e29b15921876da36f9a",
                    "displayName": format!("User_{}", account_id),
                })),
            )
        }
        "device_auth" => {
            let account_id = params.get("account_id").cloned().unwrap_or_default();
            (
                StatusCode::OK,
                Json(json!({
                    "access_token": format!("refreshed_token_{}", account_id),
                    "token_type": "bearer",
                    "expires_in": 28800,
                    "account_id": account_id.clone(),
                    "client_id": "34a02cf8f4414e29b15921876da36f9a",
                    "displayName": format!("User_{}", account_id),
                })),
            )
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "errorCode": "errors.com.epicgames.oauth.unsupported_grant_type",
                "errorMessage": "Unsupported grant type"
            })),
        ),
    }
}

async fn mock_create_device_auth_handler(
    State(state): State<MockServerState>,
    Path(account_id): Path<String>,
) -> impl IntoResponse {
    let count = state.device_auth_count.fetch_add(1, Ordering::Relaxed);
    (
        StatusCode::OK,
        Json(json!({
            "accountId": account_id,
            "deviceId": format!("dev_{}_{}", account_id, count),
            "secret": format!("sec_{}_{}", account_id, count),
            "userAgent": "swch_challenger",
            "created": {
                "dateTime": "2026-09-13T12:00:00Z"
            }
        })),
    )
}

async fn mock_delete_device_auth_handler() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

async fn mock_exchange_handler(
    State(state): State<MockServerState>,
) -> impl IntoResponse {
    let count = state.exchange_count.fetch_add(1, Ordering::Relaxed);
    (
        StatusCode::OK,
        Json(json!({
            "code": format!("exchange_code_{count:032}"),
            "creatingClientId": "34a02cf8f4414e29b15921876da36f9a",
            "expiresInSeconds": 300
        })),
    )
}

// ============================================================================
// Section 2: Adversarial Tests for `sanitize_auth_code`
// ============================================================================

#[test]
fn test_adv_sanitize_zero_width_space_and_multibyte_panics() {
    // Zero-width space \u{200B} is 3 bytes in UTF-8 (0xE2, 0x80, 0x8B).
    // If placed at byte offset 15, slicing &cleaned[..16] cuts through the multibyte character.
    let malicious_inputs = [
        "123456789012345\u{200B}",                   // 15 ascii + 3 byte char = 18 bytes (index 16 inside char)
        "12345678901234\u{1F600}",                    // 14 ascii + 4 byte emoji = 18 bytes (index 16 inside char)
        "123456789012345\u{00A9}",                    // 15 ascii + 2 byte char = 17 bytes (index 16 inside char)
        "абвгдеж",                                    // 7 cyrillic chars = 14 bytes
        "абвгдежз",                                   // 8 cyrillic chars = 16 bytes
        "привет мир как дела",                        // Cyrillic sentence
        "\u{FEFF}0123456789abcdef0123456789abcdef",  // BOM prefixed
        "0123456789abcdef0123456789abcdef\u{200B}",  // ZWS suffix
        "0123456789\u{200B}abcdef0123456789abcdef",  // ZWS infix
    ];

    let mut panicked = Vec::new();
    for input in malicious_inputs {
        let res = std::panic::catch_unwind(|| {
            sanitize_auth_code(input)
        });
        if res.is_err() {
            panicked.push(input);
        }
    }

    if !panicked.is_empty() {
        panic!(
            "VULNERABILITY CONFIRMED: sanitize_auth_code panicked on {} inputs due to non-char-boundary slicing: {:?}",
            panicked.len(),
            panicked
        );
    }
}

#[test]
fn test_adv_sanitize_nested_json_and_variations() {
    // 1. Nested JSON with authorizationCode deep inside
    let nested_json = r#"{"data": {"authorizationCode": "0123456789abcdef0123456789abcdef"}}"#;
    assert!(sanitize_auth_code(nested_json).is_err()); // Current design only parses top-level authorizationCode

    // 2. Nested JSON where top-level authorizationCode is an object or array
    let invalid_json_type = r#"{"authorizationCode": {"nested": "0123456789abcdef0123456789abcdef"}}"#;
    assert!(sanitize_auth_code(invalid_json_type).is_err());

    // 3. Valid JSON with extra fields and formatting
    let rich_json = r#"{
        "status": "success",
        "authorizationCode": "0123456789abcdef0123456789abcdef",
        "sid": "some_session_id"
    }"#;
    let res = sanitize_auth_code(rich_json);
    assert!(res.is_ok(), "Failed to parse valid multi-field JSON: {:?}", res);
    assert_eq!(res.unwrap(), "0123456789abcdef0123456789abcdef");

    // 4. JSON redirectUrl format
    let redirect_url_json = r#"{"redirectUrl": "https://www.epicgames.com/id/api/redirect?code=0123456789abcdef0123456789abcdef"}"#;
    let res2 = sanitize_auth_code(redirect_url_json);
    assert!(res2.is_ok(), "Failed on JSON redirectUrl: {:?}", res2);
    assert_eq!(res2.unwrap(), "0123456789abcdef0123456789abcdef");
}

#[test]
fn test_adv_sanitize_sql_injection_patterns() {
    let injections = [
        "' OR '1'='1",
        "0123456789abcdef0123456789abcdef'; DROP TABLE accounts; --",
        "admin'--",
        "1' UNION SELECT * FROM accounts--",
        "0123456789abcdef0123456789abcdef' OR '1'='1",
    ];

    for inj in injections {
        let res = sanitize_auth_code(inj);
        assert!(res.is_err(), "SQL injection string unexpectedly accepted: {inj}");
    }
}

#[test]
fn test_adv_sanitize_length_and_whitespace_extremes() {
    // 1. Off-by-one lengths
    assert!(sanitize_auth_code("0123456789abcdef0123456789abcde").is_err()); // 31 chars
    assert!(sanitize_auth_code("0123456789abcdef0123456789abcdef0").is_err()); // 33 chars

    // 2. Huge string (DoS / memory stress)
    let huge_string = "a".repeat(100_000);
    let start = Instant::now();
    assert!(sanitize_auth_code(&huge_string).is_err());
    assert!(start.elapsed() < Duration::from_millis(50), "Sanitizer took too long on 100k input");

    // 3. Trailing and leading newlines/tabs/spaces
    let messy = "\r\n\t  0123456789abcdef0123456789abcdef  \t\r\n";
    assert_eq!(sanitize_auth_code(messy).unwrap(), "0123456789abcdef0123456789abcdef");

    // 4. Multiple quotes
    let multi_quotes = "\"\"'0123456789abcdef0123456789abcdef'\"\"";
    assert_eq!(sanitize_auth_code(multi_quotes).unwrap(), "0123456789abcdef0123456789abcdef");
}

// ============================================================================
// Section 3: High-Concurrency Stress Tests for EpicAuthStore
// ============================================================================

#[tokio::test]
async fn test_adv_concurrent_account_switching_stress() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let keyring = Arc::new(MemoryCredentialStore::new());
    let mock_state = MockServerState::default();
    let router = create_mock_epic_router(mock_state);
    let client = Arc::new(EpicClient::from_router(router));
    let store = Arc::new(EpicAuthStore::new(db.clone(), keyring.clone(), client));

    // Pre-populate 5 Epic accounts
    let num_accounts = 5;
    for i in 0..num_accounts {
        let acc_id = format!("epic_acc_{i}");
        let acc = Account {
            id: acc_id.clone(),
            launcher: "epic".to_string(),
            username: format!("User {i}"),
            display_name: Some(format!("User {i}")),
            email: Some(format!("user_{i}@example.com")),
            is_active: i == 0,
            last_used: None,
        };
        db.upsert_epic_account(&acc).await.unwrap();

        let auth = DeviceAuth::new(&acc_id, format!("dev_{i}"), format!("sec_{i}"));
        keyring.set_device_auth(&auth).unwrap();
    }

    // Spawn 20 concurrent workers switching between accounts concurrently
    let num_tasks = 20;
    let switches_per_task = 25;
    let mut handles = Vec::new();

    let start_time = Instant::now();

    for task_id in 0..num_tasks {
        let store_clone = Arc::clone(&store);
        let handle = tokio::spawn(async move {
            let mut switch_latencies = Vec::new();
            for step in 0..switches_per_task {
                let target_acc = format!("epic_acc_{}", (task_id + step) % num_accounts);
                let switch_start = Instant::now();
                let switched = store_clone.switch_active_account(&target_acc).await;
                let latency = switch_start.elapsed();
                switch_latencies.push(latency);

                assert!(switched.is_ok(), "Switch failed: {:?}", switched.err());
                assert_eq!(switched.unwrap().account_id, target_acc);

                // Also check reading active account
                let active = store_clone.get_active_account().await;
                assert!(active.is_ok());
                assert!(active.unwrap().is_some());
            }
            switch_latencies
        });
        handles.push(handle);
    }

    let mut all_latencies = Vec::new();
    for handle in handles {
        let latencies = handle.await.unwrap();
        all_latencies.extend(latencies);
    }

    let total_elapsed = start_time.elapsed();
    let total_switches = num_tasks * switches_per_task;
    println!(
        "High-concurrency test completed: {} switches in {:?}. Average switch latency: {:?}",
        total_switches,
        total_elapsed,
        total_elapsed / total_switches as u32
    );

    // Verify invariants after concurrent switching:
    // 1. Exactly ONE account must be active in SQLite
    let active_in_db = db.get_active_epic_account().await.unwrap();
    assert!(active_in_db.is_some(), "No active account found in DB after concurrent switches");
    let active_db_acc = active_in_db.unwrap();

    let all_accounts = db.get_epic_accounts().await.unwrap();
    let active_count = all_accounts.iter().filter(|a| a.is_active).count();
    assert_eq!(
        active_count, 1,
        "Invariant violated: exactly 1 account must be active, found {}",
        active_count
    );

    // 2. In-memory active account should match DB active account
    let active_in_memory = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(
        active_in_memory.account_id, active_db_acc.id,
        "In-memory active account '{}' does not match DB active account '{}'",
        active_in_memory.account_id, active_db_acc.id
    );

    // 3. Switching SLA: median latency must be < 5ms
    all_latencies.sort();
    let median_latency = all_latencies[all_latencies.len() / 2];
    let p95_latency = all_latencies[(all_latencies.len() as f64 * 0.95) as usize];
    println!("Switching latency: median = {median_latency:?}, p95 = {p95_latency:?}");
    assert!(
        median_latency < Duration::from_millis(5),
        "Median switch latency exceeded 5ms SLA: {:?}",
        median_latency
    );
}

#[tokio::test]
async fn test_adv_parallel_token_requests_cold_cache() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let keyring = Arc::new(MemoryCredentialStore::new());
    let mock_state = MockServerState::default();
    let router = create_mock_epic_router(mock_state.clone());
    let client = Arc::new(EpicClient::from_router(router));
    let store = Arc::new(EpicAuthStore::new(db.clone(), keyring.clone(), client));

    let acc_id = "epic_cold_acc";
    let acc = Account {
        id: acc_id.to_string(),
        launcher: "epic".to_string(),
        username: "Cold User".to_string(),
        display_name: Some("Cold User".to_string()),
        email: None,
        is_active: true,
        last_used: None,
    };
    db.upsert_epic_account(&acc).await.unwrap();

    let auth = DeviceAuth::new(acc_id, "dev_cold", "sec_cold");
    keyring.set_device_auth(&auth).unwrap();

    // 50 parallel tasks simultaneously asking for access token on cold cache
    let num_parallel = 50;
    let mut handles = Vec::new();

    for _ in 0..num_parallel {
        let store_clone = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            store_clone.get_valid_access_token(acc_id).await
        }));
    }

    let mut tokens = Vec::new();
    for handle in handles {
        let res = handle.await.unwrap();
        assert!(res.is_ok(), "Token fetch failed: {:?}", res.err());
        tokens.push(res.unwrap());
    }

    assert_eq!(tokens.len(), num_parallel);
    for token in &tokens {
        assert!(token.starts_with("refreshed_token_epic_cold_acc"));
    }

    // Now verify warm cache: next 50 calls should be instant (< 100 microseconds each)
    let warm_start = Instant::now();
    for _ in 0..50 {
        let token = store.get_valid_access_token(acc_id).await.unwrap();
        assert!(token.starts_with("refreshed_token_epic_cold_acc"));
    }
    let warm_elapsed = warm_start.elapsed();
    assert!(
        warm_elapsed < Duration::from_millis(5),
        "Warm cache 50 fetches took too long: {:?}",
        warm_elapsed
    );
}

#[tokio::test]
async fn test_adv_simultaneous_logins_same_account() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let keyring = Arc::new(MemoryCredentialStore::new());
    let mock_state = MockServerState::default();
    let router = create_mock_epic_router(mock_state.clone());
    let client = Arc::new(EpicClient::from_router(router));
    let store = Arc::new(EpicAuthStore::new(db.clone(), keyring.clone(), client));

    // 10 tasks trying to login with different codes that resolve to the SAME account ID
    let num_logins = 10;
    let mut handles = Vec::new();

    for i in 0..num_logins {
        let store_clone = Arc::clone(&store);
        // Code starts with "11223344" (8 valid hex chars) so mock server assigns account_id "acc_for_code_11223344"
        let code = format!("11223344{:024x}", i);
        handles.push(tokio::spawn(async move {
            store_clone.login_with_code(&code).await
        }));
    }

    let mut success_accounts = Vec::new();
    for handle in handles {
        let res = handle.await.unwrap();
        assert!(res.is_ok(), "Login failed under concurrency: {:?}", res.err());
        success_accounts.push(res.unwrap());
    }

    assert_eq!(success_accounts.len(), num_logins);

    // Verify only ONE account row exists in DB and it is active
    let accounts_in_db = db.get_epic_accounts().await.unwrap();
    assert_eq!(accounts_in_db.len(), 1, "Expected 1 deduplicated account in DB");
    assert!(accounts_in_db[0].is_active);
}

#[tokio::test]
async fn test_adv_relogin_active_account_deactivates_it() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let keyring = Arc::new(MemoryCredentialStore::new());
    let mock_state = MockServerState::default();
    let router = create_mock_epic_router(mock_state.clone());
    let client = Arc::new(EpicClient::from_router(router));
    let store = Arc::new(EpicAuthStore::new(db.clone(), keyring.clone(), client));

    // 1. First login to account: it becomes active
    let code1 = "11112222333344445555666677778888";
    let acc1 = store.login_with_code(code1).await.unwrap();
    assert!(acc1.is_active, "First login should be active");

    let active_before = db.get_active_epic_account().await.unwrap();
    assert!(active_before.is_some(), "Database must have active account after first login");
    assert_eq!(active_before.unwrap().id, acc1.account_id);

    // 2. Re-login to the EXACT SAME account (e.g. user re-authorizes)
    // Code produces the same account_id prefix
    let code2 = "11112222000000000000000000000000";
    let acc2 = store.login_with_code(code2).await.unwrap();

    // BUG CHECK: Does re-logging into the active account deactivate it?
    let active_after = db.get_active_epic_account().await.unwrap();
    assert!(
        active_after.is_some(),
        "CRITICAL BUG: Re-logging into active account '{}' deactivated it! No active account in DB.",
        acc1.account_id
    );
    assert!(acc2.is_active, "Account should still be active after re-login");
}

#[tokio::test]
async fn test_adv_concurrent_logins_multiple_fresh_accounts_race() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let keyring = Arc::new(MemoryCredentialStore::new());
    let mock_state = MockServerState::default();
    let router = create_mock_epic_router(mock_state.clone());
    let client = Arc::new(EpicClient::from_router(router));
    let store = Arc::new(EpicAuthStore::new(db.clone(), keyring.clone(), client));

    // 10 tasks trying to login with 10 DIFFERENT accounts simultaneously on empty DB
    let num_logins = 10;
    let mut handles = Vec::new();

    for i in 0..num_logins {
        let store_clone = Arc::clone(&store);
        let code = format!("{i:08x}000000000000000000000000");
        handles.push(tokio::spawn(async move {
            store_clone.login_with_code(&code).await
        }));
    }

    for handle in handles {
        let res = handle.await.unwrap();
        assert!(res.is_ok());
    }

    let accounts_in_db = db.get_epic_accounts().await.unwrap();
    assert_eq!(accounts_in_db.len(), num_logins);
    let active_count = accounts_in_db.iter().filter(|a| a.is_active).count();
    
    // Invariant: At all times, exactly ONE account may be active.
    // If TOCTOU race exists, multiple accounts will be marked active.
    assert_eq!(
        active_count, 1,
        "TOCTOU RACE DETECTED: Expected exactly 1 active account after concurrent logins, but found {} active accounts!",
        active_count
    );
}

// ============================================================================
// Section 4: Race Condition & Data Corruption Stress on MemoryCredentialStore
// ============================================================================

#[test]
fn test_adv_memory_credential_store_thread_hammer() {
    let store = Arc::new(MemoryCredentialStore::new());
    let num_threads = 24;
    let ops_per_thread = 200;

    let mut handles = Vec::new();

    for thread_idx in 0..num_threads {
        let store_clone = Arc::clone(&store);
        handles.push(std::thread::spawn(move || {
            for op in 0..ops_per_thread {
                let acc_id = format!("acc_{}", (thread_idx * 7 + op) % 10);
                match op % 5 {
                    0 => {
                        let auth = DeviceAuth::new(&acc_id, format!("dev_{op}"), format!("sec_{op}"));
                        store_clone.set_device_auth(&auth).unwrap();
                    }
                    1 => {
                        let _ = store_clone.get_device_auth(&acc_id).unwrap();
                    }
                    2 => {
                        let _ = store_clone.delete_device_auth(&acc_id).unwrap();
                    }
                    3 => {
                        let snap = store_clone.snapshot().unwrap();
                        let _ = snap.len();
                    }
                    4 => {
                        let _ = store_clone.len();
                    }
                    _ => unreachable!(),
                }
            }
        }));
    }

    for handle in handles {
        handle.join().expect("Worker thread panicked!");
    }

    // Verify store is healthy and can perform standard CRUD after heavy hammer
    let final_auth = DeviceAuth::new("final_acc", "final_dev", "final_sec");
    store.set_device_auth(&final_auth).unwrap();
    let retrieved = store.get_device_auth("final_acc").unwrap().unwrap();
    assert_eq!(retrieved.account_id, "final_acc");
    assert_eq!(retrieved.device_id, "final_dev");
    assert_eq!(retrieved.secret, "final_sec");

    store.clear().unwrap();
    assert!(store.is_empty());
}

// ============================================================================
// Section 5: Adversarial Account Removal & Lifecycle Verification
// ============================================================================

#[tokio::test]
async fn test_adv_sole_active_account_removal_adversarial() {
    let mock_state = MockServerState::default();
    let router = create_mock_epic_router(mock_state);
    let client = Arc::new(EpicClient::from_router(router));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client);

    // 1. Log in sole account
    let acc = store
        .login_with_code("3b2a0c4f8d914652a92db1029c73e1a0")
        .await
        .expect("login should succeed");
    assert!(acc.is_active, "Sole account must be active initially");

    // Verify DB and Store active state
    let active_store = store.get_active_account().await.unwrap();
    assert!(active_store.is_some());
    assert_eq!(active_store.unwrap().account_id, acc.account_id);

    let active_db = db.get_active_epic_account().await.unwrap();
    assert!(active_db.is_some());
    assert_eq!(active_db.unwrap().id, acc.account_id);

    // Verify raw SQLite accounts table
    {
        let accounts = db.get_epic_accounts().await.unwrap();
        let count_active = accounts.iter().filter(|a| a.is_active).count();
        assert_eq!(count_active, 1);
    }

    // 2. Remove the sole active account
    store
        .remove_account(&acc.account_id)
        .await
        .expect("remove_account should succeed");

    // 3. Verify get_active_account() returns Ok(None)
    let active_after = store.get_active_account().await.unwrap();
    assert!(
        active_after.is_none(),
        "store.get_active_account() must return Ok(None) after sole account removal"
    );

    // 4. Verify SQLite has NO active account and zero accounts
    let db_active_after = db.get_active_epic_account().await.unwrap();
    assert!(
        db_active_after.is_none(),
        "db.get_active_epic_account() must return Ok(None)"
    );

    {
        let accounts = db.get_epic_accounts().await.unwrap();
        let count_active = accounts.iter().filter(|a| a.is_active).count();
        assert_eq!(count_active, 0, "SQLite must have 0 active Epic accounts");
        assert_eq!(accounts.len(), 0, "SQLite must have 0 Epic accounts total");
    }

    // 5. Verify Keyring credential was deleted
    assert!(
        keyring.get_device_auth(&acc.account_id).unwrap().is_none(),
        "Keyring entry must be deleted"
    );

    // 6. Verify token fetch fails
    let token_res = store.get_valid_access_token(&acc.account_id).await;
    assert!(
        token_res.is_err(),
        "Fetching access token for removed account must fail"
    );

    // 7. Verify subsequent removal returns AccountNotFound error
    let remove_again_res = store.remove_account(&acc.account_id).await;
    assert!(
        remove_again_res.is_err(),
        "Subsequent removal of already removed account must return Err"
    );
    match remove_again_res.unwrap_err() {
        swch::launchers::epic::EpicError::AccountNotFound(id) => {
            assert_eq!(id, acc.account_id);
        }
        other => panic!("Expected AccountNotFound, got {:?}", other),
    }
}

#[tokio::test]
async fn test_adv_multi_account_active_promotion_and_lifecycle_matrix() {
    let mock_state = MockServerState::default();
    let router = create_mock_epic_router(mock_state);
    let client = Arc::new(EpicClient::from_router(router));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client);

    // 1. Authenticate three distinct accounts
    // Mock handler derives IDs: code_11112222 -> acc_for_code_11112222
    let acc1 = store
        .login_with_code("11112222333344445555666677778888")
        .await
        .unwrap();
    let acc2 = store
        .login_with_code("22223333444455556666777788889999")
        .await
        .unwrap();
    let acc3 = store
        .login_with_code("33334444555566667777888899990000")
        .await
        .unwrap();

    // Account 1 was first, so it is active. Accounts 2 and 3 are inactive.
    assert!(acc1.is_active);
    assert!(!acc2.is_active);
    assert!(!acc3.is_active);

    assert_eq!(
        store.get_active_account().await.unwrap().unwrap().account_id,
        acc1.account_id
    );

    // 2. Remove active account acc1
    store.remove_account(&acc1.account_id).await.unwrap();

    // 3. Verify promotion: the top remaining account (ordered by last_used DESC, username ASC) is promoted
    let remaining_accounts = store.get_accounts().await.unwrap();
    assert_eq!(remaining_accounts.len(), 2);
    let expected_promoted = remaining_accounts.iter().find(|a| a.is_active).unwrap().clone();

    let active_after_first_removal = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(
        active_after_first_removal.account_id, expected_promoted.account_id,
        "Top remaining account should be promoted to active"
    );
    assert!(active_after_first_removal.is_active);

    // Verify in SQLite: exactly 1 active account, which matches the promoted account
    {
        let accounts = db.get_epic_accounts().await.unwrap();
        let active_accounts: Vec<_> = accounts.iter().filter(|a| a.is_active).collect();
        assert_eq!(active_accounts.len(), 1);
        assert_eq!(active_accounts[0].id, expected_promoted.account_id);
    }

    // Find the inactive account among the remaining ones
    let inactive_account = remaining_accounts
        .iter()
        .find(|a| !a.is_active)
        .unwrap()
        .clone();

    // 4. Remove the INACTIVE account
    store.remove_account(&inactive_account.account_id).await.unwrap();

    // Verify the promoted active account REMAINS active without disturbance
    let active_after_inactive_removal = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(
        active_after_inactive_removal.account_id, expected_promoted.account_id,
        "Promoted active account must remain active after removing inactive account"
    );

    let db_active = db.get_active_epic_account().await.unwrap().unwrap();
    assert_eq!(db_active.id, expected_promoted.account_id);

    // 5. Remove remaining active account
    store.remove_account(&expected_promoted.account_id).await.unwrap();

    // 6. Verify zero accounts remaining and active account is None
    assert!(store.get_active_account().await.unwrap().is_none());
    assert!(db.get_active_epic_account().await.unwrap().is_none());
    assert!(store.get_accounts().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_adv_remove_nonexistent_and_injection_adversarial() {
    let mock_state = MockServerState::default();
    let router = create_mock_epic_router(mock_state);
    let client = Arc::new(EpicClient::from_router(router));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client);

    // Setup an existing active account
    let acc = store
        .login_with_code("3b2a0c4f8d914652a92db1029c73e1a0")
        .await
        .unwrap();

    // Adversarial removal targets:
    let hostile_targets = vec![
        "",
        "   ",
        "ghost_account_id_unknown",
        "'; DROP TABLE accounts; --",
        "nonexistent' OR '1'='1",
        "acc_for_code_3b2a0c4f8d914652a92db1029c73e1a0\0injected",
    ];

    for target in hostile_targets {
        let res = store.remove_account(target).await;
        assert!(
            res.is_err(),
            "Removal of hostile/nonexistent target '{target}' must fail"
        );
        match res.unwrap_err() {
            swch::launchers::epic::EpicError::AccountNotFound(id) => {
                assert_eq!(id, target);
            }
            other => panic!("Expected AccountNotFound for '{target}', got {:?}", other),
        }
    }

    // Verify existing active account was NOT affected by failed removal calls
    let active = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(active.account_id, acc.account_id);
    assert!(active.is_active);

    let all_accs = store.get_accounts().await.unwrap();
    assert_eq!(all_accs.len(), 1);
    assert_eq!(all_accs[0].account_id, acc.account_id);
}

#[tokio::test]
async fn test_adv_remove_by_username_lifecycle() {
    let mock_state = MockServerState::default();
    let router = create_mock_epic_router(mock_state);
    let client = Arc::new(EpicClient::from_router(router));
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let store = EpicAuthStore::new(db.clone(), keyring.clone(), client);

    // Log in account 1
    let acc1 = store
        .login_with_code("3b2a0c4f8d914652a92db1029c73e1a0")
        .await
        .unwrap();
    let acc2 = store
        .login_with_code("7f8e9d0a1b2c3d4e5f6a7b8c9d0e1f2a")
        .await
        .unwrap();

    // acc1 has username e.g. "User_3b2a0c4f8d914652a92db1029c73e1a0" or similar
    let username1 = acc1.display_name.clone();

    // Remove acc1 using its username instead of account_id
    store
        .remove_account(&username1)
        .await
        .expect("Removing account by username should succeed");

    // Verify acc1 is gone from Keyring (by canonical ID) and DB
    assert!(keyring.get_device_auth(&acc1.account_id).unwrap().is_none());
    let accounts = store.get_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_id, acc2.account_id);

    // Verify acc2 was promoted to active
    let active = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(active.account_id, acc2.account_id);
}

