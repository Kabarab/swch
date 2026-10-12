pub mod browser_claim;
pub mod browser_profile;
pub mod catalog;
pub mod fanatical;
pub mod gog_claim;
pub mod keys;
pub mod microsoft;
pub mod mobile;
pub mod native_epic;
pub mod native_steam;
pub mod notifications;
pub mod offers;
pub mod platforms;
pub mod providers;
pub mod steam_auth;
pub mod store_discovery;
pub mod web;
pub mod web_rules;
mod web_vault;

use crate::{
    launchers::epic::EpicAuthStore,
    storage::{ClaimHistory, Database},
};
use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use offers::Offer;
use providers::{Claimed, Provider, Stores};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, RwLock};

pub const SUPPORTED_PLATFORMS: &[&str] = &[
    "steam",
    "epic",
    "gog",
    "prime",
    "microsoft",
    "ubisoft",
    "fanatical",
    "itchio",
    "indiegala",
    "alienware",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub steam: bool,
    pub epic: bool,
    pub interval_minutes: u32,
    pub platforms: Vec<String>,
    pub gamerpower: bool,
    pub epic_mobile: bool,
    pub dry_run: bool,
    pub run_on_startup: bool,
    pub schedule_mode: String,
    pub daily_times: Vec<String>,
    pub timezone: String,
    pub retry_attempts: u8,
    pub retry_backoff_seconds: u16,
    pub browser_enabled: bool,
    pub browser_login_timeout_seconds: u16,
    pub browser_operation_timeout_seconds: u16,
    pub notifications: bool,
    pub browser_visible: bool,
    pub browser_name: String,
    pub epic_launcher_claims: bool,
    pub steamdb: bool,
    pub external_notifications: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            steam: true,
            epic: true,
            interval_minutes: 60,
            platforms: SUPPORTED_PLATFORMS
                .iter()
                .skip(2)
                .map(|id| (*id).into())
                .collect(),
            gamerpower: true,
            epic_mobile: true,
            dry_run: false,
            run_on_startup: true,
            schedule_mode: "interval".into(),
            daily_times: vec![],
            timezone: "local".into(),
            retry_attempts: 2,
            retry_backoff_seconds: 10,
            browser_enabled: true,
            browser_login_timeout_seconds: 180,
            browser_operation_timeout_seconds: 120,
            notifications: false,
            browser_visible: false,
            browser_name: "auto".into(),
            epic_launcher_claims: false,
            steamdb: true,
            external_notifications: false,
        }
    }
}
impl Settings {
    pub fn platform_enabled(&self, id: &str) -> bool {
        match id {
            "steam" => self.steam,
            "epic" => self.epic,
            _ => SUPPORTED_PLATFORMS.contains(&id) && self.platforms.iter().any(|v| v == id),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !browser_profile::valid_choice(&self.browser_name) {
            return Err("Выбери Chrome, Edge или автоматический выбор браузера.".into());
        }
        if !(15..=1440).contains(&self.interval_minutes) {
            return Err("Интервал проверки должен быть от 15 до 1440 минут.".into());
        }
        let mut platforms = BTreeSet::new();
        if self
            .platforms
            .iter()
            .any(|id| !SUPPORTED_PLATFORMS.contains(&id.as_str()) || !platforms.insert(id))
        {
            return Err("Список платформ содержит неизвестную платформу или повтор.".into());
        }
        if !matches!(
            self.schedule_mode.as_str(),
            "interval" | "daily" | "combined"
        ) {
            return Err("Выбери интервальное, ежедневное или комбинированное расписание.".into());
        }
        let mut times = BTreeSet::new();
        if self
            .daily_times
            .iter()
            .any(|time| parse_time(time).is_none() || !times.insert(time))
        {
            return Err("Время проверки должно иметь формат ЧЧ:ММ без повторов.".into());
        }
        if self.schedule_mode != "interval" && self.daily_times.is_empty() {
            return Err("Добавь хотя бы одно время ежедневной проверки.".into());
        }
        if self.timezone != "local" && self.timezone.parse::<chrono_tz::Tz>().is_err() {
            return Err(
                "Укажи local или действующий часовой пояс IANA, например Europe/Minsk.".into(),
            );
        }
        if self.retry_attempts > 5 || self.retry_backoff_seconds > 300 {
            return Err("Допускается до 5 повторов с начальной паузой до 300 секунд.".into());
        }
        if !(15..=600).contains(&self.browser_login_timeout_seconds)
            || !(15..=600).contains(&self.browser_operation_timeout_seconds)
        {
            return Err("Таймаут браузера должен быть от 15 до 600 секунд.".into());
        }
        Ok(())
    }
}

fn parse_time(value: &str) -> Option<NaiveTime> {
    let bytes = value.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 2 || b.is_ascii_digit())
    {
        return None;
    }
    NaiveTime::from_hms_opt(value[..2].parse().ok()?, value[3..].parse().ok()?, 0)
}

