use chrono::{DateTime, NaiveDate, Utc};
use serde_json::json;
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use swch::{
    auto_claim::{
        daily_slot_id, next_due,
        offers::Offer,
        providers::{Call, Claimed, Provider},
        retryable_error, AutoClaimer, Run, Settings,
    },
    storage::{Account, Database},
};
use tokio::sync::Notify;

fn at(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}
fn daily(timezone: &str, times: &[&str]) -> Settings {
    Settings {
        schedule_mode: "daily".into(),
        timezone: timezone.into(),
        daily_times: times.iter().map(|v| (*v).into()).collect(),
        ..Settings::default()
    }
}

#[test]
fn legacy_settings_get_new_defaults_and_keep_steam_epic_flags() {
    let settings: Settings = serde_json::from_value(
        json!({"enabled":false,"steam":false,"epic":true,"interval_minutes":90}),
    )
    .unwrap();
    settings.validate().unwrap();
    assert!(!settings.platform_enabled("steam"));
    assert!(settings.platform_enabled("epic"));
    for id in [
        "gog",
        "prime",
        "microsoft",
        "ubisoft",
        "fanatical",
        "itchio",
        "indiegala",
        "alienware",
    ] {
        assert!(settings.platform_enabled(id), "{id}");
    }
    assert!(!settings.platform_enabled("fab"));
    assert!(
        settings.gamerpower
            && settings.run_on_startup
            && settings.epic_mobile
            && settings.browser_enabled
    );
    assert!(!settings.dry_run);
    assert_eq!(settings.retry_attempts, 2);
    assert_eq!(settings.browser_login_timeout_seconds, 180);
    assert_eq!(settings.browser_operation_timeout_seconds, 120);
}

#[test]
fn schedules_reject_unknown_duplicate_and_malformed_values() {
    for value in [
        json!({"platforms":["fab"]}),
        json!({"platforms":["gog","gog"]}),
        json!({"schedule_mode":"weekly"}),
        json!({"schedule_mode":"daily"}),
        json!({"daily_times":["9:00"]}),
        json!({"daily_times":["24:00"]}),
        json!({"daily_times":["09:00","09:00"]}),
        json!({"timezone":"Mars/Olympus"}),
        json!({"retry_attempts":6}),
        json!({"retry_backoff_seconds":301}),
        json!({"browser_login_timeout_seconds":601}),
        json!({"browser_operation_timeout_seconds":0}),
    ] {
        let settings: Settings = serde_json::from_value(value.clone()).unwrap();
        assert!(settings.validate().is_err(), "{value}");
    }
    let settings = Settings {
        retry_attempts: 0,
        retry_backoff_seconds: 0,
        ..Settings::default()
    };
    settings.validate().unwrap();
}

#[test]
fn interval_and_combined_use_the_earliest_deadline() {
    let now = at("2026-10-07T10:00:00Z");
    let completed = BTreeSet::new();
    assert_eq!(
        next_due(&Settings::default(), now, None, &completed).unwrap(),
        Some(at("2026-10-07T11:00:00Z"))
    );
    assert_eq!(
        next_due(
            &Settings::default(),
            now,
            Some(at("2026-10-07T08:00:00Z")),
            &completed
        )
        .unwrap(),
        Some(at("2026-10-07T09:00:00Z"))
    );
    let mut settings = daily("Europe/Minsk", &["14:00", "13:15"]);
    settings.schedule_mode = "combined".into();
    assert_eq!(
        next_due(&settings, now, Some(now), &completed).unwrap(),
        Some(at("2026-10-07T10:15:00Z"))
    );
    settings.enabled = false;
    assert_eq!(
        next_due(&settings, now, Some(now), &completed).unwrap(),
        None
    );
}

#[test]
fn daily_uses_account_independent_timezone_and_consumed_wall_clock_slots() {
    let settings = daily("Europe/Minsk", &["14:00", "13:15"]);
    let now = at("2026-10-07T10:00:00Z");
    let mut completed = BTreeSet::new();
    completed.insert(daily_slot_id(
        "Europe/Minsk",
        NaiveDate::from_ymd_opt(2026, 10, 7).unwrap(),
        "13:15",
    ));
    assert_eq!(
        next_due(&settings, now, None, &completed).unwrap(),
        Some(at("2026-10-07T11:00:00Z"))
    );
    assert_eq!(
        next_due(&daily("UTC", &["10:00"]), now, None, &BTreeSet::new()).unwrap(),
        Some(now)
    );
    assert!(
        next_due(&daily("local", &["10:00"]), now, None, &BTreeSet::new())
            .unwrap()
            .is_some()
    );
}

