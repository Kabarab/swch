pub(super) use crate::sys::process::LAUNCHER_SWITCH_LOCK as SWITCH_API;
static AUTO_RESTRICTIONS_CHECK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const AUTO_RESTRICTIONS_INTERVAL_SECS: i64 = 15 * 60;
use std::collections::HashSet;
use std::sync::Arc;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};

use crate::launchers::epic::{
    EpicAccount, EpicAccountDetails, EpicAuthStore, EpicClient, EpicError, EpicSessionManager,
    FortniteAccountSummary, FortniteCosmeticItem, FortniteCosmeticType,
};
use crate::launchers::steam::{SteamError, SteamSessionManager};
use crate::storage::models::AccountRestrictions;
use crate::storage::{Account, Database, Game};
use crate::modules::ModuleManager;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub database: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Deserialize)]
pub struct SwitchAccountRequest {
    pub account_name: Option<String>,
    pub account_id: Option<String>,
    #[serde(default)]
    pub open_url: Option<String>,
}

pub fn sanitize_steam_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    const BAN_SUPPORT_URL: &str = "https://help.steampowered.com/en/wizard/VacBans";
    if trimmed == BAN_SUPPORT_URL {
        return Some(format!("steam://openurl/{trimmed}"));
    }
    if trimmed == format!("steam://openurl/{BAN_SUPPORT_URL}") {
        return Some(trimmed.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("steam://openurl/") {
        if rest.starts_with("https://steamcommunity.com/")
            || rest.starts_with("https://store.steampowered.com/")
        {
            return Some(trimmed.to_string());
        }
    } else if trimmed.starts_with("https://steamcommunity.com/")
        || trimmed.starts_with("https://store.steampowered.com/")
    {
        return Some(format!("steam://openurl/{}", trimmed));
    } else if trimmed == "steam://open/main" || trimmed == "steam://open/inventory" {
        return Some(trimmed.to_string());
    }
    None
}

impl SwitchAccountRequest {
    pub fn target_account(&self) -> Option<&str> {
        self.account_name
            .as_deref()
            .or(self.account_id.as_deref())
    }
}

#[derive(Debug, Serialize)]
pub struct SwitchAccountResponse {
    pub status: &'static str,
    pub account_name: String,
    pub previous_account: Option<String>,
    pub steam_id: Option<String>,
    pub message: String,
}

/// Handler for GET /api/health
pub async fn health(
    State(db): State<Database>,
) -> Result<Json<HealthResponse>, (StatusCode, Json<ErrorResponse>)> {
    match db.ping().await {
        Ok(_) => Ok(Json(HealthResponse {
            status: "ok",
            version: env!("CARGO_PKG_VERSION"),
            database: "connected",
        })),
        Err(e) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Database health check failed: {}", e),
            }),
        )),
    }
}

static SHUTDOWN_NOTIFY: std::sync::OnceLock<Arc<tokio::sync::Notify>> = std::sync::OnceLock::new();

/// Returns a shared Notify instance for graceful server shutdown.
pub fn get_shutdown_notify() -> Arc<tokio::sync::Notify> {
    SHUTDOWN_NOTIFY.get_or_init(|| Arc::new(tokio::sync::Notify::new())).clone()
}

/// Handler for POST /api/system/shutdown
pub async fn shutdown() -> &'static str {
    tracing::info!("Received graceful shutdown request via /api/system/shutdown");
    get_shutdown_notify().notify_waiters();
    "shutdown initiated"
}

/// Handler for GET /api/games
pub async fn get_games(
    State(db): State<Database>,
    State(module_manager): State<Arc<ModuleManager>>,
    axum::Extension(mods): axum::Extension<Arc<crate::mods::ModManager>>,
) -> Result<Json<Vec<Game>>, (StatusCode, Json<ErrorResponse>)> {
    match db.get_games().await {
        Ok(mut games) => {
            let mod_platforms = mods.platforms().await;
            games.retain(|g| !g.launcher.starts_with("mod:") || mod_platforms.iter().any(|p|p.id==g.launcher));
            let active = module_manager.get_active_launcher_ids().await;
            let filtered = if active.is_empty() {
                games
            } else {
                games
                    .into_iter()
                    .filter(|g| active.iter().any(|l| l.eq_ignore_ascii_case(&g.launcher)) || mod_platforms.iter().any(|p|p.id==g.launcher))
                    .collect()
            };
            Ok(Json(filtered))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to retrieve games: {}", e),
            }),
        )),
    }
}

/// Handler for GET /api/accounts
pub async fn get_accounts(
    State(db): State<Database>,
    State(module_manager): State<Arc<ModuleManager>>,
    axum::Extension(mods): axum::Extension<Arc<crate::mods::ModManager>>,
) -> Result<Json<Vec<Account>>, (StatusCode, Json<ErrorResponse>)> {
    match db.get_accounts().await {
        Ok(mut accounts) => {
            let mod_platforms=mods.platforms().await;
            accounts.retain(|a|!a.launcher.starts_with("mod:") || mod_platforms.iter().any(|p|p.id==a.launcher));
            // Also merge Riot sessions from disk so saved Riot accounts are always available
            let riot_mgr = crate::launchers::riot::session::RiotSessionManager::new();
            let riot_current = riot_mgr.read_current_account();
            for s in riot_mgr.list_sessions() {
                let id = format!("riot_{}", s.account_id);
                if !accounts.iter().any(|a| a.id.eq_ignore_ascii_case(&id) || a.username.eq_ignore_ascii_case(&s.account_id)) {
                    let uname = s.username
                        .or_else(|| s.display_name.as_ref().map(|d| d.split('#').next().unwrap_or(d).to_string()))
                        .unwrap_or_else(|| s.account_id.clone());
                    let dname = s.display_name.clone().or_else(|| Some(uname.clone()));
                    let is_active = riot_current.account_id.as_deref() == Some(&s.account_id);
                    accounts.push(crate::storage::Account {
                        id,
                        launcher: "riot".to_string(),
                        username: uname,
                        display_name: dname,
                        email: None,
                        is_active,
                        last_used: Some(s.backed_up_at),
                    });
                }
            }

            // Also merge HoYoverse accounts from disk so saved accounts are always available
            let hoyo_mgr = crate::launchers::hoyoverse::session::HoyoverseSessionManager::default();
            if let Ok(hoyo_accounts) = hoyo_mgr.accounts(None) {
                for h in hoyo_accounts {
                    if !accounts.iter().any(|a| a.id.eq_ignore_ascii_case(&h.id)) {
                        accounts.push(h);
                    }
                }
            }

            let active = module_manager.get_active_launcher_ids().await;
            let filtered = if active.is_empty() {
                accounts
            } else {
                accounts
                    .into_iter()
                    .filter(|a| {
                        active.iter().any(|l| l.eq_ignore_ascii_case(&a.launcher))
                            || a.launcher.eq_ignore_ascii_case("riot")
                            || a.launcher.eq_ignore_ascii_case("hoyoverse")
                            || mod_platforms.iter().any(|p|p.id==a.launcher)
                    })
                    .collect()
            };
            Ok(Json(filtered))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to retrieve accounts: {}", e),
            }),
        )),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RestrictionsListResponse {
    pub restrictions: Vec<AccountRestrictions>,
}

#[derive(Debug, Deserialize)]
pub struct CheckRestrictionsRequest {
    pub account_id: Option<String>,
    #[serde(default)]
    pub account_ids: Vec<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckRestrictionsResponse {
    pub status: &'static str,
    pub restrictions: Vec<AccountRestrictions>,
    #[serde(default)]
    pub checked_account_ids: Vec<String>,
}

/// Handler for GET /api/accounts/restrictions
pub async fn get_restrictions(
    State(db): State<Database>,
) -> Result<Json<RestrictionsListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let restrictions = db.get_all_account_restrictions().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to retrieve account restrictions: {}", e),
            }),
        )
    })?;

    // Online checks use the canonical SteamIDs supplied by the library after boot.
    // Stored launcher accounts can still have legacy username-based IDs.
    Ok(Json(RestrictionsListResponse { restrictions }))
}

/// Handler for POST /api/accounts/restrictions
pub async fn save_restrictions(
    State(db): State<Database>,
    Json(mut payload): Json<AccountRestrictions>,
) -> Result<Json<AccountRestrictions>, (StatusCode, Json<ErrorResponse>)> {
    if payload.account_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "account_id must not be empty".to_string(),
            }),
        ));
    }
    payload.updated_at = chrono::Utc::now().to_rfc3339();
    if let Err(e) = db.upsert_account_restrictions(&payload).await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to save account restrictions: {}", e),
            }),
        ));
    }
    Ok(Json(db.get_account_restrictions(&payload.account_id).await.ok().flatten().unwrap_or(payload)))
}