pub fn daily_slot_id(timezone: &str, date: NaiveDate, time: &str) -> String {
    format!("{timezone}|{date}|{time}")
}

fn daily_slots<T: TimeZone>(
    settings: &Settings,
    now: DateTime<Utc>,
    zone: T,
    days: i64,
) -> Vec<(DateTime<Utc>, String)> {
    let today = now.with_timezone(&zone).date_naive();
    let mut slots = Vec::new();
    for day in 0..=days {
        let Some(date) = today.checked_add_signed(chrono::Duration::days(day)) else {
            continue;
        };
        for time in &settings.daily_times {
            let Some(time_value) = parse_time(time) else {
                continue;
            };
            // A repeated wall-clock time gets one slot, at its first occurrence.
            // A nonexistent spring-forward time is skipped instead of shifted.
            let at = match zone.from_local_datetime(&date.and_time(time_value)) {
                LocalResult::Single(at) => at.with_timezone(&Utc),
                LocalResult::Ambiguous(a, b) => a.with_timezone(&Utc).min(b.with_timezone(&Utc)),
                LocalResult::None => continue,
            };
            slots.push((at, daily_slot_id(&settings.timezone, date, time)));
        }
    }
    slots.sort_by_key(|(at, _)| *at);
    slots
}

fn zoned_slots(
    settings: &Settings,
    now: DateTime<Utc>,
    days: i64,
) -> Result<Vec<(DateTime<Utc>, String)>, String> {
    if settings.timezone == "local" {
        Ok(daily_slots(settings, now, Local, days))
    } else {
        let zone = settings
            .timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|_| "Неизвестный часовой пояс.".to_string())?;
        Ok(daily_slots(settings, now, zone, days))
    }
}

/// Computes the next configured deadline. An interval deadline in the past means
/// it is already due. Missing anchors start a fresh interval at `now`.
pub fn next_due(
    settings: &Settings,
    now: DateTime<Utc>,
    interval_anchor: Option<DateTime<Utc>>,
    completed_daily_slots: &BTreeSet<String>,
) -> Result<Option<DateTime<Utc>>, String> {
    settings.validate()?;
    if !settings.enabled {
        return Ok(None);
    }
    let interval = (settings.schedule_mode != "daily").then(|| {
        interval_anchor.unwrap_or(now) + chrono::Duration::minutes(settings.interval_minutes as i64)
    });
    let daily = if settings.schedule_mode != "interval" {
        zoned_slots(settings, now, 7)?
            .into_iter()
            .find(|(at, id)| *at >= now && !completed_daily_slots.contains(id))
            .map(|(at, _)| at)
    } else {
        None
    };
    Ok(match (interval, daily) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(at), None) | (None, Some(at)) => Some(at),
        (None, None) => None,
    })
}

/// Retry only explicit transport/service failures. Login, consent, price,
/// region and browser challenge failures always require the next normal run.
pub fn retryable_error(message: &str) -> bool {
    if is_browser_challenge_error(message) {
        return false;
    }
    let message = message.to_lowercase();
    if [
        "captcha",
        "challenge",
        "http 401",
        "http 403",
        "http 404",
        "needs_login",
        "eula",
        "цена",
        "нулевую цену",
        "регион",
        "подтверждение возраста",
    ]
    .iter()
    .any(|v| message.contains(v))
    {
        return false;
    }
    // Some providers attach generic login advice to every HTTP failure. A
    // known temporary status remains retryable unless a challenge is explicit.
    if [
        "http 408", "http 425", "http 429", "http 500", "http 502", "http 503", "http 504",
    ]
    .iter()
    .any(|v| message.contains(v))
    {
        return true;
    }
    if [
        "повтори вход",
        "повторный вход",
        "войти",
        "отклонил сохранённый вход",
        "пароль",
        "login expired",
        "authentication",
        "sign in",
        "log in",
    ]
    .iter()
    .any(|v| message.contains(v))
    {
        return false;
    }
    [
        "network error",
        "timeout",
        "timed out",
        "connection reset",
        "service unavailable",
        "temporarily unavailable",
        "временно ограничил подключение",
        "магазин недоступен",
        "серверы steam недоступны",
        "сервис оформления epic недоступен",
        "связь с steam прервалась",
        "steam не ответил",
    ]
    .iter()
    .any(|v| message.contains(v))
}

