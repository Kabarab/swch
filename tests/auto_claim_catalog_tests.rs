use serde_json::{json, Value};
use swch::auto_claim::catalog::{gamerpower_candidates, trusted_claim_url};

const NOW: i64 = 1_791_244_800; // 2026-10-06 00:00:00 UTC

fn giveaway(id: u64) -> Value {
    json!({
        "id": id,
        "title": "A paid game (Epic Games) Giveaway",
        "worth": "$29.99",
        "type": "Game",
        "platforms": "PC, Epic Games Store",
        "status": "Active",
        "published_date": "2026-10-01 11:00:00",
        "end_date": "2026-10-08 23:59:00",
        "gamerpower_url": "https://www.gamerpower.com/a-paid-game-giveaway",
        "open_giveaway_url": "https://www.gamerpower.com/open/a-paid-game-giveaway",
        "description": "Add this normally paid full game to your library before the promotion ends.",
        "instructions": "Visit the official store and verify its price before claiming."
    })
}

fn parse(entry: Value) -> Vec<swch::auto_claim::catalog::GiveawayCandidate> {
    gamerpower_candidates(&json!([entry]), NOW).unwrap()
}

#[test]
fn actual_redirect_feed_shape_remains_untrusted_discovery() {
    let result = parse(giveaway(3797));
    assert_eq!(result.len(), 1);
    let item = &result[0];
    assert_eq!(item.source_id, "3797");
    assert_eq!(item.platform, "epic");
    assert_eq!(item.worth, 2999);
    assert!(item.ends_at > NOW);
    assert_eq!(item.claim_url, None);
    assert_eq!(
        item.source_url,
        "https://www.gamerpower.com/open/a-paid-game-giveaway"
    );
    assert!(item.needs_store_validation);
}

#[test]
fn direct_store_link_is_retained_without_authorizing_checkout() {
    let mut entry = giveaway(1);
    entry["claim_link"] = json!("https://store.epicgames.com/en-US/p/paid-game");
    let result = parse(entry);
    assert_eq!(
        result[0].claim_url.as_deref(),
        Some("https://store.epicgames.com/en-US/p/paid-game")
    );
    assert!(result[0].needs_store_validation);
}

#[test]
fn dedup_uses_canonical_source_id_and_store_not_title() {
    let first = giveaway(7);
    let mut repeated = giveaway(7);
    repeated["id"] = json!("007");
    repeated["title"] = json!("Different edited title");
    let mut same_title_new_id = giveaway(8);
    same_title_new_id["title"] = first["title"].clone();
    let mut another_store_same_id = giveaway(7);
    another_store_same_id["platforms"] = json!("PC, GOG");
    let result = gamerpower_candidates(
        &json!([first, repeated, same_title_new_id, another_store_same_id]),
        NOW,
    )
    .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(
        result
            .iter()
            .map(|item| (&*item.platform, &*item.source_id))
            .collect::<Vec<_>>(),
        vec![("epic", "7"), ("epic", "8"), ("gog", "7")]
    );
}

#[test]
fn no_unknown_missing_zero_or_negative_value_can_be_a_candidate() {
    for worth in [
        Value::Null,
        json!("N/A"),
        json!("Free"),
        json!("$0.00"),
        json!("$-3.00"),
        json!("-$3.00"),
        json!(29.99),
        json!("$NaN"),
        json!("$1.234"),
        json!("$9223372036854775807"),
    ] {
        let mut entry = giveaway(1);
        entry["worth"] = worth.clone();
        assert!(parse(entry).is_empty(), "accepted estimated value {worth}");
    }
    let mut entry = giveaway(1);
    entry.as_object_mut().unwrap().remove("worth");
    assert!(parse(entry).is_empty());
}

#[test]
fn finite_active_interval_is_required_even_if_feed_says_active() {
    for end in [
        Value::Null,
        json!("N/A"),
        json!("2026-99-99 00:00:00"),
        json!("2026-10-06T00:00:00Z"),
        json!("2026-09-30 23:59:00"),
    ] {
        let mut entry = giveaway(1);
        entry["end_date"] = end;
        assert!(parse(entry).is_empty());
    }
    for start in [
        json!("2026-10-07T00:00:00Z"),
        json!("not a date"),
        Value::Null,
    ] {
        let mut entry = giveaway(1);
        entry["start_date"] = start;
        assert!(parse(entry).is_empty());
    }
    let mut entry = giveaway(1);
    entry["published_date"] = json!("2027-01-01 00:00:00");
    assert!(parse(entry).is_empty());
    let mut entry = giveaway(1);
    entry["status"] = json!("Expired");
    assert!(parse(entry).is_empty());
}

#[test]
fn timezone_aware_dates_use_their_offset_and_respect_start_boundary() {
    let mut entry = giveaway(1);
    entry["start_date"] = json!("2026-10-06T03:00:00+03:00");
    entry["end_date"] = json!("2026-10-06T03:00:01+03:00");
    let result = parse(entry);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].ends_at, NOW + 1);
}

#[test]
fn full_game_type_excludes_loot_dlc_beta_and_unproven_type() {
    for kind in [
        json!("Loot"),
        json!("DLC"),
        json!("Beta"),
        json!("Game, DLC"),
        json!("game-key"),
        Value::Null,
    ] {
        let mut entry = giveaway(1);
        entry["type"] = kind;
        assert!(parse(entry).is_empty());
    }
}

