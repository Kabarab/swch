//! Normalized, credential-free snapshots, isolated by Riot identity and product.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section<T> {
    pub value: Option<T>,
    pub updated_at: Option<String>,
    pub error: Option<String>,
}
impl<T> Section<T> {
    pub fn from_result(result: Result<T, String>) -> Self {
        match result {
            Ok(value) => Self {
                value: Some(value),
                updated_at: Some(chrono::Utc::now().to_rfc3339()),
                error: None,
            },
            Err(error) => Self {
                value: None,
                updated_at: None,
                error: Some(error),
            },
        }
    }
    fn retain_on_error(&mut self, old: Self) {
        if self.value.is_none() {
            self.value = old.value;
            self.updated_at = old.updated_at;
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playtime {
    pub seconds: u64,
    pub matches: usize,
    /// Always a history window, never claimed to be lifetime playtime.
    pub scope: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedInfo {
    pub rank: Option<String>,
    pub rating: Option<u64>,
    pub season_id: Option<String>,
    pub placements_remaining: Option<u64>,
    pub eligibility: Option<bool>,
    pub note: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub account_id: String,
    pub app_id: String,
    pub display_name: String,
    pub level: Option<u64>,
    pub source: String,
    pub synced_at: String,
    pub has_play_history: bool,
    pub champions: Section<Vec<Item>>,
    pub skins: Section<Vec<Item>>,
    pub playtime: Section<Playtime>,
    #[serde(default)]
    pub ranked: Option<Section<RankedInfo>>,
    #[serde(default)]
    pub buddies: Option<Section<Vec<Item>>>,
}

/// A Riot ID is shared across products. Choose the latest valid snapshot,
/// rather than letting an old League name overwrite a renamed VALORANT player.
pub fn latest_display_name<'a>(profiles: &'a [Profile], account_id: &str) -> Option<&'a str> {
    profiles.iter()
        .filter(|p| p.account_id.eq_ignore_ascii_case(account_id))
        .filter(|p| !p.display_name.trim().is_empty() && p.display_name.contains('#'))
        .filter_map(|p| chrono::DateTime::parse_from_rfc3339(&p.synced_at).ok().map(|at| (at, p)))
        .max_by_key(|(at, _)| *at)
        .map(|(_, p)| p.display_name.as_str())
}
impl Profile {
    pub fn retain_failed_sections(&mut self, old: Self) {
        if self.account_id != old.account_id || self.app_id != old.app_id {
            return;
        }
        if !self.display_name.contains('#') && old.display_name.contains('#') {
            self.display_name = old.display_name.clone();
        }
        self.has_play_history |= old.has_play_history;
        self.level = self.level.or(old.level);
        self.champions.retain_on_error(old.champions);
        self.skins.retain_on_error(old.skins);
        self.playtime.retain_on_error(old.playtime);
        if let Some(new) = &mut self.ranked {
            if let Some(previous) = old.ranked { new.retain_on_error(previous); }
        } else { self.ranked = old.ranked; }
        if let Some(new) = &mut self.buddies {
            if let Some(previous) = old.buddies { new.retain_on_error(previous); }
        } else { self.buddies = old.buddies; }
    }
}

pub fn league_items(data: &Value, skins: bool) -> Result<Vec<Item>, String> {
    let rows = data
        .as_array()
        .ok_or("Клиент вернул неизвестный формат коллекции.")?;
    let mut items = BTreeMap::new();
    for row in rows {
        let owned = row
            .pointer("/ownership/owned")
            .and_then(Value::as_bool)
            .ok_or("Клиент не указал владение предметами.")?;
        if !owned {
            continue;
        }
        let id = row["id"].as_u64().ok_or("Нет идентификатора предмета.")?;
        // Base champion appearances are not purchased skins. Rental/free rotation is excluded by owned.
        if skins && (row["isBase"].as_bool() == Some(true) || id % 1000 == 0) {
            continue;
        }
        let name = row["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("#{}", id));
        items.insert(
            id,
            Item {
                id: id.to_string(),
                name,
                image_url: league_image(row).or_else(|| (!skins).then(|| format!(
                    "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/champion-icons/{id}.png"
                ))),
            },
        );
    }
    // The endpoint includes owned mode-specific copies (e.g. Shen 98 and
    // Shen 60098). Sorted IDs keep the original champion and its portrait.
    let mut names = std::collections::HashSet::new();
    Ok(items.into_values().filter(|item| skins || names.insert(item.name.trim().to_lowercase())).collect())
}

/// Skins belonging to the original champion roster; mode copies are separate
/// catalog entries and must not inflate the player's collection.
pub fn league_skins(data: &Value, champions: &Value) -> Result<Vec<Item>, String> {
    let roster: std::collections::HashSet<_> = league_items(champions, false)?
        .into_iter().map(|item| item.id).collect();
    let rows = data.as_array().ok_or("Клиент вернул неизвестный формат коллекции.")?;
    let mut original = Vec::new();
    for row in rows {
        let champion = row["championId"].as_u64().filter(|id| *id > 0)
            .ok_or("Клиент не указал чемпиона для облика.")?;
        if roster.contains(&champion.to_string()) { original.push(row.clone()); }
    }
    league_items(&Value::Array(original), true)
}

pub fn league_playtime(data: &Value) -> Result<Playtime, String> {
    let rows = data
        .pointer("/games/games")
        .and_then(Value::as_array)
        .ok_or("История матчей недоступна.")?;
    let mut matches = BTreeMap::new();
    for row in rows {
        let id = row["gameId"]
            .as_u64()
            .ok_or("Неизвестный формат истории матчей.")?;
        let seconds = row["gameDuration"]
            .as_u64()
            .filter(|s| *s <= 86400)
            .ok_or("Длительность матча недоступна.")?;
        if row["gameMode"].as_str() != Some("TFT") && seconds > 0 {
            matches.insert(id, seconds);
        }
    }
    Ok(Playtime {
        seconds: matches.values().sum(),
        matches: matches.len(),
        scope: "recent_matches".into(),
    })
}

pub fn league_ranked(data: &Value, level: Option<u64>) -> Result<RankedInfo, String> {
    let queues = data["queues"].as_array().ok_or("Рейтинг League of Legends недоступен.")?;
    let solo = queues.iter().find(|row| row["queueType"] == "RANKED_SOLO_5x5");
    let rank = solo.and_then(|row| row["tier"].as_str()).filter(|tier| !tier.is_empty() && *tier != "NONE" && *tier != "UNRANKED")
        .map(|tier| format!("{} {}", tier, solo.and_then(|row| row["division"].as_str()).unwrap_or("")).trim().to_string());
    Ok(RankedInfo {
        eligibility:if rank.is_some() { Some(true) } else { level.map(|v| v >= 30) },
        rating:if rank.is_some() { solo.and_then(|row| row["leaguePoints"].as_u64()) } else { None },
        rank, season_id:None, placements_remaining:None,
        note:"Ранг одиночной/парной очереди. Для рейтинга также нужны чемпионы; ограничения проверяются в игре.".into(),
    })
}

pub fn safe_identity(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn league_image(row: &Value) -> Option<String> {
    let path = row["splashPath"]
        .as_str()
        .or(row["squarePortraitPath"].as_str())?;
    let suffix = path.strip_prefix("/lol-game-data/assets/")?;
    if suffix.contains("..")
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
    {
        return None;
    }
    Some(format!(
        "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/{}",
        suffix.to_ascii_lowercase()
    ))
}
