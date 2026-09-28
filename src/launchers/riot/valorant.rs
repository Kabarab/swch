//! Read-only VALORANT collection for the locally authenticated player.
//! Remote credentials go exclusively to an allowlisted Riot shard; public assets use a separate client.
use super::{
    local::{riot_paths, LocalClient},
    profile::{safe_identity, Item, Playtime, Profile, Section},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
const AGENTS: &str = "01bb38e1-da47-4e6a-9b3d-945fe4655707";
const SKINS: &str = "e7c63390-eda7-46e0-bb7a-a6abdacd2433";
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
async fn public_catalog(http: &reqwest::Client, path: &str) -> Value {
    match http
        .get(format!("https://valorant-api.com/v1/{path}"))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
        _ => Value::Null,
    }
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
    let local = LocalClient::discover(riot_paths())?;
    let tokens = local.get("/entitlements/v1/token").await?;
    let puuid = tokens["subject"]
        .as_str()
        .filter(|s| safe_identity(s))
        .ok_or("Войди в Riot Client.")?;
    let sessions = local.get("/product-session/v1/external-sessions").await?;
    let (shard, version) = session_config(&sessions)?;
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
    let (agents, skins, playtime, alias, xp, agent_catalog, skin_catalog) = tokio::join!(
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
    Ok(Profile {
        account_id: format!("riot_{puuid}"),
        app_id: "valorant".into(),
        display_name,
        level: xp
            .ok()
            .and_then(|v| v.pointer("/Progress/Level").and_then(Value::as_u64)),
        source: "valorant_session".into(),
        synced_at: chrono::Utc::now().to_rfc3339(),
        has_play_history: playtime.value.as_ref().is_some_and(|p| p.seconds > 0),
        playtime,
        champions: Section::from_result(
            agents
                .and_then(|v| owned_ids(&v, AGENTS))
                .map(|ids| collection(&ids, &agent_catalog, false)),
        ),
        skins: Section::from_result(skins.and_then(|v| owned_ids(&v, SKINS)).and_then(|ids| {
            if skin_catalog["data"].as_array().is_none() {
                return Err("Каталог названий скинов недоступен. Повтори обновление.".into());
            }
            Ok(collection(&ids, &skin_catalog, true))
        })),
    })
}
