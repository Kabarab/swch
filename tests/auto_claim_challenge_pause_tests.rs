use std::sync::{Arc, Mutex};
use swch::{
    auto_claim::{
        is_browser_challenge_error,
        offers::Offer,
        providers::{Call, Claimed, Provider},
        retryable_error, AutoClaimer, Settings,
    },
    storage::{Account, Database},
};

struct ChallengeProvider {
    error: Mutex<Option<String>>,
    calls: Mutex<Vec<(String, String)>>,
}

impl Provider for ChallengeProvider {
    fn country<'a>(&'a self, _: &'a Account) -> Call<'a, String> {
        Box::pin(async { Ok("BY".into()) })
    }

    fn discover<'a>(&'a self, launcher: &'a str, _: &'a str) -> Call<'a, Vec<Offer>> {
        Box::pin(async move {
            Ok(["first", "second"]
                .into_iter()
                .map(|id| Offer {
                    launcher: launcher.into(),
                    id: id.into(),
                    namespace: "giveaway".into(),
                    title: format!("Paid game {id}"),
                    url: format!("https://store.epicgames.com/en-US/p/{id}"),
                    original_price: Some(1000),
                    normally_paid: true,
                    final_price: 0,
                    ends_at: Some(chrono::Utc::now().timestamp() + 3600),
                    item_ids: vec![id.into()],
                })
                .collect())
        })
    }

    fn claim<'a>(&'a self, account: &'a Account, offer: &'a Offer, _: &'a str) -> Call<'a, Claimed> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((account.id.clone(), offer.id.clone()));
            if account.id == "blocked" {
                if let Some(message) = self.error.lock().unwrap().clone() {
                    return Err(message);
                }
            }
            Ok(Claimed::Added)
        })
    }
}

async fn service(message: &str) -> (Database, Arc<AutoClaimer>, Arc<ChallengeProvider>) {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    for id in ["blocked", "unaffected"] {
        db.upsert_account(&Account {
            id: id.into(),
            launcher: "epic".into(),
            username: id.into(),
            display_name: None,
            email: None,
            is_active: false,
            last_used: None,
        })
        .await
        .unwrap();
    }
    let provider = Arc::new(ChallengeProvider {
        error: Mutex::new(Some(message.into())),
        calls: Mutex::new(vec![]),
    });
    let service = AutoClaimer::with_provider(db.clone(), provider.clone());
    service
        .save_settings(&Settings {
            retry_attempts: 5,
            retry_backoff_seconds: 0,
            ..Settings::default()
        })
        .await
        .unwrap();
    (db, service, provider)
}

#[tokio::test]
async fn security_check_skips_other_offers_only_for_this_account_and_this_run() {
    let message = "Магазин показал проверку безопасности; CAPTCHA не завершена.";
    let (db, service, provider) = service(message).await;
    let settings = service.settings().await.unwrap();
    let run = service.check().await.unwrap();
    let calls = provider.calls.lock().unwrap().clone();
    assert_eq!(calls.iter().filter(|(id, _)| id == "blocked").count(), 1);
    assert_eq!(calls.iter().filter(|(id, _)| id == "unaffected").count(), 2);
    let blocked = run
        .results
        .iter()
        .filter(|row| row.account_id == "blocked")
        .collect::<Vec<_>>();
    assert_eq!(blocked.len(), 2);
    assert!(blocked.iter().all(|row| row.status == "attention"));
    assert_eq!(blocked[0].message, message);
    assert!(blocked[1].message.contains("Получение не запускалось"));
    assert!(blocked[1].message.contains("пропущены в этом запуске"));
    assert_ne!(blocked[0].offer_id, blocked[1].offer_id);
    let history = db.get_claim_history().await.unwrap();
    assert_eq!(history.len(), 2);
    assert!(history
        .iter()
        .all(|row| row.account_id.as_deref() == Some("unaffected")));
    assert_eq!(service.settings().await.unwrap(), settings);

    // Resolving the challenge permits a later run, without reordering already
    // claimed games on the other account or changing the user's preferences.
    *provider.error.lock().unwrap() = None;
    let next = service.check().await.unwrap();
    let calls = provider.calls.lock().unwrap().clone();
    assert_eq!(calls.iter().filter(|(id, _)| id == "blocked").count(), 3);
    assert_eq!(calls.iter().filter(|(id, _)| id == "unaffected").count(), 2);
    assert!(next.results.iter().all(|row| row.status == "claimed"));
    assert_eq!(db.get_claim_history().await.unwrap().len(), 4);
}

#[tokio::test]
async fn plain_http_forbidden_does_not_skip_other_offers_for_the_account() {
    let (db, service, provider) = service("Epic отклонил оформление (HTTP 403).").await;
    let run = service.check().await.unwrap();
    let calls = provider.calls.lock().unwrap().clone();
    assert_eq!(calls.iter().filter(|(id, _)| id == "blocked").count(), 2);
    assert_eq!(calls.iter().filter(|(id, _)| id == "unaffected").count(), 2);
    assert_eq!(db.get_claim_history().await.unwrap().len(), 2);
    assert!(!run
        .results
        .iter()
        .any(|row| row.message.contains("Получение не запускалось")));
}

#[test]
fn only_explicit_security_checks_block_the_account_and_prevent_transport_retries() {
    for message in [
        "CAPTCHA",
        "Cloudflare: One more step",
        "Please complete a security check to continue",
        "Verify you are human",
        "Проверка безопасности магазина не завершилась",
        "Магазин показал проверку безопасности",
        "Проверка браузера требует действия пользователя",
        "HTTP 503: CAPTCHA timeout",
    ] {
        assert!(is_browser_challenge_error(message), "{message}");
        assert!(!retryable_error(message), "{message}");
    }
    for message in [
        "HTTP 403",
        "HTTP 503",
        "Login expired",
        "Network error",
        "Магазин не подтвердил вход этого аккаунта",
        "Не удалось проверить библиотеку Epic",
    ] {
        assert!(!is_browser_challenge_error(message), "{message}");
    }
    assert!(retryable_error("HTTP 503"));
}
