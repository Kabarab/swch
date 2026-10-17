use axum::{routing::get, Json, Router};
use serde_json::json;
use swch::launchers::epic::{EpicClient, EpicError};
use swch::parsers::epic_manifest::EpicManifestItem;

#[tokio::test]
async fn launch_version_check_accepts_only_matching_build_and_identity() {
    let manifest: EpicManifestItem = serde_json::from_value(json!({
        "AppName":"Fortnite", "CatalogItemId":"item", "CatalogNamespace":"fn",
        "DisplayName":"Fortnite", "InstallationGuid":"test", "InstallLocation":"test",
        "AppVersionString":"installed"
    })).unwrap();
    for (assets, expected) in [
        (json!([{"appName":"Fortnite","namespace":"fn","catalogItemId":"item","buildVersion":"installed"}]), "ready"),
        (json!([{"appName":"Fortnite","namespace":"fn","catalogItemId":"item","buildVersion":"new"}]), "update"),
        (json!([{"appName":"Fortnite","namespace":"other","catalogItemId":"item","buildVersion":"installed"}]), "unknown"),
        (json!([]), "unknown"),
        (json!({"unexpected":"response"}), "unknown"),
    ] {
        let client = EpicClient::from_router(Router::new().route(
            "/launcher/api/public/assets/:platform", get(move |headers: axum::http::HeaderMap| async move {
                assert_eq!(headers["authorization"], "Bearer test-token");
                Json(assets)
            })));
        let result = client.check_installed_version("test-token", &manifest).await;
        assert_eq!(match result { Ok(()) => "ready", Err(EpicError::UpdateRequired) => "update",
            Err(EpicError::UpdateCheckUnavailable) => "unknown", Err(e) => panic!("{e}") }, expected);
    }
    let client = EpicClient::from_router(Router::new());
    assert!(matches!(client.check_installed_version("test-token", &manifest).await,
        Err(EpicError::UpdateCheckUnavailable)));
    let mut missing = manifest;
    missing.app_version_string = None;
    assert!(matches!(client.check_installed_version("test-token", &missing).await,
        Err(EpicError::UpdateCheckUnavailable)));
}
