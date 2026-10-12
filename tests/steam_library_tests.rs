use serde_json::json;
use std::fs;
use swch::launchers::steam::{
    library::{merge_remote, scan, valid_steam_id, SteamGame, STEAM_ID_BASE},
    remote::{parse_family, parse_inventory, parse_owned, SteamClient},
    SteamPaths,
};

#[test]
fn local_scan_uses_steamid64_correct_config_path_and_never_infers_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::create_dir_all(root.join("userdata/1/config")).unwrap();
    fs::create_dir_all(root.join("userdata/2/config")).unwrap();
    fs::write(root.join("config/loginusers.vdf"),r#""users" { "76561197960265729" { "AccountName" "one" "PersonaName" "One" "MostRecent" "1" } "76561197960265730" { "AccountName" "two" } }"#).unwrap();
    fs::write(
        root.join("steamapps/appmanifest_730.acf"),
        r#""AppState" { "name" "CS2" "StateFlags" "4" "installdir" "cs" }"#,
    )
    .unwrap();
    fs::write(
        root.join("steamapps/appmanifest_570.acf"),
        r#""AppState" { "name" "Dota" "StateFlags" "1026" }"#,
    )
    .unwrap();
    fs::write(root.join("steamapps/appmanifest_999.acf"), "broken {").unwrap();
    for (id, time) in [(1, 120), (2, 0)] {
        fs::write(root.join(format!("userdata/{id}/config/localconfig.vdf")),format!(r#""UserLocalConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "apps" {{ "730" {{ "Playtime" "{time}" }} "620" {{ "LastPlayed" "100" }} }} }} }} }} }}"#)).unwrap();
    }
    let data = scan(&SteamPaths::new(root)).unwrap();
    assert_eq!(data.accounts.len(), 2);
    assert!(data.accounts[0].is_active);
    let one = data
        .games
        .iter()
        .find(|g| g.game.app_id == "730" && g.account_id.as_deref() == Some("76561197960265729"))
        .unwrap();
    assert_eq!(one.playtime_minutes, Some(120));
    assert!(one.game.is_installed);
    assert_eq!(one.access, "local");
    assert!(one.owner_steamids.is_empty());
    let two = data
        .games
        .iter()
        .find(|g| g.game.app_id == "730" && g.account_id.as_deref() == Some("76561197960265730"))
        .unwrap();
    assert_eq!(two.playtime_minutes, Some(0));
    assert!(
        !data
            .games
            .iter()
            .find(|g| g.game.app_id == "570")
            .unwrap()
            .game
            .is_installed
    );
    assert!(data
        .games
        .iter()
        .find(|g| g.game.app_id == "620")
        .unwrap()
        .playtime_minutes
        .is_none());
    assert!(data.warnings.iter().any(|w| w.contains("999")));
    assert!(valid_steam_id(&(STEAM_ID_BASE + 1).to_string()));
    assert!(!valid_steam_id("../../bad"));
}
#[test]
fn remote_ownership_and_family_exclusions_are_explicit() {
    let id = "76561197960265729";
    assert!(parse_owned(&json!({"response":{}}), id).is_err());
    assert!(parse_owned(&json!({"response":{"game_count":0}}), id)
        .unwrap()
        .is_empty());
    let owned = parse_owned(
        &json!({"response":{"games":[{"appid":730,"name":"CS2","playtime_forever":0}]}}),
        id,
    )
    .unwrap();
    assert_eq!(owned[0].playtime_minutes, Some(0));
    let family=parse_family(&json!({"response":{"apps":[{"appid":620,"name":"Portal","owner_steamids":["76561197960265730"],"rt_playtime":999},{"appid":730,"owner_steamids":[id]},{"appid":999,"owner_steamids":["76561197960265730"],"exclude_reason":1}]}}),id).unwrap();
    assert_eq!(family.len(), 2);
    assert_eq!(family[0].access, "family");
    assert!(family[0].playtime_minutes.is_none());
    assert_eq!(family[1].access, "owned");
    let mut library = swch::launchers::steam::library::SteamLibrary::default();
    let mut local = SteamGame::new("620", Some(id.into()));
    local.playtime_minutes = Some(60);
    local.game.is_installed = true;
    local.game.install_path = Some("/test".into());
    library.games.push(local);
    merge_remote(&mut library, family);
    assert_eq!(library.games.len(), 2);
    assert_eq!(library.games[0].playtime_minutes, Some(60));
    assert!(library.games[0].game.is_installed);
}
#[test]
fn inventory_joins_instances_keeps_asset_ids_and_paginates() {
    let value = json!({"success":1,"total_inventory_count":3,"more_items":1,"last_assetid":"9007199254740999","assets":[{"assetid":"9007199254740999","classid":"a","instanceid":"2","amount":"3"}],"descriptions":[{"classid":"a","instanceid":"1","name":"Wrong"},{"classid":"a","instanceid":"2","name":"Skin","type":"Rifle","tradable":1,"marketable":0,"icon_url":"abc_-xyz","tags":[{"localized_tag_name":"Rare"}]}]});
    let page = parse_inventory(&value).unwrap();
    assert_eq!(page.items[0].name, "Skin");
    assert_eq!(page.items[0].amount, 3);
    assert!(page.items[0].tradable);
    assert!(!page.items[0].marketable);
    assert_eq!(page.next_cursor.as_deref(), Some("9007199254740999"));
    assert_eq!(page.items[0].tags, vec!["Rare"]);
    assert!(parse_inventory(&json!({"success":false})).is_err());
    assert!(parse_inventory(&json!({"success":1,"total_inventory_count":10})).is_err());
    assert!(
        parse_inventory(&json!({"success":1,"total_inventory_count":0}))
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        parse_inventory(&json!({"success":1,"total_inventory_count":0,"more_items":1})).is_err()
    );
}
#[tokio::test]
async fn remote_errors_do_not_expose_credentials() {
    use axum::{http::StatusCode, routing::get, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(get(|| async { StatusCode::FORBIDDEN })),
        )
        .await
        .unwrap()
    });
    let client = SteamClient::new(&format!("http://{addr}"), &format!("http://{addr}"));
    let error = client
        .owned("76561197960265729", "secret-test-key")
        .await
        .unwrap_err();
    assert!(!error.contains("secret-test-key"));
    assert!(error.contains("доступ"));
    server.abort();
}
#[tokio::test]
async fn cache_persists_and_isolates_account_game_context_and_cursor() {
    use swch::storage::Database;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("test.db");
    let db = Database::open(&path).unwrap();
    db.run_migrations().await.unwrap();
    db.steam_cache_put("inventory:a:730:2:first", "{\"items\":[]}")
        .await
        .unwrap();
    drop(db);
    let db = Database::open(path).unwrap();
    db.run_migrations().await.unwrap();
    assert!(db
        .steam_cache_get("inventory:a:730:2:first")
        .await
        .unwrap()
        .is_some());
    for key in [
        "inventory:b:730:2:first",
        "inventory:a:570:2:first",
        "inventory:a:730:6:first",
        "inventory:a:730:2:second",
    ] {
        assert!(db.steam_cache_get(key).await.unwrap().is_none());
    }
}
#[tokio::test]
async fn inventory_api_rejects_path_injection_before_network() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use swch::{server::create_app, storage::Database};
    use tower::ServiceExt;
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let response = create_app(db)
        .oneshot(
            Request::builder()
                .uri("/api/steam/inventory?account_id=bad&app_id=730")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn prices_parse_usd_exactly_and_do_not_turn_missing_prices_into_zero() {
    use swch::launchers::steam::remote::usd_cents;
    assert_eq!(usd_cents("$1,234.56"), Some(123456));
    assert_eq!(usd_cents("$0.03 USD"), Some(3));
    for bad in [
        "$1,23.45",
        "1,23€",
        "125 руб.",
        "$--",
        "$-1.00",
        "$1.2",
        "$1.00garbage",
        "",
    ] {
        assert_eq!(usd_cents(bad), None, "{bad}");
    }
}
#[test]
fn avatar_parser_only_accepts_steam_https_images() {
    use swch::launchers::steam::remote::avatar_from_xml;
    assert_eq!(
        avatar_from_xml(
            "<avatarFull><![CDATA[https://avatars.steamstatic.com/abc_full.jpg]]></avatarFull>"
        ),
        Some("https://avatars.steamstatic.com/abc_full.jpg".into())
    );
    for url in [
        "http://avatars.steamstatic.com/a.jpg",
        "https://evil.test/a.jpg",
        "https://steamstatic.com.evil.test/a.jpg",
        "javascript:alert(1)",
    ] {
        assert!(avatar_from_xml(&format!("<avatarFull>{url}</avatarFull>")).is_none());
    }
}
#[tokio::test]
async fn price_client_uses_usd_and_preserves_no_listing() {
    use axum::{extract::Query, routing::get, Json, Router};
    use std::collections::HashMap;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new().route(
        "/market/priceoverview/",
        get(|Query(q): Query<HashMap<String, String>>| async move {
            assert_eq!(q.get("currency").map(String::as_str), Some("1"));
            assert_eq!(q.get("country").map(String::as_str), Some("US"));
            Json(if q["market_hash_name"] == "Missing" {
                json!({"success":true})
            } else {
                json!({"success":true,"lowest_price":"$2.50"})
            })
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = SteamClient::new(&format!("http://{addr}"), &format!("http://{addr}"));
    assert_eq!(
        client.price("730", "Skin").await.unwrap().unit_price_cents,
        Some(250)
    );
    assert_eq!(
        client
            .price("730", "Missing")
            .await
            .unwrap()
            .unit_price_cents,
        None
    );
    server.abort();
}
#[tokio::test]
async fn price_endpoint_reuses_persisted_quotes_without_network() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use swch::{server::create_app, storage::Database};
    use tower::ServiceExt;
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    db.steam_cache_put("price:USD:730:Skin",&json!({"unit_price_cents":123,"currency":"USD","updated_at":"2026-09-15T00:00:00Z","cached":false,"warning":null}).to_string()).await.unwrap();
    let r = create_app(db)
        .oneshot(
            Request::builder()
                .uri("/api/steam/price?app_id=730&market_hash_name=Skin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = r.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["unit_price_cents"], 123);
    assert_eq!(v["cached"], true);
}

#[test]
fn inventory_catalog_uses_app_ids_and_ignores_trailing_javascript() {
    use swch::launchers::steam::remote::inventory_apps_from_html;
    let apps=inventory_apps_from_html(r#"<script>var g_rgAppContextData = {"730":{"name":"CS2"},"753":{},"invalid":{}}; doSomething();</script>"#).unwrap();
    assert_eq!(apps, vec!["730", "753"]);
    assert!(inventory_apps_from_html("<html>error</html>").is_none());
}

#[test]
fn steam_contexts_preserve_all_games_and_sections_and_reject_private_profiles() {
    use swch::launchers::steam::remote::inventory_contexts_from_html;
    let html = r#"var g_rgAppContextData={"730":{"name":"CS2","rgContexts":{"2":{"asset_count":4},"3":{"asset_count":"2"}}},"570":{"name":"Dota","rgContexts":{"2":{"asset_count":0}}}}; ignored();"#;
    let contexts = inventory_contexts_from_html(html).unwrap();
    assert_eq!(contexts.len(), 3);
    assert_eq!(
        contexts.iter().find(|c| c.context_id == "3").unwrap().count,
        Some(2)
    );
    assert!(
        inventory_contexts_from_html(&format!("var g_bInventoryIsPrivate = true; {html}"))
            .is_none()
    );
    assert!(inventory_contexts_from_html("<html>Sign in</html>").is_none());
}
#[test]
fn inventory_retains_category_and_internal_tag_values_for_steam_filters() {
    let page=parse_inventory(&json!({"success":1,"total_inventory_count":1,"assets":[{"assetid":"1","classid":"2","instanceid":"0","amount":"1"}],"descriptions":[{"classid":"2","instanceid":"0","name":"Skin","tags":[{"category":"Quality","localized_category_name":"Качество","internal_name":"strange","localized_tag_name":"StatTrak"}]}]})).unwrap();
    assert_eq!(page.items[0].tag_details[0].category, "Quality");
    assert_eq!(page.items[0].tag_details[0].value, "strange");
    assert_eq!(page.items[0].tags, vec!["StatTrak"]);
}
#[test]
fn manifest_update_progress_survives_remote_library_overlay() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("steamapps")).unwrap();
    fs::write(temp.path().join("steamapps/appmanifest_730.acf"),r#""AppState" {"StateFlags" "1030" "installdir" "cs" "BytesToDownload" "1000" "BytesDownloaded" "250" "BytesToStage" "2000" "BytesStaged" "500"}"#).unwrap();
    let mut data = scan(&SteamPaths::new(temp.path())).unwrap();
    let update = data.games[0].update.as_ref().unwrap();
    assert!(update.required);
    assert_eq!(update.phase, "downloading");
    assert_eq!(update.downloaded, 250);
    assert_eq!(update.download_total, 1000);
    merge_remote(
        &mut data,
        vec![SteamGame::new("730", Some("76561197960265729".into()))],
    );
    assert_eq!(data.games[0].update.as_ref().unwrap().staged, 500);
}
#[tokio::test]
async fn inventory_cache_hydrates_page_chains_prices_and_keeps_other_cache_private() {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use swch::{server::create_app, storage::Database};
    use tower::ServiceExt;
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let page = |asset: &str, next: Option<&str>, date: &str| json!({"items":[{"asset_id":asset,"name":"Skin","amount":1,"marketable":true,"tradable":true,"market_hash_name":"Skin","item_type":"","icon_url":null,"tags":[]}],"total_count":2,"next_cursor":next,"updated_at":date,"cached":false,"warning":null});
    db.steam_cache_put(
        "inventory:a:730:2:first",
        &page("1", Some("1"), "2026-09-15T00:00:00Z").to_string(),
    )
    .await
    .unwrap();
    db.steam_cache_put(
        "inventory:a:730:2:1",
        &page("2", None, "2026-09-15T00:00:01Z").to_string(),
    )
    .await
    .unwrap();
    db.steam_cache_put(
        "price:USD:730:Skin:Variant",
        r#"{"unit_price_cents":100,"currency":"USD"}"#,
    )
    .await
    .unwrap();
    db.steam_cache_put("unrelated", "secret").await.unwrap();
    let get = || {
        Request::builder()
            .uri("/api/steam/inventory-cache")
            .body(Body::empty())
            .unwrap()
    };
    let response = create_app(db.clone()).oneshot(get()).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value["pages"][0]["page"]["items"].as_array().unwrap().len(),
        2
    );
    assert_eq!(value["quotes"][0]["name"], "Skin:Variant");
    assert!(!String::from_utf8_lossy(&body).contains("secret"));
    db.steam_cache_put(
        "inventory:a:730:2:first",
        &page("3", Some("1"), "2026-09-15T00:00:03Z").to_string(),
    )
    .await
    .unwrap();
    let response = create_app(db).oneshot(get()).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value["pages"][0]["page"]["items"].as_array().unwrap().len(),
        1,
        "new first page must not merge older tail"
    );
    assert_eq!(value["pages"][0]["page"]["next_cursor"], "1");
}

#[test]
fn parse_app_details_extracts_name_and_is_free() {
    use swch::launchers::steam::remote::parse_app_details;
    let json = json!({
        "1453490": {
            "success": true,
            "data": {
                "name": "Deducto",
                "is_free": true
            }
        }
    });
    let details = parse_app_details(&json, "1453490").unwrap();
    assert_eq!(details.name.as_deref(), Some("Deducto"));
    assert_eq!(details.is_free, Some(true));

    let failed = json!({
        "123": {
            "success": false
        }
    });
    assert!(parse_app_details(&failed, "123").is_err());
}

#[test]
fn artwork_uses_current_store_hashes_with_safe_fallbacks() {
    use swch::launchers::steam::remote::parse_artwork;
    let mut data = json!({"response":{"store_items":[{"id":3478870,"assets":{
        "asset_url_format":"steam/apps/3478870/${FILENAME}?t=123",
        "library_capsule_2x":"abc123/library_capsule_2x.jpg",
        "library_hero":"def456/library_hero.jpg","header":"abc/header.jpg"
    }}]}});
    let art=parse_artwork(&data,"3478870").unwrap();
    assert_eq!(art.poster.as_deref(),Some("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3478870/abc123/library_capsule_2x.jpg?t=123"));
    assert!(art.hero.unwrap().ends_with("def456/library_hero.jpg?t=123"));
    data["response"]["store_items"][0]["assets"]["library_capsule_2x"]=json!("../../other.png");
    assert!(parse_artwork(&data,"3478870").unwrap().poster.unwrap().ends_with("abc/header.jpg?t=123"));
    assert!(parse_artwork(&data,"570").is_err());
    data["response"]["store_items"][0]["assets"]["asset_url_format"]=json!("https://elsewhere.test/${FILENAME}");
    assert!(parse_artwork(&data,"3478870").is_err());
}

#[test]
fn local_scan_correctly_identifies_last_owner_and_family_sharing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::create_dir_all(root.join("userdata/1/config")).unwrap();
    fs::create_dir_all(root.join("userdata/2/config")).unwrap();
    fs::write(
        root.join("config/loginusers.vdf"),
        r#""users" { "76561197960265729" { "AccountName" "owner_user" "MostRecent" "1" } "76561197960265730" { "AccountName" "family_user" } }"#,
    ).unwrap();
    // App 322170 has LastOwner 76561197960265729 (owner_user)
    fs::write(
        root.join("steamapps/appmanifest_322170.acf"),
        r#""AppState" { "name" "Geometry Dash" "StateFlags" "4" "LastOwner" "76561197960265729" }"#,
    ).unwrap();
    // App 730 is in family_user's localconfig (uninstalled), with FamilyGroup linking user 1 and user 2
    fs::write(
        root.join("userdata/2/config/localconfig.vdf"),
        r#""UserLocalConfigStore" { "FamilyGroup" { "groupid" "12345" "name" "test family" "members" { "0" { "accountid" "1" } "1" { "accountid" "2" } } } "Software" { "Valve" { "Steam" { "apps" { "730" { "Playtime" "50" } } } } } }"#,
    ).unwrap();

    let data = scan(&SteamPaths::new(root)).unwrap();
    // For owner_user: 322170 should be owned
    let owner_gd = data.games.iter().find(|g| g.game.app_id == "322170" && g.account_id.as_deref() == Some("76561197960265729")).unwrap();
    assert_eq!(owner_gd.access, "owned");
    assert_eq!(owner_gd.owner_steamids, vec!["76561197960265729".to_string()]);

    // For family_user: 322170 should be family shared from owner_user
    let family_gd = data.games.iter().find(|g| g.game.app_id == "322170" && g.account_id.as_deref() == Some("76561197960265730")).unwrap();
    assert_eq!(family_gd.access, "family");
    assert_eq!(family_gd.owner_steamids, vec!["76561197960265729".to_string()]);

    // Playtime links this account to 730 without proving ownership.
    let family_cs = data.games.iter().find(|g| g.game.app_id == "730" && g.account_id.as_deref() == Some("76561197960265730")).unwrap();
    assert_eq!(family_cs.access, "local");
    assert_eq!(family_cs.playtime_minutes, Some(50));
    assert!(family_cs.owner_steamids.is_empty());
}

#[test]
fn local_scan_does_not_share_games_with_accounts_outside_family() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::create_dir_all(root.join("userdata/1/config")).unwrap();
    fs::create_dir_all(root.join("userdata/3/config")).unwrap();
    fs::create_dir_all(root.join("userdata/4/config")).unwrap();
    fs::write(
        root.join("config/loginusers.vdf"),
        r#""users" { "76561197960265729" { "AccountName" "owner_user" "MostRecent" "1" } "76561197960265731" { "AccountName" "no_family_user" } "76561197960265732" { "AccountName" "solo_family_user" } }"#,
    ).unwrap();
    fs::write(
        root.join("steamapps/appmanifest_322170.acf"),
        r#""AppState" { "name" "Geometry Dash" "StateFlags" "4" "LastOwner" "76561197960265729" }"#,
    ).unwrap();
    // user 3 has no family group
    fs::write(
        root.join("userdata/3/config/localconfig.vdf"),
        r#""UserLocalConfigStore" { "Software" { "Valve" { "Steam" { "apps" { "730" { "Playtime" "10" } } } } } }"#,
    ).unwrap();
    // user 4 has a solo family group with only themselves
    fs::write(
        root.join("userdata/4/config/localconfig.vdf"),
        r#""UserLocalConfigStore" { "FamilyGroup" { "groupid" "99999" "name" "solo" "members" { "0" { "accountid" "4" } } } "Software" { "Valve" { "Steam" { "apps" { "730" { "Playtime" "20" } } } } } }"#,
    ).unwrap();

    let data = scan(&SteamPaths::new(root)).unwrap();
    // Neither user 3 nor user 4 should receive Geometry Dash
    assert!(!data.games.iter().any(|g| g.game.app_id == "322170" && g.account_id.as_deref() == Some("76561197960265731")));
    assert!(!data.games.iter().any(|g| g.game.app_id == "322170" && g.account_id.as_deref() == Some("76561197960265732")));
}

#[test]
fn local_scan_discovers_family_games_from_cloud_and_cache() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(root.join("logs")).unwrap();
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::create_dir_all(root.join("userdata/1/config")).unwrap();
    fs::create_dir_all(root.join("userdata/2/config/cloudstorage")).unwrap();
    fs::create_dir_all(root.join("userdata/2/config/librarycache")).unwrap();

    fs::write(
        root.join("config/loginusers.vdf"),
        r#""users" { "76561197960265729" { "AccountName" "owner_user" "MostRecent" "1" } "76561197960265730" { "AccountName" "family_user" } }"#,
    ).unwrap();
    fs::write(
        root.join("userdata/2/config/localconfig.vdf"),
        r#""UserLocalConfigStore" { "FamilyGroup" { "groupid" "12345" "name" "test family" "members" { "0" { "accountid" "1" } "1" { "accountid" "2" } } } "Software" { "Valve" { "Steam" { "apps" { } } } } }"#,
    ).unwrap();

    // Cloud storage with NewContentRollup_444090 and collection with 1607250
    let cloud_json = json!([
        ["NewContentRollup_444090", {"value": "{\"rtOld\":0}"}],
        ["user-collections.test", {"value": "{\"added\":[1607250]}"}]
    ]);
    fs::write(
        root.join("userdata/2/config/cloudstorage/cloud-storage-namespace-1.json"),
        serde_json::to_string(&cloud_json).unwrap(),
    ).unwrap();

    // Library cache with 578080.json
    fs::write(root.join("userdata/2/config/librarycache/578080.json"), "{}").unwrap();

    // librarysharing_log.txt attributing 444090 to owner 1
    fs::write(
        root.join("logs/librarysharing_log.txt"),
        "[2026-01-01 00:00:00] Shared lock for app 444090 changed : available owner 0, preferred owner 1, current player 2\n",
    ).unwrap();

    let data = scan(&SteamPaths::new(root)).unwrap();
    let family_games: Vec<_> = data
        .games
        .iter()
        .filter(|g| g.account_id.as_deref() == Some("76561197960265730"))
        .collect();

    assert_eq!(family_games.len(), 3);
    for g in &family_games {
        assert_eq!(g.access, "family");
        assert_eq!(g.source, "steam_family");
    }

    let g444090 = family_games.iter().find(|g| g.game.app_id == "444090").unwrap();
    assert_eq!(g444090.owner_steamids, vec!["76561197960265729".to_string()]);

    let g1607250 = family_games.iter().find(|g| g.game.app_id == "1607250").unwrap();
    assert_eq!(g1607250.owner_steamids, vec!["76561197960265729".to_string()]);

    let g578080 = family_games.iter().find(|g| g.game.app_id == "578080").unwrap();
    assert_eq!(g578080.owner_steamids, vec!["76561197960265729".to_string()]);
}

