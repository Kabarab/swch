use swch::auto_claim::{
    native_epic::{decide, product_uri, Decision, Rect, Snapshot, Word},
    offers::Offer,
};
fn offer() -> Offer {
    Offer {
        launcher: "epic".into(),
        id: "offer".into(),
        namespace: "game-ns".into(),
        title: "TerraScape".into(),
        url: "https://store.epicgames.com/en-US/p/terrascape-123".into(),
        normally_paid: true,
        original_price: Some(1500),
        final_price: 0,
        ends_at: Some(chrono::Utc::now().timestamp() + 10000),
        item_ids: vec!["item".into()],
    }
}
fn snapshot(lines: &[(&str, f64, f64)]) -> Snapshot {
    Snapshot {
        ok: true,
        version: 1,
        hwnd: "123".into(),
        pid: 44,
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 1400.0,
            height: 900.0,
        },
        width: 1400.0,
        height: 900.0,
        image_hash: "a".repeat(64),
        last_input_tick: 100,
        words: lines
            .iter()
            .enumerate()
            .flat_map(|(line, (text, x, y))| {
                text.split_whitespace().enumerate().map(move |(i, t)| Word {
                    text: t.into(),
                    x: *x + i as f64 * 70.0,
                    y: *y,
                    width: 65.0,
                    height: 22.0,
                    line: line as u32,
                })
            })
            .collect(),
    }
}
fn product() -> Snapshot {
    snapshot(&[
        ("TerraScape", 50.0, 80.0),
        ("Base Game", 950.0, 430.0),
        ("Free", 1050.0, 480.0),
        ("-100%", 950.0, 480.0),
        ("Get", 1050.0, 540.0),
    ])
}
#[test]
fn get_requires_exact_full_game_and_free_price_by_the_button() {
    assert!(matches!(decide(&product(),&offer(),false),Decision::Click(b) if b.text=="Get"));
    let paid = snapshot(&[
        ("TerraScape", 50.0, 80.0),
        ("Base Game", 950.0, 430.0),
        ("$5.00", 1050.0, 480.0),
        ("Get", 1050.0, 540.0),
        ("Free", 10.0, 800.0),
    ]);
    assert!(matches!(
        decide(&paid, &offer(), false),
        Decision::Attention(_)
    ));
    let wrong = snapshot(&[
        ("TerraScape Deluxe", 50.0, 80.0),
        ("Base Game", 950.0, 430.0),
        ("Free", 1050.0, 480.0),
        ("-100%", 950.0, 480.0),
        ("Get", 1050.0, 540.0),
    ]);
    assert!(matches!(decide(&wrong, &offer(), false), Decision::Wait));
    let dlc = snapshot(&[
        ("TerraScape", 50.0, 80.0),
        ("Add On", 950.0, 430.0),
        ("Free", 1050.0, 480.0),
        ("-100%", 950.0, 480.0),
        ("Get", 1050.0, 540.0),
    ]);
    assert!(matches!(
        decide(&dlc, &offer(), false),
        Decision::Attention(_)
    ));
}
#[test]
fn confirmation_requires_prior_validated_get_and_explicit_zero() {
    let add = snapshot(&[
        ("TerraScape", 50.0, 80.0),
        ("This is free. Add it to your library", 200.0, 300.0),
        ("Add to library", 450.0, 450.0),
    ]);
    assert!(matches!(
        decide(&add, &offer(), false),
        Decision::Attention(_)
    ));
    assert!(matches!(decide(&add,&offer(),true),Decision::Click(b) if b.text=="Add to library"));
    let unknown = snapshot(&[("TerraScape", 50.0, 80.0), ("Add to library", 450.0, 450.0)]);
    assert!(matches!(
        decide(&unknown, &offer(), true),
        Decision::Attention(_)
    ));
    for total in ["Total £2.00", "Total unknown", "Total -0.00"] {
        let s = snapshot(&[
            ("TerraScape", 50.0, 80.0),
            ("1 item", 400.0, 180.0),
            (total, 800.0, 500.0),
            ("Place order", 450.0, 600.0),
        ]);
        assert!(matches!(decide(&s, &offer(), true), Decision::Attention(_)));
    }
    let zero = snapshot(&[
        ("TerraScape", 50.0, 80.0),
        ("1 item", 400.0, 180.0),
        ("Total £0.00", 800.0, 500.0),
        ("Place order", 450.0, 600.0),
    ]);
    assert!(matches!(
        decide(&zero, &offer(), true),
        Decision::Attention(_)
    ));
    for count in ["11 items", "21 items", "1 item 1 item"] {
        let s = snapshot(&[
            ("TerraScape", 50.0, 80.0),
            (count, 400.0, 180.0),
            ("Total £0.00", 800.0, 500.0),
            ("Place order", 450.0, 600.0),
        ]);
        assert!(matches!(decide(&s, &offer(), true), Decision::Attention(_)));
    }
    let many = snapshot(&[
        ("TerraScape", 50.0, 80.0),
        ("2 items", 400.0, 180.0),
        ("Total £0.00", 800.0, 500.0),
        ("Place order", 450.0, 600.0),
    ]);
    assert!(matches!(
        decide(&many, &offer(), true),
        Decision::Attention(_)
    ));
}
#[test]
fn captcha_login_duplicates_and_invalid_geometry_cannot_click() {
    for notice in [
        "One more step",
        "Verify you are human",
        "Enter your age",
        "Parental PIN",
        "Email address",
    ] {
        let mut s = product();
        s.words.extend(
            snapshot(&[(notice, 50.0, 350.0)])
                .words
                .into_iter()
                .map(|mut w| {
                    w.line = 10;
                    w
                }),
        );
        assert!(matches!(
            decide(&s, &offer(), false),
            Decision::Attention(_)
        ));
    }
    let mut duplicate = product();
    duplicate.words.push(Word {
        text: "Get".into(),
        x: 1050.0,
        y: 700.0,
        width: 65.0,
        height: 22.0,
        line: 10,
    });
    assert!(matches!(
        decide(&duplicate, &offer(), false),
        Decision::Attention(_)
    ));
    let mut invalid = product();
    invalid.words[0].width = f64::NAN;
    assert!(matches!(
        decide(&invalid, &offer(), false),
        Decision::Attention(_)
    ));
    let mut expired = offer();
    expired.ends_at = Some(1);
    assert!(matches!(
        decide(&product(), &expired, false),
        Decision::Attention(_)
    ));
    let mut permanent = offer();
    permanent.ends_at = None;
    assert!(matches!(
        decide(&product(), &permanent, false),
        Decision::Attention(_)
    ));
    assert!(matches!(decide(&product(), &offer(), true), Decision::Wait));
    let mut no_discount = product();
    no_discount.words.retain(|w| w.text != "-100%");
    assert!(matches!(
        decide(&no_discount, &offer(), false),
        Decision::Attention(_)
    ));
}
#[test]
fn native_product_links_never_launch_games_or_accept_external_or_injected_paths() {
    assert_eq!(
        product_uri(&offer().url).unwrap(),
        "com.epicgames.launcher://store/product/terrascape-123"
    );
    for url in [
        "https://evil.example/p/game",
        "https://store.epicgames.com.evil.example/p/game",
        "https://store.epicgames.com/p/game%2fapps",
        "https://store.epicgames.com/p/game/other",
        "https://store.epicgames.com/free-games",
    ] {
        assert!(product_uri(url).is_err());
    }
}
