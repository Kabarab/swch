//! Read-only VALORANT collection for the locally authenticated player.
//! Remote credentials go exclusively to an allowlisted Riot shard; public assets use a separate client.
use super::{
    local::LocalClient,
    profile::{safe_identity, Item, Playtime, Profile, Section, RankedInfo},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
    time::Duration,
};
const AGENTS: &str = "01bb38e1-da47-4e6a-9b3d-945fe4655707";
const SKINS: &str = "e7c63390-eda7-46e0-bb7a-a6abdacd2433";
const BUDDIES: &str = "dd3bf334-87f3-40bd-b043-682a57a8dc3a";
const PLATFORM:&str="ew0KCSJwbGF0Zm9ybVR5cGUiOiAiUEMiLA0KCSJwbGF0Zm9ybU9TIjogIldpbmRvd3MiLA0KCSJwbGF0Zm9ybU9TVmVyc2lvbiI6ICIxMC4wLjE5MDQyLjEuMjU2LjY0Yml0IiwNCgkicGxhdGZvcm1DaGlwc2V0IjogIlVua25vd24iDQp9";

pub fn session_config(sessions: &Value) -> Result<(String, String), String> {
    let rows = sessions.as_object().ok_or("Нет сессии VALORANT.")?;
    for row in rows
        .values()
        .filter(|r| r["productId"] == "valorant" && r["patchlineId"] == "live")
    {
        let Some(args) = row
            .pointer("/launchConfiguration/arguments")
            .and_then(Value::as_array)
        else {
            continue;
        };
        let shard = args
            .iter()
            .filter_map(Value::as_str)
            .find_map(|a| a.strip_prefix("-ares-deployment="));
        if let Some(shard) = shard.filter(|s| ["na", "eu", "ap", "kr", "pbe"].contains(s)) {
            if let Some(version) = row["version"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() < 200 && !s.contains(['\r', '\n']))
            {
                return Ok((shard.into(), version.into()));
            }
        }
    }
    Err("Открой VALORANT, чтобы определить регион и версию клиента.".into())
}

/// Automatically detects VALORANT region/shard and version from Riot Client settings, local API, and remote public version endpoint.
pub async fn resolve_automatic_session_config(local: &LocalClient) -> Result<(String, String), String> {
    // 1. Shard determination
    let shard = resolve_shard(local).await.ok_or("Не удалось автоматически определить регион Riot.")?;

    // 2. Version determination: query valorant-api.com/v1/version
    let version = if let Ok(http) = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(6))
        .build()
    {
        fetch_live_client_version(&http)
            .await
            .ok_or("Не удалось получить текущую версию VALORANT. Проверь подключение и повтори обновление.")?
    } else {
        return Err("Не удалось проверить версию VALORANT.".into());
    };

    Ok((shard, version))
}

pub async fn fetch_live_client_version(http: &reqwest::Client) -> Option<String> {
    let res = http.get("https://valorant-api.com/v1/version").send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    let val: Value = res.json().await.ok()?;
    val.pointer("/data/riotClientVersion")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn map_country_to_shard(country: &str) -> &'static str {
    match country.to_uppercase().as_str() {
        "USA" | "CAN" | "MEX" | "BRA" | "ARG" | "CHL" | "COL" | "PER" | "US" | "CA" | "BR" | "MX" => "na",
        "KOR" | "KR" => "kr",
        "JPN" | "AUS" | "NZL" | "SGP" | "THA" | "VNM" | "PHL" | "IDN" | "MYS" | "TWN" | "HKG" | "JP" | "AU" | "SG" => "ap",
        _ => "eu",
    }
}

