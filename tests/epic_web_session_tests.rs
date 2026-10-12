use swch::auto_claim::{providers::epic_web_session_restorable, web::{Cookie, Session}};

fn session(domain: &str) -> Session {
    Session {
        platform: "epic".into(), account_id: "account".into(), identity: "account".into(),
        country: "US".into(), connected_at: String::new(), identity_proof: "store_api".into(),
        store_identity: None, web_storage: vec![],
        cookies: ["EPIC_SSO", "EPIC_BEARER_TOKEN"].into_iter().map(|name| Cookie {
            name: name.into(), value: "fixture".into(), domain: domain.into(), path: "/".into(),
            secure: true, http_only: true, expires: Some(5000.), same_site: Some("None".into()),
        }).collect(),
    }
}

#[test]
fn legacy_host_only_bearer_proof_requires_a_new_sso_bootstrap() {
    assert!(!epic_web_session_restorable(&session("epicgames.com"), 1000));
    assert!(!epic_web_session_restorable(&session("www.epicgames.com"), 1000));
    assert!(epic_web_session_restorable(&session(".epicgames.com"), 1000));
}

#[test]
fn expired_missing_or_wrong_owner_sso_is_not_reused() {
    let mut s = session(".epicgames.com");
    s.cookies[0].expires = Some(900.);
    assert!(!epic_web_session_restorable(&s, 1000));
    s.cookies[0].expires = Some(5000.);
    s.identity = "other".into();
    assert!(!epic_web_session_restorable(&s, 1000));
    s.identity = s.account_id.clone();
    s.cookies.retain(|c| c.name != "EPIC_SSO");
    assert!(!epic_web_session_restorable(&s, 1000));
}

#[test]
fn chromium_session_cookie_sentinel_is_distinct_from_deleted_epoch_cookie() {
    let mut s = session(".epicgames.com");
    s.cookies[0].expires = Some(-1.);
    assert!(epic_web_session_restorable(&s, 1000));
    s.cookies[0].expires = Some(0.);
    assert!(!epic_web_session_restorable(&s, 1000));
}
