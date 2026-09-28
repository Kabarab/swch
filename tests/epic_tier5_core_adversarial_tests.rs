//! Tier 5 Core Adversarial Coverage Hardening Test Suite
//!
//! White-box stress testing, failure mode verification, and edge-case boundary hardening for:
//! 1. OS Keyring Store (`src/launchers/epic/keyring.rs`)
//! 2. Epic HTTP Client & API Protocol (`src/launchers/epic/client.rs`)
//! 3. Manifest Parser & Path Resolver (`src/parsers/epic_manifest.rs`)
//! 4. Direct Game Launch Engine (`src/launchers/epic/launch.rs`)
//! 5. Epic Auth Store & Session Manager (`src/launchers/epic/store.rs`)

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use chrono::Utc;
use tempfile::tempdir;

use axum::{
    extract::Query,
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};

use swch::launchers::epic::{
    client::EpicClient,
    keyring::{default_store, probe_os_keyring, CredentialStore, MemoryCredentialStore},
    launch::{
        build_launch_parameters, execute_game_with_options, mask_arg,
        mask_launch_arguments, prepare_launch_command, LaunchCommand, LaunchError,
        LaunchGameOptions, LaunchOptions,
    },
    models::{
        sanitize_auth_code, DeviceAuth, DeviceAuthCreated,
        EpicError, ExchangeCodeResponse, OAuthTokenResponse,
    },
    store::{CachedToken, EpicAuthStore},
};
use swch::parsers::{
    parse_manifest_from_str, scan_manifest_directory,
    EpicManifestItem, ManifestError,
};
use swch::storage::{Account, Database};

// ============================================================================
// HELPER FIXTURES & ROUTER BUILDERS
// ============================================================================

fn sample_device_auth(id: &str) -> DeviceAuth {
    DeviceAuth {
        account_id: id.to_string(),
        device_id: format!("dev_{id}"),
        secret: format!("sec_{id}"),
        user_agent: Some("swch_tier5_adversarial_test".to_string()),
        created: Some(DeviceAuthCreated {
            location: Some("SecretBase".to_string()),
            ip_address: Some("127.0.0.1".to_string()),
            date_time: Some("2026-09-14T12:00:00Z".to_string()),
        }),
        ..Default::default()
    }
}

async fn setup_test_db() -> Database {
    let db = Database::open_in_memory().expect("Failed to open in-memory DB");
    db.run_migrations().await.expect("Failed to run DB migrations");
    db
}

// ============================================================================
// SECTION 1: KEYRING ADVERSARIAL HARDENING (`src/launchers/epic/keyring.rs`)
// ============================================================================

#[test]
fn test_t5_keyring_unicode_and_special_symbols_in_account_id() {
    let store = MemoryCredentialStore::new();
    let exotic_ids = [
        "acc_⚡_🦀_🎮_🔥",
        "acc_тест_пользователь_123",
        "acc_日本語_アカウント_識別子",
        "acc_مستخدم_جديد_نظام",
        "acc with spaces and\ttabs",
        "acc:with:multiple:colons",
        "acc/with/forward/slashes",
        "acc\\with\\backslashes",
        "acc\"with\"quotes'and`ticks",
        "acc\u{200B}zero\u{200C}width\u{200D}chars",
        "!@#$%^&*()_+-=[]{}|;:,.<>?",
    ];

    for id in exotic_ids {
        let auth = DeviceAuth::new(id, format!("dev_{id}"), format!("secret_{id}"));
        store.set_device_auth(&auth).expect("Should set exotic account ID");

        let retrieved = store.get_device_auth(id)
            .expect("Should query without error")
            .expect("Credential must be present");
        assert_eq!(retrieved.account_id, id);
        assert_eq!(retrieved.device_id, format!("dev_{id}"));
        assert_eq!(retrieved.secret, format!("secret_{id}"));

        // Idempotent delete verification
        store.delete_device_auth(id).expect("Should delete");
        assert!(store.get_device_auth(id).unwrap().is_none());
    }
}

#[test]
fn test_t5_keyring_massive_secret_payload_stress() {
    let store = MemoryCredentialStore::new();

    // Test secret sizes: 64 KB, 256 KB, and 1 MB
    for size_kb in [64, 256, 1024] {
        let large_secret: String = (0..size_kb * 1024)
            .map(|i| ((i % 26) as u8 + b'a') as char)
            .collect();
        let id = format!("acc_large_{size_kb}kb");
        let auth = DeviceAuth::new(&id, format!("dev_{id}"), &large_secret);

        store.set_device_auth(&auth).expect("Must store large secret");
        let read_back = store.get_device_auth(&id).unwrap().expect("Must read large secret back");
        assert_eq!(read_back.secret.len(), size_kb * 1024);
        assert_eq!(read_back.secret, large_secret);

        // Verify JSON roundtrip
        let serialized = serde_json::to_string(&read_back).expect("Must serialize large auth");
        let deserialized: DeviceAuth = serde_json::from_str(&serialized).expect("Must deserialize");
        assert_eq!(read_back, deserialized);
    }
}

#[test]
fn test_t5_keyring_deletion_and_mutation_concurrency_race() {
    let store = Arc::new(MemoryCredentialStore::new());
    let mut handles = Vec::new();
    let num_threads = 24;
    let iterations_per_thread = 50;

    for thread_idx in 0..num_threads {
        let store_clone = Arc::clone(&store);
        handles.push(std::thread::spawn(move || {
            for iter in 0..iterations_per_thread {
                let id = format!("race_acc_{}", iter % 5);
                let auth = sample_device_auth(&id);

                // Alternating write, read, snapshot, delete, clear
                let _ = store_clone.set_device_auth(&auth);
                let _ = store_clone.get_device_auth(&id);
                if iter % 10 == 0 {
                    let _ = store_clone.snapshot();
                }
                if (thread_idx + iter) % 3 == 0 {
                    let _ = store_clone.delete_device_auth(&id);
                }
            }
        }));
    }

    for h in handles {
        h.join().expect("Concurrent keyring thread must not panic or poison lock");
    }

    // Verify store is healthy and can be cleared
    assert!(store.clear().is_ok());
    assert!(store.is_empty());
}

#[test]
fn test_t5_keyring_device_auth_debug_redaction_with_adversarial_secrets() {
    let tricky_secrets = [
        "\" && rm -rf / #",
        "secret with { \"nested\": \"json\" }",
        "secret with \n newlines \r\n and \t tabs",
        "secret with format string: {} {:?} %s %x %d",
        "secret with [REDACTED] inside",
    ];

    for s in tricky_secrets {
        let auth = DeviceAuth::new("acc_test", "dev_test", s);
        let debug_str = format!("{auth:?}");
        assert!(
            debug_str.contains("[REDACTED]"),
            "Debug string must contain [REDACTED]"
        );
        assert!(
            !debug_str.contains(s),
            "Debug output must NEVER leak secret: '{s}'"
        );
    }
}

#[test]
fn test_t5_keyring_probe_and_env_switch_robustness() {
    // OS probe should return a boolean without panic
    let _ = probe_os_keyring();

    // Verify explicit memory backend override via SWCH_KEYRING_BACKEND
    std::env::set_var("SWCH_KEYRING_BACKEND", "memory");
    let store = default_store();
    let auth = sample_device_auth("env_probe_acc");
    store.set_device_auth(&auth).expect("Memory backend should write");
    assert_eq!(store.get_device_auth("env_probe_acc").unwrap().unwrap().account_id, "env_probe_acc");
    std::env::remove_var("SWCH_KEYRING_BACKEND");
}

