//! Independent browser process and per-account OS-vault sessions. Never attaches
//! to the user's browser, patches browser fingerprints, or solves challenges.
use super::platforms::trusted_url;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

#[derive(Clone, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    #[serde(rename = "httpOnly", default)]
    pub http_only: bool,
    #[serde(default)]
    pub expires: Option<f64>,
    #[serde(rename = "sameSite", default)]
    pub same_site: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub platform: String,
    pub account_id: String,
    pub identity: String,
    pub country: String,
    pub cookies: Vec<Cookie>,
    pub connected_at: String,
    #[serde(default)]
    pub identity_proof: String,
    #[serde(default)]
    pub store_identity: Option<String>,
    #[serde(default)]
    pub web_storage: Vec<StorageEntry>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct StorageEntry {
    pub origin: String,
    pub name: String,
    pub value: String,
}
impl Session {
    pub fn validate(&self) -> Result<(), String> {
        if super::platforms::platform(&self.platform).is_none()
            || self.account_id.is_empty()
            || self.account_id.len() > 256
            || self.identity.is_empty()
            || self.identity.len() > 256
            || (self.cookies.is_empty() && self.web_storage.is_empty())
            || self.cookies.len() > 256
            || self.country.len() != 2
            || !self.country.bytes().all(|b| b.is_ascii_uppercase())
            || !matches!(
                self.identity_proof.as_str(),
                "" | "store_api" | "store_page" | "login_form"
            )
            || self
                .store_identity
                .as_ref()
                .is_some_and(|s| s.len() > 256 || s.chars().any(char::is_control))
        {
            return Err("Не удалось подтвердить браузерный вход.".into());
        }
        for c in &self.cookies {
            let domain = c.domain.trim_start_matches('.');
            if !trusted_url(&self.platform, &format!("https://{domain}/"))
                || c.name.is_empty()
                || c.name.len() > 256
                || c.value.len() > 32768
                || !c
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_graphic() && !b";=,\"".contains(&b))
                || c.value.chars().any(|c| c == '\r' || c == '\n')
                || !c.path.starts_with('/')
                || c.expires.is_some_and(|v| !v.is_finite())
            {
                return Err("Браузерная сессия содержит данные другого магазина.".into());
            }
        }
        if self.web_storage.len() > 2
            || self.web_storage.iter().any(|e| {
                self.platform != "fanatical"
                    || !matches!(
                        e.origin.as_str(),
                        "https://www.fanatical.com" | "https://fanatical.com"
                    )
                    || !matches!(e.name.as_str(), "bsauth" | "bsatok")
                    || e.value.len() > 32768
                    || serde_json::from_str::<Value>(&e.value).is_err()
            })
        {
            return Err("Сохранённые данные входа не относятся к этому магазину.".into());
        }
        Ok(())
    }
    pub fn load(platform: &str, id: &str) -> Result<Self, String> {
        let value = super::web_vault::load(platform, id)?;
        let session: Self = serde_json::from_str(&value)
            .map_err(|_| "Браузерный вход повреждён; подключи его заново.".to_string())?;
        session.validate()?;
        if session.platform != platform || session.account_id != id {
            return Err("Вход относится к другому аккаунту.".into());
        }
        Ok(session)
    }
    pub fn save(&self) -> Result<(), String> {
        self.validate()?;
        let value = serde_json::to_string(self)
            .map_err(|_| "Не удалось подготовить сохранение браузерного входа.".to_string())?;
        super::web_vault::save(&self.platform, &self.account_id, &value)
    }
    pub fn delete(platform: &str, id: &str) -> Result<(), String> {
        super::browser_profile::delete(platform, id)?;
        super::web_vault::delete(platform, id)
    }
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

fn active_cookie(cookie: &Cookie, now: f64) -> bool {
    !cookie.value.is_empty()
        && cookie
            .expires
            .is_none_or(|expires| expires == -1.0 || expires > now)
}

fn epic_auth_cookie(cookie: &Cookie) -> bool {
    cookie.domain.trim_start_matches('.') == "epicgames.com"
        && matches!(
            cookie.name.as_str(),
            "EPIC_SSO" | "EPIC_BEARER_TOKEN" | "EPIC_SESSION_AP"
        )
}

