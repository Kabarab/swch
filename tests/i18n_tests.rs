use serde_json::json;
use std::collections::BTreeMap;
use swch::server::i18n::{startup_html, validate, Selection};
#[test]
fn language_choices_require_full_compatible_text_and_preserve_slots() {
    let source = json!({"revision":"one","messages":{"Hello {0}":"Hello {0}","Name":"Name"}});
    let mut choice = Selection {
        locale: "ar".into(),
        preference: None,
        revision: "one".into(),
        machine: true,
        messages: BTreeMap::from([
            ("Hello {0}".into(), "مرحبا {0}".into()),
            ("Name".into(), "اسم".into()),
        ]),
    };
    assert!(validate(&choice, &source));
    choice.preference = Some("system".into());
    assert!(validate(&choice, &source));
    choice.preference = Some("en".into());
    assert!(!validate(&choice, &source));
    choice.preference = None;
    choice.locale = "../ar".into();
    assert!(!validate(&choice, &source));
    choice.locale = "ar".into();
    choice.messages.insert("Hello {0}".into(), "مرحبا".into());
    assert!(!validate(&choice, &source));
    choice.messages.remove("Name");
    assert!(!validate(&choice, &source));
}
#[test]
fn native_templates_escape_translations_and_keep_retry_behavior() {
    let choice = Selection {
        locale: "ar".into(),
        preference: None,
        revision: "one".into(),
        machine: true,
        messages: BTreeMap::from([(
            "Не удалось запустить SWCH".into(),
            "<script>alert('bad')</script>".into(),
        )]),
    };
    let windows = startup_html(&choice, true, true);
    assert!(windows.contains("dir=\"rtl\""));
    assert!(windows.contains("&lt;script&gt;"));
    assert!(!windows.contains("<script>"));
    assert!(windows.contains("window.chrome.webview.postMessage('retry')"));
    let mac = startup_html(&choice, true, false);
    assert!(mac.contains("http://127.0.0.1:4242/"));
    let english = Selection {
        locale: "en".into(),
        preference: Some("system".into()),
        revision: "one".into(),
        machine: false,
        messages: BTreeMap::new(),
    };
    let fallback = startup_html(&english, true, true);
    assert!(fallback.contains("Unable to start SWCH"));
    assert!(fallback.contains("Try again"));
    assert!(!fallback.contains("Повторить"));
}
#[tokio::test]
async fn selection_is_persistent_atomic_shared_and_rejects_foreign_origins() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Router,
    };
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tower::ServiceExt;
    let folder = tempfile::tempdir().unwrap();
    std::env::set_var("SWCH_I18N_DIR", folder.path());
    let ui = folder.path().join("web");
    std::fs::create_dir_all(ui.join("ui/lang")).unwrap();
    std::fs::write(ui.join("index.html"), "<!doctype html>").unwrap();
    // Simulate UI assets from a newer build than the server executable.
    let mut source: serde_json::Value =
        serde_json::from_str(include_str!("../ui/lang/source.json")).unwrap();
    source["revision"] = json!("runtime-catalog");
    let source_path = ui.join("ui/lang/source.json");
    std::fs::write(&source_path, source.to_string()).unwrap();
    std::env::set_var("INDEX_HTML_PATH", ui.join("index.html"));
    let db = swch::storage::Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let state = swch::server::AppState::new(
        db.clone(),
        Arc::new(swch::launchers::epic::EpicAuthStore::default_store(db)),
    );
    let app = Router::new()
        .merge(swch::server::i18n::router())
        .with_state(state);
    std::env::remove_var("INDEX_HTML_PATH");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/i18n/selection")
                .header("host", "127.0.0.1:4242")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let initial: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(initial["locale"], "en");
    assert_eq!(initial["preference"], "system");
    assert_eq!(initial["saved"], false);
    assert!(initial["systemLanguages"]
        .as_array()
        .is_some_and(|languages| !languages.is_empty()));
    let body = json!({"locale":"ru","revision":source["revision"],"machine":false,"messages":source["messages"]});
    let make = |body: &serde_json::Value| {
        Request::builder()
            .method("POST")
            .uri("/api/i18n/selection")
            .header("host", "127.0.0.1:4242")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(make(&body)).await.unwrap().status(),
        StatusCode::OK
    );
    let pointer = std::fs::read_to_string(folder.path().join("active.txt")).unwrap();
    assert_eq!(pointer.len(), 64);
    let native = std::fs::read_to_string(
        folder
            .path()
            .join("versions")
            .join(&pointer)
            .join("loading-windows.html"),
    )
    .unwrap();
    assert!(native.contains("Запуск SWCH..."));
    let mut invalid = body.clone();
    invalid["revision"] = json!("old");
    assert_eq!(
        app.clone().oneshot(make(&invalid)).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        std::fs::read_to_string(folder.path().join("active.txt")).unwrap(),
        pointer
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/i18n/selection")
                .header("host", "127.0.0.1:4242")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let result: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(result["locale"], "ru");
    assert_eq!(result["preference"], "ru");
    assert_eq!(result["saved"], true);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/i18n/packs/ru")
                .header("host", "127.0.0.1:4242")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cached: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(cached["messages"], source["messages"]);
    // A subsequent UI update must be recognized without restarting the server.
    source["revision"] = json!("updated-runtime-catalog");
    std::fs::write(&source_path, source.to_string()).unwrap();
    assert_eq!(
        app.clone().oneshot(make(&body)).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let mut updated = body.clone();
    updated["revision"] = source["revision"].clone();
    assert_eq!(
        app.clone().oneshot(make(&updated)).await.unwrap().status(),
        StatusCode::OK
    );
    let latest = std::fs::read_to_string(folder.path().join("active.txt")).unwrap();
    // Missing or broken catalogs must not commit an unvalidated selection.
    std::fs::write(&source_path, "broken JSON").unwrap();
    assert_eq!(
        app.clone().oneshot(make(&updated)).await.unwrap().status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    std::fs::remove_file(&source_path).unwrap();
    assert_eq!(
        app.clone().oneshot(make(&updated)).await.unwrap().status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        std::fs::read_to_string(folder.path().join("active.txt")).unwrap(),
        latest
    );
    let mut foreign = make(&body);
    foreign
        .headers_mut()
        .insert("origin", "https://example.com".parse().unwrap());
    assert_eq!(
        app.oneshot(foreign).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    std::env::remove_var("SWCH_I18N_DIR");
}