#[test]
fn test_t5_keyring_from_entries_with_duplicates_and_empty() {
    let empty_store = MemoryCredentialStore::from_entries(Vec::new());
    assert!(empty_store.is_empty());
    assert_eq!(empty_store.len(), 0);

    // Multiple entries with duplicate account_id: last one must win
    let mut auth1 = sample_device_auth("dup_acc");
    auth1.device_id = "first_dev".to_string();

    let mut auth2 = sample_device_auth("dup_acc");
    auth2.device_id = "second_dev".to_string();

    let store = MemoryCredentialStore::from_entries(vec![auth1, auth2]);
    assert_eq!(store.len(), 1);
    let retrieved = store.get_device_auth("dup_acc").unwrap().unwrap();
    assert_eq!(retrieved.device_id, "second_dev");
}

// ============================================================================
// SECTION 2: CLIENT ADVERSARIAL HARDENING (`src/launchers/epic/client.rs`)
// ============================================================================

#[tokio::test]
async fn test_t5_client_http_500_internal_server_error_handling() {
    let router = Router::new()
        .route("/account/api/oauth/token", post(|| async {
            (StatusCode::INTERNAL_SERVER_ERROR, "Epic Internal Server Error 500").into_response()
        }))
        .route("/account/api/public/account/:account_id/deviceAuth", post(|| async {
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal Database Failure").into_response()
        }))
        .route("/account/api/oauth/exchange", get(|| async {
            (StatusCode::INTERNAL_SERVER_ERROR, "Exchange Service Down").into_response()
        }))
        .route("/entitlement/api/account/:account_id/entitlements", get(|| async {
            (StatusCode::INTERNAL_SERVER_ERROR, "Entitlement Cluster Unavailable").into_response()
        }));

    let client = EpicClient::from_router(router);

    // 1. OAuth code exchange on 500
    let err = client.exchange_code_for_token("0123456789abcdef0123456789abcdef")
        .await
        .expect_err("HTTP 500 must fail token exchange");
    match err {
        EpicError::Api { status, message, .. } => {
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert!(message.contains("Epic Internal Server Error 500"));
        }
        other => panic!("Expected EpicError::Api on 500, got: {other:?}"),
    }

    // 2. Create DeviceAuth on 500
    let err = client.create_device_auth("acc_1", "tok_1")
        .await
        .expect_err("HTTP 500 must fail create_device_auth");
    match err {
        EpicError::Api { status, .. } => assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR),
        other => panic!("Expected EpicError::Api, got: {other:?}"),
    }

    // 3. Get Exchange Code on 500
    let err = client.get_exchange_code("tok_1")
        .await
        .expect_err("HTTP 500 must fail exchange code");
    match err {
        EpicError::Api { status, .. } => assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR),
        other => panic!("Expected EpicError::Api, got: {other:?}"),
    }

    // 4. Fetch Entitlements on 500
    let err = client.fetch_entitlements("tok_1", "acc_1", 0, 10)
        .await
        .expect_err("HTTP 500 must fail fetch_entitlements");
    match err {
        EpicError::Api { status, .. } => assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR),
        other => panic!("Expected EpicError::Api, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_t5_client_cloudflare_html_error_responses() {
    let html_page = r#"<!DOCTYPE html>
<html>
<head><title>502 Bad Gateway</title></head>
<body>
<center><h1>502 Bad Gateway</h1></center>
<hr><center>cloudflare</center>
</body>
</html>"#;

    let router = Router::new().route(
        "/account/api/oauth/token",
        post(move || {
            let body = html_page.to_string();
            async move { (StatusCode::BAD_GATEWAY, [("content-type", "text/html")], body).into_response() }
        }),
    );

    let client = EpicClient::from_router(router);
    let err = client.exchange_code_for_token("0123456789abcdef0123456789abcdef")
        .await
        .expect_err("HTML 502 Bad Gateway must not panic json deserializer");

    match err {
        EpicError::Api { status, message, .. } => {
            assert_eq!(status, StatusCode::BAD_GATEWAY);
            assert!(message.contains("502 Bad Gateway"));
        }
        other => panic!("Expected EpicError::Api with HTML body, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_t5_client_truncated_json_payloads() {
    let router = Router::new().route(
        "/account/api/oauth/token",
        post(|| async {
            // Truncated / malformed JSON returned with 200 OK
            (StatusCode::OK, "{\"access_token\": \"eg1~truncated_token").into_response()
        }),
    );

    let client = EpicClient::from_router(router);
    let err = client.exchange_code_for_token("0123456789abcdef0123456789abcdef")
        .await
        .expect_err("Truncated JSON must fail parsing");

    assert!(matches!(err, EpicError::Serialization(_)));
}

#[tokio::test]
async fn test_t5_client_missing_required_fields_in_oauth_tokens() {
    // Test missing fields one by one: access_token, token_type, expires_in, account_id, client_id
    let invalid_payloads = [
        json!({ "token_type": "bearer", "expires_in": 3600, "account_id": "acc", "client_id": "cli" }), // missing access_token
        json!({ "access_token": "tok", "expires_in": 3600, "account_id": "acc", "client_id": "cli" }),   // missing token_type
        json!({ "access_token": "tok", "token_type": "bearer", "account_id": "acc", "client_id": "cli" }), // missing expires_in
        json!({ "access_token": "tok", "token_type": "bearer", "expires_in": 3600, "client_id": "cli" }), // missing account_id
        json!({ "access_token": "tok", "token_type": "bearer", "expires_in": 3600, "account_id": "acc" }), // missing client_id
    ];

    for payload in invalid_payloads {
        let payload_str = payload.to_string();
        let router = Router::new().route(
            "/account/api/oauth/token",
            post(move || {
                let p = payload_str.clone();
                async move { (StatusCode::OK, p).into_response() }
            }),
        );

        let client = EpicClient::from_router(router);
        let err = client.exchange_code_for_token("0123456789abcdef0123456789abcdef")
            .await
            .expect_err("Missing required field must cause deserialization error");

        assert!(matches!(err, EpicError::Serialization(_)));
    }
}

#[tokio::test]
async fn test_t5_client_missing_required_fields_in_device_auth_response() {
    let invalid_device_auths = [
        json!({ "deviceId": "dev1", "secret": "sec1" }), // missing accountId
        json!({ "accountId": "acc1", "secret": "sec1" }), // missing deviceId
        json!({ "accountId": "acc1", "deviceId": "dev1" }), // missing secret
    ];

    for payload in invalid_device_auths {
        let payload_str = payload.to_string();
        let router = Router::new().route(
            "/account/api/public/account/:account_id/deviceAuth",
            post(move || {
                let p = payload_str.clone();
                async move { (StatusCode::OK, p).into_response() }
            }),
        );

        let client = EpicClient::from_router(router);
        let err = client.create_device_auth("acc1", "tok")
            .await
            .expect_err("Missing DeviceAuth field must fail deserialization");

        assert!(matches!(err, EpicError::Serialization(_)));
    }
}

#[tokio::test]
async fn test_t5_client_missing_required_fields_in_exchange_code_response() {
    let invalid_exchanges = [
        json!({ "creatingClientId": "cli", "expiresInSeconds": 300 }), // missing code
        json!({ "code": "ex_123", "expiresInSeconds": 300 }), // missing creatingClientId
        json!({ "code": "ex_123", "creatingClientId": "cli" }), // missing expiresInSeconds
    ];

    for payload in invalid_exchanges {
        let payload_str = payload.to_string();
        let router = Router::new().route(
            "/account/api/oauth/exchange",
            get(move || {
                let p = payload_str.clone();
                async move { (StatusCode::OK, p).into_response() }
            }),
        );

        let client = EpicClient::from_router(router);
        let err = client.get_exchange_code("tok")
            .await
            .expect_err("Missing exchange code fields must fail deserialization");

        assert!(matches!(err, EpicError::Serialization(_)));
    }
}

#[tokio::test]
async fn test_t5_client_rate_limiting_429_retry_after() {
    let rate_limit_payload = json!({
        "errorCode": "errors.com.epicgames.common.throttled",
        "errorMessage": "Too many requests. Please retry in 60 seconds.",
        "numericErrorCode": 1015,
        "originatingService": "account"
    }).to_string();

    let router = Router::new().route(
        "/account/api/oauth/token",
        post(move || {
            let p = rate_limit_payload.clone();
            async move {
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("retry-after", "60"), ("content-type", "application/json")],
                    p,
                ).into_response()
            }
        }),
    );

    let client = EpicClient::from_router(router);
    let err = client.exchange_code_for_token("0123456789abcdef0123456789abcdef")
        .await
        .expect_err("HTTP 429 must return Api error");

    match err {
        EpicError::Api { status, code, numeric, message } => {
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(code.as_deref(), Some("errors.com.epicgames.common.throttled"));
            assert_eq!(numeric, Some(1015));
            assert!(message.contains("Too many requests"));
        }
        other => panic!("Expected EpicError::Api for 429, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_t5_client_unexpected_empty_http_status_codes() {
    // Status 404 with empty body
    let router = Router::new().route(
        "/account/api/oauth/exchange",
        get(|| async { (StatusCode::NOT_FOUND, "").into_response() }),
    );
    let client = EpicClient::from_router(router);
    let err = client.get_exchange_code("tok").await.unwrap_err();
    match err {
        EpicError::Api { status, message, .. } => {
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(message, "Not Found");
        }
        other => panic!("Expected Not Found, got: {other:?}"),
    }

    // Status 503 Service Unavailable with empty body
    let router503 = Router::new().route(
        "/account/api/oauth/exchange",
        get(|| async { (StatusCode::SERVICE_UNAVAILABLE, "").into_response() }),
    );
    let client503 = EpicClient::from_router(router503);
    let err503 = client503.get_exchange_code("tok").await.unwrap_err();
    match err503 {
        EpicError::Api { status, message, .. } => {
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(message, "Service Unavailable");
        }
        other => panic!("Expected Service Unavailable, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_t5_client_catalog_wire_format_polymorphism() {
    // Epic Catalog bulk endpoint returns either a JSON Map or a JSON Array.
    // Both must be parsed identically.

    // 1. Empty Object `{}`
    let router_empty_map = Router::new().route(
        "/catalog/api/shared/namespace/:namespace/bulk/items",
        get(|| async { (StatusCode::OK, "{}").into_response() }),
    );
    let client1 = EpicClient::from_router(router_empty_map);
    let res1 = client1.fetch_catalog_items("tok", "ns", &[]).await.unwrap();
    assert!(res1.is_empty());

    // 2. Empty Array `[]`
    let router_empty_arr = Router::new().route(
        "/catalog/api/shared/namespace/:namespace/bulk/items",
        get(|| async { (StatusCode::OK, "[]").into_response() }),
    );
    let client2 = EpicClient::from_router(router_empty_arr);
    let res2 = client2.fetch_catalog_items("tok", "ns", &[]).await.unwrap();
    assert!(res2.is_empty());

    // 3. Array of Items `[{...}]`
    let items_array = json!([
        {
            "id": "item_alpha",
            "title": "Alpha Game",
            "namespace": "ns_alpha"
        },
        {
            "id": "item_beta",
            "title": "Beta Expansion",
            "namespace": "ns_alpha",
            "mainGameItem": { "id": "item_alpha" }
        }
    ]).to_string();

    let router_arr = Router::new().route(
        "/catalog/api/shared/namespace/:namespace/bulk/items",
        get(move || {
            let b = items_array.clone();
            async move { (StatusCode::OK, b).into_response() }
        }),
    );
    let client3 = EpicClient::from_router(router_arr);
    let res3 = client3.fetch_catalog_items("tok", "ns_alpha", &[]).await.unwrap();
    assert_eq!(res3.len(), 2);
    assert_eq!(res3["item_alpha"].title, "Alpha Game");
    assert!(res3["item_alpha"].is_base_game());
    assert_eq!(res3["item_beta"].title, "Beta Expansion");
    assert!(res3["item_beta"].is_dlc());
}

#[tokio::test]
async fn test_t5_client_entitlements_pagination_termination() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = call_count.clone();

    // Server returns exactly 1000 items on page 0, and 0 items on page 1
    let router = Router::new().route(
        "/entitlement/api/account/:account_id/entitlements",
        get(move |Query(q): Query<HashMap<String, String>>| {
            let counter = call_count_clone.clone();
            async move {
                let start: usize = q.get("start").and_then(|s| s.parse().ok()).unwrap_or(0);
                counter.fetch_add(1, Ordering::SeqCst);
                if start == 0 {
                    let items: Vec<Value> = (0..1000)
                        .map(|i| json!({
                            "id": format!("ent_{i}"),
                            "entitlementName": format!("Game {i}"),
                            "namespace": "ns",
                            "catalogItemId": format!("cat_{i}"),
                            "accountId": "acc_test",
                            "entitlementType": "EXECUTABLE",
                            "status": "ACTIVE",
                            "active": true
                        }))
                        .collect();
                    (StatusCode::OK, Json(items)).into_response()
                } else {
                    let empty: Vec<Value> = Vec::new();
                    (StatusCode::OK, Json(empty)).into_response()
                }
            }
        }),
    );

    let client = EpicClient::from_router(router);
    let all = client.fetch_all_entitlements("tok", "acc_test").await.unwrap();

    assert_eq!(all.len(), 1000);
    assert_eq!(call_count.load(Ordering::SeqCst), 2, "Pagination must terminate after empty second batch");
}

#[test]
fn test_t5_client_sanitize_auth_code_adversarial_matrix() {
    // 1. Zero-width spaces and invisible characters
    let with_zw = "\u{200B}34a02cf8f4414e29b15921876da36f9a\u{FEFF}\u{2060}";
    assert_eq!(
        sanitize_auth_code(with_zw).unwrap(),
        "34a02cf8f4414e29b15921876da36f9a"
    );

    // 2. Uppercase hex converted to lowercase
    let upper = "34A02CF8F4414E29B15921876DA36F9A";
    assert_eq!(
        sanitize_auth_code(upper).unwrap(),
        "34a02cf8f4414e29b15921876da36f9a"
    );

    // 3. Quoted inside JSON
    let json_wrapped = r#"  { "authorizationCode": "  34a02cf8f4414e29b15921876da36f9a  ", "other": 123 }  "#;
    assert_eq!(
        sanitize_auth_code(json_wrapped).unwrap(),
        "34a02cf8f4414e29b15921876da36f9a"
    );

    // 4. URL query and fragment
    let url_query = "https://localhost:3000/callback?code=34a02cf8f4414e29b15921876da36f9a&state=xyz";
    assert_eq!(
        sanitize_auth_code(url_query).unwrap(),
        "34a02cf8f4414e29b15921876da36f9a"
    );

    let url_fragment = "https://localhost:3000/callback#code=34a02cf8f4414e29b15921876da36f9a";
    assert_eq!(
        sanitize_auth_code(url_fragment).unwrap(),
        "34a02cf8f4414e29b15921876da36f9a"
    );

    // 5. Huge input string does not panic
    let huge_noise: String = "X".repeat(50_000);
    assert!(sanitize_auth_code(&huge_noise).is_err());

    // 6. Multi-byte Cyrillic looking like hex
    let cyrillic = "34а02сf8f4414е29b15921876da36f9a"; // 'а', 'с', 'е' are Cyrillic
    assert!(sanitize_auth_code(cyrillic).is_err());
}

// ============================================================================
// SECTION 3: MANIFEST ADVERSARIAL HARDENING (`src/parsers/epic_manifest.rs`)
// ============================================================================

#[test]
fn test_t5_manifest_empty_and_whitespace_rejections() {
    assert!(matches!(parse_manifest_from_str(""), Err(ManifestError::Invalid(_))));
    assert!(matches!(parse_manifest_from_str("   "), Err(ManifestError::Invalid(_))));
    assert!(matches!(parse_manifest_from_str("\t\r\n  \r\n"), Err(ManifestError::Invalid(_))));
    assert!(matches!(parse_manifest_from_str("\u{FEFF}"), Err(ManifestError::Invalid(_))));
}

#[test]
fn test_t5_manifest_corrupted_json_syntax() {
    // Truncated JSON
    let truncated = "{\"FormatVersion\": 0, \"AppName\": \"Cyberpunk";
    assert!(matches!(parse_manifest_from_str(truncated), Err(ManifestError::Json(_))));

    // Single quotes (invalid JSON)
    let single_quotes = "{'AppName': 'Game', 'CatalogItemId': '1'}";
    assert!(matches!(parse_manifest_from_str(single_quotes), Err(ManifestError::Json(_))));

    // Non-JSON binary data
    let binary_data = "\x7F\x45\x4C\x46\x02\x01\x01\x00";
    assert!(matches!(parse_manifest_from_str(binary_data), Err(ManifestError::Json(_))));
}

#[test]
fn test_t5_manifest_missing_and_empty_required_fields() {
    // Missing AppName
    let no_app_name = r#"{
        "CatalogItemId": "id",
        "CatalogNamespace": "ns",
        "DisplayName": "Game",
        "InstallationGuid": "guid",
        "InstallLocation": "/Games/Game"
    }"#;
    assert!(matches!(parse_manifest_from_str(no_app_name), Err(ManifestError::Json(_))));

    // Empty or whitespace-only AppName
    let empty_app_name = r#"{
        "AppName": "   ",
        "CatalogItemId": "id",
        "CatalogNamespace": "ns",
        "DisplayName": "Game",
        "InstallationGuid": "guid",
        "InstallLocation": "/Games/Game"
    }"#;
    let err = parse_manifest_from_str(empty_app_name).expect_err("Empty AppName must fail");
    assert!(matches!(err, ManifestError::Invalid(msg) if msg.contains("AppName is empty")));

    // Missing InstallLocation
    let no_install_location = r#"{
        "AppName": "GameApp",
        "CatalogItemId": "id",
        "CatalogNamespace": "ns",
        "DisplayName": "Game",
        "InstallationGuid": "guid"
    }"#;
    assert!(matches!(parse_manifest_from_str(no_install_location), Err(ManifestError::Json(_))));
}

