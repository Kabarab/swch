use serde_json::json;
use swch::launchers::steam::deadlock_stats::{build, requirements};
const ID: &str = "76561197960265729";
fn seasons() -> serde_json::Value {
    json!([{"name":"Current","ranked_type":"normal","min_wins":60,"min_hero_wins":15,"min_hero_unlocks":3,"intervals":[{"start_timestamp":100,"end_timestamp":1000}]}])
}
#[test]
fn character_stats_and_rank_progress_are_account_isolated_and_modes_are_checked() {
    let history = json!([
     {"account_id":1,"hero_id":1,"match_id":10,"game_mode":1,"match_mode":4,"start_time":200,"ranked_display_badge":75,"player_team":0,"match_result":0,"player_kills":5,"player_deaths":1,"player_assists":2},
     {"account_id":1,"hero_id":1,"match_id":10,"game_mode":1,"match_mode":4,"start_time":200},
     {"account_id":1,"hero_id":2,"match_id":11,"game_mode":4,"match_mode":4,"start_time":200},
     {"account_id":2,"hero_id":2,"match_id":12,"game_mode":1,"match_mode":4,"start_time":200},
     {"account_id":1,"hero_id":3,"match_id":13,"game_mode":1,"match_mode":4,"start_time":50},
     {"account_id":1,"hero_id":4,"match_id":14,"game_mode":1,"match_mode":2,"start_time":200}
    ]);
    let normal = json!([{"account_id":1,"hero_id":1,"matches_played":30,"wins":20,"kills":100,"deaths":50,"assists":20,"time_played":3600}]);
    let standard = json!([{"account_id":1,"hero_id":1,"matches_played":30,"wins":19},{"account_id":1,"hero_id":1,"matches_played":10,"wins":9},{"account_id":2,"hero_id":2,"matches_played":500,"wins":400}]);
    let data = build(ID, &history, &normal, &standard, &seasons(), 500);
    assert!(data.ranked_played);
    assert_eq!(data.standard_wins, Some(19));
    assert_eq!(data.heroes.len(), 2);
    let hero = data.heroes.iter().find(|h| h.hero_id == 1).unwrap();
    assert_eq!(hero.ranked_matches, Some(1));
    assert_eq!(hero.last_rank, Some(75));
    assert_eq!(hero.standard_wins, Some(19));
    assert_eq!(
        hero.metrics
            .iter()
            .find(|m| m.label == "K/D")
            .unwrap()
            .value,
        Some(json!(2.0))
    );
    assert_eq!(
        data.heroes
            .iter()
            .find(|h| h.hero_id == 3)
            .unwrap()
            .ranked_matches,
        Some(0)
    );
}
#[test]
fn empty_private_invalid_and_expired_data_are_unknown_not_locked_or_zero() {
    let data = build(ID, &json!([]), &json!([]), &json!([]), &seasons(), 500);
    assert_eq!(data.standard_wins, None);
    assert!(data.heroes.is_empty());
    assert!(!data.ranked_played);
    assert!(requirements(&seasons(), 1000).is_none());
    assert!(requirements(&json!({}), 500).is_none());
    let data = build(
        ID,
        &json!([]),
        &json!([{"account_id":1,"hero_id":1,"matches_played":1,"wins":2}]),
        &json!({}),
        &json!([]),
        500,
    );
    assert!(data.heroes.is_empty());
    assert!(data.requirements.is_none());
}
#[tokio::test]
async fn public_fetch_filters_standard_and_never_requires_native_launcher_or_paid_account_stats() {
    use axum::{extract::Query, routing::get, Json, Router};
    use std::collections::HashMap;
    let now = chrono::Utc::now().timestamp();
    let router=Router::new().route("/v1/players/1/match-history",get(||async{Json(json!([]))}))
 .route("/v1/players/hero-stats",get(|Query(q):Query<HashMap<String,String>>|async move{
  assert_eq!(q.get("account_ids").map(String::as_str),Some("1"));assert_eq!(q.get("game_mode").map(String::as_str),Some("normal"));
  let wins=match q.get("match_mode").map(String::as_str){Some("unranked")=>15,Some("unranked,ranked")=>20,_=>panic!("incorrect mode")};
  Json(json!([{"account_id":1,"hero_id":1,"matches_played":30,"wins":wins}]))
 }))
 .route("/v1/assets/ranked-seasons",get(move||async move{Json(json!([{"name":"Current","ranked_type":"normal","min_wins":60,"min_hero_wins":15,"min_hero_unlocks":3,"intervals":[{"start_timestamp":now-100,"end_timestamp":now+1000}]}]))}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let job = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let result = swch::launchers::steam::game_stats::StatsClient::new(&base, &base, &base)
        .fetch(ID, "1422450", None)
        .await;
    assert_eq!(result.status, "partial");
    let detail = result.deadlock.unwrap();
    assert_eq!(detail.standard_wins, Some(15));
    assert_eq!(detail.heroes[0].standard_wins, Some(15));
    assert_eq!(detail.requirements.unwrap().hero_wins, 15);
    job.abort();
}
