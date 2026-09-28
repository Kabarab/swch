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
}
impl Profile {
    pub fn retain_failed_sections(&mut self, old: Self) {
        if self.account_id != old.account_id || self.app_id != old.app_id {
            return;
        }
        self.has_play_history |= old.has_play_history;
        self.level = self.level.or(old.level);
        self.champions.retain_on_error(old.champions);
        self.skins.retain_on_error(old.skins);
        self.playtime.retain_on_error(old.playtime);
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
                image_url: league_image(row),
            },
        );
    }
    Ok(items.into_values().collect())
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