#[test]
fn test_t5_manifest_boundary_values_and_unicode() {
    let json_boundary = r#"{
        "FormatVersion": 0,
        "AppName": "Game_🎮",
        "CatalogItemId": "cat_123",
        "CatalogNamespace": "ns_456",
        "DisplayName": "Cyberpunk 2077 🚀 Специальное Издание",
        "InstallationGuid": "{GUID-999}",
        "InstallLocation": "/Games/Cyberpunk 2077",
        "InstallSize": -1,
        "LaunchExecutable": "",
        "bIsIncompleteInstall": false,
        "bCanRunOffline": true,
        "bRequiresAuth": false,
        "UnknownFutureProperty": { "deep": [1, 2, 3] }
    }"#;

    let item = parse_manifest_from_str(json_boundary).expect("Boundary JSON should parse");
    assert_eq!(item.app_name, "Game_🎮");
    assert_eq!(item.display_name, "Cyberpunk 2077 🚀 Специальное Издание");
    assert_eq!(item.install_size, -1);
    assert_eq!(item.launch_executable, "");
    assert!(item.is_valid_installation());
}

#[test]
fn test_t5_manifest_macos_bundle_executable_resolution_adversarial() {
    let tmp = tempdir().unwrap();
    let app_bundle = tmp.path().join("AdversarialGame.app");
    let macos_dir = app_bundle.join("Contents/MacOS");
    fs::create_dir_all(&macos_dir).unwrap();

    // 1. Bundle with Info.plist containing empty CFBundleExecutable string:
    // <key>CFBundleExecutable</key><string></string>
    let plist_empty_exe = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>   </string>
</dict>
</plist>"#;
    fs::write(app_bundle.join("Contents/Info.plist"), plist_empty_exe).unwrap();

    // Create fallback binary matching bundle stem "AdversarialGame"
    let stem_binary = macos_dir.join("AdversarialGame");
    fs::write(&stem_binary, b"#!/bin/sh\necho hi\n").unwrap();

    let manifest = EpicManifestItem {
        format_version: Some(0),
        app_name: "AdvGame".to_string(),
        catalog_item_id: "cat_1".to_string(),
        catalog_namespace: "ns_1".to_string(),
        display_name: "Adv Game".to_string(),
        installation_guid: "guid".to_string(),
        install_location: tmp.path().to_string_lossy().to_string(),
        install_size: 100,
        launch_executable: "AdversarialGame.app".to_string(),
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: Some(false),
        can_run_offline: Some(true),
        requires_auth: Some(true),
        app_version_string: None,
    };

    let resolved = manifest.resolve_executable_path();
    assert_eq!(resolved, stem_binary, "Must fall back to bundle stem binary when CFBundleExecutable is empty");
}

