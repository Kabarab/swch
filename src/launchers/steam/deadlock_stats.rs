//! Character statistics and ranked progress from public, potentially partial data.
use super::game_stats::{account32, parse_deadlock, GameStats, Metric};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RankedRequirements {
    pub season: String,
    pub total_wins: u64,
    pub hero_wins: u64,
    pub heroes: u64,
    pub start: i64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Hero {
    pub hero_id: u64,
    pub metrics: Vec<Metric>,
    pub standard_wins: Option<u64>,
    pub ranked_matches: Option<u64>,
    pub last_rank: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeadlockDetail {
    pub requirements: Option<RankedRequirements>,
    pub standard_wins: Option<u64>,
    pub heroes: Vec<Hero>,
    pub ranked_played: bool,
}
pub fn requirements(value: &Value, now: i64) -> Option<RankedRequirements> {
    value
        .as_array()?
        .iter()
        .filter(|s| s["ranked_type"] == "normal")
        .filter_map(|s| {
            let intervals = s["intervals"].as_array()?;
            if !intervals.iter().any(|i| {
                i["start_timestamp"].as_i64().is_some_and(|t| t <= now)
                    && i["end_timestamp"].as_i64().is_some_and(|t| now < t)
            }) {
                return None;
            }
            Some(RankedRequirements {
                season: s["name"].as_str()?.into(),
                total_wins: s["min_wins"].as_u64()?,
                hero_wins: s["min_hero_wins"].as_u64()?,
                heroes: s["min_hero_unlocks"].as_u64()?,
                start: intervals
                    .iter()
                    .filter_map(|i| i["start_timestamp"].as_i64())
                    .min()?,
            })
        })
        .max_by_key(|s| s.start)
}
fn summaries<'a>(value: &'a Value, account: u64) -> BTreeMap<u64, &'a Value> {
    let mut result = BTreeMap::new();
    for row in value.as_array().into_iter().flatten() {
        if row["account_id"].as_u64() != Some(account) {
            continue;
        }
        let Some(hero) = row["hero_id"].as_u64().filter(|h| *h > 0) else {
            continue;
        };
        let (Some(matches), Some(wins)) = (row["matches_played"].as_u64(), row["wins"].as_u64())
        else {
            continue;
        };
        if wins > matches {
            continue;
        }
        if result
            .get(&hero)
            .is_none_or(|old: &&Value| old["matches_played"].as_u64().unwrap_or(0) < matches)
        {
            result.insert(hero, row);
        }
    }
    result
}
pub fn build(
    id: &str,
    history: &Value,
    normal: &Value,
    standard: &Value,
    seasons: &Value,
    now: i64,
) -> DeadlockDetail {
    let account = account32(id).map(u64::from).unwrap_or(u64::MAX);
    let requirements = requirements(seasons, now);
    let normal = summaries(normal, account);
    let standard = summaries(standard, account);
    let mut seen = BTreeSet::new();
    let history: Vec<_> = history
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| {
            r["account_id"].as_u64() == Some(account)
                && r["match_id"].as_u64().is_some_and(|m| seen.insert(m))
                && r["game_mode"].as_u64() == Some(1)
                && matches!(r["match_mode"].as_u64(), Some(1 | 4))
        })
        .collect();
    let ids: BTreeSet<_> = normal
        .keys()
        .chain(standard.keys())
        .copied()
        .chain(
            history
                .iter()
                .filter_map(|r| r["hero_id"].as_u64().filter(|h| *h > 0)),
        )
        .collect();
    let mut heroes = vec![];
    for hero_id in ids {
        let rows: Vec<_> = history
            .iter()
            .copied()
            .filter(|r| r["hero_id"].as_u64() == Some(hero_id))
            .collect();
        let ranked: Vec<_> = rows
            .iter()
            .filter(|r| {
                r["match_mode"].as_u64() == Some(4)
                    && requirements.as_ref().is_some_and(|s| {
                        r["start_time"]
                            .as_i64()
                            .is_some_and(|t| t >= s.start && t <= now)
                    })
            })
            .collect();
        let mut stats = GameStats::new(id, "1422450");
        parse_deadlock(
            &mut stats,
            &Value::Array(rows.iter().map(|r| (*r).clone()).collect()),
        );
        let metrics = if let Some(row) = normal.get(&hero_id) {
            let total = row["matches_played"].as_u64().unwrap_or(0);
            let wins = row["wins"].as_u64().unwrap_or(0);
            let kills = row["kills"].as_u64();
            let deaths = row["deaths"].as_u64();
            let mut metrics = vec![];
            let mut add = |label: &str, value: Option<Value>, unit: &str| {
                metrics.push(Metric {
                    label: label.into(),
                    value,
                    unit: unit.into(),
                })
            };
            add("Матчи в источнике", Some(json!(total)), "");
            add("Победы", Some(json!(wins)), "");
            add("Поражения", Some(json!(total - wins)), "");
            add(
                "Процент побед",
                (total > 0)
                    .then(|| json!(((wins as f64 / total as f64) * 10000.0).round() / 100.0)),
                "%",
            );
            for (label, key) in [
                ("Убийства", "kills"),
                ("Смерти", "deaths"),
                ("Помощь", "assists"),
            ] {
                add(label, row[key].as_u64().map(Value::from), "");
            }
            add(
                "K/D",
                kills
                    .zip(deaths)
                    .filter(|(_, d)| *d > 0)
                    .map(|(k, d)| json!((k as f64 / d as f64 * 100.0).round() / 100.0)),
                "",
            );
            add(
                "Время по матчам",
                row["time_played"]
                    .as_u64()
                    .map(|n| json!((n as f64 / 3600.0 * 10.0).round() / 10.0)),
                "ч",
            );
            add(
                "Урон героям",
                row["total_player_damage"].as_u64().map(Value::from),
                "",
            );
            metrics
        } else {
            stats.metrics
        };
        let last_rank = ranked
            .iter()
            .filter_map(|r| {
                Some((
                    r["start_time"].as_i64()?,
                    r["ranked_display_badge"]
                        .as_u64()
                        .filter(|n| *n >= 11 && *n <= 116 && (1..=6).contains(&(n % 10)))?,
                ))
            })
            .max_by_key(|(at, _)| *at)
            .map(|(_, rank)| rank);
        heroes.push(Hero {
            hero_id,
            metrics,
            standard_wins: standard.get(&hero_id).and_then(|r| r["wins"].as_u64()),
            ranked_matches: (!rows.is_empty() && requirements.is_some())
                .then_some(ranked.len() as u64),
            last_rank,
        });
    }
    let standard_wins = if standard.is_empty() {
        None
    } else {
        standard
            .values()
            .try_fold(0u64, |n, r| n.checked_add(r["wins"].as_u64()?))
    };
    let ranked_played = heroes
        .iter()
        .any(|h| h.ranked_matches.is_some_and(|n| n > 0));
    DeadlockDetail {
        requirements,
        standard_wins,
        heroes,
        ranked_played,
    }
}