#[test]
fn test_live_steam_scan_respects_family_group_boundaries() {
    if let Ok(paths) = SteamPaths::detect() {
        if paths.steam_dir.exists() {
            let lib = scan(&paths).unwrap();
            for acc in &lib.accounts {
                let games: Vec<_> = lib.games.iter().filter(|g| g.account_id.as_deref() == Some(&acc.id)).collect();
                let family = games.iter().filter(|g| g.access == "family").count();
                if acc.id == "76561198809112858" || acc.id == "76561199103381372" {
                    assert_eq!(family, 0, "Account {} should not have family games", acc.username);
                }
                if acc.id == "76561199123282553" {
                    assert!(family >= 100, "nazario2015 should have 100+ family games, got {}", family);
                }
            }
        }
    }
}

#[test]
fn ignored_steam_system_apps_are_excluded() {
    use swch::launchers::steam::library::is_ignored_steam_app;

    assert!(is_ignored_steam_app("7", ""));
    assert!(is_ignored_steam_app("760", ""));
    assert!(is_ignored_steam_app("2371090", ""));
    assert!(is_ignored_steam_app("12345", "Steam Client"));
    assert!(is_ignored_steam_app("12345", "steam screenshots"));
    assert!(is_ignored_steam_app("12345", "Steam Game Notes"));
    assert!(!is_ignored_steam_app("730", "Counter-Strike 2"));

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::create_dir_all(root.join("userdata/1/config")).unwrap();
    fs::write(
        root.join("config/loginusers.vdf"),
        r#""users" { "76561197960265729" { "AccountName" "user1" "MostRecent" "1" } }"#,
    ).unwrap();
    fs::write(
        root.join("steamapps/appmanifest_7.acf"),
        r#""AppState" { "name" "Steam Client" "StateFlags" "4" }"#,
    ).unwrap();
    fs::write(
        root.join("steamapps/appmanifest_760.acf"),
        r#""AppState" { "name" "Steam Screenshots" "StateFlags" "4" }"#,
    ).unwrap();
    fs::write(
        root.join("userdata/1/config/localconfig.vdf"),
        r#""UserLocalConfigStore" { "Software" { "Valve" { "Steam" { "apps" { "7" { "Playtime" "10" } "760" { "Playtime" "20" } "2371090" { "Playtime" "30" } "730" { "Playtime" "40" } } } } } }"#,
    ).unwrap();

    let data = scan(&SteamPaths::new(root)).unwrap();
    assert_eq!(data.games.len(), 1);
    assert_eq!(data.games[0].game.app_id, "730");
    assert!(!data.games.iter().any(|g| g.game.app_id == "7" || g.game.app_id == "760" || g.game.app_id == "2371090"));

    let owned = parse_owned(
        &json!({"response":{"games":[{"appid":7,"name":"Steam Client"},{"appid":760,"name":"Steam Screenshots"},{"appid":2371090,"name":"Steam Game Notes"},{"appid":730,"name":"CS2"}]}}),
        "76561197960265729",
    ).unwrap();
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].game.app_id, "730");
}