#[test]
fn test_t5_manifest_directory_scanner_resilience_to_hostile_directory() {
    let tmp = tempdir().unwrap();

    // 1. One valid .item file
    let valid_json = r#"{
        "AppName": "ValidApp",
        "CatalogItemId": "cat_valid",
        "CatalogNamespace": "ns",
        "DisplayName": "Valid Game",
        "InstallationGuid": "guid_1",
        "InstallLocation": "/Games/Valid"
    }"#;
    fs::write(tmp.path().join("valid.item"), valid_json).unwrap();

    // 2. Zero-byte .item file
    fs::write(tmp.path().join("zero.item"), b"").unwrap();

    // 3. Corrupt JSON .item file
    fs::write(tmp.path().join("broken.item"), b"{ AppName: unquoted }").unwrap();

    // 4. Empty AppName .item file
    fs::write(tmp.path().join("empty_name.item"), r#"{"AppName": "", "CatalogItemId": "c", "CatalogNamespace": "n", "DisplayName": "d", "InstallationGuid": "g", "InstallLocation": "l"}"#).unwrap();

    // 5. Subdirectory named something.item (must not crash reader)
    fs::create_dir_all(tmp.path().join("folder.item")).unwrap();

    // 6. Non-item files
    fs::write(tmp.path().join("readme.txt"), b"documentation").unwrap();

    let scanned = scan_manifest_directory(tmp.path()).expect("Directory scan must not fail on corrupt entries");
    assert_eq!(scanned.len(), 1, "Must successfully recover only the single valid manifest");
    assert_eq!(scanned[0].app_name, "ValidApp");
}

#[test]
fn test_t5_manifest_is_valid_and_is_dlc_adversarial_truth_table() {
    let mut base = EpicManifestItem {
        format_version: Some(0),
        app_name: "GameA".to_string(),
        catalog_item_id: "cat_a".to_string(),
        catalog_namespace: "ns_a".to_string(),
        display_name: "Game A".to_string(),
        installation_guid: "guid_a".to_string(),
        install_location: "/Games/A".to_string(),
        install_size: 1000,
        launch_executable: "game.exe".to_string(),
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: None,
        can_run_offline: None,
        requires_auth: None,
        app_version_string: None,
    };

    // Valid base game
    assert!(base.is_valid_installation());
    assert!(!base.is_dlc());

    // Incomplete install flags
    base.is_incomplete_install = Some(true);
    assert!(!base.is_valid_installation());
    base.is_incomplete_install = Some(false);
    assert!(base.is_valid_installation());

    // Empty install location
    base.install_location = "   ".to_string();
    assert!(!base.is_valid_installation());
    base.install_location = "/Games/A".to_string();

    // DLC configurations
    base.main_game_app_name = Some("GameA".to_string()); // same as app_name -> NOT DLC
    assert!(!base.is_dlc());

    base.main_game_app_name = Some("   ".to_string()); // whitespace -> NOT DLC
    assert!(!base.is_dlc());

    base.main_game_app_name = Some("OtherParentGame".to_string()); // different -> IS DLC
    assert!(base.is_dlc());
}

