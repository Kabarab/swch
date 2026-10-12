use axum::{
    body::Body,
    extract::Path as AxumPath,
    http::{HeaderMap, Request, StatusCode},
    routing::get,
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use swch::{
    launchers::epic::{EpicAuthStore, EpicClient, MemoryCredentialStore},
    server::{
        create_app_with_state,
        playtime::{epic_records, galaxy_records, minecraft_records, minecraft_seconds, Record},
        AppState,
    },
    storage::{Account, Database},
};
use tower::ServiceExt;

#[test]
fn epic_seconds_account_isolation_unknown_and_zero() {
    let rows = epic_records(
        &json!([
         {"accountId":"one","artifactId":"Fortnite","totalTime":7200},
         {"accountId":"one","artifactId":"Fortnite","totalTime":3600},
         {"accountId":"two","artifactId":"Fortnite","totalTime":99999},
         {"artifactId":"zero","totalTime":0},
         {"artifactId":"missing","totalTime":null},
         {"artifactId":"negative","totalTime":-1}
        ]),
        "one",
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].seconds, 7200.0);
    assert!(rows.iter().all(|r| r.account_id.as_deref() == Some("one")));
    assert_eq!(
        rows.iter().find(|r| r.app_id == "zero").unwrap().seconds,
        0.0
    );
    assert!(epic_records(&json!({"error":"unavailable"}), "one").is_err());
}
#[tokio::test]
async fn epic_fetch_uses_get_and_the_requested_bearer() {
    async fn mock(AxumPath(account): AxumPath<String>, headers: HeaderMap) -> Json<Value> {
        assert_eq!(account, "one");
        assert_eq!(headers["authorization"], "Bearer test-token");
        Json(json!([{"accountId":account,"artifactId":"Fortnite","totalTime":3600}]))
    }
    let client = EpicClient::from_router(Router::new().route(
        "/library/api/public/playtime/account/:account/all",
        get(mock),
    ));
    let data = client.fetch_playtime("test-token", "one").await.unwrap();
    assert_eq!(epic_records(&data, "one").unwrap()[0].seconds, 3600.0);
}
#[test]
fn minecraft_ticks_and_duplicate_roots() {
    let modern = json!({"stats":{"minecraft:custom":{"minecraft:play_time":72000,"minecraft:play_one_minute":144000}}});
    assert_eq!(minecraft_seconds(&modern), Some(3600.0));
    assert_eq!(
        minecraft_seconds(&json!({"stat.playOneMinute":1200})),
        Some(60.0)
    );
    assert_eq!(
        minecraft_seconds(&json!({"stats":{"minecraft:custom":{"minecraft:play_time":0}}})),
        Some(0.0)
    );
    assert_eq!(minecraft_seconds(&json!({})), None);
    let temp = tempfile::tempdir().unwrap();
    let stats = temp.path().join("saves/world/stats");
    std::fs::create_dir_all(&stats).unwrap();
    let uuid = "12345678-1234-1234-1234-123456789abc";
    std::fs::write(stats.join(format!("{uuid}.json")), modern.to_string()).unwrap();
    let accounts=vec![serde_json::from_value(json!({"account_id":"a","username":"Player","account_type":"msa","uuid":uuid,"avatar_url":null,"skin_url":null,"is_active":true,"access_token":null,"raw_json":null,"last_used":null,"updated_at":"2026-10-11"})).unwrap()];
    let rows = minecraft_records(&[temp.path().into(), temp.path().into()], &accounts);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].seconds, 3600.0);
    assert_eq!(rows[0].scope, "local_worlds");
    assert_eq!(rows[0].account_id.as_deref(), Some("a"));
}
#[test]
fn galaxy_local_minutes_are_read_only_not_assigned_to_another_account() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("galaxy.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE GameTimes(releaseKey TEXT,minutesInGame REAL); INSERT INTO GameTimes VALUES('gog_1',60),('gog_1',30),('uplay_2',120),('epic_Fortnite',180),('gog_0',0),('gog_bad',NULL),('gog_negative',-10),('invalid',50);").unwrap();
    drop(conn);
    let before = std::fs::read(&path).unwrap();
    let rows = galaxy_records(&path).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows.iter()
            .find(|r| r.launcher == "gog" && r.app_id == "1")
            .unwrap()
            .seconds,
        3600.0
    );
    assert!(rows
        .iter()
        .all(|r| r.account_id.is_none() && r.scope == "galaxy_local"));
    assert_eq!(before, std::fs::read(path).unwrap());
}
#[tokio::test]
async fn epic_route_preserves_cached_time_on_auth_failure_and_never_switches_accounts() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    db.upsert_account(&Account {
        id: "one".into(),
        launcher: "epic".into(),
        username: "One".into(),
        display_name: None,
        email: None,
        is_active: true,
        last_used: None,
    })
    .await
    .unwrap();
    let row = Record {
        launcher: "epic".into(),
        app_id: "Fortnite".into(),
        account_id: Some("one".into()),
        seconds: 3600.0,
        scope: "lifetime".into(),
        source: "Epic Games".into(),
        cached: false,
        identity: None,
    };
    db.steam_cache_put(
        "playtime:epic:one",
        &serde_json::to_string(&vec![row]).unwrap(),
    )
    .await
    .unwrap();
    let store = Arc::new(EpicAuthStore::new(
        db.clone(),
        Arc::new(MemoryCredentialStore::new()),
        Arc::new(EpicClient::from_router(Router::new())),
    ));
    let app = create_app_with_state(AppState::new(db.clone(), store));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/playtime?launcher=epic&account_id=one&refresh=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["records"][0]["seconds"].as_f64(), Some(3600.0));
    assert_eq!(value["records"][0]["cached"], true);
    assert!(db.get_accounts().await.unwrap()[0].is_active);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/playtime?launcher=epic&account_id=another")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
