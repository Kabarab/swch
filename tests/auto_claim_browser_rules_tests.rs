use serde_json::json;
use swch::auto_claim::{
    keys, mobile, platforms,
    web_rules::{decision, Decision, Snapshot},
};

#[test]
fn epic_locale_aliases_match_only_the_same_product() {
    let native = "https://store.epicgames.com/en-US/p/paid-game-123";
    assert!(platforms::same_product_url("epic", native,
        "https://store.epicgames.com/p/paid-game-123?lang=ru"));
    assert!(!platforms::same_product_url("epic", native,
        "https://store.epicgames.com/en-US/p/paid-game-deluxe-456"));
    assert!(!platforms::same_product_url("epic", native,
        "https://store.epicgames.com.example.org/p/paid-game-123"));
}

fn ready() -> Snapshot {
    Snapshot {
        signed_in: true,
        identity: Some("expected".into()),
        checkout_offer_matches: true,
        price_zero: true,
        claim_label: Some("Get".into()),
        ..Snapshot::default()
    }
}
#[test]
fn wrong_account_product_or_unknown_price_cannot_submit_even_free_checkout() {
    let s = ready();
    assert_eq!(decision(&s, "expected", true, true), Decision::Get);
    assert_eq!(decision(&s, "other", true, true), Decision::Reject);
    let mut s = ready();
    s.checkout_offer_matches = false;
    assert_eq!(decision(&s, "expected", true, true), Decision::Reject);
    let mut s = ready();
    s.price_zero = false;
    assert_eq!(decision(&s, "expected", true, true), Decision::Reject);
    assert_eq!(
        decision(&ready(), "expected", false, true),
        Decision::Reject
    );
    assert_eq!(
        decision(&ready(), "expected", true, false),
        Decision::Reject
    );
}
#[test]
fn place_order_needs_exactly_one_matching_item_and_explicit_zero_total() {
    let mut s = ready();
    s.claim_label = Some("Place order".into());
    assert_eq!(decision(&s, "expected", true, true), Decision::Reject);
    s.checkout_total_zero = true;
    assert_eq!(decision(&s, "expected", true, true), Decision::Reject);
    s.checkout_item_count = Some(2);
    assert_eq!(decision(&s, "expected", true, true), Decision::Reject);
    s.checkout_item_count = Some(1);
    assert_eq!(decision(&s, "expected", true, true), Decision::Confirm);
    s.checkout_total_zero = false;
    assert_eq!(decision(&s, "expected", true, true), Decision::Reject);
}
#[test]
fn challenges_age_and_subscription_never_become_ownership_or_consume_a_code() {
    let mut s = ready();
    s.code = Some("ABC12-DEF34-GHI56".into());
    s.in_library = true;
    s.challenge = true;
    assert_eq!(decision(&s, "expected", true, true), Decision::Challenge);
    s.challenge = false;
    s.age_gate = true;
    assert_eq!(
        decision(&s, "expected", true, true),
        Decision::NeedsConfirmation
    );
    s.age_gate = false;
    s.requires_confirmation = true;
    assert_eq!(
        decision(&s, "expected", true, true),
        Decision::NeedsConfirmation
    );
    s.requires_confirmation = false;
    s.in_library = false;
    s.requires_subscription = true;
    assert_eq!(
        decision(&s, "expected", true, true),
        Decision::NeedsSubscription
    );
    s.requires_subscription = false;
    assert_eq!(decision(&s, "expected", true, true), Decision::ClaimCode);
    s.signed_in = false;
    assert_eq!(decision(&s, "expected", true, true), Decision::NeedsLogin);
}
#[test]
fn product_links_cannot_be_login_logout_or_mutation_links() {
    for (p, url) in [
        (
            "epic",
            "https://www.epicgames.com/id/api/set-sid?sid=secret",
        ),
        ("steam", "https://store.steampowered.com/account/"),
        ("gog", "https://www.gog.com/account/settings/subscriptions"),
        ("epic", "https://evil.epicgames.com.evil/p/game"),
        (
            "epic",
            "https://store.epicgames.com/p/game?access_token=secret",
        ),
    ] {
        assert!(!platforms::product_url(p, url), "{url}");
    }
    assert!(platforms::product_url(
        "epic",
        "https://store.epicgames.com/en-US/p/a-game"
    ));
    assert!(platforms::product_url(
        "itchio",
        "https://publisher.itch.io/a-game"
    ));
}
#[test]
fn receipt_identity_is_structured_and_not_title_or_global_key_dedup() {
    assert_ne!(
        keys::receipt_id("prime", "a", "x"),
        keys::receipt_id("prime", "b", "x")
    );
    assert_ne!(
        keys::receipt_id("a:b", "c", "d"),
        keys::receipt_id("a", "b:c", "d")
    );
    for value in [
        "password",
        "1234567",
        "1234 5678",
        "ABCD1234\r\n",
        "12345678<script>",
    ] {
        assert!(!keys::valid_code(value));
    }
    assert!(keys::valid_code("AB123-CD456-EF789"));
}
#[test]
fn steam_activation_needs_a_specific_success_receipt_for_the_requested_game() {
    let good = json!({"success":1,"purchase_result_details":0,"purchase_receipt_info":{"line_items":[{"packageid":123,"line_item_description":"Paid Game"}]}});
    assert_eq!(
        keys::steam_receipt_proof(&good, "Paid Game").unwrap(),
        "123"
    );
    assert!(keys::steam_receipt_proof(&good, "Other Game").is_err());
    let mut bad = good.clone();
    bad["success"] = json!(0);
    assert!(keys::steam_receipt_proof(&bad, "Paid Game").is_err());
    let mut bad = good.clone();
    bad["purchase_result_details"] = json!(9);
    assert!(keys::steam_receipt_proof(&bad, "Paid Game").is_err());
    let mut bad = good.clone();
    bad["purchase_receipt_info"]["line_items"][0]["packageid"] = json!(0);
    assert!(keys::steam_receipt_proof(&bad, "Paid Game").is_err());
    let mut bad = good.clone();
    bad["purchase_receipt_info"]["line_items"] = json!([{"packageid":123,"line_item_description":"Paid Game"},{"packageid":456,"line_item_description":"Other Game"}]);
    assert!(keys::steam_receipt_proof(&bad, "Paid Game").is_err());
}
fn mobile_game() -> serde_json::Value {
    json!({"data":[{"type":"freeGame","offers":[{"content":{"id":"offer","namespace":"catalog","title":"Paid Mobile Game","mapping":{"slug":"paid-mobile-game"},"items":[{"id":"mobile-item"}],"purchase":[{"purchaseType":"Claim","price":{"decimalPrice":0},"endDate":"2030-10-15T10:00:00Z"},{"purchaseType":"Purchase","price":{"decimalPrice":10}}]}}]}]})
}
#[test]
fn mobile_claim_does_not_conflate_pc_and_different_platform_skus() {
    let data = mobile_game();
    let android = mobile::offers(&data, "android", 1).unwrap();
    let ios = mobile::offers(&data, "ios", 1).unwrap();
    assert_eq!(android[0].item_ids, vec!["mobile-item"]);
    assert_ne!(android[0].key(), ios[0].key());
    assert!(mobile::offers(&data, "pc", 1).is_err());
    let mut bad = data.clone();
    bad["data"][0]["offers"][0]["content"]["purchase"][0]["price"]["decimalPrice"] = json!(1);
    assert!(mobile::offers(&bad, "android", 1).unwrap().is_empty());
    let mut bad = data.clone();
    bad["data"][0]["offers"][0]["content"]["purchase"][0]["endDate"] = json!("unknown");
    assert!(mobile::offers(&bad, "android", 1).unwrap().is_empty());
    let mut bad = data.clone();
    bad["data"][0]["offers"][0]["content"]["purchase"][1]["price"]["decimalPrice"] = json!(0);
    assert!(mobile::offers(&bad, "android", 1).unwrap().is_empty());
    let mut bad = data.clone();
    bad["data"][0]["offers"][0]["content"]["items"] = json!([]);
    assert!(mobile::offers(&bad, "android", 1).unwrap().is_empty());
    let mut bad = data.clone();
    bad["data"][0]["offers"][0]["content"]["isFreeToPlay"] = json!(true);
    assert!(mobile::offers(&bad, "android", 1).unwrap().is_empty());
}
#[tokio::test]
async fn key_metadata_cannot_erase_a_selected_destination_or_contain_a_game_key() {
    let temp = tempfile::tempdir().unwrap();
    let db = swch::storage::Database::open(temp.path().join("db.sqlite")).unwrap();
    db.run_migrations().await.unwrap();
    db.set_auto_claim_value(
        "key-receipt:123",
        &json!({"status":"redeeming","target_account":"selected"}).to_string(),
    )
    .await
    .unwrap();
    db.record_auto_claim_key(
        "123",
        &json!({"status":"code_received","target_account":null}).to_string(),
    )
    .await
    .unwrap();
    let saved = db.auto_claim_values("key-receipt:").await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&saved[0]).unwrap();
    assert_eq!(value["target_account"], "selected");
    assert_eq!(value["status"], "redeeming");
    assert!(!saved[0].contains("ABC12-DEF34-GHI56"));
}

