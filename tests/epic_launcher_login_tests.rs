use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use axum::{Router, routing::{get, post}, Json, http::HeaderMap};
use serde_json::json;
use swch::{storage::Database, launchers::epic::{EpicAuthStore, EpicClient, MemoryCredentialStore, DeviceAuth}};

#[tokio::test]
async fn launcher_login_gets_fresh_exchange_for_selected_account_without_cookies() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    db.save_epic_credential("selected", &serde_json::to_string(&DeviceAuth::new("selected", "device", "secret")).unwrap()).await.unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let router = Router::new()
        .route("/account/api/oauth/token", post(|| async { Json(json!({
            "access_token":"selected_access", "expires_in":3600, "token_type":"bearer", "account_id":"selected", "client_id":"test"
        })) }))
        .route("/account/api/oauth/exchange", get(move |headers: HeaderMap| {
            let requests = requests.clone();
            async move {
                assert_eq!(headers["authorization"], "Bearer selected_access");
                let number = requests.fetch_add(1, Ordering::SeqCst);
                Json(json!({"code":format!("single_use_{number}"), "creatingClientId":"test", "expiresInSeconds":300}))
            }
        }));
    let store = EpicAuthStore::new(db, Arc::new(MemoryCredentialStore::new()), Arc::new(EpicClient::from_router(router)));
    assert_eq!(store.launcher_exchange_code("selected").await.unwrap(), "single_use_0");
    assert_eq!(store.launcher_exchange_code("selected").await.unwrap(), "single_use_1");
    assert!(store.launcher_exchange_code("missing").await.is_err());
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn revoked_refresh_token_does_not_mark_target_active_or_launch_epic() {
    use axum::{extract::State, http::StatusCode};
    use swch::{storage::Account, server::handlers::{open_epic_launcher, SwitchEpicAccountRequest}};
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    for id in ["current", "revoked"] {
        db.upsert_account(&Account { id: id.into(), launcher: "epic".into(), username: id.into(),
            display_name: Some(id.into()), email: None, is_active: id == "current", last_used: None }).await.unwrap();
    }
    let mut credential = DeviceAuth::new("revoked", "", "");
    credential.refresh_token = Some("revoked_refresh".into());
    db.save_epic_credential("revoked", &serde_json::to_string(&credential).unwrap()).await.unwrap();
    let router = Router::new().route("/account/api/oauth/token", post(|| async {
        (StatusCode::BAD_REQUEST, Json(json!({
            "errorCode":"errors.com.epicgames.account.auth_token.invalid_refresh_token",
            "errorMessage":"Do not expose revoked_refresh"
        })))
    }));
    let store = Arc::new(EpicAuthStore::new(db, Arc::new(MemoryCredentialStore::new()), Arc::new(EpicClient::from_router(router))));
    store.switch_active_account("current").await.unwrap();
    let request: SwitchEpicAccountRequest = serde_json::from_value(json!({"account_id":"revoked"})).unwrap();
    let (status, error) = open_epic_launcher(State(store.clone()), Json(request)).await.unwrap_err();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(error.error.contains("invalid_refresh_token"));
    assert!(!error.error.contains("revoked_refresh"));
    assert_eq!(store.get_active_account().await.unwrap().unwrap().account_id, "current");
}

#[tokio::test]
async fn launcher_session_uses_dedicated_refresh_and_checks_account_identity() {
    use axum::extract::Form;
    use std::collections::HashMap;
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    db.save_epic_credential("selected", &serde_json::to_string(&DeviceAuth::new("selected", "device", "secret")).unwrap()).await.unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let router = Router::new()
        .route("/account/api/oauth/token", post(move |Form(form): Form<HashMap<String, String>>| {
            let requests = requests.clone();
            async move {
                let exchange = form["grant_type"] == "exchange_code";
                if exchange {
                    assert_eq!(form["exchange_code"], "native_exchange");
                    requests.fetch_add(1, Ordering::SeqCst);
                }
                Json(json!({"access_token":"selected_access", "expires_in":3600, "token_type":"bearer",
                    "account_id": if requests.load(Ordering::SeqCst) > 1 { "wrong_account" } else { "selected" },
                    "client_id":"test", "refresh_token": if exchange { "native_refresh" } else { "swch_refresh" }}))
            }
        }))
        .route("/account/api/oauth/exchange", get(|| async {
            Json(json!({"code":"native_exchange", "creatingClientId":"test", "expiresInSeconds":300}))
        }));
    let store = EpicAuthStore::new(db.clone(), Arc::new(MemoryCredentialStore::new()), Arc::new(EpicClient::from_router(router)));
    assert_eq!(store.launcher_refresh_token("selected").await.unwrap(), "native_refresh");
    let saved: DeviceAuth = serde_json::from_str(&db.get_epic_credential("selected").await.unwrap().unwrap()).unwrap();
    assert_eq!(saved.refresh_token.as_deref(), Some("swch_refresh"));
    assert!(store.launcher_refresh_token("selected").await.is_err());
}