pub async fn resolve_shard(local: &LocalClient) -> Option<String> {
    // A. Check Riot Client local APIs
    if let Ok(info) = local.get("/riotclient/region-locale").await {
        if let Some(reg) = info["region"].as_str() {
            let r = reg.to_uppercase();
            if ["NA", "BR", "LATAM"].contains(&r.as_str()) { return Some("na".into()); }
            if r == "KR" { return Some("kr".into()); }
            if ["AP", "JP", "OCE"].contains(&r.as_str()) { return Some("ap".into()); }
            if ["EU", "RU", "TR", "EUNE", "EUW"].contains(&r.as_str()) { return Some("eu".into()); }
        }
    }

    if let Ok(userinfo) = local.get("/rso-auth/v1/authorization/userinfo").await {
        if let Some(country) = userinfo["country"].as_str().or_else(|| userinfo.pointer("/userInfo/country").and_then(Value::as_str)) {
            return Some(map_country_to_shard(country).to_string());
        }
    }

    // B. Check RiotClientSettings.yaml on disk
    let settings_path = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(|p| std::path::PathBuf::from(p).join("Riot Games/Riot Client/Config/RiotClientSettings.yaml"))
    } else {
        dirs::home_dir().map(|p| p.join("Library/Application Support/Riot Games/Riot Client/Config/RiotClientSettings.yaml"))
    };

    if let Some(path) = settings_path {
        if let Ok(content) = std::fs::read_to_string(path) {
            let lines: Vec<&str> = content.lines().collect();
            let mut in_affinity = false;
            let mut in_valorant = false;
            for line in lines {
                let trimmed = line.trim();
                if trimmed.starts_with("player-affinity:") {
                    in_affinity = true;
                    continue;
                }
                if in_affinity && (trimmed.starts_with("valorant:") || trimmed.starts_with("lion:")) {
                    in_valorant = true;
                    continue;
                }
                if in_affinity && in_valorant && trimmed.starts_with("live:") {
                    if let Some(shard) = trimmed.strip_prefix("live:").map(str::trim).map(|s| s.trim_matches('"')) {
                        let clean = shard.to_lowercase();
                        if ["na", "eu", "ap", "kr", "pbe"].contains(&clean.as_str()) {
                            return Some(clean);
                        }
                    }
                }
                if in_affinity && in_valorant && !line.starts_with(' ') && !line.starts_with('\t') {
                    in_valorant = false;
                }
                if in_affinity && trimmed.starts_with("region:") {
                    if let Some(reg) = trimmed.strip_prefix("region:").map(str::trim).map(|s| s.trim_matches('"')) {
                        return Some(map_country_to_shard(reg).to_string());
                    }
                }
            }
        }
    }

    // C. Check RiotClientPrivateSettings.yaml
    let private_path = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(|p| std::path::PathBuf::from(p).join("Riot Games/Riot Client/Data/RiotClientPrivateSettings.yaml"))
    } else {
        dirs::home_dir().map(|p| p.join("Library/Application Support/Riot Games/Riot Client/Data/RiotClientPrivateSettings.yaml"))
    };

    if let Some(path) = private_path {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("region:") {
                    if let Some(reg) = trimmed.strip_prefix("region:").map(str::trim).map(|s| s.trim_matches('"')) {
                        return Some(map_country_to_shard(reg).to_string());
                    }
                }
            }
        }
    }

    None
}