// ============================================================================
// SECTION 4: LAUNCH ENGINE ADVERSARIAL HARDENING (`src/launchers/epic/launch.rs`)
// ============================================================================

#[test]
fn test_t5_launch_parameter_injection_resistance() {
    let injection_payloads = [
        "; rm -rf / ;",
        "&& sudo reboot",
        "| cat /etc/shadow",
        "$(whoami)",
        "`id`",
        "new\nline\ninjection",
        "-AUTH_PASSWORD=injected_leak",
        "--custom-flag=\"quoted argument with spaces\"",
    ];

    let custom_args: Vec<String> = injection_payloads.iter().map(|s| s.to_string()).collect();
    let raw_args = build_launch_parameters(
        "secret_token_123",
        "GameApp",
        "acc_user",
        "UserDisplayName",
        "game_ns",
        Some("en"),
        Some(&custom_args),
    );

    // 1. Verify standard parameters are ordered properly
    assert_eq!(raw_args[0], "-AUTH_LOGIN=unused");
    assert_eq!(raw_args[1], "-AUTH_PASSWORD=secret_token_123");
    assert_eq!(raw_args[2], "-AUTH_TYPE=exchangecode");
    assert_eq!(raw_args[3], "-epicapp=GameApp");
    assert_eq!(raw_args[4], "-epicenv=Prod");
    assert_eq!(raw_args[5], "-EpicPortal");

    // 2. Verify all custom injection arguments are preserved as individual elements
    for payload in &injection_payloads {
        assert!(
            raw_args.contains(&payload.to_string()),
            "Argument slice must contain payload without shell splitting: '{payload}'"
        );
    }

    // 3. Masked arguments must mask both original and injected -AUTH_PASSWORD
    let masked = mask_launch_arguments(&raw_args);
    assert_eq!(masked[1], "-AUTH_PASSWORD=********");
    assert!(
        !masked.iter().any(|arg| arg.contains("secret_token_123")),
        "Raw secret token must not exist in masked arguments"
    );
    assert!(
        !masked.iter().any(|arg| arg == "-AUTH_PASSWORD=injected_leak"),
        "Injected auth password must also be masked"
    );
}

#[test]
fn test_t5_launch_secret_masking_comprehensive_matrix() {
    let test_cases = [
        // (input, expected)
        ("-AUTH_PASSWORD=my_confidential_token", "-AUTH_PASSWORD=********"),
        ("-auth_password=lowercase_token", "-AUTH_PASSWORD=********"),
        ("-Auth_Password=MixedCase", "-AUTH_PASSWORD=********"),
        ("-AUTH_PASSWORD=", "-AUTH_PASSWORD=********"),
        ("-epicovt=confidential_ovt_token", "-epicovt=[REDACTED]"),
        ("-EPICOVT=UPPER_OVT", "-epicovt=[REDACTED]"),
        ("-EpicOvt=mixed_ovt", "-epicovt=[REDACTED]"),
        // Non-matching arguments should remain untouched
        ("-epicapp=Fortnite", "-epicapp=Fortnite"),
        ("-AUTH_LOGIN=unused", "-AUTH_LOGIN=unused"),
        ("--not-auth-password=secret", "--not-auth-password=secret"),
    ];

    for (input, expected) in test_cases {
        assert_eq!(
            mask_arg(input),
            expected,
            "Masking failed for input: '{input}'"
        );
    }
}

#[test]
fn test_t5_launch_command_display_and_redacted_args_never_leak() {
    let secret = "sensitive_exchange_code_9876543210";
    let cmd = LaunchCommand {
        executable_path: PathBuf::from("/games/TestGame.exe"),
        args: vec![
            "-AUTH_LOGIN=unused".to_string(),
            format!("-AUTH_PASSWORD={secret}"),
            "-epicovt=super_secret_ovt".to_string(),
        ],
        display_args: vec![
            "-AUTH_LOGIN=unused".to_string(),
            "-AUTH_PASSWORD=********".to_string(),
            "-epicovt=[REDACTED]".to_string(),
        ],
        working_dir: PathBuf::from("/games"),
        env: HashMap::new(),
    };

    let display_cmd = cmd.display_command_line();
    assert!(!display_cmd.contains(secret));
    assert!(!display_cmd.contains("super_secret_ovt"));
    assert!(display_cmd.contains("-AUTH_PASSWORD=********"));
    assert!(display_cmd.contains("-epicovt=[REDACTED]"));

    let redacted_fmt = format!("{}", cmd.redacted_args());
    assert!(!redacted_fmt.contains(secret));
    assert!(redacted_fmt.contains("-AUTH_PASSWORD=********"));

    let redacted_debug = format!("{:?}", cmd.redacted_args());
    assert!(!redacted_debug.contains(secret));
    assert!(redacted_debug.contains("-AUTH_PASSWORD=********"));
}

#[test]
fn test_t5_launch_process_nonexistent_executable_fails() {
    let cmd = LaunchCommand {
        executable_path: PathBuf::from("/definitely/nonexistent/binary_location_123.exe"),
        args: vec!["-AUTH_LOGIN=unused".to_string()],
        display_args: vec!["-AUTH_LOGIN=unused".to_string()],
        working_dir: PathBuf::from("/tmp"),
        env: HashMap::new(),
    };

    let options = LaunchOptions::default();
    let res = execute_game_with_options(&cmd, &options);
    assert!(res.is_err(), "Must fail when binary does not exist");
    let err = match res {
        Err(e) => e,
        Ok(_) => panic!("Must fail when binary does not exist"),
    };
    assert!(matches!(err, LaunchError::ExecutableNotFound(_)));
}

#[test]
fn test_t5_launch_process_nonexistent_working_dir_fails() {
    // Find an existing binary on the system (e.g. /bin/echo or /bin/sh on Unix, or cargo executable)
    let candidate_bin = if Path::new("/bin/echo").is_file() {
        PathBuf::from("/bin/echo")
    } else if Path::new("/bin/sh").is_file() {
        PathBuf::from("/bin/sh")
    } else {
        std::env::current_exe().unwrap()
    };

    let cmd = LaunchCommand {
        executable_path: candidate_bin,
        args: vec!["test".to_string()],
        display_args: vec!["test".to_string()],
        working_dir: PathBuf::from("/definitely/nonexistent/working/dir/999"),
        env: HashMap::new(),
    };

    let options = LaunchOptions::default();
    let res = execute_game_with_options(&cmd, &options);
    assert!(res.is_err(), "Must fail when working_dir does not exist");
    let err = match res {
        Err(e) => e,
        Ok(_) => panic!("Must fail when working_dir does not exist"),
    };
    assert!(matches!(err, LaunchError::ProcessFailed(_)));
}

#[test]
fn test_t5_launch_process_simulation_and_lifecycle() {
    let cmd = LaunchCommand {
        executable_path: PathBuf::from("/games/Simulated.exe"),
        args: vec!["-AUTH_LOGIN=unused".to_string()],
        display_args: vec!["-AUTH_LOGIN=unused".to_string()],
        working_dir: PathBuf::from("/games"),
        env: HashMap::new(),
    };

    let options = LaunchOptions::simulated();
    let mut proc = execute_game_with_options(&cmd, &options).expect("Dry-run must succeed");

    assert_eq!(proc.pid(), 42424);
    assert!(proc.is_simulated());
    assert_eq!(proc.try_wait().unwrap(), None);
    assert!(proc.wait().unwrap().success());
    assert!(proc.kill().is_ok());
    assert!(proc.take_child().is_none());
}

