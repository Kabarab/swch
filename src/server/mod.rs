pub mod handlers;
pub mod auto_claim;
pub mod epic_browser;
pub mod modules;
pub mod riot;
pub mod roblox;
pub mod steam;
pub mod currency;
pub mod minecraft;
pub mod updater;
pub mod ubisoft;
pub mod gog;
pub mod hoyoverse;
pub mod playtime;
pub mod mods;
pub mod i18n;

use axum::{
    routing::{delete, get, post},
    Router,
};
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::launchers::epic::EpicAuthStore;
use crate::launchers::minecraft::MinecraftManager;
use crate::launchers::roblox::RobloxManager;
use crate::modules::ModuleManager;
use crate::storage::Database;
use crate::server::updater::UpdateManager;

/// Combined application state supporting FromRef sub-state extraction.
#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub epic_store: Arc<EpicAuthStore>,
    pub module_manager: Arc<ModuleManager>,
    pub roblox_manager: Arc<RobloxManager>,
    pub minecraft_manager: Arc<MinecraftManager>,
    pub update_manager: Arc<UpdateManager>,
}

impl AppState {
    pub fn new(db: Database, epic_store: Arc<EpicAuthStore>) -> Self {
        let module_manager = Arc::new(ModuleManager::new_with_epic(db.clone(), Some(epic_store.clone())));
        let roblox_manager = Arc::new(RobloxManager::new(db.clone()));
        let minecraft_manager = Arc::new(MinecraftManager::new(db.clone()));
        let update_manager = Arc::new(UpdateManager::new());
        Self {
            db,
            epic_store,
            module_manager,
            roblox_manager,
            minecraft_manager,
            update_manager,
        }
    }

    pub fn new_with_modules(db: Database, epic_store: Arc<EpicAuthStore>, module_manager: Arc<ModuleManager>) -> Self {
        let roblox_manager = Arc::new(RobloxManager::new(db.clone()));
        let minecraft_manager = Arc::new(MinecraftManager::new(db.clone()));
        let update_manager = Arc::new(UpdateManager::new());
        Self {
            db,
            epic_store,
            module_manager,
            roblox_manager,
            minecraft_manager,
            update_manager,
        }
    }
}

