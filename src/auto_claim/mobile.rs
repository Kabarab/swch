//! Mobile SKUs remain separate from PC. The reference discovers zero-price
//! Claim entries; SWCH also requires positive regular price, an expiry and
//! catalog item IDs so a PC receipt cannot stand in for a mobile licence.
use super::offers::Offer;
use chrono::DateTime;
use serde_json::Value;
use std::collections::BTreeSet;
fn time(v: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|v| v.timestamp())
}
fn id(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    (!s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)))
    .then(|| s.to_string())
}
pub fn offers(data: &Value, platform: &str, now: i64) -> Result<Vec<Offer>, String> {
    if !matches!(platform, "android" | "ios") {
        return Err("Неизвестная мобильная платформа Epic.".into());
    }
    let sections = data["data"]
        .as_array()
        .ok_or("Epic Mobile не прислал каталог раздач.".to_string())?;
    let mut output = vec![];
    let mut seen = BTreeSet::new();
    for section in sections.iter().filter(|s| s["type"] == "freeGame") {
        for raw in section["offers"].as_array().into_iter().flatten() {
            let c = &raw["content"];
            if ["isFreeToPlay", "isDemo", "isTrial"]
                .iter()
                .any(|f| c[*f].as_bool() == Some(true))
            {
                continue;
            }
            let purchase = c["purchase"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            let Some(free) = purchase.iter().find(|p| {
                p["purchaseType"] == "Claim"
                    && p.pointer("/price/decimalPrice").and_then(Value::as_f64) == Some(0.0)
            }) else {
                continue;
            };
            let original = free
                .pointer("/price/originalPrice")
                .and_then(Value::as_f64)
                .or_else(|| {
                    purchase
                        .iter()
                        .filter(|p| p["purchaseType"] == "Purchase")
                        .filter_map(|p| p.pointer("/price/decimalPrice").and_then(Value::as_f64))
                        .find(|p| *p > 0.0)
                });
            let Some(original) =
                original.filter(|p| p.is_finite() && *p > 0.0 && *p <= 1_000_000.0)
            else {
                continue;
            };
            let Some(end) = [
                &free["endDate"],
                &free["expiryDate"],
                &c["endDate"],
                &c["expiryDate"],
            ]
            .iter()
            .filter_map(|v| time(v))
            .min()
            .filter(|e| *e > now) else {
                continue;
            };
            if [&free["startDate"], &c["startDate"]]
                .iter()
                .filter_map(|v| time(v))
                .any(|t| t > now)
            {
                continue;
            }
            let Some(offer_id) = id(&free["offerId"]).or_else(|| id(&c["id"])) else {
                continue;
            };
            let Some(namespace) = id(&free["namespace"]).or_else(|| id(&c["namespace"])) else {
                continue;
            };
            let item_ids = free["items"]
                .as_array()
                .or_else(|| c["items"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| id(&v["id"]))
                .collect::<Vec<_>>();
            if item_ids.is_empty() {
                continue;
            }
            let Some(slug) = c
                .pointer("/mapping/slug")
                .and_then(Value::as_str)
                .filter(|s| {
                    !s.is_empty()
                        && s.len() < 256
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                })
            else {
                continue;
            };
            let Some(title) = c["title"]
                .as_str()
                .filter(|s| !s.trim().is_empty() && s.len() <= 1024)
            else {
                continue;
            };
            let ns = format!("mobile:{platform}:{namespace}");
            if !seen.insert((ns.clone(), offer_id.clone())) {
                continue;
            }
            output.push(Offer {
                launcher: "epic".into(),
                id: offer_id,
                namespace: ns,
                title: title.into(),
                url: format!("https://store.epicgames.com/en-US/p/{slug}"),
                original_price: Some((original * 100.0).round() as i64),
                normally_paid: true,
                final_price: 0,
                ends_at: Some(end),
                item_ids,
            });
        }
    }
    Ok(output)
}
