use serde_json::json;
use swch::{launchers::steam::remote::parse_game_price, server::currency::parse_rates};

#[test]
fn game_prices_keep_discounts_free_and_unavailable_distinct() {
    let price=parse_game_price(&json!({"620":{"success":true,"data":{"is_free":false,"price_overview":{"currency":"USD","final":999,"initial":1999}}}}),"620").unwrap();
    assert_eq!(price.price_cents, Some(999));
    assert_eq!(price.regular_price_cents, Some(1999));
    let free = parse_game_price(
        &json!({"730":{"success":true,"data":{"is_free":true}}}),
        "730",
    )
    .unwrap();
    assert_eq!(free.price_cents, Some(0));
    for response in [
        json!({"success":false}),
        json!({"success":true,"data":{"is_free":false}}),
        json!({"success":true,"data":{"price_overview":{"currency":"EUR","final":1000}}}),
    ] {
        let quote = parse_game_price(&json!({"1":response}), "1").unwrap();
        assert!(quote.price_cents.is_none());
        assert!(quote.warning.is_some());
    }
    assert!(parse_game_price(&json!({}), "1").is_err());
    let sale=parse_game_price(&json!({"1":{"success":true,"data":{"is_free":false,"price_overview":{"currency":"USD","final":0,"initial":1000}}}}),"1").unwrap();
    assert_eq!(sale.price_cents, Some(0));
    assert_eq!(sale.is_free, Some(false));
}
#[test]
fn fx_accounts_for_scales_and_dates_and_exposes_only_steam_currencies() {
    let rate = |code: &str, scale: u64, value: f64, date: &str| json!({"Cur_Abbreviation":code,"Cur_Scale":scale,"Cur_OfficialRate":value,"Date":date});
    let value = json!([
        rate("USD", 1, 3.0, "2026-09-15T00:00:00"),
        rate("RUB", 100, 3.5, "2026-09-15T00:00:00"),
        rate("KZT", 1000, 6.0, "2026-09-15T00:00:00"),
        rate("EUR", 1, 3.3, "2026-09-14T00:00:00"),
        rate("GBP", 1, 0.0, "2026-09-15T00:00:00"),
        rate("XXX", 1, 4.0, "2026-09-15T00:00:00")
    ]);
    let result = parse_rates(&value).unwrap();
    assert_eq!(result.rates["USD"], 1.0);
    assert!((result.rates["RUB"] - 85.714285714).abs() < 0.00001);
    assert_eq!(result.rates["KZT"], 500.0);
    for absent in ["BYN", "EUR", "GBP", "XXX"] {
        assert!(!result.rates.contains_key(absent));
    }
    assert!(parse_rates(&json!([])).is_err());
    assert!(parse_rates(&json!([rate("USD", 0, 3.0, "2026-09-15T00:00:00")])).is_err());
}
#[tokio::test]
async fn price_and_currency_routes_reuse_separate_persisted_caches() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use swch::{server::create_app, storage::Database};
    use tower::ServiceExt;
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let price=parse_game_price(&json!({"620":{"success":true,"data":{"is_free":false,"price_overview":{"currency":"USD","final":999,"initial":1999}}}}),"620").unwrap();
    db.steam_cache_put(
        "game-price:US:USD:620",
        &serde_json::to_string(&price).unwrap(),
    )
    .await
    .unwrap();
    let date = (chrono::Utc::now() + chrono::Duration::hours(3))
        .format("%Y-%m-%d")
        .to_string();
    db.steam_cache_put("fx:nbrb:steam:USD:v1",&json!({"base":"USD","date":date,"source":"НБРБ","rates":{"USD":1,"RUB":90},"cached":false,"warning":null}).to_string()).await.unwrap();
    for path in [
        "/api/steam/game-price?app_id=620",
        "/api/steam/game-prices",
        "/api/currencies",
    ] {
        let response = create_app(db.clone())
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        if path.ends_with("game-prices") {
            assert_eq!(value[0]["price_cents"], 999);
        } else {
            assert_eq!(value["cached"], true);
        }
    }
    let response = create_app(db)
        .oneshot(
            Request::builder()
                .uri("/api/steam/game-price?app_id=bad")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn epic_price_routes_support_cached_and_fallback_prices() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use swch::{server::create_app, storage::Database};
    use tower::ServiceExt;

    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let cached_price = json!({
        "app_id": "epic_sugar",
        "currency": "USD",
        "region": "US",
        "price_cents": 1499,
        "regular_price_cents": 1999,
        "is_free": false,
        "updated_at": "2026-09-15T00:00:00Z",
        "cached": false,
        "warning": null
    });
    db.steam_cache_put(
        "game-price-epic:US:USD:epic_sugar",
        &serde_json::to_string(&cached_price).unwrap(),
    )
    .await
    .unwrap();

    let app = create_app(db.clone());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/epic/game-price?app_id=epic_sugar&cc=US&currency=USD")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(val["price_cents"], 1499);
    assert_eq!(val["cached"], true);

    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/epic/game-prices")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response2.status(), StatusCode::OK);
    let body2 = response2.into_body().collect().await.unwrap().to_bytes();
    let list: serde_json::Value = serde_json::from_slice(&body2).unwrap();
    assert_eq!(list[0]["app_id"], "epic_sugar");
    assert_eq!(list[0]["price_cents"], 1499);

    let response3 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/epic/game-price")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response3.status(), StatusCode::BAD_REQUEST);

    let response4 = app
        .oneshot(
            Request::builder()
                .uri("/api/epic/game-price?app_id=unknown_epic_game&cc=US&currency=USD")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response4.status(), StatusCode::OK);
    let body4 = response4.into_body().collect().await.unwrap().to_bytes();
    let val4: serde_json::Value = serde_json::from_slice(&body4).unwrap();
    assert_eq!(val4["app_id"], "unknown_epic_game");
    assert_eq!(val4["price_cents"], 0);
    assert_eq!(val4["is_free"], true);
}

