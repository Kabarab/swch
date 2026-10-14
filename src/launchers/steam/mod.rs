mod appinfo;
pub mod library;
pub mod client_launch;
pub mod downloads;
pub mod remote;
pub mod game_stats;
pub mod deadlock_stats;
pub mod session;

pub use session::{
    detect_active_steam_user, resolve_swch_sessions_dir, SteamError, SteamPaths, SteamSessionManager,
    SwitchAccountResult, STEAM_PROCESS_NAMES,
};
