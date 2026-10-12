//! Read existing platform totals; never launch games or switch native sessions.
use super::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub launcher: String,
    pub app_id: String,
    pub account_id: Option<String>,
    pub seconds: f64,
    pub scope: String,
    pub source: String,
    #[serde(default)]
    pub cached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
}
#[derive(Deserialize)]
pub struct PlaytimeQuery {
    pub launcher: String,
    pub account_id: Option<String>,
    #[serde(default)]
    pub refresh: bool,
}
fn record(
    launcher: &str,
    app: &str,
    account: Option<String>,
    seconds: f64,
    scope: &str,
    source: &str,
) -> Record {
    Record {
        launcher: launcher.into(),
        app_id: app.into(),
        account_id: account,
        seconds,
        scope: scope.into(),
        source: source.into(),
        cached: false,
        identity: None,
    }
}
fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite() && *n >= 0.0)
}

/// Epic totalTime is seconds, not milliseconds/minutes. Unknown is not zero.
pub fn epic_records(data: &Value, account: &str) -> Result<Vec<Record>, &'static str> {
    let items = data
        .as_array()
        .ok_or("Epic returned an unknown playtime format")?;
    let mut records = BTreeMap::new();
    for item in items {
        if item["accountId"].as_str().is_some_and(|id| id != account) {
            continue;
        }
        let Some(app) = item["artifactId"].as_str().filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(seconds) = number(&item["totalTime"]) else {
            continue;
        };
        let row = record(
            "epic",
            app,
            Some(account.into()),
            seconds,
            "lifetime",
            "Epic Games",
        );
        if records
            .get(app)
            .is_none_or(|old: &Record| old.seconds < seconds)
        {
            records.insert(app.to_string(), row);
        }
    }
    Ok(records.into_values().collect())
}

/// Per-player Java stats are game ticks (20 ticks/s). Legacy counters are a
/// fallback, never added to the renamed modern counter.
pub fn minecraft_seconds(data: &Value) -> Option<f64> {
    [
        "/stats/minecraft:custom/minecraft:play_time",
        "/stats/minecraft:custom/minecraft:play_one_minute",
        "/stat.playOneMinute",
    ]
    .iter()
    .find_map(|key| data.pointer(key).and_then(number))
    .map(|ticks| ticks / 20.0)
}
pub fn minecraft_records(
    roots: &[PathBuf],
    accounts: &[crate::storage::models::MinecraftAccount],
) -> Vec<Record> {
    let mut seen = HashSet::new();
    let mut totals: BTreeMap<String, f64> = BTreeMap::new();
    for root in roots {
        let Ok(worlds) = std::fs::read_dir(root.join("saves")) else {
            continue;
        };
        for world in worlds.flatten().take(1000) {
            if !world.path().is_dir() {
                continue;
            }
            for account in accounts {
                let uuid = account.uuid.replace('-', "").to_ascii_lowercase();
                if uuid.len() != 32 || !uuid.chars().all(|c| c.is_ascii_hexdigit()) {
                    continue;
                }
                let uuid = format!(
                    "{}-{}-{}-{}-{}",
                    &uuid[..8],
                    &uuid[8..12],
                    &uuid[12..16],
                    &uuid[16..20],
                    &uuid[20..]
                );
                let file = world.path().join("stats").join(format!("{uuid}.json"));
                let Ok(canonical) = file.canonicalize() else {
                    continue;
                };
                if !seen.insert((canonical.clone(), account.account_id.clone())) {
                    continue;
                }
                if std::fs::metadata(&canonical).is_ok_and(|m| m.len() <= 4 * 1024 * 1024) {
                    if let Ok(bytes) = std::fs::read(canonical) {
                        if let Ok(data) = serde_json::from_slice(&bytes) {
                            if let Some(seconds) = minecraft_seconds(&data) {
                                *totals.entry(account.account_id.clone()).or_default() += seconds;
                            }
                        }
                    }
                }
            }
        }
    }
    totals
        .into_iter()
        .map(|(id, seconds)| {
            let uuid = accounts
                .iter()
                .find(|a| a.account_id == id)
                .map(|a| a.uuid.replace('-', "").to_ascii_lowercase())
                .unwrap_or_default();
            let mut row = record(
                "minecraft",
                "minecraft_java",
                Some(id),
                seconds,
                "local_worlds",
                "Minecraft · статистика локальных миров",
            );
            row.identity = Some(format!("minecraft:{uuid}"));
            row
        })
        .collect()
}

