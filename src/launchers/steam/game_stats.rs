//! Read-only statistics. Missing and private data never become zeroes.
use super::library::{valid_steam_id, STEAM_ID_BASE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Metric {
    pub label: String,
    pub value: Option<Value>,
    pub unit: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GameStats {
    pub account_id: String,
    pub app_id: String,
    pub status: String,
    pub source: String,
    pub source_url: String,
    pub fetched_at: String,
    pub cached: bool,
    pub stale: bool,
    pub metrics: Vec<Metric>,
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadlock: Option<super::deadlock_stats::DeadlockDetail>,
}
impl GameStats {
    pub fn new(id: &str, app: &str) -> Self {
        let (source, url, note) = match app {
            "730" => ("Steam Web API", format!("https://steamcommunity.com/profiles/{id}/stats/CSGO"), "Счётчики Steam для CS2 / CS:GO могут включать старые данные CS:GO. Premier-рейтинг, звание, Prime и Trust Factor этот источник не предоставляет."),
            "570" => ("OpenDota", format!("https://www.opendota.com/players/{}", account32(id).unwrap_or(0)), "Статистика по матчам, известным OpenDota; история может быть неполной. Ранг — последний известный источнику, не точный MMR. Для обновления включи публичную историю матчей в Dota 2."),
            _ => ("Deadlock API", "https://api.deadlock-api.com/docs".into(), "Статистика по доступной истории Deadlock API, включая разные режимы; история может быть неполной. Победы определяются по победившей команде; процент побед учитывает только матчи с известным результатом."),
        };
        Self {
            account_id: id.into(),
            app_id: app.into(),
            status: "unavailable".into(),
            source: source.into(),
            source_url: url,
            fetched_at: chrono::Utc::now().to_rfc3339(),
            cached: false,
            stale: false,
            metrics: vec![],
            notes: vec![note.into()],
            deadlock: None,
        }
    }
    fn metric(&mut self, label: &str, value: Option<Value>, unit: &str) {
        self.metrics.push(Metric {
            label: label.into(),
            value,
            unit: unit.into(),
        });
    }
    fn finish(&mut self) {
        self.status = if self.metrics.iter().any(|m| m.value.is_some()) {
            "available"
        } else {
            "unavailable"
        }
        .into();
        if self.status == "unavailable" {
            self.notes.push("Источник не вернул статистику: профиль может быть закрыт или история ещё не собрана.".into());
        }
    }
}
pub fn supported(app: &str) -> bool {
    matches!(app, "730" | "570" | "1422450")
}
pub fn account32(id: &str) -> Option<u32> {
    if !valid_steam_id(id) {
        return None;
    }
    u32::try_from(id.parse::<u64>().ok()?.checked_sub(STEAM_ID_BASE)?).ok()
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite() && *n >= 0.0)
}
fn ratio(a: Option<f64>, b: Option<f64>, scale: f64) -> Option<Value> {
    let (a, b) = (a?, b?);
    (b > 0.0).then(|| Value::from((a / b * scale * 100.0).round() / 100.0))
}
pub fn parse_cs(out: &mut GameStats, value: &Value) {
    let stats: HashMap<&str, f64> = value["playerstats"]["stats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some((s["name"].as_str()?, number(&s["value"])?)))
        .collect();
    let get = |key| stats.get(key).copied();
    for (label, key) in [
        ("Убийства", "total_kills"),
        ("Смерти", "total_deaths"),
        ("Матчи", "total_matches_played"),
        ("Победы в матчах", "total_matches_won"),
        ("MVP", "total_mvps"),
    ] {
        out.metric(label, get(key).map(Value::from), "");
    }
    out.metric(
        "K/D",
        ratio(get("total_kills"), get("total_deaths"), 1.0),
        "",
    );
    out.metric(
        "Попадания в голову",
        ratio(get("total_kills_headshot"), get("total_kills"), 100.0),
        "%",
    );
    out.metric(
        "Точность",
        ratio(get("total_shots_hit"), get("total_shots_fired"), 100.0),
        "%",
    );
    out.finish();
}
pub fn parse_dota(out: &mut GameStats, profile: &Value, wl: &Value) {
    let ranks = [
        "",
        "Рекрут",
        "Страж",
        "Рыцарь",
        "Герой",
        "Легенда",
        "Властелин",
        "Божество",
        "Титан",
    ];
    let rank = profile["rank_tier"].as_u64().and_then(|n| {
        let tier = n / 10;
        let stars = n % 10;
        if tier == 8 {
            Some(Value::from("Титан"))
        } else if (1..=7).contains(&tier) && (1..=5).contains(&stars) {
            Some(Value::from(format!("{} {}", ranks[tier as usize], stars)))
        } else {
            None
        }
    });
    out.metric("Ранг", rank, "");
    out.metric(
        "Место в рейтинге",
        profile["leaderboard_rank"]
            .as_u64()
            .filter(|n| *n > 0)
            .map(Value::from),
        "",
    );
    let wins = number(&wl["win"]);
    let losses = number(&wl["lose"]);
    let total = wins.zip(losses).map(|(w, l)| w + l);
    // An empty OpenDota history is not proof that the account has never played.
    let has_history = total.is_some_and(|n| n > 0.0);
    out.metric(
        "Матчи в OpenDota",
        total.filter(|_| has_history).map(Value::from),
        "",
    );
    out.metric("Победы", wins.filter(|_| has_history).map(Value::from), "");
    out.metric(
        "Поражения",
        losses.filter(|_| has_history).map(Value::from),
        "",
    );
    out.metric("Процент побед", ratio(wins, total, 100.0), "%");
    out.finish();
}
pub fn parse_deadlock(out: &mut GameStats, value: &Value) {
    let mut seen = HashSet::new();
    let id = account32(&out.account_id).map(u64::from);
    let rows: Vec<&Value> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| {
            m["account_id"].as_u64() == id && m["match_id"].as_u64().is_some_and(|n| seen.insert(n))
        })
        .collect();
    // match_result is the winning team. player_match_outcome describes personal
    // scoring/penalties and is often invalid for older, otherwise completed matches.
    let results: Vec<bool> = rows
        .iter()
        .filter_map(|m| {
            let team = m["player_team"].as_u64().filter(|n| *n <= 1)?;
            let winner = m["match_result"].as_u64().filter(|n| *n <= 1)?;
            Some(team == winner)
        })
        .collect();
    let wins = results.iter().filter(|won| **won).count();
    let losses = results.len() - wins;
    let unknown = rows.len() - results.len();
    let nonempty = !rows.is_empty();
    let outcomes_known = !results.is_empty();
    out.metric(
        "Матчи в источнике",
        nonempty.then(|| Value::from(rows.len())),
        "",
    );
    out.metric("Победы", outcomes_known.then(|| Value::from(wins)), "");
    out.metric("Поражения", outcomes_known.then(|| Value::from(losses)), "");
    out.metric(
        "Процент побед",
        ratio(Some(wins as f64), Some(results.len() as f64), 100.0),
        "%",
    );
    if unknown > 0 {
        out.metric("Без результата", Some(Value::from(unknown)), "");
        out.notes.push(format!("У {unknown} матчей неизвестен результат. Они исключены из подсчёта побед, поражений и процента побед."));
    }
    let sum = |key: &str| -> Option<f64> {
        if !nonempty {
            return None;
        }
        rows.iter().map(|r| number(&r[key])).sum()
    };
    let kills = sum("player_kills");
    let deaths = sum("player_deaths");
    let assists = sum("player_assists");
    out.metric("Убийства", kills.map(Value::from), "");
    out.metric("Смерти", deaths.map(Value::from), "");
    out.metric("Помощь", assists.map(Value::from), "");
    out.metric("K/D", ratio(kills, deaths, 1.0), "");
    out.finish();
    if unknown > 0 {
        out.status = "partial".into();
    }
}

pub struct StatsClient {
    client: reqwest::Client,
    steam: String,
    dota: String,
    deadlock: String,
}
impl Default for StatsClient {
    fn default() -> Self {
        Self::new(
            "https://api.steampowered.com",
            "https://api.opendota.com/api",
            "https://api.deadlock-api.com",
        )
    }
}
impl StatsClient {
    pub fn new(steam: &str, dota: &str, deadlock: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("SWCH/0.1")
                .build()
                .expect("HTTP client"),
            steam: steam.into(),
            dota: dota.into(),
            deadlock: deadlock.into(),
        }
    }
    async fn get(&self, url: String, query: &[(&str, &str)]) -> Result<Value, String> {
        let res = self
            .client
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(|_| "Не удалось связаться с источником. Повтори позже.".to_string())?;
        match res.status().as_u16() {
            200 => res
                .json()
                .await
                .map_err(|_| "Источник вернул неподдерживаемый ответ.".into()),
            401 | 403 => {
                Err("Источник ограничил доступ: проверь публичность профиля и ключ API.".into())
            }
            429 => {
                Err("Источник ограничил частоту запросов. Повтори через несколько минут.".into())
            }
            _ => Err("Статистика недоступна у источника. Повтори позже.".into()),
        }
    }
    pub async fn fetch(&self, id: &str, app: &str, key: Option<&str>) -> GameStats {
        let mut out = GameStats::new(id, app);
        let Some(account) = account32(id).filter(|_| supported(app)) else {
            out.notes
                .push("Некорректный аккаунт или неподдерживаемая игра.".into());
            return out;
        };
        match app {
            "730" => {
                let Some(key) = key.filter(|s| !s.trim().is_empty()) else {
                    out.status = "needs_key".into();
                    out.notes.push("Для статистики CS2 нужен ключ Steam Web API. Введи его ниже — ключ используется только для этого запроса и не сохраняется.".into());
                    return out;
                };
                match self
                    .get(
                        format!("{}/ISteamUserStats/GetUserStatsForGame/v2/", self.steam),
                        &[("steamid", id), ("appid", "730"), ("key", key)],
                    )
                    .await
                {
                    Ok(v) => parse_cs(&mut out, &v),
                    Err(e) => out.notes.push(e),
                }
            }
            "570" => {
                let (p, w) = tokio::join!(
                    self.get(format!("{}/players/{account}", self.dota), &[]),
                    self.get(format!("{}/players/{account}/wl", self.dota), &[])
                );
                for e in [p.as_ref().err(), w.as_ref().err()].into_iter().flatten() {
                    if !out.notes.contains(e) {
                        out.notes.push(e.clone());
                    }
                }
                let partial = p.is_err() || w.is_err();
                parse_dota(
                    &mut out,
                    &p.unwrap_or(Value::Null),
                    &w.unwrap_or(Value::Null),
                );
                if partial && out.status == "available" {
                    out.status = "partial".into();
                }
            }
            _ => {
                let account=account.to_string();
                let normal_query=[("account_ids",account.as_str()),("game_mode","normal"),("match_mode","unranked,ranked")];
                let standard_query=[("account_ids",account.as_str()),("game_mode","normal"),("match_mode","unranked")];
                let (history,normal,standard,seasons)=tokio::join!(
                    self.get(format!("{}/v1/players/{account}/match-history",self.deadlock),&[]),
                    self.get(format!("{}/v1/players/hero-stats",self.deadlock),&normal_query),
                    self.get(format!("{}/v1/players/hero-stats",self.deadlock),&standard_query),
                    self.get(format!("{}/v1/assets/ranked-seasons",self.deadlock),&[])
                );
                for error in [history.as_ref().err(),normal.as_ref().err(),standard.as_ref().err(),seasons.as_ref().err()].into_iter().flatten() {
                    if !out.notes.contains(error){out.notes.push(error.clone());}
                }
                if let Ok(value)=&history {parse_deadlock(&mut out,value);}
                let detail=super::deadlock_stats::build(id,history.as_ref().unwrap_or(&Value::Null),normal.as_ref().unwrap_or(&Value::Null),standard.as_ref().unwrap_or(&Value::Null),seasons.as_ref().unwrap_or(&Value::Null),chrono::Utc::now().timestamp());
                if !detail.heroes.is_empty()&&out.status=="unavailable"{out.status="partial".into();}
                out.deadlock=Some(detail);
                out.notes.push("Статистика персонажей учитывает Standard и Ranked; Brawl и приватные игры исключены. Доступ к рейтингу оценивается по известным победам Standard и рейтинговым матчам текущего сезона. Неполная история не подтверждает закрытый рейтинг; ограничения входа проверяются в игре.".into());
            },
        }
        out
    }
}