#[test]
fn epic_connection_opens_login_and_redirects_only_to_the_store() {
    let target="https://store.epicgames.com/en-US/p/paid-game";
    let url=reqwest::Url::parse(&platforms::connection_url("epic",target).unwrap()).unwrap();
    assert_eq!(url.host_str(),Some("www.epicgames.com"));assert_eq!(url.path(),"/id/login");
    assert_eq!(url.query_pairs().find(|(k,_)|k=="redirectUrl").unwrap().1,target);
    assert!(platforms::connection_url("epic","https://epicgames.com.evil.example/").is_err());
}
#[test]
fn manual_library_check_requires_every_active_item_in_the_exact_account_and_namespace() {
    use swch::auto_claim::{offers::Offer,providers::epic_owns_offer};
    let mut offer=Offer{launcher:"epic".into(),id:"gift".into(),namespace:"ns".into(),title:"Game".into(),url:"https://store.epicgames.com/p/game".into(),original_price:Some(1000),normally_paid:true,final_price:0,ends_at:Some(2000000000),item_ids:vec!["a".into(),"b".into()]};
    let item=|id:&str,account:&str,ns:&str,active:bool|serde_json::from_value(json!({"id":id,"entitlementName":"Game","namespace":ns,"catalogItemId":id,"accountId":account,"entitlementType":"EXECUTABLE","status":"ACTIVE","active":active})).unwrap();
    assert!(!epic_owns_offer(&[item("a","owner","ns",true)],"owner",&offer));
    assert!(!epic_owns_offer(&[item("a","owner","ns",true),item("b","other","ns",true)],"owner",&offer));
    assert!(!epic_owns_offer(&[item("a","owner","ns",true),item("b","owner","other",true)],"owner",&offer));
    assert!(!epic_owns_offer(&[item("a","owner","ns",true),item("b","owner","ns",false)],"owner",&offer));
    assert!(epic_owns_offer(&[item("a","owner","ns",true),item("b","owner","ns",true)],"owner",&offer));
    offer.item_ids.clear();assert!(!epic_owns_offer(&[],"owner",&offer));
}
