//! A single zero-price diagnostic order; prints neither tokens nor server bodies.
use serde_json::json;
use std::sync::Arc;
use swch::{
    auto_claim::providers::{Provider, Stores},
    launchers::epic::{EpicAuthStore, EpicError},
    storage::Database,
};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::open_default()?;
    let epic = Arc::new(EpicAuthStore::default_store(db.clone()));
    let stores = Stores::new(epic.clone());
    let Some(account) = db
        .get_accounts()
        .await?
        .into_iter()
        .find(|a| a.launcher == "epic")
    else {
        return Ok(());
    };
    let country = stores.country(&account).await?;
    let offers = stores.discover("epic", &country).await?;
    let mode = std::env::args().nth(1);
    if !matches!(
        mode.as_deref(),
        Some(
            "--claim-one"
                | "--checkout-page"
                | "--payment-page"
                | "--payment-preview"
                | "--web-api-login"
                | "--browser-login"
                | "--authenticated-browser"
        )
    ) {
        println!("{}", json!({"offers":offers.len()}));
        return Ok(());
    }
    let offer = offers
        .into_iter()
        .find(|o| o.eligible(chrono::Utc::now().timestamp()))
        .ok_or("No active zero-price giveaway")?;
    let token = epic.get_valid_access_token(&account.id).await?;
    if mode.as_deref() == Some("--authenticated-browser") {
        match epic.client().store_web_session(&token, &account.id).await {
            Ok((_, cookies)) => {
                println!(
                    "{}",
                    json!({"verified_web_session":true,"cookies":cookies.len()})
                );
                let session = swch::auto_claim::web::Session {
                    platform: "epic".into(),
                    account_id: account.id.clone(),
                    identity: account.id.clone(),
                    country: country.clone(),
                    cookies,
                    connected_at: chrono::Utc::now().to_rfc3339(),
                    identity_proof: "store_api".into(),
                    store_identity: None,
                    web_storage: vec![],
                };
                let mut browser = swch::auto_claim::web::Browser::start(false).await?;
                browser.restore(&session).await?;
                browser.navigate("epic", &offer.url).await?;
                tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                let data=browser.evaluate("(()=>({signed_in:document.querySelector('egs-navigation')?.getAttribute('isloggedin')==='true',purchase_label:document.querySelector('[data-testid=\"purchase-cta-button\"]')?.textContent?.trim(),challenge:!!document.querySelector('#challenge-running')||/verify you are human|checking your browser/i.test(document.body.innerText)}))()",None).await?;
                println!("{}", json!({"authenticated_browser":data}));
                let page=browser.evaluate("(()=>({title:document.title,path:location.origin+location.pathname,text:document.body.innerText.slice(0,280)}))()",None).await?;
                println!("{}", json!({"page":page}));
                browser.close().await;
            }
            Err(EpicError::Api { status, .. }) => println!(
                "{}",
                json!({"verified_web_session":false,"http":status.as_u16()})
            ),
            Err(_) => println!(
                "{}",
                json!({"verified_web_session":false,"error":"identity_not_verified"})
            ),
        }
        return Ok(());
    }
    if mode.as_deref() == Some("--browser-login") {
        use std::io::Write;
        let exchange = epic.client().get_exchange_code(&token).await?;
        let mut child = std::process::Command::new("node")
            .arg("output/epic-browser-login.cjs")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("No private input pipe")?
            .write_all(json!({"code":exchange.code}).to_string().as_bytes())?;
        let status = child.wait()?;
        if !status.success() {
            return Err("Browser probe failed".into());
        }
        return Ok(());
    }
    if mode.as_deref() == Some("--web-api-login") {
        let client = reqwest::Client::builder()
            .no_proxy()
            .cookie_store(true)
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(swch::launchers::epic::EPIC_USER_AGENT)
            .build()?;
        let reputation = client
            .get("https://www.epicgames.com/id/api/reputation")
            .send()
            .await?;
        let http = reputation.status().as_u16();
        let xsrf = reputation
            .cookies()
            .find(|c| c.name() == "XSRF-TOKEN")
            .map(|c| c.value().to_string());
        println!(
            "{}",
            json!({"api_reputation":{"http":http,"xsrf":xsrf.is_some()}})
        );
        if let Some(xsrf) = xsrf {
            let exchange = epic.client().get_exchange_code(&token).await?;
            let response = client
                .post("https://www.epicgames.com/id/api/exchange")
                .header("x-xsrf-token", xsrf)
                .json(&json!({"exchangeCode":exchange.code}))
                .send()
                .await?;
            let http = response.status().as_u16();
            println!("{}", json!({"api_exchange":{"http":http}}));
            if http == 200 {
                let response = client
                    .get("https://www.epicgames.com/id/api/redirect")
                    .send()
                    .await?;
                let http = response.status().as_u16();
                let data: serde_json::Value =
                    response.json().await.unwrap_or(serde_json::Value::Null);
                println!(
                    "{}",
                    json!({"api_redirect":{"http":http,"sid":data["sid"].as_str().is_some()}})
                );
                if let Some(sid) = data["sid"].as_str() {
                    for base in [
                        "https://www.epicgames.com",
                        "https://payment-website-pci.ol.epicgames.com",
                    ] {
                        let response = client
                            .get(format!("{base}/id/api/set-sid"))
                            .query(&[("sid", sid)])
                            .send()
                            .await?;
                        let cookie_scope = response
                            .cookies()
                            .map(|c| json!({"name":c.name(),"domain":c.domain(),"path":c.path()}))
                            .collect::<Vec<_>>();
                        println!(
                            "{}",
                            json!({"set_sid":{"payment_domain":base=="https://payment-website-pci.ol.epicgames.com","http":response.status().as_u16(),"cookie_scope":cookie_scope}})
                        );
                    }
                }
                let response = client
                    .get("https://payment-website-pci.ol.epicgames.com/purchase")
                    .query(&[
                        ("showNavigation", "true"),
                        ("namespace", offer.namespace.as_str()),
                        ("offers", offer.id.as_str()),
                    ])
                    .bearer_auth(&token)
                    .send()
                    .await?;
                let http = response.status().as_u16();
                let html = response.text().await?;
                let purchase_token = html.split("<input").find_map(|tag| {
                    let tag = tag.split('>').next().unwrap_or_default();
                    (swch::auto_claim::offers::attr(tag, "id") == Some("purchaseToken"))
                        .then(|| swch::auto_claim::offers::attr(tag, "value"))
                        .flatten()
                });
                println!(
                    "{}",
                    json!({"authenticated_checkout":{"http":http,"token":purchase_token.is_some()}})
                );
                if let Some(purchase_token) = purchase_token {
                    let response = client
                        .post("https://payment-website-pci.ol.epicgames.com/purchase/order-preview")
                        .bearer_auth(&token)
                        .header("x-requested-with", purchase_token)
                        .form(&[
                            ("useDefault", "false"),
                            ("setDefault", "false"),
                            ("namespace", offer.namespace.as_str()),
                            ("country", country.as_str()),
                            ("offers[0]", offer.id.as_str()),
                            ("offerPrice", ""),
                        ])
                        .send()
                        .await?;
                    let http = response.status().as_u16();
                    let body: serde_json::Value =
                        response.json().await.unwrap_or(serde_json::Value::Null);
                    println!(
                        "{}",
                        json!({"authenticated_preview":{"http":http,"json":!body.is_null(),"zero":body["orderResponse"]["totalPrice"].as_f64()==Some(0.0)||body["orderResponse"]["totalPrice"].as_str()==Some("0"),"namespace_matches":body["namespace"].as_str()==Some(offer.namespace.as_str()),"offer_matches":body["offers"].as_array().is_some_and(|a|a.len()==1&&a[0].as_str()==Some(offer.id.as_str())),"country_matches":body["country"].as_str()==Some(country.as_str()),"sync_token":body["syncToken"].as_str().is_some()}})
                    );
                }
            }
        }
        return Ok(());
    }
    if matches!(
        mode.as_deref(),
        Some("--checkout-page" | "--payment-page" | "--payment-preview")
    ) {
        let url = if matches!(
            mode.as_deref(),
            Some("--payment-page" | "--payment-preview")
        ) {
            "https://payment-website-pci.ol.epicgames.com/purchase"
        } else {
            "https://launcher-website-prod07.ol.epicgames.com/purchase"
        };
        let client = reqwest::Client::builder()
            .no_proxy()
            .cookie_store(true)
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        let response = client
            .get(url)
            .query(&[
                ("showNavigation", "true"),
                ("namespace", offer.namespace.as_str()),
                ("offers", offer.id.as_str()),
            ])
            .bearer_auth(&token)
            .header(
                reqwest::header::USER_AGENT,
                swch::launchers::epic::EPIC_USER_AGENT,
            )
            .send()
            .await?;
        let http = response.status().as_u16();
        let html = response.text().await?;
        let lower = html.to_ascii_lowercase();
        println!(
            "{}",
            json!({"checkout_page":{"http":http,"purchase_token":html.contains("purchaseToken"),"challenge":lower.contains("challenge-platform")||lower.contains("cf-chl-"),"captcha":lower.contains("captcha"),"json":serde_json::from_str::<serde_json::Value>(&html).is_ok()}})
        );
        if mode.as_deref() == Some("--payment-preview") {
            let purchase_token = html.split("<input").find_map(|tag| {
                let tag = tag.split('>').next().unwrap_or_default();
                (swch::auto_claim::offers::attr(tag, "id") == Some("purchaseToken"))
                    .then(|| swch::auto_claim::offers::attr(tag, "value"))
                    .flatten()
            });
            println!(
                "{}",
                json!({"hidden_purchase_token":purchase_token.is_some()})
            );
            if let Some(purchase_token) = purchase_token {
                let response = client
                    .post("https://payment-website-pci.ol.epicgames.com/purchase/order-preview")
                    .bearer_auth(&token)
                    .header(
                        reqwest::header::USER_AGENT,
                        swch::launchers::epic::EPIC_USER_AGENT,
                    )
                    .header("x-requested-with", purchase_token)
                    .form(&[
                        ("useDefault", "false"),
                        ("setDefault", "false"),
                        ("namespace", offer.namespace.as_str()),
                        ("country", country.as_str()),
                        ("offers[0]", offer.id.as_str()),
                        ("offerPrice", ""),
                    ])
                    .send()
                    .await?;
                let http = response.status().as_u16();
                let body: serde_json::Value =
                    response.json().await.unwrap_or(serde_json::Value::Null);
                println!(
                    "{}",
                    json!({"preview":{"http":http,"json":!body.is_null(),"zero":body["orderResponse"]["totalPrice"].as_f64()==Some(0.0)||body["orderResponse"]["totalPrice"].as_str()==Some("0"),"namespace_matches":body["namespace"].as_str()==Some(offer.namespace.as_str()),"offer_matches":body["offers"].as_array().is_some_and(|a|a.len()==1&&a[0].as_str()==Some(offer.id.as_str())),"country_matches":body["country"].as_str()==Some(country.as_str()),"sync_token":body["syncToken"].as_str().is_some()}})
                );
            }
        }
        return Ok(());
    }
    let owns = |items: &[swch::launchers::epic::Entitlement]| {
        offer.item_ids.iter().all(|id| {
            items.iter().any(|e| {
                e.active
                    && e.account_id == account.id
                    && e.namespace == offer.namespace
                    && e.catalog_item_id == *id
            })
        })
    };
    let before = epic
        .client()
        .fetch_all_entitlements(&token, &account.id)
        .await?;
    if owns(&before) {
        println!("{}", json!({"already_owned":true}));
        return Ok(());
    }
    match epic
        .client()
        .claim_free_offer(&token, &account.id, &offer.namespace, &offer.id, &country)
        .await
    {
        Ok(()) => {
            let after = epic
                .client()
                .fetch_all_entitlements(&token, &account.id)
                .await?;
            let confirmed = owns(&after);
            if confirmed {
                db.record_claim_once(&swch::storage::ClaimHistory {
                    id: None,
                    offer_id: Some(offer.key()),
                    launcher: "epic".into(),
                    game_title: offer.title.clone(),
                    account_id: Some(account.id.clone()),
                    claimed_at: chrono::Utc::now().to_rfc3339(),
                })
                .await?;
                let _ = epic.sync_library(&account.id, None).await;
            }
            println!("{}", json!({"confirmed":confirmed}));
        }
        Err(EpicError::Api {
            status,
            code,
            numeric,
            ..
        }) => println!(
            "{}",
            json!({"error":"api","http":status.as_u16(),"code":code.filter(|c|c.len()<256&&c.bytes().all(|b|b.is_ascii_alphanumeric()||b"._:-".contains(&b))),"numeric":numeric})
        ),
        Err(EpicError::Auth(message)) => {
            let reason = match message.as_str() {
                "Epic rejected this free order" => "quick_purchase_rejected",
                "Epic checkout unavailable" => "checkout_unavailable",
                "Epic checkout needs browser verification" => "checkout_verification_required",
                "Epic checkout is not a zero-price order for this offer" => {
                    "preview_identity_or_zero_total_not_confirmed"
                }
                "Epic checkout preview incomplete" => "preview_incomplete",
                _ => "authentication",
            };
            println!("{}", json!({"error":"auth","reason":reason}));
        }
        Err(EpicError::Reqwest(e)) => println!(
            "{}",
            json!({"error":"network","timeout":e.is_timeout(),"connection":e.is_connect(),"http":e.status().map(|s|s.as_u16())})
        ),
        Err(_) => println!("{}", json!({"error":"unexpected_response"})),
    }
    Ok(())
}
