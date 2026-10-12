use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use swch::{server::create_app, storage::Database};
use tower::ServiceExt;

async fn request(path: &str, body: Option<&str>) -> (StatusCode, Value) {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let request = Request::builder()
        .uri(path)
        .method(if body.is_some() { "POST" } else { "GET" })
        .header("content-type", "application/json")
        .body(Body::from(body.unwrap_or("").to_string()))
        .unwrap();
    let response = create_app(db).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn catalog_is_local_and_capabilities_are_honest() {
    let (status, data) = request("/api/riot/library", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["capabilities"]["account_switch"], true);
    assert_eq!(data["capabilities"]["inventory"], true);
    assert_eq!(data["capabilities"]["launch"], data["client_installed"]);
    let games = data["games"].as_array().unwrap();
    assert!(games.iter().all(|g| g["launcher"] == "riot"));
    if cfg!(target_os = "macos") {
        assert_eq!(games.len(), 1);
        assert_eq!(games[0]["app_id"], "league_of_legends");
    }
}
#[tokio::test]
async fn rejects_unknown_products_and_account_switch_without_launching() {
    let (status, data) = request(
        "/api/riot/launch",
        Some(r#"{"app_id":"valorant; echo injected","dry_run":true}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(data["code"], "unknown_product");
    let (status, data) = request("/api/riot/switch-account", Some("{}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(data["code"], "missing_account");
    assert_eq!(
        request("/api/riot/launch", Some("{}")).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            "/api/riot/launch",
            Some(r#"{"app_id":"league_of_legends","account_id":"fake"}"#)
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
#[tokio::test]
async fn dry_run_never_launches_process() {
    let (status, data) = request("/api/riot/open-launcher", Some(r#"{"dry_run":true}"#)).await;
    assert!([
        StatusCode::OK,
        StatusCode::NOT_FOUND,
        StatusCode::NOT_IMPLEMENTED
    ]
    .contains(&status));
    if status == StatusCode::OK {
        assert_eq!(data["status"], "dry_run");
        assert_eq!(data["args"], serde_json::json!([]));
    }
}

#[tokio::test]
async fn riot_profile_endpoints_preserve_unknown_and_reject_unsupported_product() {
    let (status, data)=request("/api/riot/accounts",None).await;
    assert_eq!(status,StatusCode::OK);
    assert_eq!(data["profiles"],serde_json::json!([]));
    let (status, data)=request("/api/riot/accounts/sync",Some(r#"{"app_id":"unknown"}"#)).await;
    assert_eq!(status,StatusCode::BAD_REQUEST);
    assert_eq!(data["code"],"unsupported_product");
}