pub(super) async fn check_and_store_steam_restrictions(
    db: &Database, steam_id: &str, key: Option<&str>,
) -> (AccountRestrictions, bool) {
    let existing = db.get_account_restrictions(steam_id).await.ok().flatten();
    let (mut fresh, success) = query_steam_restrictions(steam_id, None, key).await;
    if !success {
        return (existing.unwrap_or(fresh), false);
    }
    // The ban API supplies AppIDs but not game titles. Resolve exact AppIDs
    // against the local Steam library before preserving the evidence.
    if fresh.game_bans.iter().any(|ban| ban.name.starts_with("Steam App #")) {
        if let Ok(games) = db.get_games().await {
            for ban in &mut fresh.game_bans {
                if !ban.name.starts_with("Steam App #") { continue; }
                if let Some(game) = games.iter().find(|game|
                    game.launcher.eq_ignore_ascii_case("steam") && game.app_id == ban.app_id
                        && !game.name.trim().is_empty()
                ) {
                    ban.name = game.name.clone();
                }
            }
        }
    }
    let mut verified = db.get_verified_account_bans(steam_id).await.unwrap_or_default();
    verified.absorb(&fresh);
    let mut updated = existing.unwrap_or_else(|| AccountRestrictions {
        account_id: steam_id.to_string(), ..Default::default()
    });
    if fresh.country.is_some() { updated.country = fresh.country; }
    if fresh.currency.is_some() { updated.currency = fresh.currency; }
    if updated.details.as_deref().unwrap_or("").is_empty() { updated.details = fresh.details; }
    updated.updated_at = chrono::Utc::now().to_rfc3339();
    verified.apply_to(&mut updated);
    if db.upsert_verified_account_bans(steam_id, &verified).await.is_err()
        || db.upsert_account_restrictions(&updated).await.is_err() {
        return (updated, false);
    }
    (updated, true)
}

/// A page reload can request restrictions twice (GET followed by the boot POST).
/// Reserve a per-account slot before contacting Steam so concurrent automatic
/// requests and failed checks do not repeatedly hit Steam Community.
async fn auto_check_steam_restrictions(
    db: &Database,
    steam_id: &str,
) -> (AccountRestrictions, bool) {
    let cache_key = format!("restrictions:auto-check:v1:{steam_id}");
    {
        let _guard = AUTO_RESTRICTIONS_CHECK.lock().await;
        let now = chrono::Utc::now().timestamp();
        if let Ok(Some((_, checked_at))) = db.steam_cache_get(&cache_key).await {
            if checked_at > now - AUTO_RESTRICTIONS_INTERVAL_SECS {
                let existing = db.get_account_restrictions(steam_id).await.ok().flatten();
                return (existing.unwrap_or_else(|| AccountRestrictions {
                    account_id: steam_id.to_string(),
                    launcher: "steam".to_string(),
                    ..Default::default()
                }), false);
            }
        }
        let _ = db.steam_cache_put(&cache_key, "attempted").await;
    }
    check_and_store_steam_restrictions(db, steam_id, None).await
}

/// Maps a Steam Community profile location string to country code (cc) and Steam store currency.
pub fn location_to_country_and_currency(loc: &str) -> (String, String) {
    let lower = loc.to_lowercase();
    if lower.contains("brazil") || lower.contains("brasil") {
        ("BR".into(), "BRL".into())
    } else if lower.contains("russia") || lower.contains("russian federation") || lower.contains("россия") {
        ("RU".into(), "RUB".into())
    } else if lower.contains("kazakhstan") || lower.contains("казахстан") {
        ("KZ".into(), "KZT".into())
    } else if lower.contains("ukraine") || lower.contains("украина") {
        ("UA".into(), "UAH".into())
    } else if lower.contains("algeria") {
        ("DZ".into(), "USD".into())
    } else if lower.contains("turkey") || lower.contains("türkiye") {
        ("TR".into(), "USD".into())
    } else if lower.contains("argentina") {
        ("AR".into(), "USD".into())
    } else if lower.contains("united states") || lower.contains("usa") {
        ("US".into(), "USD".into())
    } else if lower.contains("united kingdom") || lower.contains("great britain") || lower.contains("england") || lower.contains("scotland") {
        ("GB".into(), "GBP".into())
    } else if lower.contains("poland") || lower.contains("polska") {
        ("PL".into(), "PLN".into())
    } else if lower.contains("germany") || lower.contains("deutschland") || lower.contains("france") || lower.contains("spain") || lower.contains("italy")
        || lower.contains("netherlands") || lower.contains("austria") || lower.contains("portugal") || lower.contains("finland")
        || lower.contains("ireland") || lower.contains("belgium") || lower.contains("greece") {
        ("DE".into(), "EUR".into())
    } else if lower.contains("china") {
        ("CN".into(), "CNY".into())
    } else if lower.contains("japan") {
        ("JP".into(), "JPY".into())
    } else if lower.contains("india") {
        ("IN".into(), "INR".into())
    } else if lower.contains("canada") {
        ("CA".into(), "CAD".into())
    } else if lower.contains("australia") {
        ("AU".into(), "AUD".into())
    } else if lower.contains("norway") {
        ("NO".into(), "NOK".into())
    } else if lower.contains("switzerland") {
        ("CH".into(), "CHF".into())
    } else if lower.contains("belarus") || lower.contains("armenia") || lower.contains("georgia") || lower.contains("azerbaijan")
        || lower.contains("uzbekistan") || lower.contains("kyrgyzstan") || lower.contains("tajikistan") || lower.contains("moldova") {
        ("CIS".into(), "USD".into())
    } else {
        ("US".into(), "USD".into())
    }
}

fn apply_player_bans(r: &mut AccountRestrictions, id: &str, p: &serde_json::Value) -> bool {
    if p["SteamId"].as_str() != Some(id) { return false; }
    let (Some(vac), Some(vacs), Some(games), Some(community), Some(economy)) = (
        p["VACBanned"].as_bool(), p["NumberOfVACBans"].as_u64().and_then(|v| u32::try_from(v).ok()),
        p["NumberOfGameBans"].as_u64().and_then(|v| u32::try_from(v).ok()),
        p["CommunityBanned"].as_bool(), p["EconomyBan"].as_str(),
    ) else { return false; };
    // A public API response can omit a ban visible only to the signed-in
    // owner in Steam Support. Keep previously confirmed positive evidence.
    r.vac_banned |= vac || vacs > 0;
    r.vac_bans_count = r.vac_bans_count.max(vacs);
    r.game_bans_count = r.game_bans_count.max(games);
    // The public response may include individual AppIDs. A range of AppIDs
    // does not identify one game, so only accept a single exact AppID.
    if let Some(bans) = p["bans"].as_array() {
        for ban in bans {
            let app = ban["AppIdMin"].as_u64().filter(|n| *n > 0 && *n <= u32::MAX as u64);
            if let Some(app) = app.filter(|n| Some(*n) == ban["AppIdMax"].as_u64()) {
                let app_id = app.to_string();
                if !r.game_bans.iter().any(|b| b.app_id == app_id) {
                    r.game_bans.push(crate::storage::models::GameBanInfo {
                        app_id,
                        name: match app {
                            730 => "Counter-Strike 2".into(),
                            570 => "Dota 2".into(),
                            1422450 => "Deadlock".into(),
                            _ => format!("Steam App #{app}"),
                        },
                        // Steam does not document the BanType enum, so avoid
                        // calling an individual record VAC or Game Ban.
                        ban_type: Some("Блокировка Steam API".into()),
                    });
                }
            }
        }
    }
    r.game_bans_count = r.game_bans_count.max(r.game_bans.len() as u32);
    r.community_banned |= community;
    if economy != "none" || r.economy_ban == "none" { r.economy_ban = economy.to_owned(); }
    if economy == "banned" { r.trade_banned = true; }
    true
}

// Read only the public ban-status line. The adjacent "days since last ban"
// line contains numbers too and must never be interpreted as a ban count.
fn public_ban_counts(html: &str) -> Option<(Option<u32>, Option<u32>)> {
    let pos = html.find("profile_ban_status")?;
    let tail: String = html[pos..].chars().take(1000).collect();
    let line = tail.split("</div>").next().unwrap_or(&tail);
    let mut visible = String::with_capacity(line.len());
    let mut tag = true;
    for c in line.chars() {
        match c {
            '<' => { tag = true; visible.push(' '); }
            '>' => tag = false,
            _ if !tag => visible.push(c),
            _ => {}
        }
    }
    let words: Vec<String> = visible.to_ascii_lowercase().split_whitespace()
        .map(|s| s.trim_matches(|c: char| !c.is_ascii_alphanumeric()).to_string())
        .collect();
    let mut vac = None;
    let mut game = None;
    for pair in words.windows(3) {
        if let Ok(count) = pair[0].parse::<u32>() {
            if count > 0 && pair[1] == "vac" && pair[2].starts_with("ban") { vac = Some(count); }
            if count > 0 && pair[1] == "game" && pair[2].starts_with("ban") { game = Some(count); }
        }
    }
    if vac.is_some() || game.is_some() { Some((vac, game)) } else { None }
}

