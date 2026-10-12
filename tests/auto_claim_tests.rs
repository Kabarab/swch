use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use swch::{
    auto_claim::{
        offers::{epic_offers, search_app_ids, steam_offer, Offer},
        providers::{Call, Claimed, Provider},
        steam_auth::SteamSession,
        AutoClaimer, Settings,
    },
    launchers::epic::{EpicAuthStore, EpicClient, MemoryCredentialStore},
    server::{create_app_with_auto_claim, AppState},
    storage::{Account, Database},
};
use tower::ServiceExt;

fn offer(launcher: &str) -> Offer {
    Offer {
        launcher: launcher.into(),
        id: "gift".into(),
        namespace: if launcher == "epic" {
            "ns".into()
        } else {
            String::new()
        },
        title: "Paid game".into(),
        url: "https://store.epicgames.com/en-US/p/game".into(),
        original_price: Some(1000),
        normally_paid: true,
        final_price: 0,
        ends_at: Some(chrono::Utc::now().timestamp() + 3600),
        item_ids: vec![if launcher == "steam" {
            "10".into()
        } else {
            "item".into()
        }],
    }
}
fn epic_game() -> Value {
    json!({"id":"offer","namespace":"ns","offerType":"BASE_GAME","title":"Paid game","items":[{"id":"item"}],
        "catalogNs":{"mappings":[{"pageSlug":"paid-game"}]},"price":{"totalPrice":{"originalPrice":1000,"discountPrice":0}},
        "promotions":{"promotionalOffers":[{"promotionalOffers":[{"startDate":"2026-10-01T00:00:00Z","endDate":"2026-10-08T00:00:00Z","discountSetting":{"discountType":"PERCENTAGE","discountPercentage":0}}]}]}})
}
fn epic_data(game: Value) -> Value {
    json!({"data":{"Catalog":{"searchStore":{"elements":[game]}}}})
}
#[test]
fn only_active_full_discounts_of_paid_games() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-06T00:00:00Z")
        .unwrap()
        .timestamp();
    assert_eq!(epic_offers(&epic_data(epic_game()), now).unwrap().len(), 1);
    for (path, value) in [
        ("/price/totalPrice/originalPrice",json!(0)), ("/price/totalPrice/discountPrice",json!(1)),
        ("/offerType",json!("DLC")), ("/offerType",json!("DEMO")),
        ("/promotions/promotionalOffers/0/promotionalOffers/0/startDate",json!("2026-10-07T00:00:00Z")),
        ("/promotions/promotionalOffers/0/promotionalOffers/0/endDate",json!("2026-10-06T00:00:00Z")),
        ("/promotions/promotionalOffers/0/promotionalOffers/0/discountSetting/discountPercentage",json!(50)),
    ] {
        let mut game=epic_game(); *game.pointer_mut(path).unwrap()=value;
        assert!(epic_offers(&epic_data(game),now).unwrap().is_empty(),"{path}");
    }
    let mut game = epic_game();
    game["promotions"] = json!({"upcomingPromotionalOffers":[{}]});
    assert!(epic_offers(&epic_data(game), now).unwrap().is_empty());
    assert!(epic_offers(&json!({}), now).is_err());
}
#[test]
fn steam_base_game_promotion_never_claims_a_demo_or_weekend() {
    let data = json!({"type":"game","name":"Paid game","price_overview":{"initial":1000,"final":0,"discount_percent":100}});
    let html="<div class=\"game_area_purchase_game_wrapper\"><span>-100%</span><a onclick=\"AddFreeLicense( 123 );\">Add to Account</a></div>";
    assert_eq!(steam_offer("10", &data, html, 1).unwrap().id, "123");
    assert!(steam_offer("10", &data, &html.replace("-100%", "Free Weekend -100%"), 1).is_none());
    assert!(steam_offer("10", &data, &html.replace("-100%", "Free"), 1).is_none());
    let mut free = data.clone();
    free["price_overview"]["initial"] = json!(0);
    assert!(steam_offer("10", &free, html, 1).is_none());
    let mut demo = data.clone();
    demo["type"] = json!("demo");
    assert!(steam_offer("10", &demo, html, 1).is_none());
    let ended = html.replace("<span>", "<span data-discount-end=\"1\">");
    assert!(steam_offer("10", &data, &ended, 1).is_none());
    let page="<div class=\"game_area_purchase_game_wrapper\">-100% Buy paid edition</div><div class=\"game_area_purchase_game_wrapper\">Free demo <a onclick=\"AddFreeLicense( 456 );\">Demo</a></div>";
    assert!(steam_offer("10", &data, page, 1).is_none());
    assert_eq!(
        search_app_ids("<a data-ds-appid=\"10\"><b>-100%</b></a><a data-ds-appid=\"20\">Free</a>"),
        vec!["10"]
    );
}
#[test]
fn steam_cookie_identity_and_header_injection_are_rejected() {
    let mut session = SteamSession {
        account_id: "76561197960265729".into(),
        login_secure: "76561197960265729%7C%7Ctoken".into(),
        session_id: "0123456789abcdef".into(),
        country: "BY".into(),
        refresh_token: None,
        auth_source: "browser".into(),
    };
    assert!(session.validate().is_ok());
    session.account_id = "76561197960265730".into();
    assert!(session.validate().is_err());
    session.account_id = "76561197960265729".into();
    session.login_secure.push_str("\r\nInjected: true");
    assert!(session.validate().is_err());
}
struct Fake {
    calls: AtomicUsize,
    country_calls: AtomicUsize,
    delay: bool,
}
impl Provider for Fake {
    fn country<'a>(&'a self, a: &'a Account) -> Call<'a, String> {
        Box::pin(async move {
            self.country_calls.fetch_add(1, Ordering::SeqCst);
            if a.username == "broken" {
                Err("Login expired".into())
            } else {
                Ok("BY".into())
            }
        })
    }
    fn discover<'a>(&'a self, launcher: &'a str, _: &'a str) -> Call<'a, Vec<Offer>> {
        Box::pin(async move {
            let mut expired = offer(launcher);
            expired.id = "ended".into();
            expired.ends_at = Some(1);
            let mut permanent = offer(launcher);
            permanent.id = "f2p".into();
            permanent.original_price = Some(0);
            permanent.normally_paid = false;
            Ok(vec![offer(launcher), expired, permanent])
        })
    }
    fn claim<'a>(&'a self, a: &'a Account, _: &'a Offer, _: &'a str) -> Call<'a, Claimed> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.delay {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            match a.username.as_str() {
                "captcha" => Err("CAPTCHA".into()),
                "owned" => Ok(Claimed::Owned),
                _ => Ok(Claimed::Added),
            }
        })
    }
}
async fn database() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    db
}
async fn account(db: &Database, id: &str, launcher: &str, name: &str) {
    db.upsert_account(&Account {
        id: id.into(),
        launcher: launcher.into(),
        username: name.into(),
        display_name: None,
        email: None,
        is_active: false,
        last_used: None,
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn every_account_including_inactive_new_and_restarted_is_independent() {
    let db = database().await;
    for (id, launcher, name) in [
        ("1", "steam", "ok"),
        ("2", "epic", "ok"),
        ("3", "epic", "broken"),
        ("4", "epic", "captcha"),
        ("5", "epic", "owned"),
        ("6", "riot", "ok"),
    ] {
        account(&db, id, launcher, name).await;
    }
    let fake = Arc::new(Fake {
        calls: AtomicUsize::new(0),
        country_calls: AtomicUsize::new(0),
        delay: false,
    });
    let service = AutoClaimer::with_provider(db.clone(), fake.clone());
    let run = service.check().await.unwrap();
    assert_eq!(run.accounts_checked, 5);
    assert_eq!(db.get_claim_history().await.unwrap().len(), 2);
    let cached = db.steam_cache_get("giveaways:1").await.unwrap().unwrap().0;
    let games: Value = serde_json::from_str(&cached).unwrap();
    assert_eq!(games[0]["app_id"], "10");
    assert_eq!(games[0]["access"], "owned");
    assert_eq!(
        run.results
            .iter()
            .filter(|r| r.status == "needs_login")
            .count(),
        1
    );
    assert_eq!(
        run.results
            .iter()
            .filter(|r| r.status == "attention")
            .count(),
        1
    );
    assert!(!run.running);
    assert!(run.next_check_at.is_some());
    account(&db, "7", "epic", "ok").await;
    let restarted = AutoClaimer::with_provider(db.clone(), fake.clone());
    assert_eq!(restarted.status().await.unwrap().accounts_checked, 5);
    let run = restarted.check().await.unwrap();
    assert_eq!(run.accounts_checked, 6);
    assert_eq!(db.get_claim_history().await.unwrap().len(), 3);
    // The two successful earlier accounts did not submit new orders.
    assert_eq!(fake.calls.load(Ordering::SeqCst), 7);
}
#[tokio::test]
async fn concurrent_runs_and_disabled_platforms_do_not_duplicate_orders() {
    let db = database().await;
    account(&db, "1", "epic", "ok").await;
    account(&db, "2", "steam", "ok").await;
    let fake = Arc::new(Fake {
        calls: AtomicUsize::new(0),
        country_calls: AtomicUsize::new(0),
        delay: true,
    });
    let service = AutoClaimer::with_provider(db.clone(), fake.clone());
    service
        .save_settings(&Settings {
            steam: false,
            ..Settings::default()
        })
        .await
        .unwrap();
    let worker = service.clone();
    let task = tokio::spawn(async move { worker.check().await });
    while fake.country_calls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    assert!(service.check().await.is_err());
    task.await.unwrap().unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert!(service
        .save_settings(&Settings {
            interval_minutes: 1,
            ..Settings::default()
        })
        .await
        .is_err());
}

#[tokio::test]
async fn disconnected_steam_account_is_not_automatically_reenrolled() {
    let db = database().await;
    account(&db, "1", "steam", "ok").await;
    account(&db, "2", "epic", "ok").await;
    db.set_auto_claim_value("steam-excluded:1", "true")
        .await
        .unwrap();
    let fake = Arc::new(Fake {
        calls: AtomicUsize::new(0),
        country_calls: AtomicUsize::new(0),
        delay: false,
    });
    let service = AutoClaimer::with_provider(db.clone(), fake.clone());
    let run = service.check().await.unwrap();
    assert_eq!(
        run.results
            .iter()
            .filter(|r| r.status == "disabled" && r.account_id == "1")
            .count(),
        1
    );
    assert_eq!(fake.country_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert_eq!(db.get_claim_history().await.unwrap().len(), 1);
}

struct PausedSteam {
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
    claims: AtomicUsize,
}
impl Provider for PausedSteam {
    fn country<'a>(&'a self, _: &'a Account) -> Call<'a, String> {
        Box::pin(async move {
            self.entered.notify_one();
            self.resume.notified().await;
            Ok("BY".into())
        })
    }
    fn discover<'a>(&'a self, launcher: &'a str, _: &'a str) -> Call<'a, Vec<Offer>> {
        Box::pin(async move { Ok(vec![offer(launcher)]) })
    }
    fn claim<'a>(&'a self, _: &'a Account, _: &'a Offer, _: &'a str) -> Call<'a, Claimed> {
        Box::pin(async move {
            self.claims.fetch_add(1, Ordering::SeqCst);
            Ok(Claimed::Added)
        })
    }
}
#[tokio::test]
async fn disconnect_during_steam_login_waits_for_save_and_prevents_the_next_claim() {
    let db = database().await;
    account(&db, "1", "steam", "ok").await;
    let provider = Arc::new(PausedSteam {
        entered: tokio::sync::Notify::new(),
        resume: tokio::sync::Notify::new(),
        claims: AtomicUsize::new(0),
    });
    let service = AutoClaimer::with_provider(db.clone(), provider.clone());
    let running = service.clone();
    let job = tokio::spawn(async move { running.check().await.unwrap() });
    provider.entered.notified().await;
    let disconnect_started = Arc::new(tokio::sync::Notify::new());
    let started = disconnect_started.clone();
    let disconnect = tokio::spawn(async move {
        started.notify_one();
        let _guard = service.steam_account_lock("1").await;
        db.set_auto_claim_value("steam-excluded:1", "true")
            .await
            .unwrap();
    });
    disconnect_started.notified().await;
    assert!(
        !disconnect.is_finished(),
        "disconnect must wait for an in-flight credential save"
    );
    provider.resume.notify_one();
    disconnect.await.unwrap();
    let run = job.await.unwrap();
    assert_eq!(provider.claims.load(Ordering::SeqCst), 0);
    assert!(run.results.iter().any(|r| r.status == "disabled"));
}

async fn checkout_client(price: Value, preview_country: &str) -> (EpicClient, Arc<AtomicUsize>) {
    let confirms = Arc::new(AtomicUsize::new(0));
    let count = confirms.clone();
    let preview_country = preview_country.to_string();
    let router = Router::new()
        .route(
            "/orderprocessor/api/shared/accounts/account/orders/quickPurchase",
            post(|| async { Json(json!({"quickPurchaseStatus":"CHECKOUT"})) }),
        )
        .route(
            "/purchase",
            get(|| async { "<input id=\"purchaseToken\" value=\"token\">" }),
        )
        .route(
            "/purchase/order-preview",
            post(move |axum::extract::Form(body): axum::extract::Form<std::collections::HashMap<String, String>>| {
                let price = price.clone();
                let preview_country = preview_country.clone();
                async move {
                    assert_eq!(body["useDefault"], "false");
                    assert_eq!(body["setDefault"], "false");
                    assert_eq!(body["namespace"], "ns");
                    assert_eq!(body["country"], "BY");
                    assert_eq!(body["offers[0]"], "gift");
                    Json(json!({"namespace":"ns","offers":["gift"],"country":preview_country,
                        "countryName":"Belarus","orderResponse":{"totalPrice":price},"syncToken":"sync"}))
                }
            }),
        )
        .route(
            "/purchase/confirm-order",
            post(move |axum::extract::Form(body): axum::extract::Form<std::collections::HashMap<String, String>>| {
                let count = count.clone();
                async move {
                    assert_eq!(body["totalAmount"], "0");
                    assert_eq!(body["includeAccountBalance"], "false");
                    assert_eq!(body["useDefault"], "false");
                    assert_eq!(body["setDefault"], "false");
                    assert_eq!(body["offers[0]"], "gift");
                    assert_eq!(body["namespace"], "ns");
                    assert_eq!(body["country"], "BY");
                    assert_eq!(body["syncToken"], "sync");
                    count.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"confirmation":true}))
                }
            }),
        );
    (EpicClient::from_router(router), confirms)
}
#[tokio::test]
async fn epic_checkout_refuses_changed_paid_or_unknown_totals() {
    for price in [json!(1), json!("9.99"), json!(null), json!("NaN")] {
        let (client, count) = checkout_client(price, "BY").await;
        assert!(client
            .claim_free_offer("token", "account", "ns", "gift", "BY")
            .await
            .is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
    for price in [json!(0), json!("0.00")] {
        let (client, count) = checkout_client(price, "BY").await;
        client
            .claim_free_offer("token", "account", "ns", "gift", "BY")
            .await
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
    let (client, count) = checkout_client(json!(0), "US").await;
    assert!(client
        .claim_free_offer("token", "account", "ns", "gift", "BY")
        .await
        .is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let client = EpicClient::from_router(Router::new().route(
        "/orderprocessor/api/shared/accounts/account/orders/quickPurchase",
        post(|| async { Json(json!({"quickPurchaseStatus":"REJECTED"})) }),
    ));
    assert!(client
        .claim_free_offer("token", "account", "ns", "gift", "BY")
        .await
        .is_err());
}
#[tokio::test]
async fn routes_persist_settings_guard_cross_origin_and_capability() {
    let db = database().await;
    account(&db, "76561197960265729", "steam", "ok").await;
    let fake = Arc::new(Fake {
        calls: AtomicUsize::new(0),
        country_calls: AtomicUsize::new(0),
        delay: false,
    });
    let service = AutoClaimer::with_provider(db.clone(), fake);
    let epic = Arc::new(EpicAuthStore::new(
        db.clone(),
        Arc::new(MemoryCredentialStore::new()),
        Arc::new(EpicClient::new()),
    ));
    let app = create_app_with_auto_claim(AppState::new(db.clone(), epic), service);
    let req = |path: &str, body: Value, origin: &str| {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .header("origin", origin)
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(req(
                "/api/free-games/settings",
                json!({"enabled":false}),
                "https://evil.example"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(req(
                "/api/free-games/settings",
                json!({"enabled":false,"steam":true,"epic":true,"interval_minutes":30}),
                "http://127.0.0.1:4242"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for endpoint in ["/api/free-games/web/manual", "/api/free-games/web/library"] {
        assert_eq!(app.clone().oneshot(req(endpoint,json!({"platform":"epic","account_id":"not-added"}),"https://evil.example")).await.unwrap().status(),StatusCode::FORBIDDEN);
        assert_eq!(app.clone().oneshot(req(endpoint,json!({"platform":"epic","account_id":"not-added"}),"http://127.0.0.1:4242")).await.unwrap().status(),StatusCode::BAD_REQUEST);
        assert_eq!(app.clone().oneshot(req(endpoint,json!({"platform":"epic","account_id":"not-added","url":"https://evil.example"}),"http://127.0.0.1:4242")).await.unwrap().status(),StatusCode::UNPROCESSABLE_ENTITY);
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/free-games")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["settings"]["enabled"], false);
    assert_eq!(value["settings"]["interval_minutes"], 30);
    assert_eq!(app.clone().oneshot(req("/api/free-games/steam/complete",json!({"session":"invalid","account_id":"76561197960265729","login_secure":"never logged","session_id":"csrf","country":"BY"}),"http://127.0.0.1:4242")).await.unwrap().status(),StatusCode::FORBIDDEN);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/free-games/steam/extension")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "application/zip");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert!(bytes.windows(13).any(|v| v == b"manifest.json"));
}