#[test]
fn test_t5_launch_dlc_and_incomplete_manifest_rejections() {
    // 1. Incomplete manifest rejection
    let incomplete_manifest = EpicManifestItem {
        format_version: Some(0),
        app_name: "GameIncomplete".to_string(),
        catalog_item_id: "cat_inc".to_string(),
        catalog_namespace: "ns".to_string(),
        display_name: "Incomplete Game".to_string(),
        installation_guid: "guid".to_string(),
        install_location: "/games/inc".to_string(),
        install_size: 100,
        launch_executable: "game.exe".to_string(),
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: Some(true),
        can_run_offline: None,
        requires_auth: None,
        app_version_string: None,
    };

    let err = prepare_launch_command(
        &incomplete_manifest,
        "token",
        "acc",
        "User",
        None,
        None,
    ).expect_err("Incomplete manifest must be rejected");
    assert!(matches!(err, LaunchError::IncompleteInstall));

    // 2. DLC rejection
    let dlc_manifest = EpicManifestItem {
        format_version: Some(0),
        app_name: "DLC_Expansion".to_string(),
        catalog_item_id: "cat_dlc".to_string(),
        catalog_namespace: "ns".to_string(),
        display_name: "Expansion Pack".to_string(),
        installation_guid: "guid_dlc".to_string(),
        install_location: "/games/base".to_string(),
        install_size: 50,
        launch_executable: "base.exe".to_string(),
        launch_command: None,
        main_game_app_name: Some("BaseGame".to_string()),
        mandatory_app_folder_name: None,
        is_incomplete_install: Some(false),
        can_run_offline: None,
        requires_auth: None,
        app_version_string: None,
    };

    let err2 = prepare_launch_command(
        &dlc_manifest,
        "token",
        "acc",
        "User",
        None,
        None,
    ).expect_err("DLC manifest must be rejected");
    match err2 {
        LaunchError::CannotLaunchDlc { app_name, main_game } => {
            assert_eq!(app_name, "DLC_Expansion");
            assert_eq!(main_game, "BaseGame");
        }
        other => panic!("Expected CannotLaunchDlc, got: {other:?}"),
    }
}

// ============================================================================
// SECTION 5: AUTH STORE ADVERSARIAL HARDENING (`src/launchers/epic/store.rs`)
// ============================================================================

#[test]
fn test_t5_store_token_cache_expiration_60s_boundary() {
    // Token validity requires > 60s remaining:
    // Utc::now() + Duration::seconds(60) < self.expires_at

    // 1. Expiring in 59 seconds: INVALID
    let expired_soon = CachedToken {
        access_token: "tok_expired_soon".to_string(),
        expires_at: Utc::now() + chrono::Duration::seconds(59),
    };
    assert!(!expired_soon.is_valid(), "Token expiring in 59s must be considered invalid/expired");

    // 2. Expiring in 65 seconds: VALID
    let valid_longer = CachedToken {
        access_token: "tok_valid_longer".to_string(),
        expires_at: Utc::now() + chrono::Duration::seconds(65),
    };
    assert!(valid_longer.is_valid(), "Token expiring in 65s must be considered valid");
}

#[tokio::test]
async fn test_t5_store_concurrent_account_switching_stress() {
    let db = setup_test_db().await;
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    // Populate Account 1 & Account 2
    let acc1 = Account {
        id: "acc_jedi".to_string(),
        launcher: "epic".to_string(),
        username: "JediKnight".to_string(),
        display_name: Some("JediKnight".to_string()),
        email: Some("jedi@temple.org".to_string()),
        is_active: true,
        last_used: Some(Utc::now().to_rfc3339()),
    };
    let acc2 = Account {
        id: "acc_sith".to_string(),
        launcher: "epic".to_string(),
        username: "SithLord".to_string(),
        display_name: Some("SithLord".to_string()),
        email: Some("sith@academy.org".to_string()),
        is_active: false,
        last_used: Some(Utc::now().to_rfc3339()),
    };

    db.upsert_epic_account(&acc1).await.unwrap();
    db.upsert_epic_account(&acc2).await.unwrap();

    let client = Arc::new(EpicClient::from_router(Router::new()));
    let store = EpicAuthStore::new(db.clone(), keyring, client);

    // Rapid concurrent account switching from 20 parallel tasks
    let mut tasks = Vec::new();
    for i in 0..20 {
        let store_c = store.clone();
        tasks.push(tokio::spawn(async move {
            let target = if i % 2 == 0 { "acc_jedi" } else { "acc_sith" };
            let switched = store_c.switch_active_account(target).await.unwrap();
            assert_eq!(switched.account_id, target);
        }));
    }

    for t in tasks {
        t.await.unwrap();
    }

    // After all switches complete, active account in memory must match active in SQLite
    let active_store = store.get_active_account().await.unwrap().expect("Active account must exist");
    let active_db = db.get_active_epic_account().await.unwrap().expect("DB active account must exist");
    assert_eq!(active_store.account_id, active_db.id);
}

#[tokio::test]
async fn test_t5_store_switch_to_nonexistent_account_preserves_state() {
    let db = setup_test_db().await;
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let acc = Account {
        id: "acc_legit".to_string(),
        launcher: "epic".to_string(),
        username: "LegitPlayer".to_string(),
        display_name: Some("LegitPlayer".to_string()),
        email: None,
        is_active: true,
        last_used: Some(Utc::now().to_rfc3339()),
    };
    db.upsert_epic_account(&acc).await.unwrap();

    let client = Arc::new(EpicClient::from_router(Router::new()));
    let store = EpicAuthStore::new(db.clone(), keyring, client);

    // Initialize active pointer
    store.switch_active_account("acc_legit").await.unwrap();

    // Attempt switch to bogus account (currently yields EpicError::Database due to db.rs set_active_epic_account check)
    let err = store.switch_active_account("acc_phantom_ghost")
        .await
        .expect_err("Switch to non-existent account must fail");
    assert!(
        matches!(err, EpicError::AccountNotFound(_) | EpicError::Database(_)),
        "Expected AccountNotFound or Database error, got: {err:?}"
    );

    // Active account must remain LegitPlayer
    let active = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(active.account_id, "acc_legit");
    let active_db = db.get_active_epic_account().await.unwrap().unwrap();
    assert_eq!(active_db.id, "acc_legit");
}

#[tokio::test]
async fn test_t5_store_account_removal_state_transitions() {
    let db = setup_test_db().await;
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let acc1 = Account {
        id: "acc_one".to_string(),
        launcher: "epic".to_string(),
        username: "PlayerOne".to_string(),
        display_name: Some("PlayerOne".to_string()),
        email: None,
        is_active: true,
        last_used: Some(Utc::now().to_rfc3339()),
    };
    let acc2 = Account {
        id: "acc_two".to_string(),
        launcher: "epic".to_string(),
        username: "PlayerTwo".to_string(),
        display_name: Some("PlayerTwo".to_string()),
        email: None,
        is_active: false,
        last_used: Some(Utc::now().to_rfc3339()),
    };

    db.upsert_epic_account(&acc1).await.unwrap();
    db.upsert_epic_account(&acc2).await.unwrap();

    let client = Arc::new(EpicClient::from_router(Router::new()));
    let store = EpicAuthStore::new(db.clone(), keyring, client);
    store.switch_active_account("acc_one").await.unwrap();

    // 1. Remove active account 1 -> Account 2 should be automatically promoted to active
    store.remove_account("acc_one").await.unwrap();
    let accounts_left = store.get_accounts().await.unwrap();
    assert_eq!(accounts_left.len(), 1);
    assert_eq!(accounts_left[0].account_id, "acc_two");

    let new_active = store.get_active_account().await.unwrap().unwrap();
    assert_eq!(new_active.account_id, "acc_two");

    // 2. Remove remaining account 2 -> active becomes None
    store.remove_account("acc_two").await.unwrap();
    assert!(store.get_accounts().await.unwrap().is_empty());
    assert!(store.get_active_account().await.unwrap().is_none());
}