#[cfg(test)]
mod player_ban_tests {
    use super::*;
    #[test]
    fn game_ban_is_not_vac_and_has_no_invented_game() {
        let mut r = AccountRestrictions::default();
        let p = serde_json::json!({"SteamId":"76561197960265729","VACBanned":false,"NumberOfVACBans":0,"NumberOfGameBans":1,"CommunityBanned":false,"EconomyBan":"none"});
        assert!(apply_player_bans(&mut r, "76561197960265729", &p));
        assert!(!r.vac_banned);
        assert_eq!(r.game_bans_count, 1);
        assert!(r.game_bans.is_empty());
        assert!(!apply_player_bans(&mut r, "76561197960265730", &p));
        assert!(!apply_player_bans(&mut r, "76561197960265729", &serde_json::json!({"SteamId":"76561197960265729"})));
        assert_eq!(r.game_bans_count, 1);
    }

    #[test]
    fn exact_app_id_identifies_game_but_range_does_not() {
        let mut r = AccountRestrictions::default();
        let p = serde_json::json!({"SteamId":"76561197960265729","VACBanned":false,
            "NumberOfVACBans":0,"NumberOfGameBans":2,"CommunityBanned":false,
            "EconomyBan":"none","bans":[
                {"AppIdMin":730,"AppIdMax":730,"BanType":2},
                {"AppIdMin":570,"AppIdMax":730,"BanType":2}]});
        assert!(apply_player_bans(&mut r, "76561197960265729", &p));
        assert_eq!(r.game_bans_count, 2);
        assert_eq!(r.game_bans.len(), 1);
        assert_eq!(r.game_bans[0].name, "Counter-Strike 2");
        assert!(!r.vac_banned);
        assert!(apply_player_bans(&mut r, "76561197960265729", &p));
        assert_eq!(r.game_bans.len(), 1);
    }

    #[test]
    fn public_status_ignores_days_since_last_ban() {
        let html = r#"<div class="profile_ban"><div class="profile_ban_status">1 game ban on record</div><div class="profile_ban_info">423 days since last ban</div></div>"#;
        assert_eq!(public_ban_counts(html), Some((None, Some(1))));
        let html = r#"<div class="profile_ban_status">2 VAC bans on record<br>3 game bans on record</div>"#;
        assert_eq!(public_ban_counts(html), Some((Some(2), Some(3))));
        assert_eq!(public_ban_counts(r#"<div class="profile_ban_info">423 days since last ban</div>"#), None);
    }

    #[test]
    fn public_api_does_not_erase_confirmed_game() {
        let mut r = AccountRestrictions { game_bans_count: 1,
            game_bans: vec![crate::storage::models::GameBanInfo {
                app_id: "730".into(), name: "Counter-Strike 2".into(),
                ban_type: Some("Steam Support".into()),
            }], ..Default::default() };
        let p = serde_json::json!({"SteamId":"76561197960265729","VACBanned":false,
            "NumberOfVACBans":0,"NumberOfGameBans":0,"CommunityBanned":false,"EconomyBan":"none"});
        assert!(apply_player_bans(&mut r, "76561197960265729", &p));
        assert_eq!(r.game_bans_count, 1);
        assert_eq!(r.game_bans[0].app_id, "730");
    }

    #[test]
    fn user_payload_cannot_claim_steam_verification() {
        let payload = serde_json::json!({"account_id":"manual",
            "verified":{"vac_bans_count":2,"game_bans_count":1}});
        let parsed: AccountRestrictions = serde_json::from_value(payload).unwrap();
        assert_eq!(parsed.verified.vac_bans_count, 0);
        assert_eq!(parsed.verified.game_bans_count, 0);
    }

    #[tokio::test]
    async fn recent_automatic_check_reuses_saved_status_without_network() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().await.unwrap();
        let id = "76561197960265729";
        let saved = AccountRestrictions {
            account_id: id.into(),
            launcher: "steam".into(),
            game_bans_count: 1,
            ..Default::default()
        };
        db.upsert_account_restrictions(&saved).await.unwrap();
        db.steam_cache_put(&format!("restrictions:auto-check:v1:{id}"), "attempted")
            .await.unwrap();

        let (observed, checked) = auto_check_steam_restrictions(&db, id).await;
        assert!(!checked);
        assert_eq!(observed.game_bans_count, 1);
    }

    #[tokio::test]
    async fn automatic_batch_checks_library_steamids_and_rejects_legacy_names() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().await.unwrap();
        let id = "76561197960265729";
        let legacy: Account = serde_json::from_value(serde_json::json!({
            "id":"steam_old_login", "launcher":"steam", "username":"old_login",
            "display_name":null, "email":null, "is_active":false, "last_used":null
        })).unwrap();
        db.upsert_account(&legacy).await.unwrap();
        db.upsert_account_restrictions(&AccountRestrictions {
            account_id:id.into(), game_bans_count:1, ..Default::default()
        }).await.unwrap();
        db.steam_cache_put(&format!("restrictions:auto-check:v1:{id}"), "attempted").await.unwrap();
        let request = serde_json::from_value(serde_json::json!({"account_ids":[id,id]})).unwrap();
        let Json(response) = check_steam_restrictions(State(db.clone()), Some(Json(request))).await.unwrap();
        assert_eq!(response.restrictions.len(), 1);
        assert_eq!(response.restrictions[0].account_id, id);
        assert_eq!(response.restrictions[0].game_bans_count, 1);
        assert!(response.checked_account_ids.is_empty());
        let request = serde_json::from_value(serde_json::json!({"account_ids":["steam_old_login"]})).unwrap();
        assert_eq!(check_steam_restrictions(State(db), Some(Json(request))).await.unwrap_err().0, StatusCode::BAD_REQUEST);
    }
}

/// Helper function to perform online check for a single Steam account.
pub(super) async fn query_steam_restrictions(steam_id: &str, existing: Option<AccountRestrictions>, supplied_key: Option<&str>) -> (AccountRestrictions, bool) {
    let mut r = existing.unwrap_or_else(|| AccountRestrictions {
        account_id: steam_id.to_string(),
        launcher: "steam".to_string(),
        ..Default::default()
    });
    let mut success = false;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko)")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_default();

    // User Web API keys belong on the public endpoint. The partner host
    // requires a publisher key and rejects ordinary account keys.
    if let Some(key) = supplied_key.map(str::to_owned).or_else(|| std::env::var("STEAM_WEB_API_KEY").ok()) {
        if !key.trim().is_empty() {
            if let Ok(resp) = client.get("https://api.steampowered.com/ISteamUser/GetPlayerBans/v1/")
                .header("x-webapi-key", key.trim())
                .query(&[("steamids", steam_id)]).send().await {
                if resp.status().is_success() {
                    if let Ok(val) = resp.json::<serde_json::Value>().await {
                        if let Some(player) = val["players"].as_array().and_then(|players|
                            players.iter().find(|player| player["SteamId"].as_str() == Some(steam_id))
                        ) {
                            success = apply_player_bans(&mut r, steam_id, player);
                        }
                    }
                }
            }
            if success {
                // A valid API response must not be overwritten by profile XML/HTML.
                r.updated_at = chrono::Utc::now().to_rfc3339();
                return (r, true);
            }
        }
    }

    if supplied_key.is_some() { return (r, false); }

    // 2. Query Steam Community profile XML (works without API key for public profiles)
    let is_id64 = crate::launchers::steam::library::valid_steam_id(steam_id);
    let xml_url = if is_id64 {
        format!("https://steamcommunity.com/profiles/{}/?xml=1", steam_id)
    } else {
        format!("https://steamcommunity.com/id/{}/?xml=1", steam_id)
    };

    if let Ok(resp) = client.get(&xml_url).send().await {
        if let Ok(xml_text) = resp.text().await {
            if xml_text.contains("<profile>") || xml_text.contains("<vacBanned>") {
                success = true;
                // Parse tags from XML
                if xml_text.contains("<vacBanned>1</vacBanned>") {
                    r.vac_banned = true;
                    if r.vac_bans_count == 0 {
                        r.vac_bans_count = 1;
                    }
                }

                if let Some(start) = xml_text.find("<tradeBanState>") {
                    if let Some(end) = xml_text[start..].find("</tradeBanState>") {
                        let state = &xml_text[start + 15..start + end];
                        if state.eq_ignore_ascii_case("none") {
                            r.trade_banned = false;
                            r.economy_ban = "none".to_string();
                        } else {
                            r.trade_banned = true;
                            r.economy_ban = state.to_string();
                        }
                    }
                }

                if let Some(start) = xml_text.find("<location>") {
                    if let Some(end) = xml_text[start..].find("</location>") {
                        let loc = xml_text[start + 10..start + end]
                            .trim()
                            .trim_start_matches("<![CDATA[")
                            .trim_end_matches("]]>")
                            .trim();
                        if !loc.is_empty() {
                            let (cc, curr) = location_to_country_and_currency(loc);
                            r.country = Some(cc);
                            r.currency = Some(curr);
                        }
                    }
                }

                if xml_text.contains("<isLimitedAccount>1</isLimitedAccount>") {
                    r.is_limited = true;
                    r.market_banned = true;
                } else if xml_text.contains("<isLimitedAccount>0</isLimitedAccount>") {
                    r.is_limited = false;
                }
            }
        }
    }

    // 3. Query Steam Community profile HTML (works without API key to detect Game Bans)
    let html_url = if is_id64 {
        format!("https://steamcommunity.com/profiles/{}/", steam_id)
    } else {
        format!("https://steamcommunity.com/id/{}/", steam_id)
    };

    if let Ok(resp) = client.get(&html_url).header("Accept-Language", "en-US,en;q=0.9").send().await {
        if let Ok(html) = resp.text().await {
            if let Some((vac, game)) = public_ban_counts(&html) {
                success = true;
                if let Some(count) = vac {
                    r.vac_banned = true;
                    r.vac_bans_count = r.vac_bans_count.max(count);
                }
                if let Some(count) = game { r.game_bans_count = r.game_bans_count.max(count); }
            }
        }
    }

    if r.game_bans_count > 0 && r.details.as_deref().unwrap_or("").is_empty() {
        r.details = Some(format!("Зафиксирована игровая блокировка (Game Ban, всего: {})", r.game_bans_count));
    } else if r.vac_banned && r.details.as_deref().unwrap_or("").is_empty() {
        r.details = Some("Зафиксирован VAC-бан в профиле Steam".to_string());
    }

    if success { r.updated_at = chrono::Utc::now().to_rfc3339(); }
    (r, success)
}