struct Remote {
    http: reqwest::Client,
    base: String,
    access: String,
    entitlement: String,
    version: String,
}
impl Remote {
    async fn get(&self, path: &str) -> Result<Value, String> {
        let response = self
            .http
            .get(format!("{}{}", self.base, path))
            .bearer_auth(&self.access)
            .header("X-Riot-Entitlements-JWT", &self.entitlement)
            .header("X-Riot-ClientVersion", &self.version)
            .header("X-Riot-ClientPlatform", PLATFORM)
            .send()
            .await
            .map_err(|_| "Сервис VALORANT не отвечает.")?;
        if !response.status().is_success() {
            return Err(format!(
                "Данные VALORANT недоступны ({}).",
                response.status().as_u16()
            ));
        }
        response
            .json()
            .await
            .map_err(|_| "Неизвестный формат данных VALORANT.".into())
    }
}
pub fn owned_ids(data: &Value, category: &str) -> Result<BTreeSet<String>, String> {
    let rows = if let Some(rows) = data["Entitlements"].as_array() {
        rows.clone()
    } else {
        let groups = data["EntitlementsByTypes"]
            .as_array()
            .ok_or("Коллекция VALORANT недоступна.")?;
        groups
            .iter()
            .filter(|g| g["ItemTypeID"] == category)
            .map(|g| {
                g["Entitlements"]
                    .as_array()
                    .cloned()
                    .ok_or("Неизвестный формат коллекции.")
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect()
    };
    rows.iter()
        .map(|row| {
            row["ItemID"]
                .as_str()
                .filter(|s| safe_identity(s))
                .map(str::to_owned)
                .ok_or("Нет ID предмета.".into())
        })
        .collect()
}

/// Every skin level is mapped to its parent skin so upgrades do not inflate the skin count.
pub fn collection(ids: &BTreeSet<String>, catalog: &Value, skins: bool) -> Vec<Item> {
    let mut result = BTreeMap::new();
    let mut matched = BTreeSet::new();
    for row in catalog["data"].as_array().into_iter().flatten() {
        let Some(id) = row["uuid"].as_str() else {
            continue;
        };
        let level_ids: Vec<_> = row["levels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["uuid"].as_str())
            .collect();
        let owned = ids.contains(id) || skins && level_ids.iter().any(|id| ids.contains(*id));
        if !owned {
            continue;
        }
        matched.insert(id.to_owned());
        for level in level_ids {
            matched.insert(level.into());
        }
        let image = row["displayIcon"]
            .as_str()
            .or_else(|| row["levels"].as_array().and_then(|levels| levels.iter().find_map(|level| level["displayIcon"].as_str())))
            .filter(|u| u.starts_with("https://media.valorant-api.com/"));
        result.insert(
            id.to_owned(),
            Item {
                id: id.into(),
                name: row["displayName"].as_str().unwrap_or(id).into(),
                image_url: image.map(str::to_owned),
            },
        );
    }
    for id in ids.difference(&matched) {
        result.insert(
            id.clone(),
            Item {
                id: id.clone(),
                name: format!("Предмет {id}"),
                image_url: None,
            },
        );
    }
    result.into_values().collect()
}

/// Buddy entitlements refer to levels; multiple copies belong to one design.
pub fn buddy_collection(data: &Value, catalog: &Value) -> Result<Vec<Item>, String> {
    if catalog["data"].as_array().is_none() {
        return Err("Каталог брелоков недоступен. Повтори обновление.".into());
    }
    Ok(collection(&owned_ids(data, BUDDIES)?, catalog, true))
}

/// The five starter agents are available without a store entitlement.
pub fn available_agents(ids: &BTreeSet<String>, catalog: &Value) -> Result<Vec<Item>, String> {
    let rows = catalog["data"].as_array().ok_or("Каталог агентов недоступен.")?;
    let mut available = ids.clone();
    for row in rows {
        if ["Brimstone", "Jett", "Phoenix", "Sage", "Sova"].contains(&row["displayName"].as_str().unwrap_or("")) {
            if let Some(id) = row["uuid"].as_str() { available.insert(id.into()); }
        }
    }
    Ok(collection(&available, catalog, false))
}

/// Read only the current act: a rank from a previous act is never presented as current.
pub fn ranked_info(mmr: &Value, seasons: &Value, tiers: &Value, level: Option<u64>, now: chrono::DateTime<chrono::Utc>) -> Result<RankedInfo, String> {
    let queue = mmr.pointer("/QueueSkills/competitive").filter(|v| v.is_object()).ok_or("Рейтинг VALORANT недоступен.")?;
    let season = seasons["data"].as_array().and_then(|rows| rows.iter().filter(|row| {
        let start = row["startTime"].as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());
        let end = row["endTime"].as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());
        start.zip(end).is_some_and(|(start,end)| start <= now && now < end) && row["borders"].is_array()
    }).max_by_key(|row| row["startTime"].as_str().unwrap_or(""))).ok_or("Не удалось определить текущий рейтинговый акт.")?;
    let season_id = season["seasonUuid"].as_str().ok_or("Неизвестный рейтинговый акт.")?;
    let rows = queue["SeasonalInfoBySeasonID"].as_object().ok_or("Рейтинг VALORANT недоступен.")?;
    let current = rows.get(season_id);
    let tier = current.and_then(|v| v["CompetitiveTier"].as_u64()).unwrap_or(0);
    let has_history = rows.values().any(|row| row["NumberOfGames"].as_u64().is_some_and(|v| v > 0));
    let rank = if tier > 2 {
        Some(tiers["data"].as_array().into_iter().flatten()
            .find(|row| row["uuid"] == season["competitiveTiersUuid"])
            .and_then(|row| row["tiers"].as_array())
            .and_then(|rows| rows.iter().find(|row| row["tier"].as_u64() == Some(tier)))
            .and_then(|row| row["tierName"].as_str()).ok_or("Название ранга недоступно.")?.into())
    } else { None };
    Ok(RankedInfo {
        rating: if rank.is_some() { current.and_then(|v| v["RankedRating"].as_u64()) } else { None },
        rank, season_id:Some(season_id.into()),
        placements_remaining:queue["CurrentSeasonGamesNeededForRating"].as_u64(),
        eligibility:if has_history { Some(true) } else { level.map(|v| v >= 20) },
        note:"Доступ по уровню и истории рейтинговых матчей. Ограничения и требования подтверждения входа проверяются в игре.".into(),
    })
}
async fn public_catalog(http: &reqwest::Client, path: &str) -> Value {
    static CATALOGS: OnceLock<Mutex<BTreeMap<String, Value>>> = OnceLock::new();
    let cache = CATALOGS.get_or_init(|| Mutex::new(BTreeMap::new()));
    // The skin catalog is substantially larger than the authenticated responses.
    // Retry a temporary failure and retain successful public catalogs between accounts.
    for _ in 0..2 {
        if let Ok(response) = http
            .get(format!("https://valorant-api.com/v1/{path}"))
            .timeout(Duration::from_secs(12))
            .send()
            .await
        {
            if response.status().is_success() {
                if let Ok(value) = response.json::<Value>().await {
                    if value["data"].as_array().is_some_and(|rows| !rows.is_empty()) {
                        if let Ok(mut entries) = cache.lock() {
                            entries.insert(path.into(), value.clone());
                        }
                        return value;
                    }
                }
            }
        }
    }
    cache.lock().ok().and_then(|entries| entries.get(path).cloned()).unwrap_or(Value::Null)
}
async fn history(remote: &Remote, puuid: &str) -> Result<Playtime, String> {
    let history = remote
        .get(&format!(
            "/match-history/v1/history/{puuid}?startIndex=0&endIndex=10"
        ))
        .await?;
    let rows = history["History"]
        .as_array()
        .ok_or("История VALORANT недоступна.")?;
    let mut seen = BTreeSet::new();
    let mut seconds = 0;
    for row in rows.iter().take(10) {
        let id = row["MatchID"]
            .as_str()
            .filter(|s| safe_identity(s))
            .ok_or("Неизвестный формат матча.")?;
        if !seen.insert(id.to_owned()) {
            continue;
        }
        let detail = remote
            .get(&format!("/match-details/v1/matches/{id}"))
            .await?;
        // Never attribute another player's match to the current account.
        if !detail["players"]
            .as_array()
            .is_some_and(|p| p.iter().any(|p| p["subject"] == puuid))
        {
            return Err("Не удалось подтвердить участника матча.".into());
        }
        seconds += detail
            .pointer("/matchInfo/gameLengthMillis")
            .and_then(Value::as_u64)
            .filter(|v| *v <= 86400000)
            .ok_or("Длительность матча неизвестна.")?
            / 1000;
    }
    Ok(Playtime {
        seconds,
        matches: seen.len(),
        scope: "recent_matches".into(),
    })
}

pub async fn sync() -> Result<Profile, String> {
    let local = LocalClient::ensure_riot_ready().await?;
    let tokens = local.get("/entitlements/v1/token").await?;
    let puuid = tokens["subject"]
        .as_str()
        .filter(|s| safe_identity(s))
        .ok_or("Войди в Riot Client.")?;
    let sessions = local.get("/product-session/v1/external-sessions").await.unwrap_or(Value::Null);
    let (shard, version) = match session_config(&sessions) {
        Ok(cfg) => cfg,
        Err(_) => resolve_automatic_session_config(&local).await?,
    };
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|_| "Ошибка подключения.")?;
    let remote = Remote {
        http: http.clone(),
        base: format!("https://pd.{shard}.a.pvp.net"),
        access: tokens["accessToken"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Сессия Riot истекла.")?
            .into(),
        entitlement: tokens["token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Сессия Riot истекла.")?
            .into(),
        version,
    };
    let (agents, skins, playtime, alias, xp, agent_catalog, skin_catalog, mmr, seasons, tiers, buddies, buddy_catalog) = tokio::join!(
        async {
            remote
                .get(&format!("/store/v1/entitlements/{puuid}/{AGENTS}"))
                .await
        },
        async {
            remote
                .get(&format!("/store/v1/entitlements/{puuid}/{SKINS}"))
                .await
        },
        tokio::time::timeout(Duration::from_secs(25), history(&remote, puuid)),
        local.get("/player-account/aliases/v1/active"),
        async { remote.get(&format!("/account-xp/v1/players/{puuid}")).await },
        public_catalog(&http, "agents?isPlayableCharacter=true"),
        public_catalog(&http, "weapons/skins"),
        async { remote.get(&format!("/mmr/v1/players/{puuid}")).await },
        public_catalog(&http, "seasons/competitive"),
        public_catalog(&http, "competitivetiers?language=ru-RU"),
        async { remote.get(&format!("/store/v1/entitlements/{puuid}/{BUDDIES}")).await },
        public_catalog(&http, "buddies"),
    );
    let after = local.get("/entitlements/v1/token").await?;
    if after["subject"] != tokens["subject"] {
        return Err("Аккаунт изменился во время загрузки. Повтори синхронизацию.".into());
    }
    let alias = alias.unwrap_or(Value::Null);
    let display_name = match (alias["game_name"].as_str(), alias["tag_line"].as_str()) {
        (Some(name), Some(tag)) => format!("{name}#{tag}"),
        _ => format!("Riot {}", puuid.chars().take(8).collect::<String>()),
    };
    let playtime = Section::from_result(
        playtime
            .unwrap_or_else(|_| Err("История не успела загрузиться. Повтори обновление.".into())),
    );
    let level = xp.ok().and_then(|v| v.pointer("/Progress/Level").and_then(Value::as_u64));
    let ranked = Section::from_result(mmr.and_then(|data| ranked_info(&data, &seasons, &tiers, level, chrono::Utc::now())));
    Ok(Profile {
        account_id: format!("riot_{puuid}"),
        app_id: "valorant".into(),
        display_name,
        level,
        source: "valorant_session".into(),
        synced_at: chrono::Utc::now().to_rfc3339(),
        has_play_history: playtime.value.as_ref().is_some_and(|p| p.seconds > 0),
        playtime,
        ranked:Some(ranked),
        buddies:Some(Section::from_result(buddies.and_then(|data| buddy_collection(&data, &buddy_catalog)))),
        champions: Section::from_result(
            agents
                .and_then(|v| owned_ids(&v, AGENTS))
                .and_then(|ids| available_agents(&ids, &agent_catalog)),
        ),
        skins: Section::from_result(skins.and_then(|v| owned_ids(&v, SKINS)).and_then(|ids| {
            if skin_catalog["data"].as_array().is_none() {
                return Err("Каталог названий скинов недоступен. Повтори обновление.".into());
            }
            Ok(collection(&ids, &skin_catalog, true))
        })),
    })
}
