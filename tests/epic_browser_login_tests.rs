use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use axum::{Router, routing::post, http::{Request, StatusCode}, body::Body, Json};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use swch::{launchers::epic::{EpicAuthStore, EpicClient, MemoryCredentialStore},
    server::{AppState, create_app_with_state}, storage::Database};

async fn setup(fail: bool) -> (Router, Arc<AtomicUsize>, Arc<EpicAuthStore>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let epic = Router::new()
        .route("/account/api/oauth/token", post(move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                if fail {
                    return (StatusCode::UNAUTHORIZED, Json(json!({"errorMessage": "secret upstream response"})));
                }
                (StatusCode::OK, Json(json!({
                    "access_token": "test-access-token", "token_type": "bearer", "expires_in": 3600,
                    "account_id": "browser-test-account", "client_id": "test-client",
                    "displayName": "Browser Test", "refresh_token": "test-refresh-token"
                })))
            }
        }))
        .route("/account/api/public/account/:account/deviceAuth", post(|| async {
            Json(json!({"accountId": "browser-test-account", "deviceId": "test-device", "secret": "test-secret"}))
        }));
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let store = Arc::new(EpicAuthStore::new(db.clone(), Arc::new(MemoryCredentialStore::new()),
        Arc::new(EpicClient::from_router(epic))));
    (create_app_with_state(AppState::new(db, store.clone())), calls, store)
}

async fn call(app: &Router, path: &str, data: Value) -> (StatusCode, Value) {
    let response = app.clone().oneshot(Request::builder().method("POST")
        .uri(format!("/api/epic/auth/browser/{path}"))
        .header("host", "127.0.0.1:4242").header("content-type", "application/json")
        .body(Body::from(data.to_string())).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn browser_login_returns_an_account_without_waiting_for_library_and_rejects_replay() {
    let (app, calls, store) = setup(false).await;
    let (status, start) = call(&app, "start", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let id = start["session_id"].as_str().unwrap();
    assert_eq!(id.len(), 64);
    assert_eq!(start["browser_url"], format!("http://127.0.0.1:4242/epic/connect#session={id}"));
    let (status, prepared) = call(&app, "prepare", json!({"session_id": id})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(prepared["auth_url"].as_str().unwrap().starts_with("https://www.epicgames.com/"));
    let body = json!({"session_id": id, "code": "0123456789abcdef0123456789abcdef"});
    let (left, right) = tokio::join!(call(&app, "complete", body.clone()), call(&app, "complete", body));
    assert!(matches!((left.0, right.0), (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let success = if left.0 == StatusCode::OK { left.1 } else { right.1 };
    assert_eq!(success["status"], "connected");
    assert_eq!(success["account"]["display_name"], "Browser Test");
    assert_eq!(success["library_status"], "syncing");
    let text = success.to_string();
    assert!(!text.contains("test-access-token") && !text.contains("test-secret") && !text.contains("test-refresh-token"));
    assert_eq!(store.get_accounts().await.unwrap().len(), 1);
    // Library failure is independent of account authentication.
    for _ in 0..100 {
        if store.browser_auth.status(id).await.unwrap().library_status != "syncing" { break; }
        tokio::task::yield_now().await;
    }
    assert_eq!(store.browser_auth.status(id).await.unwrap().status, "connected");
    assert_eq!(call(&app, "complete", json!({"session_id": id, "code": "0123456789abcdef0123456789abcdef"})).await.0,
        StatusCode::CONFLICT);
}

#[tokio::test]
async fn cancellation_invalid_capabilities_and_bad_codes_never_call_epic() {
    let (app, calls, _) = setup(false).await;
    assert_eq!(call(&app, "status", json!({"session_id": "missing"})).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "complete", json!({"session_id": "a".repeat(64), "code": "0123456789abcdef0123456789abcdef"})).await.0,
        StatusCode::NOT_FOUND);
    let (_, start) = call(&app, "start", json!({})).await;
    let id = &start["session_id"];
    assert_eq!(call(&app, "complete", json!({"session_id": id, "code": "invalid"})).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(call(&app, "status", json!({"session_id": id})).await.1["status"], "pending");
    assert_eq!(call(&app, "cancel", json!({"session_id": id})).await.1["status"], "cancelled");
    assert_eq!(call(&app, "complete", json!({"session_id": id, "code": "0123456789abcdef0123456789abcdef"})).await.0,
        StatusCode::CONFLICT);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cross_site_or_non_local_starts_are_rejected_and_epic_errors_are_redacted() {
    let (app, _, _) = setup(true).await;
    for (host, origin) in [("127.0.0.1:4242", "https://example.com"), ("evil.test:4242", "http://evil.test:4242")] {
        let response = app.clone().oneshot(Request::builder().method("POST").uri("/api/epic/auth/browser/start")
            .header("host", host).header("origin", origin).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let (_, start) = call(&app, "start", json!({})).await;
    let id = &start["session_id"];
    let (status, result) = call(&app, "complete", json!({"session_id": id, "code": "0123456789abcdef0123456789abcdef"})).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(!result.to_string().contains("secret upstream response"));
    assert_eq!(call(&app, "status", json!({"session_id": id})).await.1["status"], "failed");
}