/// Handler for POST/GET /api/steam/check-restrictions
pub async fn check_steam_restrictions(
    State(db): State<Database>,
    payload: Option<Json<CheckRestrictionsRequest>>,
) -> Result<Json<CheckRestrictionsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut ids_to_check = Vec::new();
    let mut supplied_key = None;
    let mut explicit_check = false;
    if let Some(Json(req)) = payload {
        if !req.account_ids.is_empty() {
            if req.account_id.is_some() || req.account_ids.len() > 64
                || req.account_ids.iter().any(|id| !crate::launchers::steam::library::valid_steam_id(id)) {
                return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
                    error: "Укажи список SteamID из библиотеки (не более 64).".into(),
                })));
            }
            ids_to_check = req.account_ids;
            ids_to_check.sort();
            ids_to_check.dedup();
        }
        supplied_key = req.api_key.map(|key| key.trim().to_owned()).filter(|key| !key.is_empty());
        explicit_check = supplied_key.is_some();
        if let Some(id) = req.account_id {
            if !id.trim().is_empty() {
                explicit_check = true;
                let trimmed = id.trim().to_string();
                if let Ok(accounts) = db.get_accounts().await {
                    if let Some(found) = accounts
                        .iter()
                        .find(|a| a.id == trimmed || a.username.eq_ignore_ascii_case(&trimmed))
                    {
                        ids_to_check.push(found.id.clone());
                    } else {
                        ids_to_check.push(trimmed);
                    }
                } else {
                    ids_to_check.push(trimmed);
                }
            }
        }
    }

    if ids_to_check.is_empty() {
        // Find all steam accounts from DB
        if let Ok(accounts) = db.get_accounts().await {
            for a in accounts {
                if a.launcher.eq_ignore_ascii_case("steam") && crate::launchers::steam::library::valid_steam_id(&a.id) {
                    ids_to_check.push(a.id);
                }
            }
        }
        if let Ok(Some((cached, _))) = db.steam_cache_get("local").await {
            if let Ok(library) = serde_json::from_str::<crate::launchers::steam::library::SteamLibrary>(&cached) {
                ids_to_check.extend(library.accounts.into_iter().map(|a| a.id)
                    .filter(|id| crate::launchers::steam::library::valid_steam_id(id)));
            }
        }
        ids_to_check.sort();
        ids_to_check.dedup();
    }

    let mut results = Vec::new();
    let mut checked_account_ids = Vec::new();
    for id in ids_to_check {
        let (updated, success) = if explicit_check {
            check_and_store_steam_restrictions(&db, &id, supplied_key.as_deref()).await
        } else {
            auto_check_steam_restrictions(&db, &id).await
        };
        if success {
            checked_account_ids.push(id.clone());
        } else if db.get_account_restrictions(&id).await.ok().flatten().is_none() {
            // A failed request does not establish a clean account status.
            continue;
        }
        results.push(updated);
    }

    Ok(Json(CheckRestrictionsResponse {
        status: "ok",
        restrictions: results,
        checked_account_ids,
    }))
}

/// Handler for POST /api/steam/switch-account
pub async fn switch_steam_account(
    State(db): State<Database>,
    Json(payload): Json<SwitchAccountRequest>,
) -> Result<Json<SwitchAccountResponse>, (StatusCode, Json<ErrorResponse>)> {
    let target = payload.target_account().unwrap_or("").trim();
    if target.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "account_name or account_id must not be empty".to_string(),
            }),
        ));
    }


    let _switch_guard = SWITCH_API.try_lock().map_err(|_| (
        StatusCode::CONFLICT,
        Json(ErrorResponse { error: "Переключение Steam уже выполняется.".into() }),
    ))?;
    let requested_url = payload.open_url.as_deref();
    let target_uri = match requested_url {
        Some(url) => sanitize_steam_url(url).ok_or_else(|| (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: "Недопустимый адрес для открытия в Steam.".into() }),
        ))?,
        None => "steam://open/main".to_string(),
    };
    let manager = SteamSessionManager::new();
    match manager.switch_account(target, Some(&db)).await {
        Ok(result) => {
            let skip_processes = std::env::var("SWCH_SKIP_PROCESS_TERMINATION")
                .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
            let has_custom_url = requested_url.is_some();
            let is_same_account = result.previous_account.as_deref() == Some(result.account_name.as_str());
            if has_custom_url && !skip_processes && !is_same_account {
                // Let the restarted client settle before sending its one page request.
                tokio::time::sleep(tokio::time::Duration::from_millis(3500)).await;
            }
            let uri_to_open = target_uri.clone();
            let launched = tokio::task::spawn_blocking(move || {
                if skip_processes { Ok(()) } else { open::that(&uri_to_open) }
            }).await;
            let message = if matches!(launched, Ok(Ok(()))) {
                if has_custom_url {
                    if is_same_account {
                        "Открываем страницу в клиенте Steam."
                    } else {
                        "Steam переключен на нужный аккаунт, открываем страницу в клиенте."
                    }
                } else if is_same_account {
                    "Steam уже запущен с этим аккаунтом."
                } else {
                    "Steam открыт с выбранным профилем. Если сессия истекла, подтверди вход в Steam Guard."
                }
            } else {
                "Профиль выбран. Не удалось открыть Steam автоматически — запусти его вручную."
            };
            Ok(Json(SwitchAccountResponse {
                status: "success", account_name: result.account_name,
                previous_account: result.previous_account, steam_id: result.steam_id,
                message: message.into(),
            }))
        },
        Err(SteamError::AccountNotFound(name)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Account '{}' not found in Steam loginusers.vdf", name),
            }),
        )),
        Err(SteamError::SteamNotFound) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Steam installation directory could not be detected".to_string(),
            }),
        )),
        Err(SteamError::ConfigNotFound(path)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Steam config file not found: {}", path.display()),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to switch Steam account: {}", e),
            }),
        )),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EpicAuthUrlResponse {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct EpicLoginRequest {
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct EpicLoginResponse {
    pub status: &'static str,
    pub account: EpicAccount,
}

#[derive(Debug, Serialize)]
pub struct EpicAccountsResponse {
    pub accounts: Vec<EpicAccount>,
    pub active_account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SwitchEpicAccountRequest {
    pub account_id: Option<String>,
    pub account_name: Option<String>,
}

impl SwitchEpicAccountRequest {
    pub fn target_account(&self) -> Option<&str> {
        self.account_id
            .as_deref()
            .or(self.account_name.as_deref())
    }
}

#[derive(Debug, Serialize)]
pub struct SwitchEpicAccountResponse {
    pub status: &'static str,
    pub account: EpicAccount,
    pub message: String,
}

/// Handler for GET /api/epic/auth/url
pub async fn epic_auth_url() -> Json<EpicAuthUrlResponse> {
    Json(EpicAuthUrlResponse {
        url: EpicClient::auth_url().to_string(),
    })
}

/// Handler for POST /api/epic/auth/open
pub async fn open_epic_auth_browser() -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let url = EpicClient::auth_url();
    let opened = tokio::task::spawn_blocking(move || {
        open::that(url)
    }).await;
    match opened {
        Ok(Ok(())) => Ok(Json(serde_json::json!({
            "status": "opened",
            "url": url,
        }))),
        _ => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Не удалось открыть браузер по умолчанию".into(),
            }),
        )),
    }
}