/// Preserve ordinary profile cookies, but never treat an OAuth bearer as proof
/// that a different SSO cookie is usable by the storefront. New vault auth
/// cookies replace old values and obsolete host-only copies of the same key.
fn cookie_restore_plan(
    session: &Session,
    existing: &[Cookie],
    now: f64,
) -> (Vec<Value>, Vec<Value>) {
    let mut deleted = Vec::new();
    let mut imported = Vec::new();
    for cookie in &session.cookies {
        if !active_cookie(cookie, now) {
            continue;
        }
        let epic_auth = session.platform == "epic" && epic_auth_cookie(cookie);
        if epic_auth && cookie.domain == ".epicgames.com" {
            for previous in existing.iter().filter(|previous| {
                previous.domain == "epicgames.com"
                    && previous.name == cookie.name
                    && previous.path == cookie.path
            }) {
                deleted.push(json!({
                    "name": previous.name,
                    "domain": previous.domain,
                    "path": previous.path,
                }));
            }
        }
        let retained = existing.iter().any(|previous| {
            previous.name == cookie.name
                && previous.domain == cookie.domain
                && previous.path == cookie.path
                && active_cookie(previous, now)
                && (!epic_auth
                    || previous.value == cookie.value
                        && previous.secure == cookie.secure
                        && previous.http_only == cookie.http_only
                        && previous.expires == cookie.expires
                        && previous.same_site == cookie.same_site)
        });
        if retained {
            continue;
        }
        let mut value = serde_json::to_value(cookie).unwrap();
        if cookie.expires.is_none() {
            value.as_object_mut().unwrap().remove("expires");
        }
        if cookie.same_site.is_none() {
            value.as_object_mut().unwrap().remove("sameSite");
        }
        imported.push(value);
    }
    (deleted, imported)
}