impl axum::extract::FromRef<AppState> for Database {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<EpicAuthStore> {
    fn from_ref(state: &AppState) -> Self {
        state.epic_store.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<ModuleManager> {
    fn from_ref(state: &AppState) -> Self {
        state.module_manager.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<RobloxManager> {
    fn from_ref(state: &AppState) -> Self {
        state.roblox_manager.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<MinecraftManager> {
    fn from_ref(state: &AppState) -> Self {
        state.minecraft_manager.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<UpdateManager> {
    fn from_ref(state: &AppState) -> Self {
        state.update_manager.clone()
    }
}

/// Resolves the path to index.html, checking custom environment variable, current directory, or binary directory.
pub fn resolve_index_path() -> PathBuf {
    if let Ok(custom_path) = std::env::var("INDEX_HTML_PATH") {
        let path = PathBuf::from(custom_path);
        if path.exists() {
            return path;
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // Check macOS bundle Resources/web
            let bundle_web = exe_dir.join("../Resources/web/index.html");
            if bundle_web.exists() {
                return bundle_web;
            }
            // Check macOS bundle Resources
            let bundle_res = exe_dir.join("../Resources/index.html");
            if bundle_res.exists() {
                return bundle_res;
            }
            // Check adjacent to binary
            let exe_local = exe_dir.join("index.html");
            if exe_local.exists() {
                return exe_local;
            }

            // In cargo dev builds (target/debug/ or target/release/)
            let workspace_root = exe_dir.join("../../index.html");
            if workspace_root.exists() {
                return workspace_root;
            }
        }
    }

    let local = PathBuf::from("index.html");
    if local.exists() {
        return local;
    }

    // Canonical frontend path in swch2 repo
    let canonical_swch2 = PathBuf::from("/Users/nazarzukov/swch2/index.html");
    if canonical_swch2.exists() {
        return canonical_swch2;
    }

    // Canonical frontend path in Documents repo
    let canonical = PathBuf::from("/Users/nazarzukov/Documents/GitHub/swch/star_wars_chronology/swch2/index.html");
    if canonical.exists() {
        return canonical;
    }

    PathBuf::from("index.html")
}

/// Constructs the Axum application router with all REST endpoints, CORS, static file serving, and DB state.
pub fn create_app(db: Database) -> Router {
    let epic_store = Arc::new(EpicAuthStore::default_store(db.clone()));
    let module_manager = Arc::new(ModuleManager::new_with_epic(db.clone(), Some(epic_store.clone())));
    let roblox_manager = Arc::new(RobloxManager::new(db.clone()));
    roblox_manager.clone().start_background_tasks();
    let minecraft_manager = Arc::new(MinecraftManager::new(db.clone()));
    minecraft_manager.clone().start_background_tasks();
    let update_manager = Arc::new(UpdateManager::new());
    create_app_with_state(AppState {
        db,
        epic_store,
        module_manager,
        roblox_manager,
        minecraft_manager,
        update_manager,
    })
}

/// Constructs the router with custom AppState (useful for tests with mock client or in-memory keyring).
pub fn create_app_with_state(state: AppState) -> Router {
    let claimer = crate::auto_claim::AutoClaimer::new(state.db.clone(), state.epic_store.clone());
    create_app_with_auto_claim(state, claimer)
}

pub fn create_app_with_auto_claim(state: AppState, claimer: Arc<crate::auto_claim::AutoClaimer>) -> Router {
    let manager=Arc::new(crate::mods::ModManager::new(state.db.clone()));
    create_app_with_auto_claim_and_mods(state,claimer,manager)
}

pub fn create_app_with_auto_claim_and_mods(state: AppState, claimer: Arc<crate::auto_claim::AutoClaimer>, manager: Arc<crate::mods::ModManager>) -> Router {
    let index_path = resolve_index_path();
    let static_dir = index_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let cors = CorsLayer::permissive();

    Router::new()
        .merge(mods::router())
        .merge(i18n::router())
        .merge(auto_claim::router(claimer.clone(), state.epic_store.clone()).with_state::<AppState>(()))
        // Core & System routes
        .route("/api/health", get(handlers::health))
        .route("/api/playtime", get(playtime::get))
        .route("/api/gog/auth/url", get(gog::auth_url))
        .route("/api/gog/auth/open", post(gog::open_auth))
        .route("/api/gog/auth/login", post(gog::login))
        .route("/api/gog/accounts", get(gog::accounts))
        .route("/api/gog/accounts/remove", post(gog::remove))
        .route("/api/gog/switch-account", post(gog::switch_account))
        .route("/api/gog/library", get(gog::library))
        .route("/api/gog/library/sync", post(gog::sync))
        .route("/api/gog/open-launcher", post(gog::open_launcher))
        .route("/api/gog/launch", post(gog::launch))
        .route("/api/ubisoft/library", get(ubisoft::library))
        .route("/api/ubisoft/library/sync", post(ubisoft::library))
        .route("/api/ubisoft/accounts", get(ubisoft::accounts))
        .route("/api/ubisoft/current-account", get(ubisoft::current_account))
        .route("/api/ubisoft/sessions/save", post(ubisoft::save_session))
        .route("/api/ubisoft/accounts/prepare", post(ubisoft::prepare_account))
        .route("/api/ubisoft/auth/status", get(ubisoft::automatic_status))
        .route("/api/ubisoft/auth/cancel", post(ubisoft::automatic_cancel))
        .route("/api/ubisoft/switch-account", post(ubisoft::switch_account))
        .route("/api/ubisoft/open-launcher", post(ubisoft::open_launcher))
        .route("/api/ubisoft/launch", post(ubisoft::launch))
        .route("/api/ubisoft/accounts/remove", post(ubisoft::remove_account))
        // HoYoverse routes
        .route("/api/hoyoverse/games", get(hoyoverse::library))
        .route("/api/hoyoverse/library", get(hoyoverse::library))
        .route("/api/hoyoverse/accounts", get(hoyoverse::accounts))
        .route("/api/hoyoverse/current-account", get(hoyoverse::current_account))
        .route("/api/hoyoverse/sessions/save", post(hoyoverse::save_session))
        .route("/api/hoyoverse/switch-account", post(hoyoverse::switch_account))
        .route("/api/hoyoverse/accounts/prepare", post(hoyoverse::prepare_account))
        .route("/api/hoyoverse/accounts/remove", post(hoyoverse::remove_account))
        .route("/api/hoyoverse/launch", post(hoyoverse::launch_game))
        .route("/api/hoyoverse/open-launcher", post(hoyoverse::open_launcher))
        .route("/api/system/shutdown", post(handlers::shutdown))
        .route("/api/modules", get(modules::list_modules))
        .route("/api/modules/scan", post(modules::scan_modules))
        .route("/api/modules/:id/install", post(modules::install_module))
        .route("/api/modules/:id/toggle", post(modules::toggle_module))
        .route("/api/modules/:id/purge", post(modules::purge_module))
        .route("/api/games", get(handlers::get_games))
        .route("/api/accounts", get(handlers::get_accounts))
        .route(
            "/api/accounts/restrictions",
            get(handlers::get_restrictions).post(handlers::save_restrictions),
        )
        .route(
            "/api/steam/check-restrictions",
            post(handlers::check_steam_restrictions).get(handlers::check_steam_restrictions),
        )
        .route(
            "/api/steam/switch-account",
            post(handlers::switch_steam_account),
        )
        .route(
            "/api/steam/open-launcher",
            post(handlers::switch_steam_account),
        )
        .route("/api/steam/launch", post(steam::launch))
        .route("/api/steam/downloads", get(steam::downloads))
        .route("/api/steam/inventory-contexts", get(steam::inventory_contexts))
        .route("/api/steam/inventory-cache", get(steam::inventory_cache))
        .route("/api/steam/library", get(steam::get_library))
        .route("/api/steam/library/sync", post(steam::sync_library))
        .route("/api/steam/inventory", get(steam::inventory))
        .route("/api/steam/avatar", get(steam::avatar))
        .route("/api/steam/price", get(steam::price))
        .route("/api/steam/game-details", get(steam::game_details))
        .route("/api/steam/artwork", get(steam::artwork))
        .route("/api/steam/game-stats/automatic-key", post(steam::automatic_key_stats))
        .route("/api/steam/open-api-key", post(steam::open_api_key))
        .route("/api/steam/game-stats", get(steam::game_stats).post(steam::game_stats_with_key))
        .route("/api/steam/game-price", get(steam::game_price))
        .route("/api/steam/game-prices", get(steam::game_prices))
        .route("/api/currencies", get(currency::exchange_rates))
        // Riot Client: local catalog and explicit capability boundaries.
        .route("/api/riot/library", get(riot::library))
        .route("/api/riot/accounts", get(riot::profiles))
        .route("/api/riot/accounts/sync", post(riot::sync_profile))
        .route("/api/riot/library/sync", post(riot::library))
        .route("/api/riot/launch", post(riot::launch))
        .route("/api/riot/open-launcher", post(riot::open_launcher))
        .route("/api/riot/switch-account", post(riot::switch_account))
        .route("/api/riot/accounts/save", post(riot::save_session))
        .route("/api/riot/accounts/remove", post(riot::remove_account))
        .route("/api/riot/current-session", get(riot::current_session))
        .route("/api/riot/sessions", get(riot::list_sessions))
        // Epic Games routes
        .route("/epic/connect", get(epic_browser::bridge))
        .route("/api/epic/auth/browser/start", post(epic_browser::start))
        .route("/api/epic/auth/browser/open", post(epic_browser::open_browser))
        .route("/api/epic/auth/browser/prepare", post(epic_browser::prepare))
        .route("/api/epic/auth/browser/status", post(epic_browser::status))
        .route("/api/epic/auth/browser/complete", post(epic_browser::complete))
        .route("/api/epic/auth/browser/cancel", post(epic_browser::cancel))
        .route("/api/epic/auth/url", get(handlers::epic_auth_url))
        .route(
            "/api/epic/auth/open",
            post(handlers::open_epic_auth_browser).get(handlers::open_epic_auth_browser),
        )
        .route("/api/epic/auth/login", post(handlers::epic_login))
        .route("/api/epic/accounts", get(handlers::get_epic_accounts))
        .route(
            "/api/epic/switch",
            post(handlers::switch_epic_account),
        )
        .route(
            "/api/epic/switch-account",
            post(handlers::switch_epic_account),
        )
        .route(
            "/api/epic/open-launcher",
            post(handlers::open_epic_launcher),
        )
        .route(
            "/api/epic/remove-account",
            post(handlers::remove_epic_account),
        )
        .route("/api/epic/library", get(handlers::get_epic_library))
        .route("/api/epic/library/sync", post(handlers::sync_epic_library))
        .route("/api/epic/launch", post(handlers::launch_epic_game))
        .route("/api/epic/launch-game", post(handlers::launch_epic_game))
        .route("/api/epic/game-price", get(handlers::epic_game_price))
        .route("/api/epic/game-prices", get(handlers::epic_game_prices))
        .route("/api/epic/account-details", get(handlers::get_epic_account_details))
        .route("/api/epic/fortnite/summary", get(handlers::get_epic_fortnite_summary))
        .route("/api/epic/fortnite/skins", get(handlers::get_epic_fortnite_skins))
        .route("/api/epic/fortnite/refresh", post(handlers::refresh_epic_fortnite))
        // Roblox routes
        .route("/api/roblox/library", get(roblox::library))
        .route("/api/roblox/accounts", get(roblox::list_accounts))
        .route("/api/roblox/accounts/scan", post(roblox::scan_accounts))
        .route("/api/roblox/accounts/add", post(roblox::add_account))
        .route("/api/roblox/accounts/remove", post(roblox::remove_account))
        .route("/api/roblox/switch-account", post(roblox::switch_account))
        .route("/api/roblox/launch", post(roblox::launch))
        .route("/api/roblox/places", get(roblox::list_places))
        .route("/api/roblox/places/add", post(roblox::add_place))
        .route("/api/roblox/places/:id", delete(roblox::delete_place))
        .route("/api/roblox/inventory", get(roblox::get_inventory))
        .route("/api/roblox/inventory/refresh", post(roblox::refresh_inventory))
        .route("/api/roblox/settings", get(roblox::get_settings).post(roblox::update_settings))
        // Minecraft routes
        .route("/api/minecraft/library", get(minecraft::library))
        .route("/api/minecraft/accounts", get(minecraft::list_accounts))
        .route("/api/minecraft/accounts/scan", post(minecraft::scan_accounts))
        .route("/api/minecraft/accounts/add", post(minecraft::add_account))
        .route("/api/minecraft/accounts/remove", post(minecraft::remove_account))
        .route("/api/minecraft/switch-account", post(minecraft::switch_account))
        .route("/api/minecraft/accounts/switch", post(minecraft::switch_account))
        .route("/api/minecraft/profiles", get(minecraft::list_profiles))
        .route("/api/minecraft/worlds", get(minecraft::list_worlds))
        .route("/api/minecraft/servers", get(minecraft::list_servers))
        .route("/api/minecraft/servers/add", post(minecraft::add_server))
        .route("/api/minecraft/servers/import", post(minecraft::import_servers))
        .route("/api/minecraft/servers/:id", delete(minecraft::delete_server))
        .route("/api/minecraft/launch", post(minecraft::launch))
        .route("/api/minecraft/open-launcher", post(minecraft::open_launcher))
        // In-app self updater routes
        .route("/api/update/check", get(updater::check_update))
        .route("/api/update/apply", post(updater::apply_update))
        .route("/api/update/status", get(updater::update_status))
        // Static UI
        .route_service("/", ServeFile::new(&index_path))
        .fallback_service(ServeDir::new(static_dir).fallback(ServeFile::new(&index_path)))
        .layer(cors)
        .layer(axum::Extension(crate::launchers::gog::auth::GogStore::shared()))
        .layer(axum::Extension(claimer))
        .layer(axum::middleware::from_fn(mods::notify_changes))
        .layer(axum::Extension(manager))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