/// Handler for POST /api/epic/auth/login
pub async fn epic_login(
    State(epic_store): State<Arc<EpicAuthStore>>,
    Json(payload): Json<EpicLoginRequest>,
) -> Result<Json<EpicLoginResponse>, (StatusCode, Json<ErrorResponse>)> {
    let code = payload.code.trim();
    if code.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Authorization code must not be empty".to_string(),
            }),
        ));
    }

    match epic_store.login_with_code(code).await {
        Ok(account) => {
            // Automatically synchronize library immediately so all games appear at once
            if let Err(e) = epic_store.sync_library(&account.account_id, None).await {
                tracing::warn!("Automatic Epic library sync on login warning: {}", e);
            }
            Ok(Json(EpicLoginResponse {
                status: "success",
                account,
            }))
        }
        Err(EpicError::InvalidAuthCode(msg)) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid authorization code: {}", msg),
            }),
        )),
        Err(EpicError::Api { message, .. }) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Epic API error: {}", message),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Authentication failed: {}", e),
            }),
        )),
    }
}

/// Handler for GET /api/epic/accounts
pub async fn get_epic_accounts(
    State(epic_store): State<Arc<EpicAuthStore>>,
) -> Result<Json<EpicAccountsResponse>, (StatusCode, Json<ErrorResponse>)> {
    match epic_store.get_accounts().await {
        Ok(accounts) => {
            let active_id = epic_store
                .get_active_account()
                .await
                .ok()
                .flatten()
                .map(|a| a.account_id);

            Ok(Json(EpicAccountsResponse {
                accounts,
                active_account_id: active_id,
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to retrieve Epic accounts: {}", e),
            }),
        )),
    }
}

/// Handler for POST /api/epic/switch-account
pub async fn switch_epic_account(
    State(epic_store): State<Arc<EpicAuthStore>>,
    Json(payload): Json<SwitchEpicAccountRequest>,
) -> Result<Json<SwitchEpicAccountResponse>, (StatusCode, Json<ErrorResponse>)> {
    open_epic_launcher(State(epic_store), Json(payload)).await
}

/// Handler for POST /api/epic/open-launcher
pub async fn open_epic_launcher(
    State(epic_store): State<Arc<EpicAuthStore>>,
    Json(payload): Json<SwitchEpicAccountRequest>,
) -> Result<Json<SwitchEpicAccountResponse>, (StatusCode, Json<ErrorResponse>)> {
    let target = payload.target_account().unwrap_or("").trim();
    if target.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "account_id or account_name must not be empty".to_string(),
            }),
        ));
    }

    let _switch_guard = SWITCH_API.try_lock().map_err(|_| (
        StatusCode::CONFLICT,
        Json(ErrorResponse {
            error: "Переключение лаунчера уже выполняется.".into(),
        }),
    ))?;

    match epic_store.find_account(target).await {
        Ok(account) => {
            // API account credentials can be valid even when the native launcher
            // has no saved cookies. Authenticate first, before stopping Epic.
            let launcher_token = epic_store.launcher_refresh_token(&account.account_id).await
                .map_err(|e| {
                    let (status, message) = match e {
                        EpicError::Reqwest(_) => (StatusCode::BAD_GATEWAY,
                            "Не удалось связаться с Epic. Проверь подключение и повтори переключение.".to_string()),
                        EpicError::Api { status, code, .. } => {
                            // Error identifiers are safe; API messages can include credentials.
                            let code = code.filter(|c| c.len() < 160 && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_'))
                                .unwrap_or_else(|| "unknown".into());
                            (StatusCode::UNAUTHORIZED, format!("Epic отклонил автоматический вход (HTTP {}, {}). Подключи аккаунт заново в SWCH.", status.as_u16(), code))
                        }
                        EpicError::DeviceAuthNotFound(_) => (StatusCode::UNAUTHORIZED,
                            "Для этого аккаунта нет данных входа. Подключи аккаунт в SWCH.".into()),
                        _ => (StatusCode::BAD_GATEWAY,
                            "Не удалось подготовить автоматический вход Epic.".into()),
                    };
                    (status, Json(ErrorResponse { error: message }))
                })?;
            EpicSessionManager::new().login_with_refresh_token(&account.account_id, &account.display_name, &launcher_token).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                    error: format!("Не удалось запустить вход Epic: {}", e),
                })))?;
            let message = format!("Epic Games Launcher: выполнен вход в '{}'", account.display_name);

            let account = epic_store.switch_active_account(&account.account_id).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse { error: e.to_string() })))?;
            Ok(Json(SwitchEpicAccountResponse {
                status: "success",
                message,
                account,
            }))
        }
        Err(EpicError::AccountNotFound(id)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Epic account '{}' not found", id),
            }),
        )),
        Err(EpicError::Database(ref msg)) if msg.contains("not found in database") => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Epic account '{}' not found", target),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to open Epic launcher: {}", e),
            }),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct RemoveEpicAccountRequest {
    pub account_id: String,
}

#[derive(Debug, Serialize)]
pub struct RemoveEpicAccountResponse {
    pub status: &'static str,
    pub message: String,
}

/// Handler for POST /api/epic/remove-account
pub async fn remove_epic_account(
    State(epic_store): State<Arc<EpicAuthStore>>,
    Json(payload): Json<RemoveEpicAccountRequest>,
) -> Result<Json<RemoveEpicAccountResponse>, (StatusCode, Json<ErrorResponse>)> {
    let target = payload.account_id.trim();
    if target.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "account_id must not be empty".to_string(),
            }),
        ));
    }

    match epic_store.remove_account(target).await {
        Ok(()) => Ok(Json(RemoveEpicAccountResponse {
            status: "success",
            message: format!("Removed Epic account '{}'", target),
        })),
        Err(EpicError::AccountNotFound(id)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Epic account '{}' not found", id),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to remove Epic account: {}", e),
            }),
        )),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct EpicLibraryQuery {
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct EpicLibrarySyncRequest {
    pub account_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EpicLibrarySyncResponse {
    pub status: &'static str,
    pub account_id: String,
    pub total_entitlements: usize,
    pub base_games_count: usize,
    pub dlc_count: usize,
    pub installed_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpicLibraryGameResponse {
    pub id: String,
    pub app_name: String,
    pub catalog_item_id: String,
    pub namespace: String,
    pub title: String,
    pub install_path: Option<String>,
    pub executable_path: Option<String>,
    pub is_installed: bool,
    pub size_bytes: i64,
    pub banner_url: Option<String>,
    pub launch_uri: Option<String>,
    pub account_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_incomplete_install: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<crate::launchers::steam::library::UpdateState>,
}

/// Handler for GET /api/epic/library
///
/// Query parameter: optional `account_id`.
/// If omitted, queries the library for the currently active Epic account.
pub async fn get_epic_library(
    State(epic_store): State<Arc<EpicAuthStore>>,
    Query(params): Query<EpicLibraryQuery>,
) -> Result<Json<Vec<EpicLibraryGameResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let account_id = match params.account_id.filter(|s| !s.trim().is_empty()) {
        Some(id) => id,
        None => {
            let active = epic_store.get_active_account().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Failed to get active account: {}", e),
                    }),
                )
            })?;

            match active {
                Some(acc) => acc.account_id,
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "No active Epic account and no account_id specified".to_string(),
                        }),
                    ))
                }
            }
        }
    };

    let db_games = epic_store
        .database()
        .get_epic_library(Some(&account_id))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to retrieve Epic library: {}", e),
                }),
            )
        })?;

    // Scan local manifests (including Pending) to resolve update/incomplete status
    let local_manifests = if epic_store.client().is_mock() {
        Vec::new()
    } else {
        let manifest_dirs = crate::parsers::resolve_default_manifest_dirs();
        let mut manifests = Vec::new();
        for dir in manifest_dirs {
            if let Ok(items) = crate::parsers::scan_manifest_directory(&dir) {
                manifests.extend(items);
            }
        }
        manifests
    };

    let mut response_games: Vec<EpicLibraryGameResponse> = Vec::with_capacity(db_games.len());
    let mut matched_manifest_keys: HashSet<String> = HashSet::new();

    for g in db_games {
        let matching_manifest = local_manifests.iter().find(|m| {
            m.app_name.eq_ignore_ascii_case(&g.app_name)
                || m.catalog_item_id.eq_ignore_ascii_case(&g.catalog_item_id)
                || m.catalog_item_id.eq_ignore_ascii_case(&g.app_name)
                || m.display_name.eq_ignore_ascii_case(&g.title)
        });

        let is_incomplete = matching_manifest
            .and_then(|m| m.is_incomplete_install)
            .unwrap_or(false);

        if let Some(m) = matching_manifest {
            matched_manifest_keys.insert(m.catalog_item_id.to_lowercase());
            matched_manifest_keys.insert(m.app_name.to_lowercase());
            matched_manifest_keys.insert(m.display_name.to_lowercase());
        }

        let (is_installed, update, is_incomplete_opt) = if is_incomplete {
            (
                false,
                Some(crate::launchers::steam::library::UpdateState {
                    required: false,
                    phase: "downloading".to_string(),
                    downloaded: 0,
                    download_total: 0,
                    staged: 0,
                    stage_total: 0,
                }),
                Some(true),
            )
        } else {
            (matching_manifest.map_or(g.is_installed, |m| m.is_valid_installation()), None, None)
        };

        response_games.push(EpicLibraryGameResponse {
            id: g.id,
            app_name: g.app_name,
            catalog_item_id: g.catalog_item_id,
            namespace: g.namespace,
            title: g.title,
            install_path: matching_manifest.map(|m| m.install_location.clone()).or(g.install_path),
            executable_path: matching_manifest.map(|m| m.resolve_executable_path().to_string_lossy().into_owned()).or(g.executable_path),
            is_installed,
            size_bytes: matching_manifest.map_or(g.size_bytes, |m| m.install_size),
            banner_url: g.banner_url,
            launch_uri: g.launch_uri,
            account_id: g.account_id,
            is_incomplete_install: is_incomplete_opt,
            update,
        });
    }

    // Also include any base game manifests with incomplete/pending installs that were not yet in db
    for m in local_manifests {
        if m.is_dlc() {
            continue;
        }
        let cat_key = m.catalog_item_id.to_lowercase();
        let app_key = m.app_name.to_lowercase();
        let title_key = m.display_name.to_lowercase();
        if matched_manifest_keys.contains(&cat_key)
            || matched_manifest_keys.contains(&app_key)
            || matched_manifest_keys.contains(&title_key)
        {
            continue;
        }
        matched_manifest_keys.insert(cat_key);
        matched_manifest_keys.insert(app_key);
        matched_manifest_keys.insert(title_key);

        let is_incomplete = m.is_incomplete_install.unwrap_or(false);
        let update = if is_incomplete {
            Some(crate::launchers::steam::library::UpdateState {
                required: false,
                phase: "downloading".to_string(),
                downloaded: 0,
                download_total: 0,
                staged: 0,
                stage_total: 0,
            })
        } else {
            None
        };

        let is_installed = m.is_valid_installation();
        let game_id = format!("epic_{}", m.catalog_item_id);

        response_games.push(EpicLibraryGameResponse {
            id: game_id,
            app_name: m.app_name.clone(),
            catalog_item_id: m.catalog_item_id.clone(),
            namespace: m.catalog_namespace.clone(),
            title: m.display_name.clone(),
            install_path: Some(m.install_location.clone()),
            executable_path: Some(m.resolve_executable_path().to_string_lossy().to_string()),
            is_installed,
            size_bytes: m.install_size,
            banner_url: None,
            launch_uri: Some(format!("epic://launch/{}", m.catalog_item_id)),
            account_id: account_id.clone(),
            is_incomplete_install: if is_incomplete { Some(true) } else { None },
            update,
        });
    }

    Ok(Json(response_games))
}