#[tokio::test]
async fn test_t5_store_session_restore_with_network_offline() {
    let db = setup_test_db().await;
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let auth = sample_device_auth("acc_offline");
    keyring.set_device_auth(&auth).unwrap();

    let acc = Account {
        id: "acc_offline".to_string(),
        launcher: "epic".to_string(),
        username: "OfflinePlayer".to_string(),
        display_name: Some("OfflinePlayer".to_string()),
        email: None,
        is_active: true,
        last_used: Some(Utc::now().to_rfc3339()),
    };
    db.upsert_epic_account(&acc).await.unwrap();

    // Router simulating offline / failed network on token refresh
    let router = Router::new().route(
        "/account/api/oauth/token",
        post(|| async { (StatusCode::SERVICE_UNAVAILABLE, "Network Offline").into_response() }),
    );

    let client = Arc::new(EpicClient::from_router(router));
    let store = EpicAuthStore::new(db.clone(), keyring, client);

    // restore_sessions must NOT fail when network refresh fails
    let report = store.restore_sessions().await.expect("Session restore must be resilient to offline boot");
    assert_eq!(report.total_accounts, 1);
    assert_eq!(report.restored_sessions, 1);
    assert_eq!(report.active_account_id.as_deref(), Some("acc_offline"));
}

#[tokio::test]
async fn test_t5_store_session_restore_missing_keyring_device_auth() {
    let db = setup_test_db().await;
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    // Account in SQLite, but NO DeviceAuth in Keyring
    let acc = Account {
        id: "acc_no_keyring".to_string(),
        launcher: "epic".to_string(),
        username: "OrphanedPlayer".to_string(),
        display_name: Some("OrphanedPlayer".to_string()),
        email: None,
        is_active: true,
        last_used: Some(Utc::now().to_rfc3339()),
    };
    db.upsert_epic_account(&acc).await.unwrap();

    let client = Arc::new(EpicClient::from_router(Router::new()));
    let store = EpicAuthStore::new(db.clone(), keyring, client);

    let report = store.restore_sessions().await.unwrap();
    assert_eq!(report.total_accounts, 1);
    assert_eq!(report.restored_sessions, 0, "Accounts without Keyring entries cannot be restored");
}

#[tokio::test]
async fn test_t5_store_sync_library_faulty_manifests() {
    let db = setup_test_db().await;
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let auth = sample_device_auth("acc_sync");
    keyring.set_device_auth(&auth).unwrap();

    let acc = Account {
        id: "acc_sync".to_string(),
        launcher: "epic".to_string(),
        username: "SyncPlayer".to_string(),
        display_name: Some("SyncPlayer".to_string()),
        email: None,
        is_active: true,
        last_used: Some(Utc::now().to_rfc3339()),
    };
    db.upsert_epic_account(&acc).await.unwrap();

    // Mock entitlements: 1 executable base game, 1 DLC
    let router = Router::new()
        .route(
            "/account/api/oauth/token",
            post(|| async {
                let resp = OAuthTokenResponse {
                    access_token: "tok_sync_valid".to_string(),
                    token_type: "bearer".to_string(),
                    expires_in: 3600,
                    expires_at: None,
                    account_id: "acc_sync".to_string(),
                    client_id: "client".to_string(),
                    display_name: Some("SyncPlayer".to_string()),
                    refresh_token: None,
                    refresh_expires: None,
                    refresh_expires_at: None,
                    device_id: None,
                    app: None,
                    in_app_id: None,
                    internal_client: None,
                    client_service: None,
                };
                (StatusCode::OK, Json(resp)).into_response()
            }),
        )
        .route(
            "/entitlement/api/account/:account_id/entitlements",
            get(|| async {
                let items = vec![
                    json!({
                        "id": "ent_game",
                        "entitlementName": "Fortnite",
                        "namespace": "fn",
                        "catalogItemId": "4b6c317ad5654dafa94fbf2a11b64e10",
                        "accountId": "acc_sync",
                        "entitlementType": "EXECUTABLE",
                        "status": "ACTIVE",
                        "active": true
                    }),
                    json!({
                        "id": "ent_dlc",
                        "entitlementName": "Fortnite SoundTrack",
                        "namespace": "fn",
                        "catalogItemId": "dlc_item_id_123",
                        "accountId": "acc_sync",
                        "entitlementType": "AUDIOTRACK",
                        "status": "ACTIVE",
                        "active": true
                    }),
                ];
                (StatusCode::OK, Json(items)).into_response()
            }),
        );

    let client = Arc::new(EpicClient::from_router(router));
    let store = EpicAuthStore::new(db.clone(), keyring, client);

    // Create temp manifest dir with 1 valid manifest and 1 corrupted manifest
    let tmp_manifests = tempdir().unwrap();
    let valid_manifest = r#"{
        "FormatVersion": 0,
        "AppName": "Fortnite",
        "CatalogItemId": "4b6c317ad5654dafa94fbf2a11b64e10",
        "CatalogNamespace": "fn",
        "DisplayName": "Fortnite",
        "InstallationGuid": "{GUID}",
        "InstallLocation": "/Games/Fortnite",
        "InstallSize": 50000000,
        "LaunchExecutable": "FortniteClient.exe"
    }"#;
    fs::write(tmp_manifests.path().join("Fortnite.item"), valid_manifest).unwrap();
    fs::write(tmp_manifests.path().join("corrupted.item"), b"{ AppName: broken").unwrap();

    let summary = store.sync_library("acc_sync", Some(tmp_manifests.path())).await.unwrap();
    assert_eq!(summary.total_entitlements, 2);
    assert_eq!(summary.base_games_count, 1);
    assert_eq!(summary.dlc_count, 1);
    assert_eq!(summary.installed_count, 1);
}