/// GALAXY keeps its recorded totals in GameTimes.minutesInGame. Open its own
/// database read-only; a local profile total is not attributed to a SWCH account.
pub fn galaxy_records(path: &Path) -> Result<Vec<Record>, rusqlite::Error> {
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(500))?;
    let mut stmt =
        conn.prepare("SELECT releaseKey, MAX(minutesInGame) FROM GameTimes GROUP BY releaseKey")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<f64>>(1)?))
    })?;
    let mut result = vec![];
    for row in rows {
        let (key, minutes) = row?;
        let Some((platform, app)) = key.split_once('_') else {
            continue;
        };
        let launcher = match platform {
            "gog" => "gog",
            "uplay" => "ubisoft",
            "epic" => "epic",
            _ => continue,
        };
        let Some(minutes) = minutes.filter(|n| n.is_finite() && *n >= 0.0) else {
            continue;
        };
        if app.is_empty() {
            continue;
        }
        let mut row = record(
            launcher,
            app,
            None,
            minutes * 60.0,
            "galaxy_local",
            "GOG GALAXY · локальная статистика",
        );
        row.identity = Some(format!("galaxy:{key}"));
        result.push(row);
    }
    Ok(result)
}
fn galaxy_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SWCH_GALAXY_PLAYTIME_DB") {
        return Some(path.into());
    }
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("ProgramData") {
        return Some(PathBuf::from(root).join("GOG.com/Galaxy/storage/galaxy-2.0.db"));
    }
    #[cfg(target_os = "macos")]
    if let Some(root) = dirs::home_dir() {
        return Some(root.join("Library/Application Support/GOG.com/Galaxy/Storage/galaxy-2.0.db"));
    }
    None
}

pub async fn get(
    State(state): State<AppState>,
    Query(q): Query<PlaytimeQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let fail = |message: &str| (StatusCode::BAD_REQUEST, Json(json!({"error":message})));
    let mut warnings = vec![];
    let records = match q.launcher.as_str() {
        "epic" => {
            let account = q
                .account_id
                .as_deref()
                .ok_or_else(|| fail("Выбери аккаунт Epic"))?;
            let attached = state.epic_store.get_accounts().await.unwrap_or_default();
            if !attached.iter().any(|a| a.account_id == account) {
                return Err(fail("Аккаунт Epic не подключён"));
            }
            let key = format!("playtime:epic:{account}");
            let saved = state.db.steam_cache_get(&key).await.ok().flatten();
            let cache = saved
                .as_ref()
                .and_then(|(data, _)| serde_json::from_str::<Vec<Record>>(data).ok())
                .map(|rows| {
                    rows.into_iter()
                        .filter(|r| {
                            r.launcher == "epic"
                                && r.account_id.as_deref() == Some(account)
                                && r.seconds.is_finite()
                                && r.seconds >= 0.0
                        })
                        .collect::<Vec<_>>()
                });
            if !q.refresh
                && saved
                    .as_ref()
                    .is_some_and(|(_, at)| chrono::Utc::now().timestamp() - at < 600)
                && cache.is_some()
            {
                cache.unwrap_or_default()
            } else {
                let fetch = async {
                    let token = state
                        .epic_store
                        .get_valid_access_token(account)
                        .await
                        .map_err(|_| ())?;
                    let data = state
                        .epic_store
                        .client()
                        .fetch_playtime(&token, account)
                        .await
                        .map_err(|_| ())?;
                    epic_records(&data, account).map_err(|_| ())
                };
                match tokio::time::timeout(std::time::Duration::from_secs(25), fetch).await {
                    Ok(Ok(rows)) => {
                        if let Ok(data) = serde_json::to_string(&rows) {
                            let _ = state.db.steam_cache_put(&key, &data).await;
                        }
                        rows
                    }
                    _ => {
                        warnings.push(
                            "Epic не передал время. Показаны сохранённые данные, если они есть.",
                        );
                        cache
                            .unwrap_or_default()
                            .into_iter()
                            .map(|mut row| {
                                row.cached = true;
                                row
                            })
                            .collect()
                    }
                }
            }
        }
        "minecraft" => {
            let accounts = state.db.get_minecraft_accounts().await.unwrap_or_default();
            let profiles = state.db.get_minecraft_profiles().await.unwrap_or_default();
            let scanner = state.minecraft_manager.scanner();
            let mut roots: Vec<_> = [
                scanner.resolve_minecraft_dir(),
                scanner.resolve_tlauncher_dir(),
            ]
            .into_iter()
            .flatten()
            .collect();
            roots.extend(
                profiles
                    .into_iter()
                    .filter_map(|p| p.game_dir)
                    .map(PathBuf::from),
            );
            tokio::task::spawn_blocking(move || minecraft_records(&roots, &accounts))
                .await
                .unwrap_or_default()
        }
        "gog" => {
            if let Some(path) = galaxy_path().filter(|p| p.is_file()) {
                match tokio::task::spawn_blocking(move || galaxy_records(&path)).await {
                    Ok(Ok(rows)) => rows,
                    _ => {
                        warnings.push("Локальная статистика GALAXY сейчас недоступна.");
                        vec![]
                    }
                }
            } else {
                vec![]
            }
        }
        _ => return Err(fail("Источник игрового времени не поддерживается")),
    };
    Ok(Json(json!({"records":records,"warnings":warnings})))
}
