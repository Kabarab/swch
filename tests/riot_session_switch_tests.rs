use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use swch::{launchers::riot::session::RiotSessionManager, server::create_app, storage::Database};
use tower::ServiceExt;

async fn request(db: &Database, path: &str, body: Value) -> (StatusCode, Value) {
    let response = create_app(db.clone())
        .oneshot(
            Request::builder()
                .uri(path)
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn login(account: &str) -> String {
    format!("riot-login:\n    persist:\n        session:\n            cookies:\n            -   name: sub\n                persistent: true\n                value: {account}\n            -   name: ssid\n                persistent: true\n                value: test-session\n")
}

// This test executable has a single test so its temporary environment cannot
// race other tests or touch the user's Riot installation/session store.
#[tokio::test]
async fn save_and_switch_use_games_login_and_reject_cleared_snapshots() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("Data");
    let sessions = tmp.path().join("sessions");
    std::fs::create_dir_all(&data).unwrap();
    std::env::set_var("SWCH_RIOT_DATA_DIR", &data);
    std::env::set_var("SWCH_RIOT_SESSIONS_DIR", &sessions);
    std::env::set_var("SWCH_RIOT_LEGACY_ACCOUNTS_DIR", tmp.path().join("legacy"));
    std::fs::create_dir_all(tmp.path().join("legacy")).unwrap();
    std::env::set_var("SWCH_SKIP_PROCESS_TERMINATION", "1");
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let client = data.join("RiotClientPrivateSettings.yaml");
    let games = data.join("RiotGamesPrivateSettings.yaml");
    std::fs::write(&client, login("stale-client")).unwrap();
    std::fs::write(&games, login("first")).unwrap();
    let (status, saved) = request(&db, "/api/riot/accounts/save", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["account"]["id"], "riot_first");

    let (status, response) = request(
        &db,
        "/api/riot/accounts/save",
        json!({"account_id":"other"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(response["code"], "account_mismatch");
    assert!(!sessions.join("other").exists());

    std::fs::write(&games, login("second")).unwrap();
    let (status, response) = request(
        &db,
        "/api/riot/switch-account",
        json!({"account_id":"riot_first", "launch":false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["restored_saved_session"], true);
    assert_eq!(
        RiotSessionManager::new()
            .read_current_account()
            .account_id
            .as_deref(),
        Some("first")
    );
    assert_eq!(
        std::fs::read_to_string(sessions.join("second/RiotGamesPrivateSettings.yaml")).unwrap(),
        login("second")
    );

    std::fs::write(
        sessions.join("second/RiotGamesPrivateSettings.yaml"),
        "riot-login:\n    persist: null\n",
    )
    .unwrap();
    let (status, response) = request(
        &db,
        "/api/riot/switch-account",
        json!({"account_id":"riot_second", "launch":false}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(response["code"], "riot_login_required");
    assert_eq!(std::fs::read_to_string(&games).unwrap(), login("first"));

    std::fs::write(&games, "riot-login:\n    persist: null\n").unwrap();
    std::fs::write(&client, "riot-login:\n    persist: null\n").unwrap();
    let (status, response) = request(&db, "/api/riot/accounts/save", json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(response["code"], "no_session");
    assert_eq!(
        std::fs::read_to_string(sessions.join("first/RiotGamesPrivateSettings.yaml")).unwrap(),
        login("first")
    );

    // The Client jar remains usable when Games stores only metadata/null persist.
    std::fs::write(&client, login("client-only")).unwrap();
    let (status, response) = request(&db, "/api/riot/accounts/save", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["account"]["id"], "riot_client-only");

    use swch::launchers::riot::profile::{Profile, Section};
    let profile = Profile {
        account_id: "riot_client-only".into(),
        app_id: "valorant".into(),
        display_name: "Client".into(),
        level: None,
        source: "test".into(),
        synced_at: "2026-10-04".into(),
        has_play_history: false,
        champions: Section::from_result(Ok(vec![])),
        skins: Section::from_result(Ok(vec![])),
        playtime: Section::from_result(Err("unavailable".into())),
        ranked:None,
        buddies:None,
    };
    db.save_riot_profile(profile.clone()).await.unwrap();
    db.save_riot_profile(Profile {
        account_id: "riot_first".into(),
        ..profile
    })
    .await
    .unwrap();
    db.upsert_account(&swch::storage::Account {
        id: "epic_same-name".into(),
        launcher: "epic".into(),
        username: "client-only".into(),
        display_name: None,
        email: None,
        is_active: false,
        last_used: None,
    })
    .await
    .unwrap();
    // Legacy copies must be deleted too, otherwise discovery imports them again.
    let legacy = tmp.path().join("legacy/client-only");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        legacy.join("RiotClientPrivateSettings.yaml"),
        login("client-only"),
    )
    .unwrap();
    let before = std::fs::read_to_string(&client).unwrap();
    let (status, response) = request(
        &db,
        "/api/riot/accounts/remove",
        json!({"account_id":"riot_client-only"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["status"], "success");
    assert!(!sessions.join("client-only").exists());
    assert!(!legacy.exists());
    assert_eq!(
        std::fs::read_to_string(&client).unwrap(),
        before,
        "Live Riot login must stay intact"
    );
    assert!(!db
        .get_accounts()
        .await
        .unwrap()
        .iter()
        .any(|a| a.id == "riot_client-only"));
    assert!(db
        .get_accounts()
        .await
        .unwrap()
        .iter()
        .any(|a| a.id == "epic_same-name"));
    let profiles = db.riot_profiles().await.unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].account_id, "riot_first");
    let mgr = RiotSessionManager::new();
    assert!(mgr.is_removed("riot_client-only"));
    let response = create_app(db.clone())
        .oneshot(
            Request::builder()
                .uri("/api/riot/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let listing: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(!listing["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["id"] == "riot_client-only"));
    mgr.switch_account("riot_first", None).await.unwrap();
    assert!(
        !sessions.join("client-only").exists(),
        "Switch must not back up a removed account again"
    );
    std::fs::write(&client, login("client-only")).unwrap();
    std::fs::write(&games, "riot-login:\n    persist: null\n").unwrap();
    let (status, _) = request(&db, "/api/riot/accounts/save", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !mgr.is_removed("riot_client-only"),
        "Explicit save can reconnect the account"
    );
    let (status, _) = request(
        &db,
        "/api/riot/accounts/remove",
        json!({"account_id":"riot_.."}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(sessions.join("first").exists());

    // Current Riot versions persist SDK refresh authorization, even while
    // their legacy riot-login persist block is null.
    use base64::Engine;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        json!({"sub":"sdk-account", "uname":"sdk-user"}).to_string());
    let modern = format!("psl:\n    authorization:\n        riot-client:\n            id_token: \"header.{payload}.signature\"\n            refresh_token: \"fixture-refresh\"\nriot-login:\n    persist: null\n");
    std::fs::write(&games, &modern).unwrap();
    let (status, response) = request(&db, "/api/riot/accounts/save", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["account"]["id"], "riot_sdk-account");
    assert!(!sessions.join("sdk-account/RiotClientPrivateSettings.yaml").exists());
    std::fs::write(&games, login("first")).unwrap();
    let (status, _) = request(&db, "/api/riot/switch-account",
        json!({"account_id":"riot_sdk-account", "launch":false})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(std::fs::read_to_string(&games).unwrap(), modern);
    assert!(!client.exists());
}
