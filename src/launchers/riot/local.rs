//! Loopback client. The lockfile password is never serialized or logged.
use super::profile::{league_items, league_skins, league_playtime, league_ranked, safe_identity, Profile, Section};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub struct LocalClient {
    http: reqwest::Client,
    origin: String,
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
            origin: format!("https://127.0.0.1:{port}"),
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
            .get(format!("{}{}", self.origin, path))
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

    /// Read an already running client, without launching or switching it.
    /// Check identity on both sides so a concurrent login cannot rename another account.
    pub async fn current_identity(&self) -> Result<(String, String), String> {
        let before = self.get("/entitlements/v1/token").await?;
        let alias = self.get("/player-account/aliases/v1/active").await?;
        let after = self.get("/entitlements/v1/token").await?;
        verified_identity(&before, &alias, &after).ok_or_else(|| "Имя Riot сейчас недоступно.".into())
    }

    // The summoner loads before the inventory plugin on a cold League start.
    async fn ready_inventory(&self, path: &str) -> Result<Value, String> {
        let read = async {
            loop {
                match self.get(path).await {
                    Ok(value) => return Ok(value),
                    Err(message) if message == "Данные клиента недоступны (404)."
                        || message == "Данные клиента недоступны (503)." => {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    Err(message) => return Err(message),
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(30), read).await
            .map_err(|_| "Клиент League ещё не загрузил коллекцию. Дождись завершения загрузки и повтори обновление.".to_string())?
    }

    /// Use the client's Play operation; CLI product selection now opens a hub.
    pub async fn launch_product(&self, product: &str) -> Result<(), String> {
        super::launch_args(Some(product)).map_err(|_| "Неизвестная игра Riot.".to_string())?;
        let path = format!("/product-launcher/v1/products/{product}/patchlines/live");
        // Entitlements become ready before the launcher has loaded its product
        // registry. A fresh client briefly returns 464 here; wait without
        // replaying the actual Play command.
        let ready = async {
            loop {
                let response = self.http.get(format!("{}{path}/eligibility", self.origin))
                    .basic_auth("riot", Some(&self.password)).send().await
                    .map_err(|_| "Riot Client не отвечает при проверке запуска.".to_string())?;
                let status = response.status().as_u16();
                if response.status().is_success() {
                    if response.json::<Value>().await.ok() == Some(Value::Bool(true)) { return Ok(()); }
                } else if !matches!(status, 464 | 503) {
                    return Err(format!("Riot Client пока не разрешает запуск игры (HTTP {status}). Проверь обновление и вход в клиенте."));
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        };
        tokio::time::timeout(Duration::from_secs(20), ready).await
            .map_err(|_| "Riot Client ещё не подготовил игру к запуску. Проверь обновление игры и окно клиента.".to_string())??;
        let response = self.http.post(format!("{}{path}", self.origin))
            .basic_auth("riot", Some(&self.password))
            .json(&serde_json::json!({}))
            .send().await.map_err(|_| "Riot Client не ответил на команду запуска игры.".to_string())?;
        if !response.status().is_success() {
            return Err(format!("Riot Client не разрешил запуск игры (HTTP {}). Проверь обновление игры и окно клиента.", response.status().as_u16()));
        }
        Ok(())
    }

    /// Enable remembered login without resetting Riot's other session options.
    /// Returns true only when the actual authentication is remembered, rather
    /// than when an idle lifecycle config happens to enable persistence.
    pub async fn enable_remember_login(&self) -> Result<bool, String> {
        const PATH: &str = "/player-session-lifecycle/v1/session";
        let state = self.get(PATH).await?;
        let config = state["config"]
            .as_object()
            .filter(|config| config.get("persistLogin").and_then(Value::as_bool).is_some())
            .ok_or("Riot Client не поддерживает настройку сохранения входа.")?;
        if config["persistLogin"] == true {
            return self.is_login_remembered().await;
        }
        let mut config = config.clone();
        config.insert("persistLogin".into(), Value::Bool(true));
        let response = self.http
            .put(format!("{}{}", self.origin, PATH))
            .basic_auth("riot", Some(&self.password))
            .json(&config)
            .send().await
            .map_err(|_| "Не удалось включить сохранение входа в Riot Client.")?;
        if !response.status().is_success() {
            return Err(format!("Riot Client не разрешил сохранение входа ({}).", response.status().as_u16()));
        }
        if self.get(PATH).await?["config"]["persistLogin"] != true {
            return Err("Riot Client не подтвердил настройку сохранения входа.".into());
        }
        self.is_login_remembered().await
    }

    async fn is_login_remembered(&self) -> Result<bool, String> {
        let auth = self.get("/rso-auth/v1/session").await?;
        Ok(auth["type"] == "authenticated" && auth["persistLogin"] == true)
    }

    /// Start the installed client when necessary and wait for its saved login.
    pub async fn ensure_riot_ready() -> Result<Self, String> {
        if let Ok(client) = Self::discover(riot_paths()) {
            if client.get("/entitlements/v1/token").await.is_ok() {
                return Ok(client);
            }
        }
        let executable = super::client().ok_or("Riot Client не найден.")?;
        let mut child = tokio::process::Command::new(executable)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn().map_err(|_| "Не удалось запустить Riot Client.")?;
        tokio::spawn(async move { let _ = child.wait().await; });
        let wait = async {
            loop {
                if let Ok(client) = Self::discover(riot_paths()) {
                    if let Ok(tokens) = client.get("/entitlements/v1/token").await {
                        if tokens["accessToken"].as_str().is_some_and(|s| !s.is_empty()) {
                            return client;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        };
        tokio::time::timeout(Duration::from_secs(30), wait).await
            .map_err(|_| "Riot Client запущен, но вход не завершён. Проверь, не требуется ли пароль или подтверждение Riot.".into())
    }

    /// Opening the installed League lobby is enough; no match is started.
    pub async fn ensure_league_ready(expected: Option<&str>) -> Result<Self, String> {
        if let Ok(client) = Self::discover(league_paths()) {
            if client.get("/lol-summoner/v1/current-summoner").await
                .is_ok_and(|value| league_identity_ready(&value, expected)) {
                return Ok(client);
            }
        }
        if super::game_path("league_of_legends").is_none() {
            return Err("Клиент League of Legends не установлен. Сохранённые данные доступны без него; для первой загрузки коллекции League нужен установленный клиент. VALORANT можно обновлять без установленной игры.".into());
        }
        let riot = Self::ensure_riot_ready().await?;
        if let Some(expected) = expected {
            let tokens = riot.get("/entitlements/v1/token").await?;
            if tokens["subject"].as_str() != Some(expected.strip_prefix("riot_").unwrap_or(expected)) {
                return Err("В Riot Client активен другой аккаунт. Повтори переключение выбранного аккаунта.".into());
            }
        }
        riot.launch_product("league_of_legends").await?;
        let wait = async {
            loop {
                if let Ok(client) = Self::discover(league_paths()) {
                    if client.get("/lol-summoner/v1/current-summoner").await
                        .is_ok_and(|value| league_identity_ready(&value, expected)) {
                        return client;
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        };
        tokio::time::timeout(Duration::from_secs(60), wait).await
            .map_err(|_| "SWCH запустил клиент League, но выбранный профиль не загрузился. Проверь окно Riot: возможно, требуется подтверждение входа или завершение обновления клиента.".into())
    }
}

pub fn league_identity_ready(value: &Value, expected: Option<&str>) -> bool {
    let Some(puuid) = value["puuid"].as_str().filter(|id| safe_identity(id)) else { return false; };
    value["summonerId"].as_u64().is_some_and(|id| id > 0)
        && expected.is_none_or(|id| id.strip_prefix("riot_").unwrap_or(id) == puuid)
}

#[cfg(all(test, any(target_os = "windows", target_os = "macos")))]
mod launch_tests {
    use super::*;
    use axum::{routing::{get, post}, http::StatusCode, Json, Router};
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    #[tokio::test]
    async fn play_waits_for_product_registry_and_submits_only_once() {
        let checks=Arc::new(AtomicUsize::new(0));
        let plays=Arc::new(AtomicUsize::new(0));
        let check_count=checks.clone();
        let play_count=plays.clone();
        let router=Router::new()
            .route("/product-launcher/v1/products/league_of_legends/patchlines/live/eligibility",get(move || {
                let count=check_count.clone();
                async move {
                    let first=count.fetch_add(1,Ordering::SeqCst)==0;
                    (if first { StatusCode::from_u16(464).unwrap() } else { StatusCode::OK },Json(!first))
                }
            }))
            .route("/product-launcher/v1/products/league_of_legends/patchlines/live",post(move |Json(body):Json<Value>| {
                let count=play_count.clone();
                async move {
                    assert_eq!(body,serde_json::json!({}));
                    count.fetch_add(1,Ordering::SeqCst);
                    StatusCode::NO_CONTENT
                }
            }));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin=format!("http://{}",listener.local_addr().unwrap());
        let server=tokio::spawn(async move { axum::serve(listener,router).await.unwrap(); });
        let mut client=LocalClient::from_lockfile("client:1:12345:test-secret:https").unwrap();
        client.origin=origin; // Only this test fixture uses cleartext loopback.
        assert!(client.launch_product("unknown/product").await.is_err());
        assert_eq!(checks.load(Ordering::SeqCst),0);
        client.launch_product("league_of_legends").await.unwrap();
        assert_eq!(checks.load(Ordering::SeqCst),2);
        assert_eq!(plays.load(Ordering::SeqCst),1);
        server.abort();
    }
}

#[cfg(test)]
mod inventory_tests {
    use super::*;
    use axum::{routing::get, Json, Router, http::StatusCode, response::IntoResponse};
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    #[tokio::test]
    async fn waits_for_cold_inventory_without_repeating_permanent_errors() {
        let checks=Arc::new(AtomicUsize::new(0));
        let reads=checks.clone();
        let router=Router::new().route("/inventory",get(move || {
            let count=reads.clone();
            async move {
                if count.fetch_add(1,Ordering::SeqCst)==0 {
                    StatusCode::NOT_FOUND.into_response()
                } else { Json(serde_json::json!([])).into_response() }
            }
        })).route("/unauthorized",get(||async { StatusCode::UNAUTHORIZED }));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client=LocalClient::from_lockfile("client:1:12345:test-secret:https").unwrap();
        client.origin=format!("http://{}",listener.local_addr().unwrap());
        let server=tokio::spawn(async move { axum::serve(listener,router).await.unwrap(); });
        assert_eq!(client.ready_inventory("/inventory").await.unwrap(),serde_json::json!([]));
        assert_eq!(checks.load(Ordering::SeqCst),2);
        assert_eq!(client.ready_inventory("/unauthorized").await.unwrap_err(),"Данные клиента недоступны (401).");
        server.abort();
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

pub fn verified_identity(before: &Value, alias: &Value, after: &Value) -> Option<(String, String)> {
    let subject = before["subject"].as_str().filter(|s| safe_identity(s))?;
    if after["subject"].as_str() != Some(subject) { return None; }
    let name = alias["game_name"].as_str()?.trim();
    let tag = alias["tag_line"].as_str()?.trim();
    if name.is_empty() || tag.is_empty() { return None; }
    Some((format!("riot_{subject}"), format!("{name}#{tag}")))
}

pub async fn sync_league(expected: Option<&str>) -> Result<Profile, String> {
    let client = LocalClient::ensure_league_ready(expected).await?;
    let summoner = client.get("/lol-summoner/v1/current-summoner").await?;
    let puuid = summoner["puuid"]
        .as_str()
        .filter(|s| safe_identity(s))
        .ok_or("Войди в аккаунт League of Legends.")?;
    let id = summoner["summonerId"]
        .as_u64()
        .filter(|v| *v > 0)
        .ok_or("Профиль League of Legends ещё не загружен.")?;
    let (champions, skins, history, ranked) = tokio::join!(
        async {
            client
                .ready_inventory(&format!(
                    "/lol-champions/v1/inventories/{id}/champions-minimal"
                ))
                .await
        },
        async {
            client
                .ready_inventory(&format!("/lol-champions/v1/inventories/{id}/skins-minimal"))
                .await
        },
        async {
            client
                .get(&format!(
                    "/lol-match-history/v1/products/lol/{puuid}/matches?begIndex=0&endIndex=19"
                ))
                .await
        },
        client.get("/lol-ranked/v1/current-ranked-stats"),
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
    let skins = skins.and_then(|value| {
        champions.as_ref().map_err(Clone::clone)
            .and_then(|roster| league_skins(&value, roster))
    });
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
        skins: Section::from_result(skins),
        ranked:Some(Section::from_result(ranked.and_then(|value| league_ranked(&value, summoner["summonerLevel"].as_u64())))),
        buddies:None,
    })
}

#[cfg(test)]
mod remember_login_tests {
    use super::*;
    use axum::{routing::get, Json, Router};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn enables_only_persistence_preserves_config_and_requires_confirmation() {
        for (initial, accept, confirm, auth_type, persistent, expected_writes, expected) in [
            (Some(false), true, true, "authenticated", true, 1, Some(true)),
            (Some(true), true, true, "authenticated", true, 0, Some(true)),
            (Some(true), true, true, "authenticated", false, 0, Some(false)),
            (Some(false), true, true, "authenticated", false, 1, Some(false)),
            (Some(true), true, true, "unknown_authentication_response", true, 0, Some(false)),
            (Some(false), false, false, "authenticated", false, 1, None),
            (Some(false), true, false, "authenticated", false, 1, None),
            (None, true, true, "authenticated", false, 0, None),
        ] {
            let mut config = json!({"scopes":["openid","offline_access"],
                "productContext":{"productId":"league_of_legends"},
                "loginStrategy":"RememberedPlayer", "requireRiotID":true,
                "additionalPermissions":["example"], "futureOption":{"keep":true}});
            if let Some(initial) = initial { config["persistLogin"] = json!(initial); }
            let state = Arc::new(Mutex::new(json!({"config":config, "loginState":"LoggedIn"})));
            let writes = Arc::new(Mutex::new(Vec::<Value>::new()));
            let read_state = state.clone();
            let write_state = state.clone();
            let received = writes.clone();
            let app = Router::new().route("/player-session-lifecycle/v1/session", get(move || {
                let state = read_state.clone();
                async move { Json(state.lock().unwrap().clone()) }
            }).put(move |Json(body): Json<Value>| {
                let state = write_state.clone();
                let received = received.clone();
                async move {
                    received.lock().unwrap().push(body.clone());
                    if confirm { state.lock().unwrap()["config"] = body; }
                    if accept { axum::http::StatusCode::NO_CONTENT }
                    else { axum::http::StatusCode::FORBIDDEN }
                }
            })).route("/rso-auth/v1/session", get(move || async move {
                Json(json!({"type":auth_type, "persistLogin":persistent}))
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let client = LocalClient { http:reqwest::Client::builder().no_proxy().build().unwrap(),
                origin, password:"fixture-only".into() };
            assert_eq!(client.enable_remember_login().await.ok(), expected);
            let writes = writes.lock().unwrap();
            assert_eq!(writes.len(), expected_writes);
            if let Some(written) = writes.first() {
                config["persistLogin"] = json!(true);
                assert_eq!(written, &config);
                // Updating configuration is not evidence of persistent auth.
                assert_eq!(state.lock().unwrap()["loginState"], "LoggedIn");
            }
            server.abort();
        }
    }
}
