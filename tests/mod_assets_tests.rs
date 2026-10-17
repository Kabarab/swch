use axum::{body::{to_bytes, Body}, http::{Request, StatusCode}, Extension};
use serde_json::json;
use std::sync::Arc;
use swch::{mods::ModManager, storage::Database};
use tower::ServiceExt;

#[tokio::test]
async fn package_assets_stream_with_mime_ranges_and_opaque_origin_without_exposing_private_paths() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("package");
    std::fs::create_dir_all(package.join("web")).unwrap();
    std::fs::write(package.join("manifest.json"), json!({
        "id":"ui.assets", "name":"Assets", "version":"1.0.0", "api":"^1.0.0",
        "platforms":["windows-x64","macos-arm64"], "ui":"web/index.html"
    }).to_string()).unwrap();
    for (name, bytes) in [
        ("index.html", b"<script type=module src=app.mjs></script>".as_slice()),
        ("app.mjs", b"let loaded = true;".as_slice()),
        ("style.css", b"body { color: red; }".as_slice()),
        ("module.wasm", b"\0asm\x01\0\0\0".as_slice()),
        ("font.woff2", b"font data".as_slice()),
    ] {
        std::fs::write(package.join("web").join(name), bytes).unwrap();
    }
    std::fs::write(package.join(".env"), "private package data").unwrap();
    std::fs::write(directory.path().join("outside.txt"), "outside package").unwrap();
    let database = Database::open_in_memory().unwrap();
    database.run_migrations().await.unwrap();
    let manager = Arc::new(ModManager::with_dir(database.clone(), directory.path().join("mods"), None));
    manager.development(package.clone()).await.unwrap();
    let epic = Arc::new(swch::launchers::epic::EpicAuthStore::default_store(database.clone()));
    let state = swch::server::AppState::new(database, epic);
    let app = swch::server::mods::router().layer(Extension(manager.clone())).with_state(state);
    let request = |path: &str, origin: &str| Request::builder()
        .uri(format!("/api/mods/ui.assets/assets/{path}"))
        .header("host", "127.0.0.1:4242").header("origin", origin)
        .header("sec-fetch-site", "cross-site").body(Body::empty()).unwrap();
    for (path, mime) in [
        ("web/app.mjs", "javascript"), ("web/style.css", "text/css"),
        ("web/module.wasm", "application/wasm"), ("web/font.woff2", "font/woff2"),
    ] {
        let response = app.clone().oneshot(request(path, "null")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.headers()["content-type"].to_str().unwrap().contains(mime));
        assert_eq!(response.headers()["access-control-allow-origin"], "null");
        assert!(!response.headers().contains_key("access-control-allow-credentials"));
        assert!(!to_bytes(response.into_body(), usize::MAX).await.unwrap().is_empty());
    }
    let mut partial = request("web/app.mjs", "null");
    partial.headers_mut().insert("range", "bytes=4-9".parse().unwrap());
    let response = app.clone().oneshot(partial).await.unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(&to_bytes(response.into_body(), usize::MAX).await.unwrap()[..], b"loaded");
    let mut head = request("web/app.mjs", "null");
    *head.method_mut() = axum::http::Method::HEAD;
    let response = app.clone().oneshot(head).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-length"], "18");
    assert!(to_bytes(response.into_body(), usize::MAX).await.unwrap().is_empty());
    for path in [".env", "web/%2e%2e/%2e%2e/outside.txt", "missing.css"] {
        assert_eq!(app.clone().oneshot(request(path, "null")).await.unwrap().status(), StatusCode::NOT_FOUND, "{path}");
    }
    assert_eq!(app.clone().oneshot(request("web/app.mjs", "https://example.com")).await.unwrap().status(), StatusCode::FORBIDDEN);
    let sdk_request = Request::builder().method("POST").uri("/api/mods/ui.assets/sdk")
        .header("host", "127.0.0.1:4242").header("origin", "null")
        .header("content-type", "application/json").body(Body::from("{}" )).unwrap();
    assert_eq!(app.clone().oneshot(sdk_request).await.unwrap().status(), StatusCode::FORBIDDEN);
    assert!(manager.asset("ui.assets", "../outside.txt").await.is_err());
    manager.toggle("ui.assets", false).await.unwrap();
    assert_eq!(app.oneshot(request("web/app.mjs", "null")).await.unwrap().status(), StatusCode::NOT_FOUND);
    manager.shutdown().await;
}
