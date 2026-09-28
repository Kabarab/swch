use serde_json::{json, Value};
use swch::launchers::steam::game_stats::{
    account32, parse_cs, parse_deadlock, parse_dota, GameStats, StatsClient,
};
const ID: &str = "76561197960265729";
fn metric(s: &GameStats, label: &str) -> Option<Value> {
    s.metrics
        .iter()
        .find(|m| m.label == label)
        .unwrap()
        .value
        .clone()
}
#[test]
fn steam_id_conversion_is_exact_and_validated() {
    assert_eq!(account32(ID), Some(1));
    assert_eq!(account32("76561202255233023"), Some(u32::MAX));
    for id in ["1", "76561197960265728", "76561202255233024", "abc"] {
        assert_eq!(account32(id), None);
    }
}
#[test]
fn cs_ratios_preserve_missing_counters_and_zero_deaths() {
    let mut s = GameStats::new(ID, "730");
    parse_cs(
        &mut s,
        &json!({"playerstats":{"stats":[{"name":"total_kills","value":20},{"name":"total_deaths","value":0},{"name":"total_kills_headshot","value":5},{"name":"total_wins","value":100}]}}),
    );
    assert_eq!(metric(&s, "Попадания в голову"), Some(json!(25.0)));
    assert_eq!(metric(&s, "K/D"), None);
    assert_eq!(metric(&s, "Смерти"), Some(json!(0.0)));
    // total_wins means rounds; it must not be presented as match wins.
    assert_eq!(metric(&s, "Победы в матчах"), None);
    let mut empty = GameStats::new(ID, "730");
    parse_cs(&mut empty, &json!({"playerstats":{"error":"private"}}));
    assert_eq!(empty.status, "unavailable");
    assert!(empty.metrics.iter().all(|m| m.value.is_none()));
}
#[test]
fn dota_rank_is_not_mmr_and_empty_history_is_unknown() {
    let mut s = GameStats::new(ID, "570");
    parse_dota(
        &mut s,
        &json!({"rank_tier":54}),
        &json!({"win":30,"lose":10}),
    );
    assert_eq!(metric(&s, "Ранг"), Some(json!("Легенда 4")));
    assert_eq!(metric(&s, "Процент побед"), Some(json!(75.0)));
    assert_eq!(metric(&s, "Матчи в OpenDota"), Some(json!(40.0)));
    let mut empty = GameStats::new(ID, "570");
    parse_dota(
        &mut empty,
        &json!({"rank_tier":null}),
        &json!({"win":0,"lose":0}),
    );
    assert_eq!(empty.status, "unavailable");
    assert!(empty.metrics.iter().all(|m| m.value.is_none()));
}
#[test]
fn deadlock_deduplicates_and_uses_winning_team_even_when_not_scored() {
    let row = |id, team, winner, outcome| json!({"account_id":1,"match_id":id,"player_match_outcome":outcome,"player_team":team,"match_result":winner,"player_kills":10,"player_deaths":5,"player_assists":7});
    let mut s = GameStats::new(ID, "1422450");
    parse_deadlock(
        &mut s,
        &json!([row(1,0,0,0),row(1,0,0,0),row(2,1,0,0),row(3,1,1,5),{"account_id":2,"match_id":4,"player_match_outcome":1}]),
    );
    assert_eq!(metric(&s, "Матчи в источнике"), Some(json!(3)));
    assert_eq!(metric(&s, "Процент побед"), Some(json!(66.67)));
    assert_eq!(metric(&s, "Убийства"), Some(json!(30.0)));
    assert_eq!(metric(&s, "K/D"), Some(json!(2.0)));
    assert_eq!(metric(&s, "Победы"), Some(json!(2)));
    assert_eq!(metric(&s, "Поражения"), Some(json!(1)));
    let mut partial = GameStats::new(ID, "1422450");
    parse_deadlock(
        &mut partial,
        &json!([row(1,0,0,0),row(2,1,9,1),{"account_id":1,"match_id":3}]),
    );
    assert_eq!(partial.status, "partial");
    assert_eq!(metric(&partial, "Без результата"), Some(json!(2)));
    assert_eq!(metric(&partial, "Процент побед"), Some(json!(100.0)));
    let mut unknown = GameStats::new(ID, "1422450");
    parse_deadlock(&mut unknown, &json!([row(1, 9, 9, 1)]));
    assert_eq!(metric(&unknown, "Победы"), None);
    assert_eq!(metric(&unknown, "Процент побед"), None);
    let mut empty = GameStats::new(ID, "1422450");
    parse_deadlock(&mut empty, &json!([]));
    assert_eq!(empty.status, "unavailable");
    assert!(empty.metrics.iter().all(|m| m.value.is_none()));
}
#[tokio::test]
async fn no_key_never_attempts_steam_request() {
    let c = StatsClient::new(
        "http://127.0.0.1:1",
        "http://127.0.0.1:1",
        "http://127.0.0.1:1",
    );
    assert_eq!(c.fetch(ID, "730", None).await.status, "needs_key");
}
#[tokio::test]
async fn provider_requests_use_account32_and_partial_failures_keep_rank() {
    use axum::{http::StatusCode, routing::get, Json, Router};
    let router = Router::new()
        .route(
            "/players/1",
            get(|| async { Json(json!({"rank_tier":80})) }),
        )
        .route(
            "/players/1/wl",
            get(|| async { StatusCode::TOO_MANY_REQUESTS }),
        )
        .route(
            "/v1/players/1/match-history",
            get(|| async { Json(json!([])) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let job = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let c = StatsClient::new(&url, &url, &url);
    let result = c.fetch(ID, "570", None).await;
    assert_eq!(result.status, "partial");
    assert_eq!(metric(&result, "Ранг"), Some(json!("Титан")));
    assert!(result.notes.iter().any(|n| n.contains("частоту")));
    assert_eq!(c.fetch(ID, "1422450", None).await.status, "unavailable");
    job.abort();
}
#[tokio::test]
async fn endpoint_validates_inputs_and_cache_is_account_and_game_specific() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let db = swch::storage::Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let mut s = GameStats::new(ID, "570");
    parse_dota(&mut s, &json!({"rank_tier":31}), &json!({"win":2,"lose":2}));
    db.steam_cache_put(
        &format!("game-stats:v1:{ID}:570"),
        &serde_json::to_string(&s).unwrap(),
    )
    .await
    .unwrap();
    let app = swch::server::create_app(db);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/steam/game-stats?account_id={ID}&app_id=570"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result: GameStats =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(result.cached);
    assert_eq!(result.account_id, ID);
    assert_eq!(result.app_id, "570");
    for query in [
        "account_id=bad&app_id=570",
        "account_id=76561197960265729&app_id=620",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/steam/game-stats?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