pub struct Browser {
    child: tokio::process::Child,
    _profile: Option<tempfile::TempDir>,
    _profile_lock: Option<std::fs::File>,
    socket: Socket,
    next_id: u64,
    pub context: String,
    pub target_session: String,
    main_target: String,
    contexts: std::collections::HashMap<(String, i64), String>,
    frame_sessions: std::collections::HashMap<String, String>,
    login_platform: Option<String>,
    login_identity: Option<String>,
    login_sessions: std::collections::HashSet<String>,
}
impl Browser {
    pub async fn start(visible: bool) -> Result<Self, String> {
        let (_, exe) = super::browser_profile::executable("auto")?;
        let temp = tempfile::Builder::new()
            .prefix("swch-claims-")
            .tempdir()
            .map_err(|_| "Не удалось подготовить временный профиль браузера.")?;
        Self::start_profile(visible, exe, temp.path().into(), Some(temp), None).await
    }
    pub async fn for_account(
        account: &crate::storage::Account,
        visible: bool,
        choice: &str,
    ) -> Result<Self, String> {
        let choice = choice.to_string();
        let platform = account.launcher.clone();
        let id = account.id.clone();
        let (exe, path, lock) = tokio::task::spawn_blocking(move || {
            let (name, exe) = super::browser_profile::executable(&choice)?;
            let (path, lock) = super::browser_profile::prepare(&platform, &id, &name)?;
            Ok::<_, String>((exe, path, lock))
        })
        .await
        .map_err(|_| "Не удалось подготовить браузер SWCH.")??;
        Self::start_profile(visible, exe, path, None, Some(lock)).await
    }
    async fn start_profile(
        visible: bool,
        exe: PathBuf,
        profile: PathBuf,
        temp: Option<tempfile::TempDir>,
        lock: Option<std::fs::File>,
    ) -> Result<Self, String> {
        // Never connect using a stale endpoint left by an earlier SWCH process.
        match tokio::fs::remove_file(profile.join("DevToolsActivePort")).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Профиль браузера SWCH занят или недоступен.".into()),
        }
        let mut command = tokio::process::Command::new(exe);
        command
            .arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--remote-debugging-address=127.0.0.1",
                "--remote-debugging-port=0",
                "--no-first-run",
                "--no-default-browser-check",
                "about:blank",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if !visible {
            command.arg("--headless=new");
        }
        #[cfg(windows)]
        if !visible {
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|_| "Не удалось открыть отдельный браузер SWCH.")?;
        let mut endpoint = None;
        for _ in 0..100 {
            if child
                .try_wait()
                .map_err(|_| "Браузер завершился.")?
                .is_some()
            {
                return Err("Браузер SWCH завершился до подключения.".into());
            }
            if let Ok(data) = tokio::fs::read_to_string(profile.join("DevToolsActivePort")).await {
                let mut lines = data.lines();
                if let (Some(port), Some(path)) = (
                    lines.next().and_then(|v| v.parse::<u16>().ok()),
                    lines.next(),
                ) {
                    if port > 0
                        && path.starts_with("/devtools/browser/")
                        && path
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"/-".contains(&b))
                    {
                        endpoint = Some(format!("ws://127.0.0.1:{port}{path}"));
                        break;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let endpoint = endpoint.ok_or("Браузер не предоставил локальное подключение.")?;
        let (socket, _) = tokio::time::timeout(Duration::from_secs(10), connect_async(&endpoint))
            .await
            .map_err(|_| "Браузер не ответил.")?
            .map_err(|_| "Не удалось подключить браузер SWCH.")?;
        let mut browser = Self {
            child,
            _profile: temp,
            _profile_lock: lock,
            socket,
            next_id: 1,
            context: String::new(),
            target_session: String::new(),
            main_target: String::new(),
            contexts: Default::default(),
            frame_sessions: Default::default(),
            login_platform: None,
            login_identity: None,
            login_sessions: Default::default(),
        };
        // Default context belongs to this SWCH-only process and persists its cookies/storage.
        // Diagnostic temporary browsers keep their original disposable context.
        if browser._profile.is_some() {
            browser.context = browser
                .call(
                    "Target.createBrowserContext",
                    json!({"disposeOnDetach":true}),
                    None,
                )
                .await?["browserContextId"]
                .as_str()
                .ok_or("Нет отдельного контекста браузера.")?
                .into();
        }
        let target_params = browser.context_params(json!({"url":"about:blank"}));
        let target = browser
            .call("Target.createTarget", target_params, None)
            .await?["targetId"]
            .as_str()
            .ok_or("Не удалось открыть вкладку SWCH.")?
            .to_string();
        browser.target_session = browser
            .call(
                "Target.attachToTarget",
                json!({"targetId":target,"flatten":true}),
                None,
            )
            .await?["sessionId"]
            .as_str()
            .ok_or("Не удалось подключить вкладку SWCH.")?
            .into();
        browser.main_target = target;
        let session = browser.target_session.clone();
        browser
            .call("Page.enable", json!({}), Some(&session))
            .await?;
        browser
            .call("Runtime.enable", json!({}), Some(&session))
            .await?;
        Ok(browser)
    }
    fn context_params(&self, mut params: Value) -> Value {
        if !self.context.is_empty() {
            params["browserContextId"] = json!(self.context);
        }
        params
    }
    fn owns_context(&self, target: &Value) -> bool {
        self.context.is_empty()
            || target["browserContextId"].as_str().unwrap_or_default() == self.context
    }
    pub async fn call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let mut message = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::Text(message.to_string().into()))
            .await
            .map_err(|_| "Соединение с браузером прервалось.".to_string())?;
        tokio::time::timeout(Duration::from_secs(35), async {
            while let Some(message) = self.socket.next().await {
                let message =
                    message.map_err(|_| "Соединение с браузером прервалось.".to_string())?;
                if let Message::Text(text) = message {
                    let reply: Value = serde_json::from_str(&text)
                        .map_err(|_| "Браузер вернул неизвестный ответ.".to_string())?;
                    let sid = reply["sessionId"].as_str().unwrap_or_default().to_string();
                    if reply["method"] == "Runtime.executionContextCreated" {
                        let c = &reply["params"]["context"];
                        if c["auxData"]["isDefault"].as_bool() == Some(true) {
                            if let (Some(id), Some(origin)) =
                                (c["id"].as_i64(), c["origin"].as_str())
                            {
                                self.contexts.insert((sid.clone(), id), origin.to_string());
                            }
                        }
                    } else if reply["method"] == "Runtime.executionContextDestroyed" {
                        if let Some(id) = reply["params"]["executionContextId"].as_i64() {
                            self.contexts.remove(&(sid, id));
                        }
                    } else if reply["method"] == "Runtime.executionContextsCleared" {
                        self.contexts.retain(|(session, _), _| session != &sid);
                    } else if reply["method"] == "Runtime.bindingCalled"
                        && reply["params"]["name"] == "__swchLoginIdentity"
                        && self.login_sessions.contains(&sid)
                    {
                        if let (Some(platform), Some(payload)) =
                            (&self.login_platform, reply["params"]["payload"].as_str())
                        {
                            if let Ok(v) = serde_json::from_str::<Value>(payload) {
                                let origin = reply["params"]["executionContextId"]
                                    .as_i64()
                                    .and_then(|id| self.contexts.get(&(sid.clone(), id)));
                                if origin.is_some_and(|o| {
                                    super::platforms::login_host_allowed(platform, &format!("{o}/"))
                                }) && super::platforms::login_host_allowed(
                                    platform,
                                    v["url"].as_str().unwrap_or_default(),
                                ) {
                                    if let Some(name) = v["identity"].as_str().filter(|s| {
                                        !s.is_empty()
                                            && s.len() <= 256
                                            && !s.chars().any(char::is_control)
                                    }) {
                                        self.login_identity = Some(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                    if reply["id"].as_u64() == Some(id) {
                        if reply.get("error").is_some() {
                            return Err("Браузер не выполнил операцию SWCH.".into());
                        }
                        return Ok(reply["result"].clone());
                    }
                }
            }
            Err("Браузер закрыл подключение SWCH.".to_string())
        })
        .await
        .map_err(|_| "Операция браузера превысила время ожидания.".to_string())?
    }
    pub async fn evaluate(
        &mut self,
        expression: &str,
        session: Option<&str>,
    ) -> Result<Value, String> {
        let own = self.target_session.clone();
        let result = self
            .call(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true,"awaitPromise":true}),
                Some(session.unwrap_or(&own)),
            )
            .await?;
        if result.get("exceptionDetails").is_some() {
            return Err("Не удалось прочитать состояние магазина.".into());
        }
        Ok(result["result"]["value"].clone())
    }
    pub async fn navigate(&mut self, platform: &str, url: &str) -> Result<(), String> {
        if !trusted_url(platform, url) {
            return Err("Адрес не относится к выбранному магазину.".into());
        }
        let session = self.target_session.clone();
        self.call("Page.navigate", json!({"url":url}), Some(&session))
            .await?;
        for _ in 0..100 {
            if let Ok(v) = self
                .evaluate("({url:location.href,ready:document.readyState})", None)
                .await
            {
                if trusted_url(platform, v["url"].as_str().unwrap_or_default())
                    && v["ready"].as_str().is_some_and(|s| s != "loading")
                {
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("Магазин не загрузил страницу вовремя.".into())
    }
    /// Normal pointer input for the main page, after the caller validates the account/product/price.
    /// Obscured, disabled, or changed controls are never clicked.
    pub async fn click_control(&mut self, selector: &str, label: &str) -> Result<Value, String> {
        let selector = json!(selector).to_string();
        let label = json!(label).to_string();
        let script = format!("(async()=>{{const e=document.querySelector({selector});if(!e||e.disabled||e.textContent.trim()!=={label})return false;if(e.tagName==='A'&&e.target==='_blank')return {{navigate:e.href}};e.scrollIntoView({{block:'center',inline:'nearest'}});await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));const b=e.getBoundingClientRect(),style=getComputedStyle(e);if(b.width<=0||b.height<=0||style.visibility==='hidden'||style.display==='none'||e.disabled||e.textContent.trim()!=={label})return false;const x=(Math.max(0,b.left)+Math.min(innerWidth,b.right))/2,y=(Math.max(0,b.top)+Math.min(innerHeight,b.bottom))/2;if(x<0||y<0||x>=innerWidth||y>=innerHeight||!e.contains(document.elementFromPoint(x,y)))return false;return {{x,y}};}})()");
        let point = self.evaluate(&script, None).await?;
        if point.get("navigate").is_some() || point.as_bool() == Some(false) {
            return Ok(point);
        }
        let (Some(x), Some(y)) = (point["x"].as_f64(), point["y"].as_f64()) else {
            return Ok(json!(false));
        };
        let session = self.target_session.clone();
        self.call(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseMoved","x":x,"y":y}),
            Some(&session),
        )
        .await?;
        self.call(
            "Input.dispatchMouseEvent",
            json!({"type":"mousePressed","x":x,"y":y,"button":"left","buttons":1,"clickCount":1}),
            Some(&session),
        )
        .await?;
        self.call(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseReleased","x":x,"y":y,"button":"left","buttons":0,"clickCount":1}),
            Some(&session),
        )
        .await?;
        Ok(json!(true))
    }
    /// Observe only the login identifier the user submits in the application's
    /// fresh browser. No password, OTP, recovery-code field or submit action.
    pub async fn watch_login(&mut self, platform: &str) -> Result<(), String> {
        self.login_platform = Some(platform.into());
        self.login_identity = None;
        let session = self.target_session.clone();
        self.watch_login_session(&session).await
    }
    async fn watch_login_session(&mut self, session: &str) -> Result<(), String> {
        if self.login_sessions.contains(session) {
            return Ok(());
        }
        self.call(
            "Runtime.addBinding",
            json!({"name":"__swchLoginIdentity"}),
            Some(session),
        )
        .await?;
        let script = r#"(()=>{if(window.__swchLoginObserver)return;window.__swchLoginObserver=true;const observe=e=>{if(!e.isTrusted)return;if(e.type==='click'&&!e.target.closest('button[type="submit"],input[type="submit"],#login_login,#signInSubmit'))return;if(e.type==='keydown'&&e.key!=='Enter')return;const root=e.target.closest('form')||document;const field=Array.from(root.querySelectorAll('input[type="email"],input[name="email"],input[name="username"],input[name="loginfmt"],#ap_email,#login_username')).find(i=>i.type!=='password'&&i.getBoundingClientRect().width>0);const identity=field?.value?.trim();if(identity&&identity.length<=256)window.__swchLoginIdentity(JSON.stringify({url:location.origin+location.pathname,identity}));};for(const event of ['submit','click','keydown'])document.addEventListener(event,observe,true);})()"#;
        self.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source":script}),
            Some(session),
        )
        .await?;
        self.evaluate(script, Some(session)).await?;
        self.login_sessions.insert(session.into());
        Ok(())
    }
    pub fn submitted_login(&self) -> Option<&str> {
        self.login_identity.as_deref()
    }
    pub async fn watch_login_popups(&mut self) -> Result<(), String> {
        let Some(platform) = self.login_platform.clone() else {
            return Ok(());
        };
        let targets = self.call("Target.getTargets", json!({}), None).await?;
        for t in targets["targetInfos"].as_array().into_iter().flatten() {
            if t["type"] != "page"
                || !self.owns_context(t)
                || !trusted_url(&platform, t["url"].as_str().unwrap_or_default())
            {
                continue;
            }
            let Some(id) = t["targetId"].as_str().filter(|v| *v != self.main_target) else {
                continue;
            };
            let sid = if let Some(s) = self.frame_sessions.get(id) {
                s.clone()
            } else {
                let reply = self
                    .call(
                        "Target.attachToTarget",
                        json!({"targetId":id,"flatten":true}),
                        None,
                    )
                    .await?;
                let s = reply["sessionId"]
                    .as_str()
                    .ok_or("Не удалось прочитать окно входа.")?
                    .to_string();
                self.call("Runtime.enable", json!({}), Some(&s)).await?;
                self.call("Page.enable", json!({}), Some(&s)).await?;
                self.frame_sessions.insert(id.into(), s.clone());
                s
            };
            self.watch_login_session(&sid).await?;
        }
        Ok(())
    }
    pub async fn frame_values(
        &mut self,
        platform: &str,
        expression: &str,
    ) -> Result<Vec<(String, i64, Value)>, String> {
        let targets = self.call("Target.getTargets", json!({}), None).await?;
        for t in targets["targetInfos"].as_array().into_iter().flatten() {
            if !matches!(t["type"].as_str(), Some("iframe" | "page"))
                || t["targetId"].as_str() == Some(&self.main_target)
                || !self.owns_context(t)
                || !trusted_url(platform, t["url"].as_str().unwrap_or_default())
            {
                continue;
            }
            let Some(id) = t["targetId"].as_str() else {
                continue;
            };
            if !self.frame_sessions.contains_key(id) {
                let attached = self
                    .call(
                        "Target.attachToTarget",
                        json!({"targetId":id,"flatten":true}),
                        None,
                    )
                    .await?;
                if let Some(s) = attached["sessionId"].as_str() {
                    let s = s.to_string();
                    self.call("Runtime.enable", json!({}), Some(&s)).await?;
                    self.frame_sessions.insert(id.into(), s);
                }
            }
        }
        let contexts = self
            .contexts
            .iter()
            .filter(|(_, origin)| trusted_url(platform, &format!("{origin}/")))
            .map(|((s, id), _)| (s.clone(), *id))
            .collect::<Vec<_>>();
        let mut values = vec![];
        for (s, id) in contexts {
            if let Ok(v) = self.evaluate_context(expression, &s, id).await {
                values.push((s, id, v));
            }
        }
        Ok(values)
    }
    pub async fn evaluate_context(
        &mut self,
        expression: &str,
        session: &str,
        context: i64,
    ) -> Result<Value, String> {
        let result=self.call("Runtime.evaluate",json!({"expression":expression,"contextId":context,"returnByValue":true,"awaitPromise":true}),Some(session)).await?;
        if result.get("exceptionDetails").is_some() {
            return Err("Не удалось прочитать оформление магазина.".into());
        }
        Ok(result["result"]["value"].clone())
    }
    pub async fn restore(&mut self, session: &Session) -> Result<(), String> {
        session.validate()?;
        let existing = if self._profile.is_none() {
            self.cookies(&session.platform).await?
        } else {
            vec![]
        };
        let now = chrono::Utc::now().timestamp() as f64;
        let (deleted, cookies) = cookie_restore_plan(session, &existing, now);
        for cookie in deleted {
            let target = self.target_session.clone();
            self.call("Network.deleteCookies", cookie, Some(&target))
                .await?;
        }
        self.call(
            "Storage.setCookies",
            self.context_params(json!({"cookies":cookies})),
            None,
        )
        .await?;
        if !session.web_storage.is_empty() {
            let entries = serde_json::to_string(&session.web_storage)
                .map_err(|_| "Не удалось восстановить вход.")?;
            let script=format!("(()=>{{for(const e of {entries}){{if(location.origin===e.origin&&!localStorage.getItem(e.name))localStorage.setItem(e.name,e.value);}}}})()");
            let target = self.target_session.clone();
            self.call(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({"source":script}),
                Some(&target),
            )
            .await?;
        }
        Ok(())
    }
    pub async fn web_storage(&mut self, platform: &str) -> Result<Vec<StorageEntry>, String> {
        if platform != "fanatical" {
            return Ok(vec![]);
        }
        let result=self.evaluate("(()=>{if(!['https://www.fanatical.com','https://fanatical.com'].includes(location.origin))return [];return ['bsauth','bsatok'].flatMap(name=>{const value=localStorage.getItem(name);return value?[{origin:location.origin,name,value}]:[];});})()",None).await?;
        serde_json::from_value(result).map_err(|_| "Не удалось сохранить вход Fanatical.".into())
    }
    pub async fn cookies(&mut self, platform: &str) -> Result<Vec<Cookie>, String> {
        let result = self
            .call("Storage.getCookies", self.context_params(json!({})), None)
            .await?;
        let cookies: Vec<Cookie> = serde_json::from_value(result["cookies"].clone())
            .map_err(|_| "Не удалось прочитать сессию магазина.".to_string())?;
        Ok(cookies
            .into_iter()
            .filter(|c| {
                trusted_url(
                    platform,
                    &format!("https://{}/", c.domain.trim_start_matches('.')),
                )
            })
            .collect())
    }
    pub async fn close(mut self) {
        let _ = self.call("Browser.close", json!({}), None).await;
        // Give Chrome/Edge time to flush the persistent profile before killing a stuck process.
        if tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
            let _ = self.child.wait().await;
        }
    }
}

#[cfg(test)]
mod cookie_restore_tests {
    use super::*;

    fn cookie(name: &str, value: &str, domain: &str) -> Cookie {
        Cookie {
            name: name.into(),
            value: value.into(),
            domain: domain.into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            expires: Some(3000.0),
            same_site: None,
        }
    }

    fn session(platform: &str, cookies: Vec<Cookie>) -> Session {
        Session {
            platform: platform.into(),
            account_id: "fixture-account".into(),
            identity: "fixture-account".into(),
            country: "US".into(),
            cookies,
            connected_at: String::new(),
            identity_proof: "store_api".into(),
            store_identity: None,
            web_storage: vec![],
        }
    }

    #[test]
    fn fresh_epic_sso_is_restored_even_with_same_unexpired_bearer() {
        let vault = session(
            "epic",
            vec![
                cookie("EPIC_SSO", "fresh-sso", ".epicgames.com"),
                cookie("EPIC_BEARER_TOKEN", "same-bearer", ".epicgames.com"),
            ],
        );
        let profile = vec![
            cookie("EPIC_SSO", "previous-sso", ".epicgames.com"),
            cookie("EPIC_BEARER_TOKEN", "same-bearer", ".epicgames.com"),
        ];
        let (deleted, imported) = cookie_restore_plan(&vault, &profile, 1000.0);
        assert!(deleted.is_empty());
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0]["name"], "EPIC_SSO");
        assert_eq!(imported[0]["value"], "fresh-sso");
        assert_eq!(imported[0]["domain"], ".epicgames.com");
    }

    #[test]
    fn legacy_host_only_auth_duplicate_is_removed_while_valid_scoped_copy_remains() {
        let scoped = cookie("EPIC_BEARER_TOKEN", "valid-bearer", ".epicgames.com");
        let vault = session("epic", vec![scoped.clone()]);
        let profile = vec![
            cookie("EPIC_BEARER_TOKEN", "stale-root-bearer", "epicgames.com"),
            scoped,
            cookie("unrelated", "host-only", "epicgames.com"),
        ];
        let (deleted, imported) = cookie_restore_plan(&vault, &profile, 1000.0);
        assert!(imported.is_empty());
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0]["name"], "EPIC_BEARER_TOKEN");
        assert_eq!(deleted[0]["domain"], "epicgames.com");
        assert_eq!(deleted[0]["path"], "/");
    }

    #[test]
    fn extended_epic_auth_cookie_replaces_the_same_value_with_old_expiry() {
        let mut refreshed = cookie("EPIC_SSO", "same-sso", ".epicgames.com");
        refreshed.expires = Some(9000.0);
        refreshed.same_site = Some("None".into());
        let old = cookie("EPIC_SSO", "same-sso", ".epicgames.com");
        let (_, imported) = cookie_restore_plan(&session("epic", vec![refreshed]), &[old], 1000.0);
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0]["expires"], 9000.0);
        assert_eq!(imported[0]["sameSite"], "None");
    }

    #[test]
    fn expired_profile_auth_is_replaced_without_expanding_unknown_host_only_cookies() {
        let vault = session(
            "epic",
            vec![
                cookie("EPIC_SSO", "fresh-sso", ".epicgames.com"),
                cookie("site-preference", "unchanged-scope", "www.epicgames.com"),
            ],
        );
        let mut expired = vault.cookies[0].clone();
        expired.expires = Some(900.0);
        let (deleted, imported) = cookie_restore_plan(&vault, &[expired], 1000.0);
        assert!(deleted.is_empty());
        assert_eq!(imported.len(), 2);
        assert_eq!(imported[0]["name"], "EPIC_SSO");
        assert_eq!(imported[1]["domain"], "www.epicgames.com");

        let legacy = session("epic", vec![cookie("EPIC_SSO", "legacy", "epicgames.com")]);
        let (_, imported) = cookie_restore_plan(&legacy, &[], 1000.0);
        assert_eq!(imported[0]["domain"], "epicgames.com");
    }

    #[test]
    fn ordinary_store_profile_cookie_is_kept_until_expired() {
        let vault = session("gog", vec![cookie("store-session", "old", "www.gog.com")]);
        let mut profile = cookie("store-session", "new", "www.gog.com");
        assert!(cookie_restore_plan(&vault, &[profile.clone()], 1000.0)
            .1
            .is_empty());
        profile.expires = Some(900.0);
        assert_eq!(cookie_restore_plan(&vault, &[profile], 1000.0).1.len(), 1);
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    /// Uses only a loopback fixture and test profiles. No login or real orders.
    #[tokio::test]
    #[ignore = "requires an installed Chrome/Edge and permission to launch a browser"]
    async fn browser_profile_survives_restart_and_separates_accounts() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                axum::Router::new().route(
                    "/",
                    axum::routing::get(|| async { "<html>Fixture only</html>" }),
                ),
            )
            .await
            .unwrap();
        });
        let root = tempfile::tempdir().unwrap();
        let (name, exe) = super::super::browser_profile::executable("auto").unwrap();
        for (account, expected) in [("first", false), ("first", true), ("second", false)] {
            let (profile, lock) =
                super::super::browser_profile::prepare_at(root.path(), "gog", account, &name)
                    .unwrap();
            let mut browser = Browser::start_profile(false, exe.clone(), profile, None, Some(lock))
                .await
                .unwrap();
            assert!(
                browser.context.is_empty(),
                "persistent profile must use the default context"
            );
            let target = browser.target_session.clone();
            browser
                .call("Page.navigate", json!({"url":origin}), Some(&target))
                .await
                .unwrap();
            let mut loaded = false;
            for _ in 0..30 {
                if browser.evaluate("location.origin", None).await.unwrap() == origin {
                    loaded = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            assert!(loaded);
            let result = browser
                .evaluate(
                    "({storage:localStorage.getItem('fixture'),cookie:document.cookie})",
                    None,
                )
                .await
                .unwrap();
            assert_eq!(result["storage"].as_str() == Some("retained"), expected);
            assert_eq!(
                result["cookie"]
                    .as_str()
                    .unwrap()
                    .contains("fixture=retained"),
                expected
            );
            browser.evaluate("localStorage.setItem('fixture','retained');document.cookie='fixture=retained; Max-Age=3600; Path=/';true", None).await.unwrap();
            browser.evaluate("document.body.innerHTML='<button id=fixture-button>Get</button>';document.querySelector('button').onclick=e=>window.fixtureTrusted=e.isTrusted;true", None).await.unwrap();
            assert_eq!(
                browser
                    .click_control("#fixture-button", "Wrong label")
                    .await
                    .unwrap(),
                json!(false)
            );
            assert_eq!(
                browser
                    .click_control("#fixture-button", "Get")
                    .await
                    .unwrap(),
                json!(true)
            );
            assert_eq!(
                browser
                    .evaluate("window.fixtureTrusted", None)
                    .await
                    .unwrap(),
                json!(true)
            );
            browser.evaluate("document.body.insertAdjacentHTML('beforeend','<div style=\"position:fixed;inset:0;background:black;z-index:9999\"></div>');true",None).await.unwrap();
            assert_eq!(
                browser
                    .click_control("#fixture-button", "Get")
                    .await
                    .unwrap(),
                json!(false)
            );
            // Also retain a newer store cookie rather than importing an older vault copy.
            browser.call("Storage.setCookies", json!({"cookies":[{"name":"fixture-check","value":"newer","domain":"www.gog.com","path":"/","secure":true,"expires":chrono::Utc::now().timestamp()+3600}]}), None).await.unwrap();
            let session = Session {
                platform: "gog".into(),
                account_id: account.into(),
                identity: account.into(),
                country: "US".into(),
                cookies: vec![Cookie {
                    name: "fixture-check".into(),
                    value: "older".into(),
                    domain: "www.gog.com".into(),
                    path: "/".into(),
                    secure: true,
                    http_only: false,
                    expires: None,
                    same_site: None,
                }],
                connected_at: chrono::Utc::now().to_rfc3339(),
                identity_proof: String::new(),
                store_identity: None,
                web_storage: vec![],
            };
            browser.restore(&session).await.unwrap();
            assert_eq!(
                browser
                    .cookies("gog")
                    .await
                    .unwrap()
                    .iter()
                    .find(|c| c.name == "fixture-check")
                    .unwrap()
                    .value,
                "newer"
            );
            // Simulate a backend restart: the old browser lives, but its backend lock is gone.
            drop(browser._profile_lock.take());
            assert!(
                super::super::browser_profile::prepare_at(root.path(), "gog", account, &name)
                    .is_err()
            );
            browser.close().await;
        }
        server.abort();
    }
}
