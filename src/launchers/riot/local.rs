//! GET-only loopback client. The lockfile password is never serialized or logged.
use super::profile::{league_items, league_playtime, safe_identity, Profile, Section};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub struct LocalClient {
    http: reqwest::Client,
    port: u16,
    password: String,
}
impl LocalClient {
    pub fn from_lockfile(text: &str) -> Result<Self, String> {
        let fields: Vec<_> = text.trim().split(':').collect();
        if fields.len() != 5
            || fields[4] != "https"
            || fields[3].is_empty()
            || fields[3].len() > 512
        {
            return Err("Некорректный файл подключения клиента.".into());
        }
        let port = fields[2]
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
            .ok_or("Некорректный порт клиента.")?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(|_| "Не удалось подключиться к локальному клиенту.")?;
        Ok(Self {
            http,
            port,
            password: fields[3].into(),
        })
    }
    pub fn discover(paths: Vec<PathBuf>) -> Result<Self, String> {
        for path in paths {
            if !path.is_absolute() {
                continue;
            }
            if std::fs::metadata(&path)
                .map(|m| m.len() > 4096)
                .unwrap_or(true)
            {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(client) = Self::from_lockfile(&text) {
                    return Ok(client);
                }
            }
        }
        Err("Открой клиент игры и войди в нужный аккаунт, затем повтори синхронизацию.".into())
    }
    pub async fn get(&self, path: &str) -> Result<Value, String> {
        if !path.starts_with('/') || path.starts_with("//") {
            return Err("Некорректный путь клиента.".into());
        }
        let response = self
            .http
            .get(format!("https://127.0.0.1:{}{}", self.port, path))
            .basic_auth("riot", Some(&self.password))
            .send()
            .await
            .map_err(|_| "Клиент не отвечает. Проверь вход в аккаунт.")?;
        if !response.status().is_success() {
            return Err(format!(
                "Данные клиента недоступны ({}).",
                response.status().as_u16()
            ));
        }
        response
            .json()
            .await
            .map_err(|_| "Неизвестный формат данных клиента.".into())
    }
}

fn league_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(p) = std::env::var_os("SWCH_LEAGUE_LOCKFILE") {
        paths.push(p.into());
    }
    if let Some(p) = super::game_path("league_of_legends") {
        paths.push(p.join(if cfg!(target_os = "macos") {
            "Contents/LoL/lockfile"
        } else {
            "lockfile"
        }));
    }
    paths
}
pub fn riot_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(p) = std::env::var_os("SWCH_RIOT_LOCKFILE") {
        paths.push(p.into());
    }
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        paths.push(Path::new(&p).join("Riot Games/Riot Client/Config/lockfile"));
    }
    paths
}

pub async fn sync_league() -> Result<Profile, String> {
    let client = LocalClient::discover(league_paths())?;
    let summoner = client.get("/lol-summoner/v1/current-summoner").await?;
    let puuid = summoner["puuid"]
        .as_str()
        .filter(|s| safe_identity(s))
        .ok_or("Войди в аккаунт League of Legends.")?;
    let id = summoner["summonerId"]
        .as_u64()
        .filter(|v| *v > 0)
        .ok_or("Профиль League of Legends ещё не загружен.")?;
    let (champions, skins, history) = tokio::join!(
        async {
            client
                .get(&format!(
                    "/lol-champions/v1/inventories/{id}/champions-minimal"
                ))
                .await
        },
        async {
            client
                .get(&format!("/lol-champions/v1/inventories/{id}/skins-minimal"))
                .await
        },
        async {
            client
                .get(&format!(
                    "/lol-match-history/v1/products/lol/{puuid}/matches?begIndex=0&endIndex=19"
                ))
                .await
        }
    );
    // Reject the whole batch if the player switched accounts during collection.
    let after = client.get("/lol-summoner/v1/current-summoner").await?;
    if after["puuid"] != summoner["puuid"] {
        return Err("Аккаунт изменился во время загрузки. Повтори синхронизацию.".into());
    }
    let playtime = Section::from_result(history.and_then(|v| league_playtime(&v)));
    let game_name = summoner["gameName"]
        .as_str()
        .or(summoner["displayName"].as_str())
        .unwrap_or("Riot");
    let tag = summoner["tagLine"].as_str().unwrap_or("");
    Ok(Profile {
        account_id: format!("riot_{puuid}"),
        app_id: "league_of_legends".into(),
        display_name: if tag.is_empty() {
            game_name.into()
        } else {
            format!("{game_name}#{tag}")
        },
        level: summoner["summonerLevel"].as_u64(),
        source: "league_client".into(),
        synced_at: chrono::Utc::now().to_rfc3339(),
        has_play_history: playtime.value.as_ref().is_some_and(|p| p.seconds > 0),
        playtime,
        champions: Section::from_result(champions.and_then(|v| league_items(&v, false))),
        skins: Section::from_result(skins.and_then(|v| league_items(&v, true))),
    })
}