/// Handler for POST /api/epic/library/sync
///
/// Request body: optional `{"account_id": "..."}`.
/// If omitted, synchronizes the currently active Epic account.
pub async fn sync_epic_library(
    State(epic_store): State<Arc<EpicAuthStore>>,
    payload: Option<Json<EpicLibrarySyncRequest>>,
) -> Result<Json<EpicLibrarySyncResponse>, (StatusCode, Json<ErrorResponse>)> {
    let req_account_id = payload
        .and_then(|Json(p)| p.account_id)
        .filter(|s| !s.trim().is_empty());

    let target_account_id = match req_account_id {
        Some(id) => id,
        None => {
            let active = epic_store.get_active_account().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Failed to get active account: {}", e),
                    }),
                )
            })?;

            match active {
                Some(acc) => acc.account_id,
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "No active Epic account to sync and no account_id specified".to_string(),
                        }),
                    ))
                }
            }
        }
    };

    match epic_store.sync_library(&target_account_id, None).await {
        Ok(summary) => Ok(Json(EpicLibrarySyncResponse {
            status: "success",
            account_id: target_account_id,
            total_entitlements: summary.total_entitlements,
            base_games_count: summary.base_games_count,
            dlc_count: summary.dlc_count,
            installed_count: summary.installed_count,
        })),
        Err(EpicError::AccountNotFound(id)) | Err(EpicError::DeviceAuthNotFound(id)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Epic account '{}' not found", id),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Library sync failed: {}", e),
            }),
        )),
    }
}

/// Request payload for `POST /api/epic/launch`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct LaunchGameRequest {
    /// Optional Epic account ID. If omitted, uses the currently active Epic account.
    #[serde(default)]
    pub account_id: Option<String>,

    /// Application name or catalog item identifier (required, non-empty).
    #[serde(default)]
    pub app_name: String,

    /// Optional catalog namespace / sandbox ID.
    #[serde(default)]
    pub namespace: Option<String>,

    /// Optional additional command-line arguments to append.
    #[serde(default)]
    pub additional_args: Option<Vec<String>>,
}

/// Successful response for `POST /api/epic/launch`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchGameResponse {
    /// Status indicator ("success").
    pub status: &'static str,

    /// Application name launched.
    pub app_name: String,

    /// Sanitized launch command-line arguments safe for API responses (secrets masked).
    pub launch_arguments: Vec<String>,

    /// Redacted launch arguments safe for logging and display.
    pub display_arguments: Vec<String>,

    /// Spawned process ID if process execution was triggered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

/// Handler for POST /api/epic/launch and POST /api/epic/launch-game
///
/// Prepares one-time exchange codes and launches an Epic Games title.
/// Accepts optional `account_id` (falls back to active account), `app_name`,
/// optional `namespace`, and optional `additional_args`.
pub async fn launch_epic_game(
    State(epic_store): State<Arc<EpicAuthStore>>,
    Json(payload): Json<LaunchGameRequest>,
) -> Result<Json<LaunchGameResponse>, (StatusCode, Json<ErrorResponse>)> {
    let app_name = payload.app_name.trim();
    if app_name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "app_name must not be empty".to_string(),
            }),
        ));
    }

    let options = crate::launchers::epic::LaunchGameOptions {
        account_id: payload.account_id.filter(|s| !s.trim().is_empty()),
        app_name: app_name.to_string(),
        namespace: payload.namespace.filter(|s| !s.trim().is_empty()),
        locale: None,
        additional_args: payload.additional_args,
        dry_run: false,
    };

    match epic_store.launch_game_with_options(options).await {
        Ok(result) => Ok(Json(LaunchGameResponse {
            status: "success",
            app_name: result.app_name,
            launch_arguments: result.display_arguments.clone(),
            display_arguments: result.display_arguments,
            pid: result.pid,
        })),
        Err(EpicError::NoActiveAccount) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "No active Epic account and no account_id specified".to_string(),
            }),
        )),
        Err(EpicError::AccountNotFound(id)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Epic account '{}' not found", id),
            }),
        )),
        Err(EpicError::DeviceAuthNotFound(id)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Device credentials not found for Epic account '{}'", id),
            }),
        )),
        Err(EpicError::Auth(msg)) | Err(EpicError::InvalidAuthCode(msg)) => Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: format!("Authentication failed: {}", msg),
            }),
        )),
        Err(EpicError::Api { message, status, code, .. }) => {
            if status == reqwest::StatusCode::UNAUTHORIZED
                || code.as_deref() == Some("errors.com.epicgames.account.invalid_account_credentials")
                || message.contains("invalid_account_credentials")
                || message.contains("invalid_grant")
            {
                Err((
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorResponse {
                        error: format!("Epic API authorization error: {}", message),
                    }),
                ))
            } else {
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Epic API error: {}", message),
                    }),
                ))
            }
        }
        Err(EpicError::Database(msg)) => {
            if msg.contains("incomplete installation")
                || msg.contains("must not be empty")
                || msg.contains("DLC")
            {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: msg,
                    }),
                ))
            } else if msg.contains("Manifest") || msg.contains("not found") || msg.contains("not installed") {
                Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: msg,
                    }),
                ))
            } else {
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: msg,
                    }),
                ))
            }
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Launch failed: {}", e),
            }),
        )),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct EpicAccountDetailsQuery {
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FortniteSummaryQuery {
    pub account_id: Option<String>,
    pub refresh: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FortniteSkinsQuery {
    pub account_id: Option<String>,
    #[serde(rename = "type")]
    pub cosmetic_type: Option<String>,
    pub search: Option<String>,
    pub rarity: Option<String>,
    pub exclusive: Option<bool>,
    pub refresh: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FortniteRefreshRequest {
    pub account_id: Option<String>,
}

/// Handler for GET /api/epic/account-details
///
/// Fetches complete Epic Games account details (email verified, 2FA enabled, country, linked external platforms).
pub async fn get_epic_account_details(
    State(epic_store): State<Arc<EpicAuthStore>>,
    Query(params): Query<EpicAccountDetailsQuery>,
) -> Result<Json<EpicAccountDetails>, (StatusCode, Json<ErrorResponse>)> {
    let account_id = match params.account_id.filter(|s| !s.trim().is_empty()) {
        Some(id) => id,
        None => {
            let active = epic_store.get_active_account().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Failed to get active Epic account: {}", e),
                    }),
                )
            })?;
            match active {
                Some(a) => a.account_id,
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "No active Epic account found and no account_id specified".to_string(),
                        }),
                    ));
                }
            }
        }
    };

    match epic_store.get_account_details(&account_id).await {
        Ok(details) => Ok(Json(details)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to retrieve Epic account details: {}", e),
            }),
        )),
    }
}

