use serde_json::json;
use swch::{launchers::riot::{local::LocalClient,profile::{league_items,league_playtime,Profile,Section},valorant::{session_config,owned_ids,collection}},storage::Database};

fn profile(id:&str)->Profile {
    Profile {account_id:id.into(),app_id:"league_of_legends".into(),display_name:"Player#TAG".into(),level:Some(10),source:"league_client".into(),synced_at:"2026-09-21T00:00:00Z".into(),has_play_history:true,
        champions:Section::from_result(league_items(&json!([{"id":1,"name":"Annie","ownership":{"owned":true}}]),false)),
        skins:Section::from_result(league_items(&json!([{"id":1001,"name":"Skin","ownership":{"owned":true}}]),true)),
        playtime:Section::from_result(league_playtime(&json!({"games":{"games":[{"gameId":1,"gameDuration":1800}]}})))}
}
#[test]
fn only_owned_items_not_rotation_or_base_skins_and_no_duplicates() {
    let items=json!([
        {"id":1000,"name":"Default","ownership":{"owned":true}},
        {"id":1001,"name":"Owned","ownership":{"owned":true}},
        {"id":1001,"name":"Owned","ownership":{"owned":true}},
        {"id":1002,"name":"Rental","ownership":{"owned":false}},
        {"id":1003,"name":"Base","isBase":true,"ownership":{"owned":true}}
    ]);
    let skins=league_items(&items,true).unwrap();assert_eq!(skins.len(),1);assert_eq!(skins[0].name,"Owned");
    assert!(league_items(&json!({}),true).is_err());
    assert!(league_items(&json!([{"id":1}]),false).is_err());
    assert!(league_items(&json!([]),false).unwrap().is_empty());
}
#[test]
fn history_is_deduplicated_and_unknown_is_not_zero() {
    let time=league_playtime(&json!({"games":{"games":[{"gameId":1,"gameDuration":60},{"gameId":1,"gameDuration":60},{"gameId":2,"gameDuration":100,"gameMode":"TFT"}]}})).unwrap();
    assert_eq!(time.seconds,60);assert_eq!(time.matches,1);assert_eq!(time.scope,"recent_matches");
    assert!(league_playtime(&json!({"games":{"games":[{"gameId":1}]}})).is_err());
    assert!(league_playtime(&json!({})).is_err());
}
#[test]
fn local_connection_rejects_invalid_ports_protocols_and_extra_fields() {
    for lock in ["client:1:0:secret:https","client:1:65536:secret:https","client:1:123:secret:http","client:1:123:secret:https:evil"] {assert!(LocalClient::from_lockfile(lock).is_err());}
    assert!(LocalClient::from_lockfile("client:1:123:secret:https").is_ok());
}
#[test]
fn valorant_shards_cannot_redirect_credentials_and_skin_levels_are_deduplicated() {
    let session=|shard:&str|json!({"x":{"productId":"valorant","patchlineId":"live","version":"release-1","launchConfiguration":{"arguments":[format!("-ares-deployment={shard}")]}}});
    assert_eq!(session_config(&session("eu")).unwrap().0,"eu");
    assert!(session_config(&session("evil.example/")).is_err());
    let ids=owned_ids(&json!({"Entitlements":[{"ItemID":"level-1"},{"ItemID":"level-2"},{"ItemID":"level-2"}]}),"skins").unwrap();
    let catalog=json!({"data":[{"uuid":"skin","displayName":"One skin","levels":[{"uuid":"level-1"},{"uuid":"level-2"}]}]});
    let items=collection(&ids,&catalog,true);assert_eq!(items.len(),1);assert_eq!(items[0].name,"One skin");
    assert!(owned_ids(&json!({}),"skins").is_err());
}
#[tokio::test]
async fn cache_preserves_account_product_isolation_and_previous_data_on_partial_error() {
    let db=Database::open_in_memory().unwrap();db.run_migrations().await.unwrap();db.run_migrations().await.unwrap();
    db.save_riot_profile(profile("riot_a")).await.unwrap();
    let mut b=profile("riot_b");b.skins=Section::from_result(Ok(vec![]));db.save_riot_profile(b).await.unwrap();
    let mut val=profile("riot_a");val.app_id="valorant".into();val.skins=Section::from_result(Ok(vec![]));db.save_riot_profile(val).await.unwrap();
    let mut failed=profile("riot_a");failed.skins=Section::from_result(Err("Offline".into()));failed.has_play_history=false;failed.playtime=Section::from_result(Err("Offline".into()));
    db.save_riot_profile(failed).await.unwrap();
    let saved=db.riot_profiles().await.unwrap();assert_eq!(saved.len(),3);
    let a=saved.iter().find(|p|p.account_id=="riot_a"&&p.app_id=="league_of_legends").unwrap();
    assert_eq!(a.skins.value.as_ref().unwrap().len(),1);assert_eq!(a.skins.error.as_deref(),Some("Offline"));assert!(a.has_play_history);
    assert_eq!(saved.iter().find(|p|p.account_id=="riot_b").unwrap().skins.value.as_ref().unwrap().len(),0);
    let serialized=serde_json::to_string(&saved).unwrap();assert!(!serialized.contains("accessToken"));assert!(!serialized.contains("password"));
}