#[test]
fn mislabeled_games_do_not_admit_trials_early_access_or_permanent_f2p() {
    for description in [
        "This is a free-to-play game",
        "Free weekend until Thursday",
        "Permanently free game",
        "Free forever",
        "Always free",
        "Get the paid DLC",
        "An early-access giveaway",
        "Demo available",
        "Beta access",
        "Playtest invitation",
        "Trial edition",
        "F2P launch reward",
    ] {
        let mut entry = giveaway(1);
        entry["description"] = json!(description);
        assert!(parse(entry).is_empty(), "accepted {description}");
    }
    for marker in ["is_free_to_play", "is_f2p", "is_permanent"] {
        let mut entry = giveaway(1);
        entry[marker] = json!(true);
        assert!(parse(entry).is_empty());
    }
    let mut entry = giveaway(1);
    entry["title"] = json!("Democracy: complete paid game");
    assert_eq!(
        parse(entry).len(),
        1,
        "word guards must not reject Democracy as a demo"
    );
}

#[test]
fn all_supported_stores_map_from_exact_labels() {
    for (label, expected) in [
        ("Steam", "steam"),
        ("Epic Games Store", "epic"),
        ("GOG", "gog"),
        ("Amazon Prime Gaming", "prime"),
        ("Microsoft Store", "microsoft"),
        ("Ubisoft Connect", "ubisoft"),
        ("Fanatical", "fanatical"),
        ("Itch.io", "itchio"),
        ("IndieGala", "indiegala"),
        ("Alienware Arena", "alienware"),
    ] {
        let mut entry = giveaway(1);
        entry["platforms"] = json!(format!("PC, {label}"));
        let result = parse(entry);
        assert_eq!(result.len(), 1, "missing {label}");
        assert_eq!(result[0].platform, expected);
        assert!(result[0].needs_store_validation);
    }
}

#[test]
fn ambiguous_platforms_require_a_compatible_direct_store_host() {
    let mut entry = giveaway(1);
    entry["platforms"] = json!("PC, Steam, GOG");
    assert!(parse(entry.clone()).is_empty());
    entry["claim_link"] = json!("https://www.gog.com/en/game/paid_game");
    assert_eq!(parse(entry.clone())[0].platform, "gog");
    entry["claim_link"] = json!("https://store.epicgames.com/en-US/p/paid-game");
    assert!(parse(entry).is_empty());

    let mut generic = giveaway(2);
    generic["platforms"] = json!("PC, DRM-Free");
    generic["claim_link"] = json!("https://publisher.itch.io/paid-game");
    assert_eq!(parse(generic)[0].platform, "itchio");
}

#[test]
fn unsupported_platform_and_deceptive_label_are_not_routed_by_title() {
    for platform in [
        "PC",
        "Playstation 5",
        "Android, iOS",
        "VR",
        "Steam malicious",
        "Not GOG",
        "PC, Steam Deck",
    ] {
        let mut entry = giveaway(1);
        entry["platforms"] = json!(platform);
        entry["title"] = json!("A Steam GOG Epic Prime giveaway");
        assert!(parse(entry).is_empty(), "guessed platform from {platform}");
    }
}

#[test]
fn untrusted_url_variants_never_pass_the_store_allowlist() {
    let hosts = [
        "store.steampowered.com",
        "store.epicgames.com",
        "www.gog.com",
        "gaming.amazon.com",
        "apps.microsoft.com",
        "store.ubisoft.com",
        "www.fanatical.com",
        "publisher.itch.io",
        "www.indiegala.com",
        "eu.alienwarearena.com",
    ];
    for host in hosts {
        assert!(trusted_claim_url(&format!("https://{host}/game")).is_some());
        for url in [
            format!("http://{host}/game"),
            format!("https://{host}.evil.test/game"),
            format!("https://{host}@evil.test/game"),
            format!("https://evil.test@{host}/game"),
            format!("https://@{host}/game"),
            format!("https://{host}:443/game"),
            format!("https://{host}:8080/game"),
            format!("https://{host}./game"),
            format!("https://{host}/game#secret"),
            format!(" https://{host}/game"),
            format!("https://{host}\\@evil.test/game"),
        ] {
            assert!(trusted_claim_url(&url).is_none(), "accepted {url}");
            let mut entry = giveaway(1);
            entry["claim_link"] = json!(url);
            assert!(
                parse(entry).is_empty(),
                "retained untrusted direct claim link"
            );
        }
    }
    for url in [
        "https://localhost/game",
        "https://127.0.0.1/game",
        "https://[::1]/game",
        "https://g%6Fg.com/game",
        "https://fakeitch.io/game",
        "file:///game",
        "javascript:alert(1)",
    ] {
        assert!(trusted_claim_url(url).is_none(), "accepted {url}");
    }
}

#[test]
fn gamerpower_redirects_can_only_be_sources_and_cannot_spoof_the_source_host() {
    assert!(trusted_claim_url("https://www.gamerpower.com/open/paid-game").is_none());
    for source in [
        "https://www.gamerpower.com.evil.test/open/game",
        "https://www.gamerpower.com:443/open/game",
        "https://user@www.gamerpower.com/open/game",
        "http://www.gamerpower.com/open/game",
    ] {
        let mut entry = giveaway(1);
        entry["open_giveaway_url"] = json!(source);
        assert!(parse(entry).is_empty());
    }
}

#[test]
fn invalid_feed_shape_and_identity_do_not_silently_create_candidates() {
    assert!(gamerpower_candidates(&json!({"unexpected":"object"}), NOW).is_err());
    assert!(gamerpower_candidates(&json!([]), NOW).unwrap().is_empty());
    for id in [
        Value::Null,
        json!(0),
        json!(-1),
        json!("../1"),
        json!("1.2"),
        json!("18446744073709551616"),
    ] {
        let mut entry = giveaway(1);
        entry["id"] = id;
        assert!(parse(entry).is_empty());
    }
}
