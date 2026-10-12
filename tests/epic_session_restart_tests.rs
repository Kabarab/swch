use axum::{
    extract::Form,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use swch::{
    launchers::epic::{DeviceAuth, EpicAuthStore, EpicClient, MemoryCredentialStore},
    storage::{Account, Database},
};

fn mock_client(refreshes: Arc<AtomicUsize>) -> Arc<EpicClient> {
    let router = Router::new()
        .route("/account/api/oauth/token", post(move |Form(form): Form<HashMap<String, String>>| {
            let refreshes = refreshes.clone();
            async move {
                if form["grant_type"] == "exchange_code" {
                    return (StatusCode::OK, Json(json!({"account_id":"saved", "client_id":"test",
                        "access_token":"native-access", "refresh_token":"native-refresh", "expires_in":3600, "token_type":"bearer"})));
                }
                assert_eq!(form["grant_type"], "refresh_token");
                let number = refreshes.fetch_add(1, Ordering::SeqCst);
                // Treat previous refresh tokens as single-use, as on the real service.
                assert_eq!(form["refresh_token"], format!("refresh-{number}"));
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                (StatusCode::OK, Json(json!({"account_id":"saved", "client_id":"test",
                    "access_token":format!("access-{number}"), "refresh_token":format!("refresh-{}", number + 1),
                    "expires_in":3600, "token_type":"bearer"})))
            }
        }))
        .route("/account/api/oauth/exchange", get(|| async { Json(json!({
            "code":"native-exchange", "creatingClientId":"test", "expiresInSeconds":300
        })) }));
    Arc::new(EpicClient::from_router(router))
}

async fn seed(db: &Database) {
    db.run_migrations().await.unwrap();
    db.upsert_epic_account(&Account {
        id: "saved".into(),
        launcher: "epic".into(),
        username: "Player".into(),
        display_name: Some("Player".into()),
        email: None,
        is_active: true,
        last_used: None,
    })
    .await
    .unwrap();
    let mut auth = DeviceAuth::new("saved", "", "");
    auth.refresh_token = Some("refresh-0".into());
    db.save_epic_credential("saved", &serde_json::to_string(&auth).unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn repeated_cold_starts_and_concurrent_requests_keep_rotated_credentials_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("swch.db");
    let db = Database::open(&path).unwrap();
    seed(&db).await;
    drop(db);
    let refreshes = Arc::new(AtomicUsize::new(0));
    let client = mock_client(refreshes.clone());
    for restart in 0..3 {
        let db = Database::open(&path).unwrap();
        // No caches or keyring entries survive between these instances.
        let store = EpicAuthStore::new(
            db.clone(),
            Arc::new(MemoryCredentialStore::new()),
            client.clone(),
        );
        let (boot, first, second) = tokio::join!(
            store.restore_sessions(),
            store.launcher_refresh_token("saved"),
            store.launcher_refresh_token("saved")
        );
        assert_eq!(boot.unwrap().restored_sessions, 1);
        assert_eq!(first.unwrap(), "native-refresh");
        assert_eq!(second.unwrap(), "native-refresh");
        assert_eq!(refreshes.load(Ordering::SeqCst), restart + 1);
        let auth: DeviceAuth =
            serde_json::from_str(&db.get_epic_credential("saved").await.unwrap().unwrap()).unwrap();
        assert_eq!(auth.refresh_token, Some(format!("refresh-{}", restart + 1)));
        assert_eq!(
            store
                .get_active_account()
                .await
                .unwrap()
                .unwrap()
                .account_id,
            "saved"
        );
    }
}

#[tokio::test]
async fn credential_write_failure_is_reported_before_using_refreshed_token() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("swch.db");
    let db = Database::open(&path).unwrap();
    seed(&db).await;
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_credentials BEFORE UPDATE ON epic_credentials
        BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END;",
        )
        .unwrap();
    let refreshes = Arc::new(AtomicUsize::new(0));
    let store = EpicAuthStore::new(
        db.clone(),
        Arc::new(MemoryCredentialStore::new()),
        mock_client(refreshes.clone()),
    );
    let error = store.launcher_refresh_token("saved").await.unwrap_err();
    assert!(matches!(
        error,
        swch::launchers::epic::EpicError::Database(_)
    ));
    let auth: DeviceAuth =
        serde_json::from_str(&db.get_epic_credential("saved").await.unwrap().unwrap()).unwrap();
    assert_eq!(auth.refresh_token.as_deref(), Some("refresh-0"));
    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
}