#[test]
fn dst_fall_uses_first_occurrence_and_never_replays_the_same_date_slot() {
    let settings = daily("America/New_York", &["01:30"]);
    let before = at("2026-11-01T04:00:00Z");
    assert_eq!(
        next_due(&settings, before, None, &BTreeSet::new()).unwrap(),
        Some(at("2026-11-01T05:30:00Z"))
    );
    let completed = BTreeSet::from([daily_slot_id(
        "America/New_York",
        NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
        "01:30",
    )]);
    assert_eq!(
        next_due(&settings, at("2026-11-01T05:30:00Z"), None, &completed).unwrap(),
        Some(at("2026-11-02T06:30:00Z"))
    );
    assert_eq!(
        next_due(
            &settings,
            at("2026-11-01T06:00:00Z"),
            None,
            &BTreeSet::new()
        )
        .unwrap(),
        Some(at("2026-11-02T06:30:00Z"))
    );
}

#[test]
fn dst_spring_skips_a_nonexistent_daily_time() {
    let settings = daily("America/New_York", &["02:30"]);
    assert_eq!(
        next_due(
            &settings,
            at("2026-03-08T05:00:00Z"),
            None,
            &BTreeSet::new()
        )
        .unwrap(),
        Some(at("2026-03-09T06:30:00Z"))
    );
}

#[test]
fn retries_only_known_transport_and_service_errors() {
    for message in [
        "HTTP 503. Возможно, нужен повторный вход",
        "Network error",
        "Connection reset",
        "HTTP 429",
        "Магазин недоступен. Проверка будет повторена.",
    ] {
        assert!(retryable_error(message), "{message}");
    }
    for message in [
        "HTTP 403: timeout",
        "HTTP 401",
        "CAPTCHA service unavailable",
        "Cloudflare challenge",
        "Login expired: timeout",
        "Регион: timeout",
        "Не удалось подтвердить нулевую цену",
        "Unknown store error",
    ] {
        assert!(!retryable_error(message), "{message}");
    }
}

