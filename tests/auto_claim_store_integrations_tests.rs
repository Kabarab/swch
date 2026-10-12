use serde_json::json;
use swch::auto_claim::{fanatical, microsoft, notifications, store_discovery as discovery};

fn ubi(title: &str, url: &str, end: &str) -> String {
    let state = json!({"NewsPersonalization":{"one":{"data":{"news":[{"placement":"freeevents","type":"gametrial","title":title,"publicationDate":"2026-10-01T00:00:00Z","expirationDate":end,"links":[{"param":url}]}]}}}});
    format!("<script>window.__PRELOADED_STATE__ = {state};</script>")
}
#[test]
fn ubisoft_feed_requires_real_finite_giveaway_and_exact_registration_host() {
    let now = 1791417600; // 2026-10-08 UTC
    let parsed = discovery::ubisoft(
        &ubi(
            "Paid Game",
            "https://register.ubisoft.com/paid-game/en-US",
            "2026-10-09T00:00:00Z",
        ),
        now,
    )
    .unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].id, "paid-game");
    assert!(parsed[0].regular_price.is_none());
    for (title, url, end) in [
        (
            "Paid Game Free Weekend",
            "https://register.ubisoft.com/paid-game/en-US",
            "2026-10-09T00:00:00Z",
        ),
        (
            "Paid Game",
            "https://register.ubisoft.com.evil.org/paid-game",
            "2026-10-09T00:00:00Z",
        ),
        (
            "Paid Game",
            "https://register.ubisoft.com/paid-game/en-US",
            "2050-10-09T00:00:00Z",
        ),
        (
            "Paid Game",
            "https://register.ubisoft.com/paid-game/en-US",
            "2026-10-01T00:00:00Z",
        ),
    ] {
        assert!(discovery::ubisoft(&ubi(title, url, end), now)
            .unwrap()
            .is_empty());
    }
    assert!(discovery::ubisoft("challenge page", now).is_err());
}
#[test]
fn steamdb_badge_is_scoped_to_its_own_card_and_not_the_faq() {
    let html = r#"FAQ: Free to Keep <div class="span4 panel-sale"><span>Play For Free</span><a href="https://store.steampowered.com/app/101/">First</a></div><div class="span4 panel-sale"><span class="cat-free-to-keep">Free to Keep</span><a href="https://store.steampowered.com/app/202/">Second</a></div>"#;
    assert_eq!(discovery::steamdb(html), vec!["202"]);
}
fn ms(price: serde_json::Value) -> serde_json::Value {
    json!({"Payload":{"ProductId":"9ABCDEFGHIJK","Title":"Paid Game","ProductType":"Game","MSRP":10.0,"Price":price,"DisplayPrice":"Free"}})
}
#[test]
fn microsoft_prices_are_authoritative_and_never_round_a_charge_to_free() {
    assert_eq!(
        discovery::microsoft_product(&ms(json!(0)), "9ABCDEFGHIJK", "Paid Game")
            .unwrap()
            .price,
        0
    );
    assert!(
        discovery::microsoft_product(&ms(json!(0.001)), "9ABCDEFGHIJK", "Paid Game")
            .unwrap()
            .price
            > 0
    );
    assert!(
        discovery::microsoft_product(&ms(json!(0)), "9ABCDEFGHIJK", "Paid Game Deluxe").is_err()
    );
    let mut data = ms(json!(0));
    data["Payload"]["DisplayPrice"] = json!("Included");
    assert!(discovery::microsoft_product(&data, "9ABCDEFGHIJK", "Paid Game").is_err());
    data = ms(json!(0));
    data["Payload"]["MSRP"] = json!(0);
    assert!(discovery::microsoft_product(&data, "9ABCDEFGHIJK", "Paid Game").is_err());
    data = ms(json!(0));
    data["Payload"]["ProductType"] = json!("Durable");
    assert!(discovery::microsoft_product(&data, "9ABCDEFGHIJK", "Paid Game").is_err());
}
#[test]
fn microsoft_id_never_comes_from_an_untrusted_host_or_auth_query() {
    assert_eq!(
        discovery::microsoft_id("https://www.xbox.com/en-US/games/store/game/9ABCDEFGHIJK")
            .as_deref(),
        Some("9ABCDEFGHIJK")
    );
    assert!(discovery::microsoft_id(
        "https://www.xbox.com.evil.org/en-US/games/store/game/9ABCDEFGHIJK"
    )
    .is_none());
    assert!(discovery::microsoft_id(
        "https://www.xbox.com/en-US/games/store/game/9ABCDEFGHIJK?access_token=secret"
    )
    .is_none());
}
fn order() -> serde_json::Value {
    json!([{"_id":"own-order","items":[{"_id":"item-1","name":"Paid Game","slug":"paid-game","drm":"Steam","serialId":"serial-1","iid":"one","key":"ABCDE-FGHI1-JKLM2"},{"_id":"item-2","name":"Paid Game Deluxe","slug":"paid-game-deluxe","drm":"Steam","key":"OTHER-KEY12-34567"}]}])
}
#[test]
fn fanatical_matches_one_specific_item_and_never_reveals_a_neighbour_key() {
    let found = fanatical::order_item(&order(), "paid-game", "Paid Game")
        .unwrap()
        .unwrap();
    assert_eq!(found["pid"], "item-1");
    assert_eq!(
        fanatical::item_key(&found["item"]).as_deref(),
        Some("ABCDE-FGHI1-JKLM2")
    );
    assert_eq!(
        fanatical::item_platform(&found["item"]).as_deref(),
        Some("steam")
    );
    assert!(fanatical::order_item(&order(), "other-slug", "Paid Game")
        .unwrap()
        .is_none());
    let duplicate = json!([order()[0].clone(), order()[0].clone()]);
    assert!(fanatical::order_item(&duplicate, "paid-game", "Paid Game").is_err());
    assert!(fanatical::item_platform(&json!({"platforms":["Steam","GOG"]})).is_none());
    assert!(
        fanatical::item_key(&json!({"key":"ABCDE-FGHI1-JKLM2","code":"OTHER-KEY12-34567"}))
            .is_none()
    );
}
#[test]
fn microsoft_redeem_rejects_different_account_product_and_non_game_prompts() {
    let mut state = microsoft::RedeemState {
        url: "https://purchase.mp.microsoft.com/redeem".into(),
        email: Some("account@example.org".into()),
        product_title: "Paid Game".into(),
        product_id: Some("9ABCDEFGHIJK".into()),
        confirm: true,
        ..Default::default()
    };
    assert!(microsoft::preview_matches(
        &state,
        "account@example.org",
        "Paid Game"
    ));
    assert!(!microsoft::preview_matches(
        &state,
        "different@example.org",
        "Paid Game"
    ));
    assert!(!microsoft::preview_matches(
        &state,
        "account@example.org",
        "Paid Game Deluxe"
    ));
    state.sensitive = true;
    assert!(!microsoft::preview_matches(
        &state,
        "account@example.org",
        "Paid Game"
    ));
    assert!(microsoft::valid_code("ABCDE-ABCDE-ABCDE-ABCDE-ABCDE"));
    assert!(!microsoft::valid_code("ABCDE-ABCDE-ABCDE"));
}
#[test]
fn prime_full_game_candidates_do_not_authorize_an_undated_claim() {
    let now = 1791417600;
    let cards = json!([{"id":"one","title":"Paid Game","url":"https://luna.amazon.com/claims/paid-game-gog/dp/one","ends_at":null,"regular_price":null},{"id":"two","title":"Paid Game Demo","url":"https://luna.amazon.com/claims/demo/dp/two","ends_at":1791504000,"regular_price":100}]);
    let out = discovery::prime_cards(&cards, now).unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].ends_at.is_none());
    assert!(out[0].regular_price.is_none());
}
#[test]
fn notification_configuration_hides_secrets_from_status_and_requires_secure_transport() {
    let config = notifications::Config {
        discord: Some("https://discord.com/api/webhooks/123/secret-token".into()),
        ..Default::default()
    };
    assert!(config.validate().is_ok());
    assert_eq!(config.channels(), vec!["Discord"]);
    assert!(!notifications::valid_webhook(
        "http://example.org/hook",
        false
    ));
    assert!(!notifications::valid_webhook(
        "https://user:password@example.org/hook",
        false
    ));
    assert!(!notifications::valid_webhook(
        "https://discord.com.evil.org/api/webhooks/secret",
        true
    ));
    let bad = notifications::Config {
        email: Some(notifications::Email {
            host: "smtp.example.org".into(),
            port: 25,
            username: "user".into(),
            password: "secret".into(),
            from: "from@example.org".into(),
            to: "to@example.org".into(),
        }),
        ..Default::default()
    };
    assert!(bad.validate().is_err());
}
#[test]
fn fanatical_storage_cannot_be_replayed_into_another_platform() {
    use swch::auto_claim::web::{Cookie, Session, StorageEntry};
    let mut session = Session {
        platform: "fanatical".into(),
        account_id: "one".into(),
        identity: "user@example.org".into(),
        country: "US".into(),
        cookies: vec![Cookie {
            name: "session".into(),
            value: "test".into(),
            domain: "www.fanatical.com".into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            expires: None,
            same_site: None,
        }],
        connected_at: String::new(),
        identity_proof: "login_form".into(),
        store_identity: None,
        web_storage: vec![StorageEntry {
            origin: "https://www.fanatical.com".into(),
            name: "bsauth".into(),
            value: r#"{"token":"test","email":"user@example.org"}"#.into(),
        }],
    };
    assert!(session.validate().is_ok());
    session.web_storage[0].origin = "https://evil.org".into();
    assert!(session.validate().is_err());
    session.web_storage[0].origin = "https://www.fanatical.com".into();
    session.web_storage[0].name = "password".into();
    assert!(session.validate().is_err());
}
#[test]
fn literal_html_redirects_do_not_execute_scripts_or_choose_ambiguous_targets() {
    use swch::auto_claim::catalog::html_redirect;
    assert_eq!(html_redirect(r#"<meta http-equiv="refresh" content="0;url=https://www.fanatical.com/en/game/paid-game">"#).as_deref(),Some("https://www.fanatical.com/en/game/paid-game"));
    assert!(
        html_redirect(r#"<script>window.location.replace('https://evil.org/')</script>"#).is_none()
    );
    assert!(html_redirect(r#"<script>window.location.replace('https://www.fanatical.com/en/game/one');window.location.replace('https://www.fanatical.com/en/game/two');</script>"#).is_none());
}
