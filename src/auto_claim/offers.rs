use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// A temporary giveaway of a normally paid base game. Prices are minor units.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Offer {
    pub launcher: String,
    pub id: String,
    pub namespace: String,
    pub title: String,
    pub url: String,
    pub original_price: Option<i64>,
    pub normally_paid: bool,
    pub final_price: i64,
    pub ends_at: Option<i64>,
    pub item_ids: Vec<String>,
}

impl Offer {
    pub fn eligible(&self, now: i64) -> bool {
        self.normally_paid
            && self.original_price.is_none_or(|price| price > 0)
            && self.final_price == 0
            && self.ends_at.is_none_or(|end| end > now)
            && !self.id.is_empty()
            && !self.title.is_empty()
    }
    pub fn key(&self) -> String {
        if self.namespace.is_empty() {
            self.id.clone()
        } else {
            format!("{}:{}", self.namespace, self.id)
        }
    }
}

fn timestamp(v: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|v| v.timestamp())
}
pub fn epic_offers(data: &Value, now: i64) -> Result<Vec<Offer>, String> {
    let elements = data
        .pointer("/data/Catalog/searchStore/elements")
        .and_then(Value::as_array)
        .ok_or("Epic вернул неизвестный формат списка раздач.")?;
    let mut result = vec![];
    let mut seen = BTreeSet::new();
    for game in elements {
        // DLC, currencies, subscriptions, demos and permanently free games are excluded.
        let kind = game["offerType"].as_str().unwrap_or_default();
        if !matches!(kind, "BASE_GAME" | "BUNDLE") {
            continue;
        }
        let original = game
            .pointer("/price/totalPrice/originalPrice")
            .and_then(Value::as_i64);
        let final_price = game
            .pointer("/price/totalPrice/discountPrice")
            .and_then(Value::as_i64);
        if !original.is_some_and(|v| v > 0) || final_price != Some(0) {
            continue;
        }
        let Some(groups) = game
            .pointer("/promotions/promotionalOffers")
            .and_then(Value::as_array)
        else {
            continue;
        };
        let end = groups.iter().filter_map(|g| g["promotionalOffers"].as_array()).flatten()
            .filter(|p| p.pointer("/discountSetting/discountType").and_then(Value::as_str) == Some("PERCENTAGE")
                // In this API 0 means the percentage of price left to pay.
                && p.pointer("/discountSetting/discountPercentage").and_then(Value::as_i64) == Some(0)
                && timestamp(&p["startDate"]).is_some_and(|t| t <= now)
                && timestamp(&p["endDate"]).is_some_and(|t| t > now))
            .filter_map(|p| timestamp(&p["endDate"])).min();
        let Some(end) = end else {
            continue;
        };
        let slug = game
            .pointer("/catalogNs/mappings/0/pageSlug")
            .and_then(Value::as_str)
            .or_else(|| game["productSlug"].as_str())
            .unwrap_or_default();
        let id = game["id"].as_str().unwrap_or_default();
        let ns = game["namespace"].as_str().unwrap_or_default();
        let item_ids: Vec<String> = game["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["id"].as_str().map(str::to_string))
            .collect();
        if !identifier(id) || !identifier(ns) || item_ids.is_empty() {
            continue;
        }
        let offer = Offer {
            launcher: "epic".into(),
            id: id.into(),
            namespace: ns.into(),
            title: game["title"].as_str().unwrap_or_default().into(),
            url: if slug.is_empty() {
                "https://store.epicgames.com/free-games".into()
            } else {
                format!(
                    "https://store.epicgames.com/en-US/p/{}",
                    url_component(slug)
                )
            },
            original_price: original,
            normally_paid: true,
            final_price: 0,
            ends_at: Some(end),
            item_ids,
        };
        if offer.eligible(now) && seen.insert(offer.key()) {
            result.push(offer);
        }
    }
    Ok(result)
}

pub fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
pub fn url_component(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
pub fn attr<'a>(html: &'a str, name: &str) -> Option<&'a str> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some((_, tail)) = html.split_once(&needle) {
            return tail.split(quote).next();
        }
    }
    None
}
pub fn search_app_ids(html: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for row in html.split("<a ").skip(1) {
        let tag = row.split('>').next().unwrap_or_default();
        if let Some(id) = attr(tag, "data-ds-appid") {
            if crate::launchers::steam::library::valid_app_id(id)
                && (row.contains("-100%") || row.contains("data-discount=\"100\""))
            {
                ids.insert(id.to_string());
            }
        }
    }
    ids.into_iter().collect()
}

/// The actual free license must be in the discounted purchase block, not a demo
/// elsewhere on the app page. A paid checkout endpoint is never used for Steam.
pub fn steam_offer(app_id: &str, data: &Value, html: &str, now: i64) -> Option<Offer> {
    if data["type"].as_str() != Some("game")
        || !crate::launchers::steam::library::valid_app_id(app_id)
    {
        return None;
    }
    for block in html.split("game_area_purchase_game_wrapper").skip(1) {
        let lower = block.to_ascii_lowercase();
        if lower.contains("free_weekend") || lower.contains("free weekend") {
            continue;
        }
        let price = &data["price_overview"];
        let original = price["initial"].as_i64().filter(|v| *v > 0);
        let json_discount = original.is_some()
            && price["final"].as_i64() == Some(0)
            && price["discount_percent"].as_i64() == Some(100);
        let marked_discount = block.contains("-100%") || block.contains("data-discount=\"100\"");
        let original_text = block
            .split_once("discount_original_price")
            .and_then(|(_, tail)| tail.split_once('>'))
            .and_then(|(_, tail)| tail.split('<').next())
            .unwrap_or_default();
        let html_discount = marked_discount
            && original_text.bytes().any(|b| matches!(b, b'1'..=b'9'))
            && (lower.contains("free to keep") || lower.contains("free_to_keep"));
        if !marked_discount || (!json_discount && !html_discount) {
            continue;
        }
        let Some((_, tail)) = block.split_once("AddFreeLicense(") else {
            continue;
        };
        let tail = tail.trim_start();
        let package: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if !crate::launchers::steam::library::valid_app_id(&package) {
            continue;
        }
        let end = attr(block, "data-discount-end").and_then(|s| s.parse::<i64>().ok());
        let offer = Offer {
            launcher: "steam".into(),
            id: package,
            namespace: String::new(),
            title: data["name"].as_str()?.into(),
            url: format!("https://store.steampowered.com/app/{app_id}/"),
            // Some free-to-keep packages omit app price_overview. The original
            // price and 100% discount are then supplied by the purchase block.
            original_price: original,
            normally_paid: true,
            final_price: 0,
            ends_at: end,
            item_ids: vec![app_id.into()],
        };
        if offer.eligible(now) {
            return Some(offer);
        }
    }
    None
}
