use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use swch::launchers::epic::EpicAuthStore;
use swch::launchers::minecraft::MinecraftManager;
use swch::launchers::roblox::RobloxManager;
use swch::server::{create_app_with_state, AppState};
use swch::storage::Database;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize structured logging and tracing
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "swch=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting swch2 backend engine...");

    // Clean up temporary update files from previous update runs
    cleanup_update_artifacts();

    // Initialize SQLite database
    let db_path = Database::default_path()?;
    info!("Database target: {}", db_path.display());

    let db = Database::open(&db_path)?;
    db.run_migrations().await?;
    info!("Database initialized and migrations applied.");

    // Initialize Modular Engine and detect installed launchers
    let module_manager = Arc::new(swch::modules::ModuleManager::new(db.clone()));
    if let Err(e) = module_manager.init().await {
        warn!("Failed to initialize modules: {}", e);
    }

    // Start background auto-discovery loop (monitors OS for newly installed or removed launchers)
    module_manager
        .clone()
        .start_auto_discovery_loop(std::time::Duration::from_secs(5));

    let epic_store = module_manager
        .epic_store()
        .await
        .unwrap_or_else(|| Arc::new(EpicAuthStore::default_store(db.clone())));

    // Bind HTTP server to 127.0.0.1:4242
    let host = [127, 0, 0, 1];
    let port = std::env::var("SWCH_PORT").ok().map(|v| v.parse::<u16>()).transpose()?.unwrap_or(4242);
    let addr = SocketAddr::from((host, port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("==================================================");
    info!("  swch2 Modular Backend Server is ready!");
    info!("  Listening at: http://{}", addr);
    info!("  Database:     {}", db_path.display());
    info!("  Auto-detect:  Active background discovery loop");
    info!("  Press Ctrl+C to shut down gracefully.");
    info!("==================================================");

    let roblox_manager = Arc::new(RobloxManager::new(db.clone()));
    roblox_manager.clone().start_background_tasks();

    let minecraft_manager = Arc::new(MinecraftManager::new(db.clone()));
    minecraft_manager.clone().start_background_tasks();

    let update_manager = Arc::new(swch::server::updater::UpdateManager::new());

    let app = create_app_with_state(AppState {
        db,
        epic_store,
        module_manager,
        roblox_manager,
        minecraft_manager,
        update_manager,
    });

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("swch2 server has stopped gracefully. Goodbye!");
    Ok(())
}

/// Asynchronously listens for shutdown signals (Ctrl+C or SIGTERM on Unix).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(e) => {
                tracing::warn!("Failed to install SIGTERM signal handler: {}", e);
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received shutdown signal (Ctrl+C)");
        },
        _ = terminate => {
            info!("Received terminate signal (SIGTERM)");
        },
    }
}

/// Automatically removes leftover update backups or replacement binaries from previous runs.
fn cleanup_update_artifacts() {
    if let Ok(exe) = std::env::current_exe() {
        let old_exe = exe.with_extension("exe.old");
        if old_exe.exists() {
            let _ = std::fs::remove_file(old_exe);
        }
        let old_bin = exe.with_extension("old");
        if old_bin.exists() {
            let _ = std::fs::remove_file(old_bin);
        }
        let temp_bin = exe.with_extension("new_exe");
        if temp_bin.exists() {
            let _ = std::fs::remove_file(temp_bin);
        }
        if let Some(bundle) = swch::server::updater::find_enclosing_app_bundle(&exe) {
            let old_bundle = bundle.with_extension("app.old");
            if old_bundle.exists() {
                let _ = std::fs::remove_dir_all(old_bundle);
            }
        }
    }
}
