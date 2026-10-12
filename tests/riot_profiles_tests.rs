use serde_json::json;
use swch::{launchers::riot::{local::LocalClient,profile::{league_items,league_playtime,Profile,Section},valorant::{session_config,owned_ids,collection}},storage::Database};

fn profile(id:&str)->Profile {
    Profile {account_id:id.into(),app_id:"league_of_legends".into(),display_name:"Player#TAG".into(),level:Some(10),source:"league_client".into(),synced_at:"2026-09-21T00:00:00Z".into(),has_play_history:true,
        champions:Section::from_result(league_items(&json!([{"id":1,"name":"Annie","ownership":{"owned":true}}]),false)),
        skins:Section::from_result(league_items(&json!([{"id":1001,"name":"Skin","ownership":{"owned":true}}]),true)),
        playtime:Section::from_result(league_playtime(&json!({"games":{"games":[{"gameId":1,"gameDuration":1800}]}}))),ranked:None,buddies:None}
}
#[test]
fn renamed_riot_id_uses_latest_product_snapshot_and_keeps_accounts_isolated() {
    use swch::launchers::riot::profile::latest_display_name;
    let mut old = profile("riot_a");
    old.display_name = "Old#TAG".into();
    let mut new = old.clone();
    new.app_id = "valorant".into();
    new.display_name = "New#TAG".into();
    new.synced_at = "2026-10-11T16:00:00Z".into();
    let mut other = new.clone();
    other.account_id = "riot_b".into();
    other.display_name = "Other#TAG".into();
    let mut failed_alias = new.clone();
    failed_alias.synced_at = "2026-10-11T17:00:00Z".into();
    failed_alias.display_name = "Riot a".into();
    let snapshots = vec![new, other, failed_alias, old];
    assert_eq!(latest_display_name(&snapshots, "riot_a"), Some("New#TAG"));
    assert_eq!(latest_display_name(&snapshots, "riot_b"), Some("Other#TAG"));
    assert_eq!(latest_display_name(&snapshots, "missing"), None);
}
#[test]
fn live_riot_alias_is_rejected_if_the_player_changes_during_refresh() {
    use swch::launchers::riot::local::verified_identity;
    let token = json!({"subject":"account-a"});
    let alias = json!({"game_name":"New Name","tag_line":"TAG"});
    assert_eq!(verified_identity(&token, &alias, &token), Some(("riot_account-a".into(), "New Name#TAG".into())));
    assert!(verified_identity(&token, &alias, &json!({"subject":"account-b"})).is_none());
    assert!(verified_identity(&token, &json!({}), &token).is_none());
}
#[tokio::test]
async fn renamed_riot_id_survives_restart_without_changing_sessions_or_stat_timestamps() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let account = swch::storage::Account { id:"riot_a".into(), launcher:"riot".into(), username:"login-name".into(), display_name:Some("Old#TAG".into()), email:None, is_active:true, last_used:Some("saved-session-time".into()) };
    db.upsert_account(&account).await.unwrap();
    let original = profile("riot_a");
    db.save_riot_profile(original.clone()).await.unwrap();
    db.save_riot_profile(profile("riot_b")).await.unwrap();
    db.update_riot_display_name("riot_a", "New#TAG").await.unwrap();
    let saved_account = db.get_accounts().await.unwrap().remove(0);
    assert_eq!(saved_account.display_name.as_deref(), Some("New#TAG"));
    assert_eq!(saved_account.username, account.username);
    assert_eq!(saved_account.is_active, account.is_active);
    assert_eq!(saved_account.last_used, account.last_used);
    let saved = db.riot_profiles().await.unwrap();
    let renamed = saved.iter().find(|p| p.account_id=="riot_a").unwrap();
    assert_eq!(renamed.display_name, "New#TAG");
    assert_eq!(renamed.synced_at, original.synced_at);
    assert_eq!(renamed.playtime.updated_at, original.playtime.updated_at);
    assert_eq!(renamed.playtime.value.as_ref().unwrap().seconds, original.playtime.value.as_ref().unwrap().seconds);
    assert_eq!(saved.iter().find(|p| p.account_id=="riot_b").unwrap().display_name, "Player#TAG");
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
fn league_mode_copies_do_not_duplicate_owned_champions() {
    let champs=league_items(&json!([
        {"id":60098,"name":"Shen","ownership":{"owned":true}},
        {"id":98,"name":"Shen","ownership":{"owned":true}},
        {"id":1,"name":"Annie","ownership":{"owned":true}}
    ]),false).unwrap();
    assert_eq!(champs.len(),2);
    assert_eq!(champs[1].id,"98");
    assert!(champs[1].image_url.as_ref().unwrap().ends_with("/champion-icons/98.png"));
    assert_eq!(league_items(&json!([
        {"id":1001,"name":"Same skin name","ownership":{"owned":true}},
        {"id":2001,"name":"Same skin name","ownership":{"owned":true}}
    ]),true).unwrap().len(),2);
}
#[test]
fn league_skin_collection_excludes_mode_copies_and_their_classic_appearances() {
    use swch::launchers::riot::profile::league_skins;
    let champions=json!([
        {"id":1,"name":"Annie","ownership":{"owned":true}},
        {"id":60001,"name":"Annie","ownership":{"owned":true}},
        {"id":2,"name":"Olaf","ownership":{"owned":true}}
    ]);
    let skins=league_skins(&json!([
        {"id":1000,"championId":1,"isBase":true,"name":"Annie","ownership":{"owned":true}},
        {"id":1012,"championId":1,"name":"Annie-Versary","ownership":{"owned":true}},
        {"id":60001012,"championId":60001,"name":"Annie-Versary","ownership":{"owned":true}},
        {"id":60001301,"championId":60001,"name":"Classic Annie","ownership":{"owned":true}},
        {"id":2001,"championId":2,"name":"Annie-Versary","ownership":{"owned":true}}
    ]),&champions).unwrap();
    assert_eq!(skins.iter().map(|item|item.id.as_str()).collect::<Vec<_>>(),vec!["1012","2001"]);
    assert!(league_skins(&json!([{"id":1012,"ownership":{"owned":true}}]),&champions).is_err());
}
#[test]
fn local_connection_rejects_invalid_ports_protocols_and_extra_fields() {
    for lock in ["client:1:0:secret:https","client:1:65536:secret:https","client:1:123:secret:http","client:1:123:secret:https:evil"] {assert!(LocalClient::from_lockfile(lock).is_err());}
    assert!(LocalClient::from_lockfile("client:1:123:secret:https").is_ok());
}
#[test]
fn league_ready_requires_the_requested_identity_and_loaded_summoner() {
    use swch::launchers::riot::local::league_identity_ready;
    let summoner=json!({"puuid":"one","summonerId":12});
    assert!(league_identity_ready(&summoner,Some("riot_one")));
    assert!(!league_identity_ready(&summoner,Some("riot_two")));
    assert!(!league_identity_ready(&json!({"puuid":"one","summonerId":0}),Some("riot_one")));
    let heroes=league_items(&json!([{"id":1,"name":"Annie","ownership":{"owned":true}}]),false).unwrap();
    assert!(heroes[0].image_url.as_ref().unwrap().ends_with("/champion-icons/1.png"));
}
#[test]
fn buddy_levels_are_owned_deduplicated_and_retained_on_partial_failure() {
    use swch::launchers::riot::valorant::buddy_collection;
    let catalog=json!({"data":[
        {"uuid":"buddy","displayName":"Buddy","displayIcon":"https://media.valorant-api.com/buddies/buddy/displayicon.png","levels":[{"uuid":"level"}]},
        {"uuid":"locked","displayName":"Not owned","levels":[{"uuid":"locked-level"}]}]});
    let data=json!({"Entitlements":[{"ItemID":"level","InstanceID":"copy-one"},{"ItemID":"level","InstanceID":"copy-two"}]});
    let buddies=buddy_collection(&data,&catalog).unwrap();
    assert_eq!(buddies.len(),1);assert_eq!(buddies[0].id,"buddy");assert!(buddies[0].image_url.is_some());
    assert!(buddy_collection(&data,&json!({})).is_err());
    let mut old=profile("one");old.buddies=Some(Section::from_result(Ok(buddies.clone())));
    let mut new=profile("one");new.buddies=Some(Section::from_result(Err("Riot unavailable".into())));
    new.retain_failed_sections(old);
    assert_eq!(new.buddies.as_ref().unwrap().value.as_ref(),Some(&buddies));
    assert_eq!(new.buddies.as_ref().unwrap().error.as_deref(),Some("Riot unavailable"));
    let mut legacy=serde_json::to_value(profile("one")).unwrap();legacy.as_object_mut().unwrap().remove("buddies");
    assert!(serde_json::from_value::<Profile>(legacy).unwrap().buddies.is_none());
}
#[test]
fn valorant_shards_cannot_redirect_credentials_and_skin_levels_are_deduplicated() {
    let session=|shard:&str|json!({"x":{"productId":"valorant","patchlineId":"live","version":"release-1","launchConfiguration":{"arguments":[format!("-ares-deployment={shard}")]}}});
    assert_eq!(session_config(&session("eu")).unwrap().0,"eu");
    assert!(session_config(&session("evil.example/")).is_err());
    let ids=owned_ids(&json!({"Entitlements":[{"ItemID":"level-1"},{"ItemID":"level-2"},{"ItemID":"level-2"}]}),"skins").unwrap();
    let catalog=json!({"data":[{"uuid":"skin","displayName":"One skin","displayIcon":null,"levels":[{"uuid":"level-1","displayIcon":"https://media.valorant-api.com/levels/fixture.png"},{"uuid":"level-2"}]}]});
    let items=collection(&ids,&catalog,true);assert_eq!(items.len(),1);assert_eq!(items[0].name,"One skin");assert!(items[0].image_url.is_some());
    assert!(owned_ids(&json!({}),"skins").is_err());
}
#[test]
fn valorant_current_act_rank_and_starter_agents_are_separate_from_store_ownership() {
    use swch::launchers::riot::valorant::{ranked_info,available_agents};
    let mmr=json!({"QueueSkills":{"competitive":{"CurrentSeasonGamesNeededForRating":0,"SeasonalInfoBySeasonID":{
        "old":{"CompetitiveTier":24,"RankedRating":88,"NumberOfGames":100},
        "current":{"CompetitiveTier":6,"RankedRating":42,"NumberOfGames":1}}}}});
    let seasons=json!({"data":[
        {"seasonUuid":"episode","startTime":"2026-01-01T00:00:00Z","endTime":"2027-01-01T00:00:00Z","borders":null},
        {"seasonUuid":"old","startTime":"2026-01-01T00:00:00Z","endTime":"2026-09-01T00:00:00Z","borders":[]},
        {"seasonUuid":"current","startTime":"2026-09-01T00:00:00Z","endTime":"2026-11-01T00:00:00Z","competitiveTiersUuid":"tiers","borders":[]}]});
    let tiers=json!({"data":[{"uuid":"tiers","tiers":[{"tier":6,"tierName":"Bronze 1"}]}]});
    let now="2026-10-04T00:00:00Z".parse().unwrap();
    let rank=ranked_info(&mmr,&seasons,&tiers,Some(163),now).unwrap();
    assert_eq!(rank.rank.as_deref(),Some("Bronze 1"));assert_eq!(rank.rating,Some(42));assert_eq!(rank.eligibility,Some(true));
    let mut no_current=mmr.clone();no_current["QueueSkills"]["competitive"]["SeasonalInfoBySeasonID"].as_object_mut().unwrap().remove("current");
    let rank=ranked_info(&no_current,&seasons,&tiers,Some(5),now).unwrap();
    assert!(rank.rank.is_none());assert!(rank.rating.is_none());assert_eq!(rank.eligibility,Some(true),"Old competitive players keep the level exemption");
    assert!(ranked_info(&json!({}),&seasons,&tiers,None,now).is_err());
    let catalog=json!({"data":[{"uuid":"jett","displayName":"Jett"},{"uuid":"owned","displayName":"Owned agent"},{"uuid":"locked","displayName":"Locked agent"}]});
    let ids=["owned".into()].into_iter().collect();
    let agents=available_agents(&ids,&catalog).unwrap();assert_eq!(agents.len(),2);assert!(!agents.iter().any(|a|a.id=="locked"));
    assert!(available_agents(&ids,&json!({})).is_err());
    let old=serde_json::to_value(profile("riot_old")).unwrap();let mut old=old.as_object().unwrap().clone();old.remove("ranked");
    assert!(serde_json::from_value::<Profile>(old.into()).unwrap().ranked.is_none());
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