/// Handler for GET /api/epic/fortnite/summary
///
/// Fetches complete Fortnite overview: V-Bucks, Battle Pass, level, STW founder status, and cosmetic counts.
pub async fn get_epic_fortnite_summary(
    State(epic_store): State<Arc<EpicAuthStore>>,
    Query(params): Query<FortniteSummaryQuery>,
) -> Result<Json<FortniteAccountSummary>, (StatusCode, Json<ErrorResponse>)> {
    let account_id = match params.account_id.filter(|s| !s.trim().is_empty()) {
        Some(id) => id,
        None => {
            let active = epic_store.get_active_account().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Failed to get active Epic account: {}", e),
                    }),
                )
            })?;
            match active {
                Some(a) => a.account_id,
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "No active Epic account found and no account_id specified".to_string(),
                        }),
                    ));
                }
            }
        }
    };

    let force_refresh = params.refresh.unwrap_or(false);

    match epic_store.get_fortnite_summary(&account_id, force_refresh).await {
        Ok(summary) => Ok(Json(summary)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to retrieve Fortnite summary: {}", e),
            }),
        )),
    }
}

/// Handler for GET /api/epic/fortnite/skins
///
/// Returns filtered list of Fortnite cosmetics (by type: outfit, pickaxe, glider, etc., rarity, search term, exclusive).
pub async fn get_epic_fortnite_skins(
    State(epic_store): State<Arc<EpicAuthStore>>,
    Query(params): Query<FortniteSkinsQuery>,
) -> Result<Json<Vec<FortniteCosmeticItem>>, (StatusCode, Json<ErrorResponse>)> {
    let account_id = match params.account_id.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(id) => id.to_string(),
        None => {
            let active = epic_store.get_active_account().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Failed to get active Epic account: {}", e),
                    }),
                )
            })?;
            match active {
                Some(a) => a.account_id,
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "No active Epic account found and no account_id specified".to_string(),
                        }),
                    ));
                }
            }
        }
    };

    let force_refresh = params.refresh.unwrap_or(false);
    let summary = epic_store
        .get_fortnite_summary(&account_id, force_refresh)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to retrieve Fortnite summary: {}", e),
                }),
            )
        })?;

    let search = params.search.as_deref().map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let rarity_filter = params.rarity.as_deref().map(|r| r.trim().to_lowercase()).unwrap_or_default();
    let type_filter = params.cosmetic_type.as_deref().map(|t| t.trim().to_lowercase()).unwrap_or_default();
    let exclusive_only = params.exclusive.unwrap_or(false);

    let filtered: Vec<FortniteCosmeticItem> = summary
        .cosmetics
        .into_iter()
        .filter(|c| {
            if exclusive_only && !c.is_exclusive {
                return false;
            }
            if !type_filter.is_empty() && type_filter != "all" {
                let matches_type = match type_filter.as_str() {
                    "outfit" | "skin" | "character" => c.cosmetic_type == FortniteCosmeticType::Outfit,
                    "backpack" | "backbling" => c.cosmetic_type == FortniteCosmeticType::Backpack,
                    "pickaxe" | "harvestingtool" => c.cosmetic_type == FortniteCosmeticType::Pickaxe,
                    "glider" => c.cosmetic_type == FortniteCosmeticType::Glider,
                    "emote" | "dance" => c.cosmetic_type == FortniteCosmeticType::Emote,
                    "wrap" => c.cosmetic_type == FortniteCosmeticType::Wrap,
                    "music" => c.cosmetic_type == FortniteCosmeticType::Music,
                    "contrail" => c.cosmetic_type == FortniteCosmeticType::Contrail,
                    "loadingscreen" => c.cosmetic_type == FortniteCosmeticType::LoadingScreen,
                    _ => true,
                };
                if !matches_type {
                    return false;
                }
            }
            if !rarity_filter.is_empty() && rarity_filter != "all" {
                if !c.rarity.eq_ignore_ascii_case(&rarity_filter) {
                    return false;
                }
            }
            if !search.is_empty() {
                let in_name = c.name.to_lowercase().contains(&search);
                let in_id = c.id.to_lowercase().contains(&search);
                if !in_name && !in_id {
                    return false;
                }
            }
            true
        })
        .collect();

    Ok(Json(filtered))
}

/// Handler for POST /api/epic/fortnite/refresh
///
/// Forces a re-fetch of Fortnite data from Epic MCP servers and invalidates cache.
pub async fn refresh_epic_fortnite(
    State(epic_store): State<Arc<EpicAuthStore>>,
    payload: Option<Json<FortniteRefreshRequest>>,
) -> Result<Json<FortniteAccountSummary>, (StatusCode, Json<ErrorResponse>)> {
    let account_id = match payload.and_then(|p| p.account_id.clone()).filter(|s| !s.trim().is_empty()) {
        Some(id) => id,
        None => {
            let active = epic_store.get_active_account().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Failed to get active Epic account: {}", e),
                    }),
                )
            })?;
            match active {
                Some(a) => a.account_id,
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "No active Epic account found and no account_id specified".to_string(),
                        }),
                    ));
                }
            }
        }
    };

    match epic_store.get_fortnite_summary(&account_id, true).await {
        Ok(summary) => Ok(Json(summary)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to refresh Fortnite data: {}", e),
            }),
        )),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct EpicGamePriceQuery {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub currency: Option<String>,
    pub cc: Option<String>,
}

pub async fn epic_game_price(
    State(db): State<Database>,
    Query(query): Query<EpicGamePriceQuery>,
) -> Result<Json<crate::launchers::steam::remote::GamePrice>, (StatusCode, Json<ErrorResponse>)> {
    use crate::launchers::steam::remote::GamePrice;
    let app_id = query.app_id.unwrap_or_default().trim().to_string();
    let title = query.title.unwrap_or_default().trim().to_string();
    if app_id.is_empty() && title.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "app_id or title required".into(),
            }),
        ));
    }
    let cc = query
        .cc
        .as_deref()
        .filter(|s| s.len() == 2 && s.chars().all(|c| c.is_ascii_alphabetic()))
        .unwrap_or("US")
        .to_uppercase();
    let currency = query
        .currency
        .as_deref()
        .filter(|s| s.len() == 3 && s.chars().all(|c| c.is_ascii_alphabetic()))
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| match cc.as_str() {
            "BR" => "BRL".into(),
            "RU" => "RUB".into(),
            "KZ" => "KZT".into(),
            "UA" => "UAH".into(),
            "DE" | "FR" | "ES" | "IT" => "EUR".into(),
            "GB" => "GBP".into(),
            "PL" => "PLN".into(),
            "CN" => "CNY".into(),
            "JP" => "JPY".into(),
            _ => "USD".into(),
        });

    let lookup_id = if !app_id.is_empty() {
        app_id.clone()
    } else {
        title.clone()
    };
    let key = format!("game-price-epic:{}:{}:{}", cc, currency, lookup_id);
    if let Ok(Some((payload, _))) = db.steam_cache_get(&key).await {
        if let Ok(mut price) = serde_json::from_str::<GamePrice>(&payload) {
            price.cached = true;
            return Ok(Json(price));
        }
    }

    if !title.is_empty() {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        let resp = http
            .get("https://store.steampowered.com/api/storesearch/")
            .query(&[("term", &title), ("l", &"english".to_string()), ("cc", &cc)])
            .send()
            .await;
        if let Ok(resp) = resp {
            if let Ok(val) = resp.json::<serde_json::Value>().await {
                if let Some(items) = val.get("items").and_then(|v| v.as_array()) {
                    if let Some(first) = items.first() {
                        let final_price = first
                            .get("price")
                            .and_then(|p| p.get("final"))
                            .and_then(|v| v.as_u64());
                        let init_price = first
                            .get("price")
                            .and_then(|p| p.get("initial"))
                            .and_then(|v| v.as_u64());
                        let is_free = first.get("price").is_none() || final_price == Some(0);
                        let price = GamePrice {
                            app_id: if !app_id.is_empty() {
                                app_id.clone()
                            } else {
                                first
                                    .get("id")
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| lookup_id.clone())
                            },
                            currency: currency.clone(),
                            region: cc.clone(),
                            price_cents: if is_free { Some(0) } else { final_price },
                            regular_price_cents: if is_free { init_price.or(Some(0)) } else { init_price },
                            is_free: Some(is_free),
                            updated_at: chrono::Utc::now().to_rfc3339(),
                            cached: false,
                            warning: None,
                        };
                        let _ = db
                            .steam_cache_put(&key, &serde_json::to_string(&price).unwrap_or_default())
                            .await;
                        return Ok(Json(price));
                    }
                }
            }
        }
    }

    let fallback = GamePrice {
        app_id: if !app_id.is_empty() {
            app_id
        } else {
            lookup_id
        },
        currency: currency.clone(),
        region: cc.clone(),
        price_cents: Some(0),
        regular_price_cents: None,
        is_free: Some(true),
        updated_at: chrono::Utc::now().to_rfc3339(),
        cached: false,
        warning: None,
    };
    let _ = db
        .steam_cache_put(&key, &serde_json::to_string(&fallback).unwrap_or_default())
        .await;
    Ok(Json(fallback))
}