fn gift(launcher: &str) -> Offer {
    Offer {
        launcher: launcher.into(),
        id: "gift".into(),
        namespace: String::new(),
        title: "Paid game giveaway".into(),
        url: "https://example.invalid/game".into(),
        original_price: Some(1000),
        normally_paid: true,
        final_price: 0,
        ends_at: Some(Utc::now().timestamp() + 3600),
        item_ids: vec!["10".into()],
    }
}
#[derive(Default)]
struct Fake {
    country_calls: AtomicUsize,
    discovery_calls: AtomicUsize,
    claim_calls: AtomicUsize,
    country_failures: usize,
    discovery_failures: usize,
    claim_failures: usize,
    claim_error: String,
    accounts_seen: Notify,
    slow_claim: bool,
    claim_started: Notify,
    run_settings: std::sync::Mutex<Vec<Settings>>,
}
impl Provider for Fake {
    fn begin_run<'a>(&'a self, settings: &'a Settings) -> Call<'a, ()> {
        Box::pin(async move {
            self.run_settings.lock().unwrap().push(settings.clone());
            Ok(())
        })
    }
    fn accounts<'a>(&'a self, accounts: Vec<Account>) -> Call<'a, Vec<Account>> {
        Box::pin(async move {
            self.accounts_seen.notify_one();
            Ok(accounts)
        })
    }
    fn country<'a>(&'a self, _: &'a Account) -> Call<'a, String> {
        Box::pin(async move {
            let attempt = self.country_calls.fetch_add(1, Ordering::SeqCst);
            if attempt < self.country_failures {
                Err("Network error".into())
            } else {
                Ok("BY".into())
            }
        })
    }
    fn discover<'a>(&'a self, launcher: &'a str, _: &'a str) -> Call<'a, Vec<Offer>> {
        Box::pin(async move {
            let attempt = self.discovery_calls.fetch_add(1, Ordering::SeqCst);
            if attempt < self.discovery_failures {
                return Err("HTTP 503".into());
            }
            let mut permanent = gift(launcher);
            permanent.id = "f2p".into();
            permanent.normally_paid = false;
            permanent.original_price = Some(0);
            let mut paid = gift(launcher);
            paid.id = "still-paid".into();
            paid.final_price = 1;
            let mut expired = gift(launcher);
            expired.id = "expired".into();
            expired.ends_at = Some(1);
            let mut wrong_store = gift("riot");
            wrong_store.id = "wrong-store".into();
            Ok(vec![gift(launcher), permanent, paid, expired, wrong_store])
        })
    }
    fn claim<'a>(&'a self, _: &'a Account, _: &'a Offer, _: &'a str) -> Call<'a, Claimed> {
        Box::pin(async move {
            if self.slow_claim {
                self.claim_started.notify_one();
                std::future::pending::<()>().await;
            }
            let attempt = self.claim_calls.fetch_add(1, Ordering::SeqCst);
            if attempt < self.claim_failures {
                Err(self.claim_error.clone())
            } else {
                Ok(Claimed::Added)
            }
        })
    }
}
async fn database() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    db
}
async fn add_account(db: &Database, id: &str, launcher: &str) {
    db.upsert_account(&Account {
        id: id.into(),
        launcher: launcher.into(),
        username: id.into(),
        display_name: None,
        email: None,
        is_active: false,
        last_used: None,
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn dry_run_validates_all_enabled_accounts_without_claim_history_or_receipts() {
    let db = database().await;
    for (id, launcher) in [("1", "steam"), ("2", "epic"), ("3", "gog"), ("4", "riot")] {
        add_account(&db, id, launcher).await;
    }
    let fake = Arc::new(Fake::default());
    let service = AutoClaimer::with_provider(db.clone(), fake.clone());
    service
        .save_settings(&Settings {
            dry_run: true,
            ..Settings::default()
        })
        .await
        .unwrap();
    let run = service.check().await.unwrap();
    assert_eq!(run.accounts_checked, 3);
    assert_eq!(fake.country_calls.load(Ordering::SeqCst), 3);
    assert_eq!(fake.discovery_calls.load(Ordering::SeqCst), 3);
    assert_eq!(fake.claim_calls.load(Ordering::SeqCst), 0);
    assert_eq!(run.results.len(), 3);
    assert!(run
        .results
        .iter()
        .all(|row| row.status == "preview" && row.offer_id == "gift"));
    assert!(db.get_claim_history().await.unwrap().is_empty());
    assert!(db
        .auto_claim_value("owned:steam:1:gift")
        .await
        .unwrap()
        .is_none());
    assert!(db.steam_cache_get("giveaways:1").await.unwrap().is_none());
}

#[tokio::test]
async fn private_prime_catalogs_are_not_shared_between_accounts_in_the_same_country() {
    let db = database().await;
    for (id, platform) in [
        ("prime-one", "prime"),
        ("prime-two", "prime"),
        ("steam-one", "steam"),
        ("steam-two", "steam"),
    ] {
        add_account(&db, id, platform).await;
    }
    let fake = Arc::new(Fake::default());
    let service = AutoClaimer::with_provider(db, fake.clone());
    service
        .save_settings(&Settings {
            dry_run: true,
            ..Settings::default()
        })
        .await
        .unwrap();
    let run = service.check().await.unwrap();
    assert_eq!(run.accounts_checked, 4);
    assert_eq!(fake.discovery_calls.load(Ordering::SeqCst), 3);
    assert_eq!(fake.claim_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn temporary_country_discovery_and_claim_failures_retry_before_recording_one_license() {
    let db = database().await;
    add_account(&db, "1", "epic").await;
    let fake = Arc::new(Fake {
        country_failures: 1,
        discovery_failures: 1,
        claim_failures: 2,
        claim_error: "HTTP 503".into(),
        ..Fake::default()
    });
    let service = AutoClaimer::with_provider(db.clone(), fake.clone());
    service
        .save_settings(&Settings {
            retry_backoff_seconds: 0,
            ..Settings::default()
        })
        .await
        .unwrap();
    let run = service.check().await.unwrap();
    assert_eq!(fake.country_calls.load(Ordering::SeqCst), 2);
    assert_eq!(fake.discovery_calls.load(Ordering::SeqCst), 2);
    assert_eq!(fake.claim_calls.load(Ordering::SeqCst), 3);
    assert_eq!(db.get_claim_history().await.unwrap().len(), 1);
    assert_eq!(run.results[0].status, "claimed");
}

#[tokio::test]
async fn forbidden_and_captcha_claims_are_never_retried_or_counted_as_ownership() {
    for message in [
        "HTTP 403",
        "CAPTCHA",
        "Повтори вход Epic Games для этого аккаунта.",
    ] {
        let db = database().await;
        add_account(&db, "1", "epic").await;
        let fake = Arc::new(Fake {
            claim_failures: 10,
            claim_error: message.into(),
            ..Fake::default()
        });
        let service = AutoClaimer::with_provider(db.clone(), fake.clone());
        service
            .save_settings(&Settings {
                retry_backoff_seconds: 0,
                ..Settings::default()
            })
            .await
            .unwrap();
        let run = service.check().await.unwrap();
        assert_eq!(fake.claim_calls.load(Ordering::SeqCst), 1, "{message}");
        assert_eq!(run.results[0].status, "attention");
        assert!(db.get_claim_history().await.unwrap().is_empty());
        assert!(db
            .auto_claim_value("owned:epic:1:gift")
            .await
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn startup_disabled_resets_a_stale_deadline_without_contacting_store() {
    let db = database().await;
    add_account(&db, "1", "epic").await;
    db.set_auto_claim_value(
        "settings",
        &serde_json::to_string(&Settings {
            run_on_startup: false,
            ..Settings::default()
        })
        .unwrap(),
    )
    .await
    .unwrap();
    db.set_auto_claim_value(
        "last_run",
        &serde_json::to_string(&Run {
            next_check_at: Some("2000-01-01T00:00:00Z".into()),
            ..Run::default()
        })
        .unwrap(),
    )
    .await
    .unwrap();
    let fake = Arc::new(Fake::default());
    let service = AutoClaimer::with_provider(db, fake.clone());
    let task = service.start();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if service
                .status()
                .await
                .unwrap()
                .next_check_at
                .as_deref()
                .is_some_and(|value| at(value) > Utc::now())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert_eq!(fake.country_calls.load(Ordering::SeqCst), 0);
    assert_eq!(fake.claim_calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn scoped_manual_run_uses_only_the_selected_account_and_does_not_change_preferences() {
    let db = database().await;
    add_account(&db, "first", "epic").await;
    add_account(&db, "second", "epic").await;
    add_account(&db, "third", "steam").await;
    let fake = Arc::new(Fake::default());
    let service = AutoClaimer::with_provider(db.clone(), fake.clone());
    let settings = Settings {
        browser_visible: false,
        epic_launcher_claims: true,
        browser_enabled: false,
        ..Settings::default()
    };
    service.save_settings(&settings).await.unwrap();
    let run = service.check_account("epic", "second", true).await.unwrap();
    assert_eq!(run.accounts_total, 1);
    assert_eq!(run.accounts_checked, 1);
    assert!(run.results.iter().all(|r| r.account_id == "second"));
    assert_eq!(fake.claim_calls.load(Ordering::SeqCst), 1);
    let effective = fake.run_settings.lock().unwrap()[0].clone();
    assert!(effective.browser_visible && effective.epic_launcher_claims);
    assert!(!effective.browser_enabled);
    assert_eq!(service.settings().await.unwrap(), settings);
    assert!(db
        .auto_claim_value("account-results:epic:second")
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn manual_background_overrides_suppress_native_input_for_all_or_one_account() {
    for target in [None, Some(("epic".into(), "second".into()))] {
        let db = database().await;
        add_account(&db, "first", "epic").await;
        add_account(&db, "second", "epic").await;
        let fake = Arc::new(Fake::default());
        let service = AutoClaimer::with_provider(db, fake.clone());
        let saved = Settings {
            browser_visible: true,
            epic_launcher_claims: true,
            browser_enabled: false,
            ..Settings::default()
        };
        service.save_settings(&saved).await.unwrap();
        let expected_accounts = if target.is_some() { 1 } else { 2 };
        let run = service.check_manual(target, Some(false)).await.unwrap();
        assert_eq!(run.accounts_checked, expected_accounts);
        let effective = fake.run_settings.lock().unwrap()[0].clone();
        assert!(!effective.browser_visible && !effective.epic_launcher_claims);
        assert!(!effective.browser_enabled);
        assert_eq!(service.settings().await.unwrap(), saved);
    }
}

#[tokio::test]
async fn saved_background_preference_suppresses_native_input_in_manual_and_scheduled_runs() {
    for scheduled in [false, true] {
        let db = database().await;
        add_account(&db, "one", "epic").await;
        let fake = Arc::new(Fake::default());
        let service = AutoClaimer::with_provider(db, fake.clone());
        let saved = Settings {
            browser_visible: false,
            epic_launcher_claims: true,
            browser_enabled: false,
            ..Settings::default()
        };
        service.save_settings(&saved).await.unwrap();
        if scheduled {
            let task = service.start();
            wait_for_completed_run(&service).await;
            task.abort();
            let _ = task.await;
        } else {
            service.check().await.unwrap();
        }
        let effective = fake.run_settings.lock().unwrap()[0].clone();
        assert!(!effective.browser_visible && !effective.epic_launcher_claims);
        assert!(!effective.browser_enabled);
        assert_eq!(fake.claim_calls.load(Ordering::SeqCst), 1);
        assert_eq!(service.settings().await.unwrap(), saved);
    }
}

async fn wait_for_completed_run(service: &AutoClaimer) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let run = service.status().await.unwrap();
            if run.started_at.is_some() && !run.running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn check_api_applies_visibility_to_global_and_scoped_runs_and_inherits_when_omitted() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Router,
    };
    use swch::{
        launchers::epic::{EpicAuthStore, EpicClient, MemoryCredentialStore},
        server::{create_app_with_auto_claim, AppState},
    };
    use tower::ServiceExt;

    for (scoped, override_visible, saved_visible) in [
        (false, Some(false), true),
        (true, Some(false), true),
        (true, None, false),
        (false, Some(true), false),
    ] {
        let db = database().await;
        add_account(&db, "one", "epic").await;
        let fake = Arc::new(Fake::default());
        let service = AutoClaimer::with_provider(db.clone(), fake.clone());
        let saved = Settings {
            browser_visible: saved_visible,
            epic_launcher_claims: true,
            browser_enabled: false,
            ..Settings::default()
        };
        service.save_settings(&saved).await.unwrap();
        let epic = Arc::new(EpicAuthStore::new(
            db.clone(),
            Arc::new(MemoryCredentialStore::new()),
            Arc::new(EpicClient::from_router(Router::new())),
        ));
        let app = create_app_with_auto_claim(AppState::new(db, epic), service.clone());
        let mut body = json!({});
        if scoped {
            body["platform"] = json!("epic");
            body["account_id"] = json!("one");
        }
        if let Some(visible) = override_visible {
            body["visible"] = json!(visible);
        }
        let request = Request::builder()
            .method("POST")
            .uri("/api/free-games/check")
            .header("content-type", "application/json")
            .header("origin", "http://127.0.0.1:4242")
            .body(Body::from(body.to_string()))
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::ACCEPTED
        );
        wait_for_completed_run(&service).await;
        let effective = fake.run_settings.lock().unwrap()[0].clone();
        let visible = override_visible.unwrap_or(saved_visible);
        assert_eq!(effective.browser_visible, visible);
        assert_eq!(effective.epic_launcher_claims, visible);
        assert!(!effective.browser_enabled);
        assert_eq!(fake.claim_calls.load(Ordering::SeqCst), 1);
        assert_eq!(service.settings().await.unwrap(), saved);
    }
}

#[tokio::test]
async fn cancellation_releases_the_shared_run_lock_and_never_invents_ownership() {
    let db = database().await;
    add_account(&db, "one", "epic").await;
    let fake = Arc::new(Fake {
        slow_claim: true,
        ..Fake::default()
    });
    let service = AutoClaimer::with_provider(db.clone(), fake.clone());
    let task = {
        let s = service.clone();
        tokio::spawn(async move { s.check().await })
    };
    tokio::time::timeout(Duration::from_secs(2), fake.claim_started.notified())
        .await
        .unwrap();
    let active = service.status().await.unwrap();
    assert!(active.running);
    assert_eq!(active.current.unwrap().account_id, "one");
    assert!(service.cancel().await);
    let run = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(run.cancelled);
    assert!(!run.running);
    assert!(run.current.is_none());
    assert!(db.get_claim_history().await.unwrap().is_empty());
    service
        .save_settings(&Settings {
            dry_run: true,
            ..Settings::default()
        })
        .await
        .unwrap();
    assert!(!service.check().await.unwrap().cancelled);
}

#[test]
fn browser_choice_defaults_for_existing_settings_and_rejects_unknown_paths() {
    let settings: Settings = serde_json::from_value(json!({"interval_minutes":1440})).unwrap();
    assert_eq!(settings.browser_name, "auto");
    assert!(settings.validate().is_ok());
    for name in ["auto", "chrome", "edge"] {
        assert!(Settings {
            browser_name: name.into(),
            ..Settings::default()
        }
        .validate()
        .is_ok());
    }
    assert!(Settings {
        browser_name: "C:/custom/browser.exe".into(),
        ..Settings::default()
    }
    .validate()
    .is_err());
}
