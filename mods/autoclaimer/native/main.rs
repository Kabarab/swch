use axum::{extract::{Request, State}, http::StatusCode, middleware::{self, Next}, response::Response, routing::get, Router};
use std::{future::IntoFuture, io::Write, sync::Arc};
use swch::{auto_claim::AutoClaimer, launchers::epic::EpicAuthStore, storage::Database};
use tokio::io::AsyncReadExt;

async fn authorize(State(token): State<Arc<String>>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    // Steam completion has its own one-use, account-scoped, ten-minute capability.
    let bridge = matches!(path, "/steam/claim-connect" | "/ui/steam-claim-bridge.js"
        | "/ui/launcher.css" | "/api/free-games/steam/extension" | "/api/free-games/steam/complete");
    let expected = format!("Bearer {token}");
    if !bridge && request.headers().get("authorization").and_then(|h| h.to_str().ok()) != Some(expected.as_str()) {
        return axum::response::IntoResponse::into_response((StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"error":"Unauthorized"}))));
    }
    next.run(request).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_env_filter("swch=info").init();
    let token = std::env::var("SWCH_AUTO_CLAIM_TOKEN")?;
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Missing mod authentication token".into());
    }
    let db = Database::open(Database::default_path()?)?;
    db.run_migrations().await?;
    let epic = Arc::new(EpicAuthStore::default_store(db.clone()));
    let service = AutoClaimer::new(db, epic.clone());
    let app = swch::server::auto_claim::router(service.clone(), epic)
        .merge(Router::new()
            .route("/ui/steam-claim-bridge.js", get(|| async { ([("content-type", "text/javascript; charset=utf-8")], include_str!("../../../ui/steam-claim-bridge.js")) }))
            .route("/ui/launcher.css", get(|| async { ([("content-type", "text/css; charset=utf-8")], include_str!("../../../ui/launcher.css")) })))
        .layer(middleware::from_fn_with_state(Arc::new(token), authorize))
        .layer(tower_http::cors::CorsLayer::permissive());
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    println!("{}", serde_json::json!({"port":listener.local_addr()?.port(),"pid":std::process::id()}));
    std::io::stdout().flush()?;
    let scheduler = if std::env::var("SWCH_AUTO_CLAIM_NO_SCHEDULER").as_deref() == Ok("1") { None } else { Some(service.start()) };
    tokio::select! {
      result = axum::serve(listener, app).into_future() => { result?; }
      _ = async {
        let mut input = tokio::io::stdin();
        let mut buffer = [0; 64];
        while matches!(input.read(&mut buffer).await, Ok(n) if n > 0) {}
      } => {}
    }
    // Closing the mod's keepalive pipe stops scheduling and in-progress claiming.
    if let Some(scheduler) = scheduler { scheduler.abort(); }
    service.cancel().await;
    // Closing Tokio cancels outstanding work; no detached scheduler remains.
    Ok(())
}