#[derive(Deserialize, Default)]
pub struct EpicGamePricesQuery {
    pub cc: Option<String>,
}

pub async fn epic_game_prices(
    State(db): State<Database>,
    Query(query): Query<EpicGamePricesQuery>,
) -> Result<Json<Vec<crate::launchers::steam::remote::GamePrice>>, (StatusCode, Json<ErrorResponse>)> {
    let mut result = vec![];
    let prefix = if let Some(ref cc) = query.cc {
        format!("game-price-epic:{}:", cc.to_uppercase())
    } else {
        "game-price-epic:".to_string()
    };
    if let Ok(entries) = db.steam_cache_prefix(&prefix).await {
        for (_, payload, _) in entries {
            if let Ok(mut price) =
                serde_json::from_str::<crate::launchers::steam::remote::GamePrice>(&payload)
            {
                price.cached = true;
                if !result.iter().any(|p: &crate::launchers::steam::remote::GamePrice| {
                    p.app_id == price.app_id && p.currency == price.currency
                }) {
                    result.push(price);
                }
            }
        }
    }
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use crate::launchers::epic::keyring::MemoryCredentialStore;
    use crate::launchers::epic::client::EpicClient;
    use crate::storage::models::Account;

    fn mock_epic_oauth_router() -> Router {
        Router::new()
            .route(
                "/account/api/oauth/token",
                axum::routing::post(|| async {
                    axum::Json(serde_json::json!({
                        "access_token": "mock_access_token_123",
                        "expires_in": 3600,
                        "token_type": "bearer",
                        "account_id": "acc_mock_test",
                        "client_id": "34a02cf8f4414e29b15921876da36f9a"
                    }))
                }),
            )
            .route(
                "/account/api/oauth/exchange",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "expiresInSeconds": 300,
                        "code": "mock_exchange_code_456",
                        "creatingClientId": "34a02cf8f4414e29b15921876da36f9a"
                    }))
                }),
            )
    }

    async fn setup_handler_test_store() -> (Database, Arc<EpicAuthStore>) {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().await.unwrap();
        let keyring = Arc::new(MemoryCredentialStore::new());
        let client = Arc::new(EpicClient::from_router(mock_epic_oauth_router()));
        let store = Arc::new(EpicAuthStore::new(db.clone(), keyring, client));
        (db, store)
    }

    #[tokio::test]
    async fn test_launch_epic_game_empty_app_name() {
        let (_, store) = setup_handler_test_store().await;
        let req = LaunchGameRequest {
            account_id: None,
            app_name: "   ".to_string(),
            namespace: None,
            additional_args: None,
        };

        let res = launch_epic_game(State(store), Json(req)).await;
        assert!(res.is_err());
        let (status, Json(err)) = res.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(err.error.contains("app_name must not be empty"));
    }

    #[tokio::test]
    async fn test_launch_epic_game_no_active_account() {
        let (_, store) = setup_handler_test_store().await;
        let req = LaunchGameRequest {
            account_id: None,
            app_name: "Fortnite".to_string(),
            namespace: None,
            additional_args: None,
        };

        let res = launch_epic_game(State(store), Json(req)).await;
        assert!(res.is_err());
        let (status, Json(err)) = res.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(err.error.contains("No active Epic account"));
    }

    #[tokio::test]
    async fn test_launch_epic_game_account_not_found() {
        let (_, store) = setup_handler_test_store().await;
        let req = LaunchGameRequest {
            account_id: Some("phantom_acc".to_string()),
            app_name: "Sugar".to_string(),
            namespace: None,
            additional_args: None,
        };

        let res = launch_epic_game(State(store), Json(req)).await;
        assert!(res.is_err());
        let (status, Json(err)) = res.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(err.error.contains("not found"));
    }

    static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn test_launch_epic_game_missing_manifest_returns_404() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

        let (db, store) = setup_handler_test_store().await;
        let account_id = "acc_404_test";
        db.upsert_account(&Account {
            id: account_id.to_string(),
            launcher: "epic".to_string(),
            username: "Test404User".to_string(),
            display_name: Some("Test404User".to_string()),
            email: Some("test404@epic.com".to_string()),
            is_active: true,
            last_used: None,
        })
        .await
        .unwrap();

        let auth = crate::launchers::epic::models::DeviceAuth {
            device_id: "dev404".to_string(),
            account_id: account_id.to_string(),
            secret: "sec404".to_string(),
            user_agent: None,
            created: None,
            ..Default::default()
        };
        store.keyring().set_device_auth(&auth).unwrap();

        let req = LaunchGameRequest {
            account_id: Some(account_id.to_string()),
            app_name: "NonExistentGame12345".to_string(),
            namespace: None,
            additional_args: None,
        };

        let res = launch_epic_game(State(store), Json(req)).await;
        assert!(res.is_err(), "Expected launch of nonexistent game to fail");
        let (status, Json(err)) = res.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            err.error.contains("Manifest or installation not found") || err.error.contains("not found"),
            "Error should indicate manifest or installation missing: got '{}'",
            err.error
        );
    }

    #[tokio::test]
    async fn test_launch_epic_game_response_redacts_secrets() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
        let (db, store) = setup_handler_test_store().await;
        let account_id = "acc_redact_test";
        db.upsert_account(&Account {
            id: account_id.to_string(),
            launcher: "epic".to_string(),
            username: "RedactUser".to_string(),
            display_name: Some("RedactUser".to_string()),
            email: Some("redact@epic.com".to_string()),
            is_active: true,
            last_used: None,
        })
        .await
        .unwrap();

        let auth = crate::launchers::epic::models::DeviceAuth {
            device_id: "dev_redact".to_string(),
            account_id: account_id.to_string(),
            secret: "sec_redact".to_string(),
            user_agent: None,
            created: None,
            ..Default::default()
        };
        store.keyring().set_device_auth(&auth).unwrap();

        let req = LaunchGameRequest {
            account_id: Some(account_id.to_string()),
            app_name: "Sugar".to_string(),
            namespace: None,
            additional_args: Some(vec!["--custom-flag=1".to_string()]),
        };

        let res = launch_epic_game(State(store), Json(req)).await;
        std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

        assert!(res.is_ok(), "Expected simulated launch to succeed: {:?}", res.err());
        let Json(resp) = res.unwrap();
        assert_eq!(resp.status, "success");
        assert_eq!(resp.app_name, "Sugar");

        // Verify neither launch_arguments nor display_arguments expose the raw exchange code
        for arg in &resp.launch_arguments {
            assert!(!arg.contains("mock_exchange_code_456"), "launch_arguments leaked secret: {}", arg);
            if arg.starts_with("-AUTH_PASSWORD=") {
                assert_eq!(arg, "-AUTH_PASSWORD=********");
            }
        }
        for arg in &resp.display_arguments {
            assert!(!arg.contains("mock_exchange_code_456"), "display_arguments leaked secret: {}", arg);
            if arg.starts_with("-AUTH_PASSWORD=") {
                assert_eq!(arg, "-AUTH_PASSWORD=********");
            }
        }
    }

    #[tokio::test]
    async fn test_launch_game_request_serde_default_empty_json() {
        let req: LaunchGameRequest = serde_json::from_str("{}").expect("Failed to deserialize empty JSON");
        assert_eq!(req.app_name, "");
        assert!(req.account_id.is_none());
        assert!(req.namespace.is_none());
        assert!(req.additional_args.is_none());

        let (_, store) = setup_handler_test_store().await;
        let res = launch_epic_game(State(store), Json(req)).await;
        assert!(res.is_err());
        let (status, Json(err)) = res.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err.error, "app_name must not be empty");
    }
}