#[tokio::test]
async fn test_t5_store_launch_game_validations() {
    let db = setup_test_db().await;
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());
    let client = Arc::new(EpicClient::from_router(Router::new()));
    let store = EpicAuthStore::new(db.clone(), keyring, client);

    // 1. Empty app_name must fail
    let err1 = store.launch_game(None, "   ", None, None).await.unwrap_err();
    assert!(matches!(err1, EpicError::Database(ref s) if s.contains("app_name must not be empty")));

    // 2. Launch when no account is active
    let err2 = store.launch_game(None, "Fortnite", None, None).await.unwrap_err();
    assert!(matches!(err2, EpicError::NoActiveAccount));

    // 3. Dry-run launch with active account and valid mock endpoints
    let auth = sample_device_auth("acc_pilot");
    store.keyring().set_device_auth(&auth).unwrap();
    let acc = Account {
        id: "acc_pilot".to_string(),
        launcher: "epic".to_string(),
        username: "Pilot".to_string(),
        display_name: Some("Pilot".to_string()),
        email: None,
        is_active: true,
        last_used: Some(Utc::now().to_rfc3339()),
    };
    db.upsert_epic_account(&acc).await.unwrap();

    let router = Router::new()
        .route(
            "/account/api/oauth/token",
            post(|| async {
                let resp = OAuthTokenResponse {
                    access_token: "tok_launch_valid".to_string(),
                    token_type: "bearer".to_string(),
                    expires_in: 3600,
                    expires_at: None,
                    account_id: "acc_pilot".to_string(),
                    client_id: "client".to_string(),
                    display_name: Some("Pilot".to_string()),
                    refresh_token: None,
                    refresh_expires: None,
                    refresh_expires_at: None,
                    device_id: None,
                    app: None,
                    in_app_id: None,
                    internal_client: None,
                    client_service: None,
                };
                (StatusCode::OK, Json(resp)).into_response()
            }),
        )
        .route(
            "/account/api/oauth/exchange",
            get(|| async {
                let resp = ExchangeCodeResponse {
                    code: "exchange_code_pilot_12345".to_string(),
                    creating_client_id: "client".to_string(),
                    expires_in_seconds: 300,
                };
                (StatusCode::OK, Json(resp)).into_response()
            }),
        );

    let client_live = Arc::new(EpicClient::from_router(router));
    let store_live = EpicAuthStore::new(db.clone(), store.keyring().clone(), client_live);
    store_live.switch_active_account("acc_pilot").await.unwrap();

    let launch_res = store_live.launch_game_with_options(LaunchGameOptions {
        account_id: None,
        app_name: "Fortnite".to_string(),
        namespace: Some("fn".to_string()),
        locale: Some("en".to_string()),
        additional_args: Some(vec!["-novid".to_string()]),
        dry_run: true,
    }).await.expect("Dry run launch should succeed");

    assert_eq!(launch_res.app_name, "Fortnite");
    assert_eq!(launch_res.pid, Some(42424));
    assert!(launch_res.launch_arguments.contains(&"-AUTH_PASSWORD=exchange_code_pilot_12345".to_string()));
    assert!(launch_res.display_arguments.contains(&"-AUTH_PASSWORD=********".to_string()));
    assert!(launch_res.display_arguments.contains(&"-novid".to_string()));
}

#[cfg(unix)]
#[test]
fn test_t5_launch_process_non_executable_permissions_fails() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let script_path = tmp.path().join("unexecutable_game.sh");
    fs::write(&script_path, b"#!/bin/sh\necho should not run\n").unwrap();

    // Set permissions to 0o644 (read/write, NO execute bit)
    let permissions = fs::Permissions::from_mode(0o644);
    fs::set_permissions(&script_path, permissions).unwrap();

    let cmd = LaunchCommand {
        executable_path: script_path,
        args: vec![],
        display_args: vec![],
        working_dir: tmp.path().to_path_buf(),
        env: HashMap::new(),
    };

    let options = LaunchOptions::default();
    let res = execute_game_with_options(&cmd, &options);
    assert!(res.is_err(), "Must fail when binary lacks execute permissions");
    let err = match res {
        Err(e) => e,
        Ok(_) => panic!("Expected process spawn to fail"),
    };
    match err {
        LaunchError::ProcessFailed(io_err) => {
            assert_eq!(io_err.kind(), std::io::ErrorKind::PermissionDenied);
        }
        other => panic!("Expected ProcessFailed(PermissionDenied), got: {other:?}"),
    }
}

#[tokio::test]
async fn test_t5_client_delete_device_auth_html_and_error_handling() {
    // 1. DELETE deviceAuth returns 500 HTML
    let router500 = Router::new().route(
        "/account/api/public/account/:account_id/deviceAuth/:device_id",
        delete(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "<html>Server Error</html>").into_response() }),
    );
    let client = EpicClient::from_router(router500);
    let err = client.delete_device_auth("acc1", "dev1", "tok").await.unwrap_err();
    match err {
        EpicError::Api { status, message, .. } => {
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert!(message.contains("Server Error"));
        }
        other => panic!("Expected Api error, got: {other:?}"),
    }

    // 2. DELETE deviceAuth returns 204 NO_CONTENT (success)
    let router204 = Router::new().route(
        "/account/api/public/account/:account_id/deviceAuth/:device_id",
        delete(|| async { StatusCode::NO_CONTENT.into_response() }),
    );
    let client204 = EpicClient::from_router(router204);
    assert!(client204.delete_device_auth("acc1", "dev1", "tok").await.is_ok());
}

#[tokio::test]
async fn test_t5_client_fetch_entitlements_403_forbidden() {
    let forbidden_payload = json!({
        "errorCode": "errors.com.epicgames.account.oauth.scope_insufficient",
        "errorMessage": "Access token does not possess entitlement read scope",
        "numericErrorCode": 18042
    }).to_string();

    let router = Router::new().route(
        "/entitlement/api/account/:account_id/entitlements",
        get(move || {
            let p = forbidden_payload.clone();
            async move { (StatusCode::FORBIDDEN, p).into_response() }
        }),
    );

    let client = EpicClient::from_router(router);
    let err = client.fetch_entitlements("bad_scope_tok", "acc_1", 0, 10).await.unwrap_err();
    match err {
        EpicError::Api { status, code, numeric, message } => {
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert_eq!(code.as_deref(), Some("errors.com.epicgames.account.oauth.scope_insufficient"));
            assert_eq!(numeric, Some(18042));
            assert!(message.contains("scope"));
        }
        other => panic!("Expected Api error, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_t5_store_get_valid_access_token_missing_in_keyring() {
    let db = setup_test_db().await;
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());
    let client = Arc::new(EpicClient::from_router(Router::new()));
    let store = EpicAuthStore::new(db, keyring, client);

    // No token in cache and no DeviceAuth in Keyring
    let err = store.get_valid_access_token("acc_unregistered").await.unwrap_err();
    assert!(matches!(err, EpicError::DeviceAuthNotFound(id) if id == "acc_unregistered"));
}

#[tokio::test]
async fn test_t5_store_get_valid_access_token_refresh_fails_invalid_credentials() {
    let db = setup_test_db().await;
    let keyring: Arc<dyn CredentialStore> = Arc::new(MemoryCredentialStore::new());

    let auth = sample_device_auth("acc_revoked");
    keyring.set_device_auth(&auth).unwrap();

    let router401 = Router::new().route(
        "/account/api/oauth/token",
        post(|| async {
            let err_body = json!({
                "errorCode": "errors.com.epicgames.account.invalid_account_credentials",
                "errorMessage": "Device auth revoked on server",
                "numericErrorCode": 18031
            }).to_string();
            (StatusCode::BAD_REQUEST, err_body).into_response()
        }),
    );

    let client = Arc::new(EpicClient::from_router(router401));
    let store = EpicAuthStore::new(db, keyring, client);

    let err = store.get_valid_access_token("acc_revoked").await.unwrap_err();
    assert!(err.is_invalid_credentials());
}

#[test]
fn test_t5_manifest_empty_launch_executable_fallback_to_install_location() {
    let manifest = EpicManifestItem {
        format_version: Some(0),
        app_name: "Celeste".to_string(),
        catalog_item_id: "cat_celeste".to_string(),
        catalog_namespace: "ns_celeste".to_string(),
        display_name: "Celeste".to_string(),
        installation_guid: "guid".to_string(),
        install_location: "/Games/Celeste".to_string(),
        install_size: 100,
        launch_executable: "   ".to_string(),
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: Some(false),
        can_run_offline: Some(true),
        requires_auth: None,
        app_version_string: None,
    };

    let resolved = manifest.resolve_executable_path();
    assert_eq!(resolved, PathBuf::from("/Games/Celeste"));
}

#[test]
fn test_t5_manifest_missing_install_location_rejected() {
    let json_no_loc = r#"{
        "AppName": "NoLocGame",
        "CatalogItemId": "cat_1",
        "CatalogNamespace": "ns_1",
        "DisplayName": "No Loc",
        "InstallationGuid": "guid"
    }"#;

    let res = parse_manifest_from_str(json_no_loc);
    assert!(matches!(res, Err(ManifestError::Json(_))));
}