/// A security challenge blocks this browser session, rather than one offer.
/// HTTP failures or expired login alone do not establish a challenge.
pub fn is_browser_challenge_error(message: &str) -> bool {
    let message = message.to_lowercase();
    [
        "captcha",
        "капч",
        "cloudflare",
        "security check",
        "verify you are human",
        "проверка безопасности",
        "проверку безопасности",
        "проверки безопасности",
        "проверка браузера",
        "проверку браузера",
        "проверки браузера",
    ]
    .iter()
    .any(|phrase| message.contains(phrase))
}

async fn retry_delay(settings: &Settings, retry: u8) {
    let seconds = u64::from(settings.retry_backoff_seconds)
        .saturating_mul(1u64 << retry.min(5))
        .min(300);
    tokio::time::sleep(Duration::from_secs(seconds)).await;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultRow {
    pub launcher: String,
    pub account_id: String,
    pub account_name: String,
    pub offer_id: String,
    pub title: String,
    pub url: String,
    pub status: String,
    pub message: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Run {
    pub running: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub next_check_at: Option<String>,
    pub accounts_checked: usize,
    pub results: Vec<ResultRow>,
    pub error: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub current: Option<ActiveWork>,
    #[serde(default)]
    pub accounts_total: usize,
    #[serde(default)]
    pub cancelled: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWork {
    pub account_id: String,
    pub account_name: String,
    pub launcher: String,
    pub title: String,
    pub stage: String,
}

pub struct AutoClaimer {
    pub db: Database,
    provider: Arc<dyn Provider>,
    lock: Mutex<()>,
    steam_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    run: RwLock<Run>,
    cancel: tokio::sync::watch::Sender<u64>,
}
impl AutoClaimer {
    pub fn new(db: Database, epic: Arc<EpicAuthStore>) -> Arc<Self> {
        Self::with_provider(db, Arc::new(Stores::new(epic)))
    }
    pub fn with_provider(db: Database, provider: Arc<dyn Provider>) -> Arc<Self> {
        Arc::new(Self {
            db,
            provider,
            lock: Mutex::new(()),
            steam_locks: Mutex::new(HashMap::new()),
            run: RwLock::new(Run::default()),
            cancel: tokio::sync::watch::channel(0).0,
        })
    }
    pub async fn status(&self) -> Result<Run, String> {
        let run = self.run.read().await.clone();
        if run.started_at.is_some() {
            return Ok(run);
        }
        let mut saved: Run = self
            .db
            .auto_claim_value("last_run")
            .await
            .map_err(|_| "Не удалось прочитать состояние автоклеймера.")?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // A process restart cannot leave an old job marked running.
        saved.running = false;
        Ok(saved)
    }
    pub async fn accounts(&self) -> Result<Vec<crate::storage::Account>, String> {
        self.provider
            .accounts(
                self.db
                    .get_accounts()
                    .await
                    .map_err(|_| "Не удалось прочитать аккаунты.".to_string())?,
            )
            .await
    }
    /// Shared by the background provider and connection/disconnection API.
    /// A confirmed disconnect cannot be followed by an older in-flight save.
    pub async fn steam_account_lock(&self, id: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = self
            .steam_locks
            .lock()
            .await
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        lock.lock_owned().await
    }
    async fn steam_disconnected(&self, id: &str) -> Result<bool, String> {
        self.db
            .auto_claim_value(&format!("steam-excluded:{id}"))
            .await
            .map(|value| value.as_deref() == Some("true"))
            .map_err(|_| "Не удалось прочитать настройки автоподключения Steam.".into())
    }
    pub async fn account_lock(&self, platform: &str, id: &str) -> tokio::sync::OwnedMutexGuard<()> {
        if platform == "steam" {
            self.steam_account_lock(id).await
        } else {
            self.steam_account_lock(&format!("{platform}:{id}")).await
        }
    }
    pub async fn disconnected(&self, platform: &str, id: &str) -> Result<bool, String> {
        if platform == "steam" {
            self.steam_disconnected(id).await
        } else {
            self.db
                .auto_claim_value(&format!("web-excluded:{platform}:{id}"))
                .await
                .map(|v| v.as_deref() == Some("true"))
                .map_err(|_| "Не удалось проверить подключение магазина.".into())
        }
    }
    pub async fn settings(&self) -> Result<Settings, String> {
        let raw = self
            .db
            .auto_claim_value("settings")
            .await
            .map_err(|_| "Не удалось прочитать настройки автоклеймера.")?;
        let settings: Settings = match raw {
            Some(raw) => serde_json::from_str(&raw)
                .map_err(|_| "Не удалось прочитать настройки автоклеймера.".to_string())?,
            None => Settings::default(),
        };
        settings.validate()?;
        Ok(settings)
    }
    async fn completed_daily_slots(&self) -> Result<BTreeSet<String>, String> {
        self.db
            .auto_claim_value("daily_slots")
            .await
            .map_err(|_| "Не удалось прочитать ежедневное расписание.".to_string())?
            .map(|raw| {
                serde_json::from_str(&raw)
                    .map_err(|_| "Не удалось прочитать ежедневное расписание.".to_string())
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }
    async fn consume_daily_slots(
        &self,
        settings: &Settings,
        now: DateTime<Utc>,
    ) -> Result<BTreeSet<String>, String> {
        let mut completed = self.completed_daily_slots().await?;
        if settings.schedule_mode == "interval" {
            return Ok(completed);
        }
        let oldest = now.date_naive() - chrono::Duration::days(30);
        completed.retain(|id| {
            id.split('|')
                .nth(1)
                .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
                .is_some_and(|date| date >= oldest)
        });
        for (at, id) in zoned_slots(settings, now, 0)? {
            if at <= now {
                completed.insert(id);
            }
        }
        // Persist before requesting a store, so restarts and DST folds cannot
        // replay a daily slot that already started.
        self.db
            .set_auto_claim_value("daily_slots", &serde_json::to_string(&completed).unwrap())
            .await
            .map_err(|_| "Не удалось сохранить ежедневное расписание.".to_string())?;
        Ok(completed)
    }
    async fn save_schedule(&self, next: Option<DateTime<Utc>>) -> Result<(), String> {
        let mut status = self.status().await?;
        status.next_check_at = next.map(|time| time.to_rfc3339());
        *self.run.write().await = status.clone();
        self.db
            .set_auto_claim_value("last_run", &serde_json::to_string(&status).unwrap())
            .await
            .map_err(|_| "Не удалось сохранить расписание автоклеймера.".to_string())
    }
    pub async fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        settings.validate()?;
        self.db
            .set_auto_claim_value("settings", &serde_json::to_string(settings).unwrap())
            .await
            .map_err(|_| "Не удалось сохранить настройки автоклеймера.".to_string())?;
        let now = Utc::now();
        let completed = self.completed_daily_slots().await?;
        self.save_schedule(next_due(settings, now, Some(now), &completed)?)
            .await
    }
    /// Shared by startup, scheduler and manual runs. Each run takes a fresh
    /// account snapshot so newly added and inactive accounts are included.
    pub async fn check(&self) -> Result<Run, String> {
        self.check_manual(None, None).await
    }
    pub async fn check_account(
        &self,
        platform: &str,
        id: &str,
        visible: bool,
    ) -> Result<Run, String> {
        self.check_manual(Some((platform.to_owned(), id.to_owned())), Some(visible))
            .await
    }
    /// A manual run for all accounts or one target. Omitted visibility inherits
    /// the saved preference; an override affects only this run.
    pub async fn check_manual(
        &self,
        target: Option<(String, String)>,
        visible: Option<bool>,
    ) -> Result<Run, String> {
        if let Some((platform, id)) = &target {
            if !self
                .accounts()
                .await?
                .iter()
                .any(|a| a.launcher == *platform && a.id == *id)
            {
                return Err("Аккаунт не добавлен в SWCH.".into());
            }
        }
        self.check_mode(true, target, visible).await
    }
    pub async fn cancel(&self) -> bool {
        if !self.run.read().await.running {
            return false;
        }
        self.cancel
            .send_modify(|value| *value = value.wrapping_add(1));
        true
    }
    async fn check_mode(
        &self,
        manual: bool,
        target: Option<(String, String)>,
        visible: Option<bool>,
    ) -> Result<Run, String> {
        let _guard = self
            .lock
            .try_lock()
            .map_err(|_| "Проверка раздач уже выполняется.".to_string())?;
        let mut cancel = self.cancel.subscribe();
        let mut settings = self.settings().await?;
        if let Some(visible) = visible {
            settings.browser_visible = visible;
        }
        // The native actor uses the foreground window and system input. A
        // background run must never enter it, including scheduled runs.
        if !settings.browser_visible {
            settings.epic_launcher_claims = false;
        }
        let started = Utc::now();
        self.provider.begin_run(&settings).await?;
        let completed = self.consume_daily_slots(&settings, started).await?;
        let mut run = Run {
            running: true,
            started_at: Some(started.to_rfc3339()),
            ..Run::default()
        };
        *self.run.write().await = run.clone();
        let (outcome, cancelled) = tokio::select! {
            biased;
            _=cancel.changed()=>(Ok(()),true),
            result=self.check_accounts(&settings, &mut run, manual,target.as_ref())=>(result,false),
        };
        run.cancelled = cancelled;
        run.current = None;
        let ids = run
            .results
            .iter()
            .map(|r| (r.launcher.clone(), r.account_id.clone()))
            .collect::<BTreeSet<_>>();
        for (platform, id) in ids {
            let rows = run
                .results
                .iter()
                .filter(|r| r.launcher == platform && r.account_id == id)
                .collect::<Vec<_>>();
            if self
                .db
                .set_auto_claim_value(
                    &format!("account-results:{platform}:{id}"),
                    &serde_json::to_string(&rows).unwrap(),
                )
                .await
                .is_err()
            {
                run.warnings
                    .push("Не удалось сохранить последнее состояние аккаунта.".into());
            }
        }
        if let Err(message) = outcome {
            run.error = Some(message);
        }
        run.warnings = self.provider.warnings().await.unwrap_or_default();
        if settings.external_notifications && !settings.dry_run && !cancelled {
            let _guard = self.account_lock("notifications", "delivery").await;
            if let Err(error) = notifications::deliver(&self.db, &run).await {
                run.warnings.push(error);
            }
        }
        run.running = false;
        let finished = Utc::now();
        run.finished_at = Some(finished.to_rfc3339());
        let latest = self.settings().await.unwrap_or(settings);
        run.next_check_at =
            next_due(&latest, finished, Some(finished), &completed)?.map(|time| time.to_rfc3339());
        *self.run.write().await = run.clone();
        self.db
            .set_auto_claim_value("last_run", &serde_json::to_string(&run).unwrap())
            .await
            .map_err(|_| "Не удалось сохранить результат проверки раздач.".to_string())?;
        Ok(run)
    }
    async fn check_accounts(
        &self,
        settings: &Settings,
        run: &mut Run,
        manual: bool,
        target: Option<&(String, String)>,
    ) -> Result<(), String> {
        let accounts = self
            .accounts()
            .await?
            .into_iter()
            .filter(|a| {
                settings.platform_enabled(&a.launcher)
                    && target.is_none_or(|(platform, id)| a.launcher == *platform && a.id == *id)
            })
            .collect::<Vec<_>>();
        run.accounts_total = accounts.len();
        let mut catalog: HashMap<(String, String), Result<Vec<Offer>, String>> = HashMap::new();
        for account in accounts
            .into_iter()
            .filter(|a| settings.platform_enabled(&a.launcher))
        {
            run.accounts_checked += 1;
            let name = account
                .display_name
                .clone()
                .unwrap_or_else(|| account.username.clone());
            run.current = Some(ActiveWork {
                account_id: account.id.clone(),
                account_name: name.clone(),
                launcher: account.launcher.clone(),
                title: String::new(),
                stage: "login".into(),
            });
            *self.run.write().await = run.clone();
            let row = |offer: Option<&Offer>, status: &str, message: String| ResultRow {
                launcher: account.launcher.clone(),
                account_id: account.id.clone(),
                account_name: name.clone(),
                offer_id: offer.map(Offer::key).unwrap_or_default(),
                title: offer.map(|v| v.title.clone()).unwrap_or_default(),
                url: offer.map(|v| v.url.clone()).unwrap_or_default(),
                status: status.into(),
                message,
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            let country_result = {
                let mut retry = 0;
                loop {
                    let steam_guard = self.account_lock(&account.launcher, &account.id).await;
                    let current = self.settings().await?;
                    if (!manual && !current.enabled)
                        || !current.platform_enabled(&account.launcher)
                        || self.disconnected(&account.launcher, &account.id).await?
                    {
                        break Ok(None);
                    }
                    let result = self.provider.country(&account).await;
                    drop(steam_guard);
                    match result {
                        Err(message)
                            if retry < settings.retry_attempts && retryable_error(&message) =>
                        {
                            retry_delay(settings, retry).await;
                            retry += 1;
                        }
                        result => break result.map(Some),
                    }
                }
            };
            let country = match country_result {
                Ok(Some(v)) => v,
                Ok(None) => {
                    run.results.push(row(
                        None,
                        "disabled",
                        "Автоклеймер отключён для этого аккаунта.".into(),
                    ));
                    *self.run.write().await = run.clone();
                    continue;
                }
                Err(message) => {
                    let status = if retryable_error(&message) {
                        "error"
                    } else {
                        "needs_login"
                    };
                    run.results.push(row(None, status, message));
                    *self.run.write().await = run.clone();
                    continue;
                }
            };
            let key = (
                if account.launcher == "prime" {
                    format!("prime:{}", account.id)
                } else {
                    account.launcher.clone()
                },
                country.clone(),
            );
            if !catalog.contains_key(&key) {
                if let Some(current) = run.current.as_mut() {
                    current.stage = "discovery".into();
                }
                *self.run.write().await = run.clone();
                let mut retry = 0;
                let discovery = loop {
                    let result = self.provider.discover_for(&account, &country).await;
                    match result {
                        Err(message)
                            if retry < settings.retry_attempts && retryable_error(&message) =>
                        {
                            retry_delay(settings, retry).await;
                            retry += 1;
                        }
                        result => break result,
                    }
                };
                catalog.insert(key.clone(), discovery);
            }
            let offers = match catalog.get(&key).unwrap() {
                Ok(v) => v,
                Err(message) => {
                    run.results.push(row(None, "error", message.clone()));
                    *self.run.write().await = run.clone();
                    continue;
                }
            };
            if offers.iter().all(|offer| {
                offer.launcher != account.launcher
                    || !offer.eligible(chrono::Utc::now().timestamp())
            }) {
                run.results.push(row(
                    None,
                    "no_offers",
                    "Сейчас подходящих раздач в регионе аккаунта нет.".into(),
                ));
            }
            let mut browser_challenge = false;
            'offers: for offer in offers {
                if offer.launcher != account.launcher
                    || !offer.eligible(chrono::Utc::now().timestamp())
                {
                    continue;
                }
                let mut retry = 0;
                loop {
                    if !offer.eligible(Utc::now().timestamp()) {
                        break;
                    }
                    let steam_guard = self.account_lock(&account.launcher, &account.id).await;
                    if self.disconnected(&account.launcher, &account.id).await? {
                        run.results.push(row(
                            None,
                            "disabled",
                            "Автоподключение отключено для этого аккаунта.".into(),
                        ));
                        break 'offers;
                    }
                    // Recheck account removal and settings after each retry wait.
                    if !self
                        .accounts()
                        .await
                        .map_err(|_| "Не удалось проверить аккаунт.")?
                        .iter()
                        .any(|a| a.id == account.id && a.launcher == account.launcher)
                    {
                        break 'offers;
                    }
                    let current = self.settings().await?;
                    if (!manual && !current.enabled) || !current.platform_enabled(&account.launcher)
                    {
                        break 'offers;
                    }
                    if settings.dry_run || current.dry_run {
                        run.results.push(row(
                            Some(offer),
                            "preview",
                            if offer.namespace=="gamerpower" {"Кандидат из каталога: цену и лицензию проверим в магазине при получении. Пробный запуск не оформляет заказ."}else{"Подходит для получения: пробный запуск без оформления."}.into(),
                        ));
                        *self.run.write().await = run.clone();
                        break;
                    }
                    if self
                        .db
                        .has_claim(&account.launcher, &account.id, &offer.key())
                        .await
                        .map_err(|_| "Не удалось прочитать историю получения.")?
                    {
                        run.results
                            .push(row(Some(offer), "claimed", "Уже получена ранее.".into()));
                        break;
                    }
                    // The provider checks ownership and current price on every
                    // attempt. Release the account guard during backoff so a
                    // disconnect can cancel the next attempt.
                    let key_id = keys::receipt_id(&account.launcher, &account.id, &offer.key());
                    if self
                        .db
                        .auto_claim_value(&format!("key-receipt:{key_id}"))
                        .await
                        .ok()
                        .flatten()
                        .is_some()
                    {
                        let present = tokio::task::spawn_blocking(move || {
                            keys::read_receipt(&key_id).is_ok()
                        })
                        .await
                        .unwrap_or(false);
                        if present {
                            run.results.push(row(Some(offer),"key_saved","Ключ этой раздачи уже сохранён в очереди; повторная выдача не требуется.".into()));
                            break;
                        }
                    }
                    if browser_challenge {
                        run.results.push(row(
                            Some(offer),
                            "attention",
                            "Получение не запускалось: предыдущую попытку этого аккаунта остановила проверка безопасности магазина. Остальные раздачи пропущены в этом запуске; другие аккаунты продолжают проверяться.".into(),
                        ));
                        *self.run.write().await = run.clone();
                        break;
                    }
                    if let Some(current) = run.current.as_mut() {
                        current.stage = "claim".into();
                        current.title = offer.title.clone();
                    }
                    *self.run.write().await = run.clone();
                    let outcome = match tokio::time::timeout(Duration::from_secs(if settings.epic_launcher_claims&&account.launcher=="epic"{u64::from(settings.browser_operation_timeout_seconds)+100}else if settings.browser_visible{u64::from(settings.browser_operation_timeout_seconds)+45}else{45}),self.provider.claim(&account, offer, &country)).await{
                        Ok(result)=>result,Err(_)=>Err("Магазин не ответил вовремя. Получение не подтверждено; перед повтором проверь библиотеку.".into())
                    };
                    if outcome
                        .as_ref()
                        .err()
                        .is_some_and(|message| retryable_error(message))
                        && retry < settings.retry_attempts
                    {
                        drop(steam_guard);
                        retry_delay(settings, retry).await;
                        retry += 1;
                        continue;
                    }
                    let result = match outcome {
                        Ok(Claimed::Added) => {
                            let claim = ClaimHistory {
                                id: None,
                                offer_id: Some(offer.key()),
                                launcher: account.launcher.clone(),
                                game_title: offer.title.clone(),
                                account_id: Some(account.id.clone()),
                                claimed_at: chrono::Utc::now().to_rfc3339(),
                            };
                            self.db.record_claim_once(&claim).await.map_err(|_| "Лицензия получена, но историю не удалось сохранить. Повторная проверка подтвердит владение.")?;
                            if account.launcher == "steam" {
                                if self.cache_steam_game(&account.id, offer).await.is_err() {
                                    tracing::warn!(
                                        "Steam giveaway claimed, SWCH library refresh deferred"
                                    );
                                }
                            }
                            row(
                                Some(offer),
                                "claimed",
                                "Игра добавлена в библиотеку.".into(),
                            )
                        }
                        Ok(Claimed::Owned) => {
                            // Persist a separate ownership receipt, so an owned game
                            // doesn't get an invented 'claimed by SWCH' history entry.
                            self.db
                                .set_auto_claim_value(
                                    &format!(
                                        "owned:{}:{}:{}",
                                        account.launcher,
                                        account.id,
                                        offer.key()
                                    ),
                                    "true",
                                )
                                .await
                                .map_err(|_| "Не удалось сохранить результат проверки лицензии.")?;
                            row(Some(offer), "owned", "Игра уже есть на аккаунте.".into())
                        }
                        Ok(Claimed::CodeReceived) => {
                            let id = keys::receipt_id(&account.launcher, &account.id, &offer.key());
                            let receipt =
                                tokio::task::spawn_blocking(move || keys::read_receipt(&id))
                                    .await
                                    .map_err(|_| {
                                        "Не удалось прочитать полученный ключ.".to_string()
                                    })??;
                            keys::record(&self.db, receipt).await?;
                            row(
                                Some(offer),
                                "code_received",
                                "Ключ сохранён в хранилище ОС. Выбери один аккаунт для активации."
                                    .into(),
                            )
                        }
                        Err(message) => {
                            browser_challenge |= is_browser_challenge_error(&message);
                            row(Some(offer), "attention", message)
                        }
                    };
                    run.results.push(result);
                    *self.run.write().await = run.clone();
                    drop(steam_guard);
                    // Requests are sequential to avoid order bursts across accounts.
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    break;
                }
            }
            *self.run.write().await = run.clone();
        }
        Ok(())
    }
    pub fn start(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        tokio::spawn(async move {
            // A maintenance restart can verify login without replaying an
            // immediate claim. This process-only flag preserves saved settings
            // and the existing schedule; subsequent ticks run normally.
            let skip_startup = std::env::var("SWCH_SKIP_AUTO_CLAIM_STARTUP").as_deref() == Ok("1");
            let mut timer = tokio::time::interval(Duration::from_secs(60));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut first_tick = true;
            let mut known = BTreeSet::new();
            loop {
                timer.tick().await;
                let settings = match service.settings().await {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let previous = service.status().await.unwrap_or_default();
                let accounts = match service.accounts().await {
                    Ok(accounts) => accounts,
                    Err(_) => continue,
                }
                .into_iter()
                .filter(|a| settings.platform_enabled(&a.launcher))
                .map(|a| format!("{}:{}", a.launcher, a.id))
                .collect::<BTreeSet<_>>();
                let initializing = first_tick;
                let newly_added = !initializing && accounts.difference(&known).next().is_some();
                first_tick = false;
                if initializing {
                    known = accounts.clone();
                }
                if !settings.enabled {
                    known = accounts;
                    continue;
                }
                if previous.running {
                    continue;
                }
                if initializing && skip_startup {
                    continue;
                }
                let now = Utc::now();
                if initializing && !settings.run_on_startup {
                    // Existing accounts are the startup baseline, not additions.
                    // Start a fresh interval and skip missed daily slots.
                    if let Ok(completed) = service.consume_daily_slots(&settings, now).await {
                        if let Ok(next) = next_due(&settings, now, Some(now), &completed) {
                            let _ = service.save_schedule(next).await;
                        }
                    }
                    continue;
                }
                let deadline = previous
                    .next_check_at
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|time| time.with_timezone(&Utc));
                let due = (initializing && settings.run_on_startup)
                    || newly_added
                    || deadline.is_some_and(|time| time <= now);
                if due {
                    match service.check_mode(false, None, None).await {
                        Ok(_) => known = accounts,
                        Err(message) => tracing::warn!("Auto claim: {}", message),
                    }
                } else if deadline.is_none() {
                    if let Ok(completed) = service.completed_daily_slots().await {
                        if let Ok(next) = next_due(&settings, now, Some(now), &completed) {
                            let _ = service.save_schedule(next).await;
                        }
                    }
                    known = accounts;
                } else {
                    known = accounts;
                }
            }
        })
    }

    async fn cache_steam_game(
        &self,
        account_id: &str,
        offer: &Offer,
    ) -> Result<(), crate::storage::DbError> {
        use crate::launchers::steam::{library::valid_app_id, library::SteamGame};
        let key = format!("giveaways:{account_id}");
        let mut games: Vec<SteamGame> = self
            .db
            .steam_cache_get(&key)
            .await?
            .and_then(|(s, _)| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        for app in offer.item_ids.iter().filter(|id| valid_app_id(id)) {
            let mut game = SteamGame::new(app, Some(account_id.into()));
            game.game.name = offer.title.clone();
            game.access = "owned".into();
            game.source = "store_license".into();
            game.owner_steamids = vec![account_id.into()];
            game.is_free = Some(false);
            games.retain(|g| g.game.app_id != *app);
            games.push(game);
        }
        self.db
            .steam_cache_put(&key, &serde_json::to_string(&games).unwrap())
            .await
    }
}
