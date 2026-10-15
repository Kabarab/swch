//! A launch request can discover an update after -applaunch was dispatched.
//! Keep that intent in the backend and retry once only after observed update work.
use super::{
    client_launch,
    downloads::{self, DownloadSnapshot},
    SteamPaths,
};
use crate::parsers::vdf::VdfDocument;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Wait,
    Finish,
    Retry,
}

#[derive(Default)]
pub struct LaunchPolicy {
    observed_update: bool,
    known_update: bool,
    observed_client: bool,
    ready_since: Option<u64>,
    retried: bool,
}
impl LaunchPolicy {
    pub fn advance(
        &mut self,
        elapsed: u64,
        requested_at: i64,
        state: Option<&DownloadSnapshot>,
        account_matches: Option<bool>,
    ) -> Decision {
        if account_matches == Some(false) || elapsed >= 24 * 3600 || self.retried {
            return Decision::Finish;
        }
        let Some(state) = state else {
            self.ready_since = None;
            return Decision::Wait;
        };
        if !state.steam_running {
            return if self.observed_client || elapsed >= 120 {
                Decision::Finish
            } else {
                Decision::Wait
            };
        }
        self.observed_client = true;
        self.known_update =
            state.update.required || matches!(state.update.phase.as_str(), "queued" | "paused");
        if state.is_running || state.last_started_at.is_some_and(|t| t >= requested_at) {
            return Decision::Finish;
        }
        if matches!(
            state.update.phase.as_str(),
            "preparing" | "downloading" | "installing" | "verifying"
        ) || state
            .last_update_finished_at
            .is_some_and(|t| t >= requested_at)
        {
            self.observed_update = true;
        }
        if !self.observed_update {
            return if elapsed >= 120 && !self.known_update {
                Decision::Finish
            } else {
                Decision::Wait
            };
        }
        let ready = state.is_installed
            && !state.update.required
            && state.update.phase == "ready"
            && !state
                .update
                .telemetry
                .as_ref()
                .is_some_and(|t| t.data_stale);
        if !ready || account_matches != Some(true) {
            self.ready_since = None;
            return Decision::Wait;
        }
        // Give Steam time to honor its original launch queue and publish App Running.
        let since = *self.ready_since.get_or_insert(elapsed);
        if elapsed.saturating_sub(since) < 10 {
            return Decision::Wait;
        }
        self.retried = true;
        Decision::Retry
    }
    pub fn waiting_for_update(&self) -> bool {
        self.observed_update || self.known_update
    }
}

#[derive(Clone, Serialize)]
pub struct LaunchStatus {
    #[serde(skip)]
    token: u64,
    pub app_id: String,
    pub account_id: String,
    pub status: String,
}
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
static PENDING: Mutex<Option<LaunchStatus>> = Mutex::new(None);

pub fn status() -> Option<LaunchStatus> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
pub fn cancel(app_id: Option<&str>) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if app_id.is_none()
        || pending
            .as_ref()
            .is_some_and(|p| Some(p.app_id.as_str()) == app_id)
    {
        *pending = None;
    }
}
fn current(token: u64) -> bool {
    status().is_some_and(|p| p.token == token)
}
fn clear(token: u64) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if pending.as_ref().is_some_and(|p| p.token == token) {
        *pending = None;
    }
}
fn update_status(token: u64, value: &str) {
    if let Some(p) = PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .filter(|p| p.token == token)
    {
        p.status = value.into();
    }
}
fn selected_account(paths: &SteamPaths, account_id: &str) -> Option<bool> {
    let doc = VdfDocument::from_file(paths.loginusers_path()).ok()?;
    let users = doc.get_obj("users")?;
    let selected = users.iter().find(|(_, v)| {
        v.as_obj()
            .is_some_and(|u| u.get_str("MostRecent") == Some("1"))
    })?;
    Some(selected.0 == account_id)
}

pub fn start(paths: SteamPaths, app_id: String, account_id: String, args: Vec<String>) {
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(LaunchStatus {
        token,
        app_id: app_id.clone(),
        account_id: account_id.clone(),
        status: "watching".into(),
    });
    let requested_at = chrono::Utc::now().timestamp();
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let mut policy = LaunchPolicy::default();
        while current(token) {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if !current(token) {
                break;
            }
            let paths_copy = paths.clone();
            let id = app_id.clone();
            let account = account_id.clone();
            let Ok((state, matches)) = tokio::task::spawn_blocking(move || {
                let state = downloads::snapshot(&paths_copy).remove(&id);
                (state, selected_account(&paths_copy, &account))
            })
            .await
            else {
                clear(token);
                break;
            };
            match policy.advance(
                start.elapsed().as_secs(),
                requested_at,
                state.as_ref(),
                matches,
            ) {
                Decision::Wait => {
                    update_status(
                        token,
                        if policy.waiting_for_update() {
                            "waiting_update"
                        } else {
                            "watching"
                        },
                    );
                }
                Decision::Finish => {
                    clear(token);
                    break;
                }
                Decision::Retry => {
                    // Serialize with all account switching / explicit Steam launches.
                    let _guard = crate::sys::process::LAUNCHER_SWITCH_LOCK.lock().await;
                    if !current(token) || selected_account(&paths, &account_id) != Some(true) {
                        clear(token);
                        break;
                    }
                    let fresh_paths = paths.clone();
                    let fresh_id = app_id.clone();
                    let fresh = tokio::task::spawn_blocking(move || {
                        downloads::snapshot(&fresh_paths).remove(&fresh_id)
                    })
                    .await
                    .ok()
                    .flatten();
                    if !current(token)
                        || !fresh.as_ref().is_some_and(|s| {
                            s.steam_running
                                && s.is_installed
                                && s.update.phase == "ready"
                                && !s.update.required
                                && !s.is_running
                                && !s.last_started_at.is_some_and(|t| t >= requested_at)
                                && !s.update.telemetry.as_ref().is_some_and(|t| t.data_stale)
                        })
                    {
                        clear(token);
                        break;
                    }
                    update_status(token, "launching");
                    // Never switch accounts here or launch executables directly.
                    let result = match client_launch::command(&paths, &app_id, false, &args) {
                        Ok(command) => command.dispatch().await,
                        Err(e) => Err(e),
                    };
                    if result.is_err() {
                        update_status(token, "failed");
                    } else {
                        clear(token);
                    }
                    break;
                }
            }
        }
    });
}
