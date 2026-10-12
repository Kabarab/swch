//! Redemption binds one code to one account and verifies the exact game.
use super::{
    browser_claim,
    catalog::canonical_game_title,
    platforms, store_discovery,
    web::{Browser, Session},
};
use crate::storage::Account;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct RedeemState {
    pub url: String,
    pub email: Option<String>,
    pub product_title: String,
    pub product_id: Option<String>,
    pub code_field: bool,
    pub next: bool,
    pub confirm: bool,
    pub done: bool,
    pub refused: bool,
    pub challenge: bool,
    pub sensitive: bool,
}
pub fn valid_code(code: &str) -> bool {
    let parts = code.split('-').collect::<Vec<_>>();
    parts.len() == 5
        && parts
            .iter()
            .all(|p| p.len() == 5 && p.bytes().all(|c| c.is_ascii_alphanumeric()))
}
pub fn preview_matches(s: &RedeemState, identity: &str, title: &str) -> bool {
    !s.challenge
        && !s.sensitive
        && !s.refused
        && s.confirm
        && s.email
            .as_ref()
            .is_none_or(|e| e.eq_ignore_ascii_case(identity))
        && canonical_game_title(&s.product_title) == canonical_game_title(title)
        && s.product_id
            .as_ref()
            .is_some_and(|id| id.len() == 12 && id.bytes().all(|c| c.is_ascii_alphanumeric()))
        && platforms::trusted_url("microsoft", &s.url)
}
pub async fn redeem_key(
    account: &Account,
    title: &str,
    code: &str,
    timeout: u16,
    visible: bool,
    browser_name: &str,
) -> Result<(), String> {
    if account.launcher != "microsoft" || !valid_code(code) {
        return Err("Для Microsoft нужен код из пяти групп по пять символов.".into());
    }
    let a = account.clone();
    let session = tokio::task::spawn_blocking(move || Session::load("microsoft", &a.id))
        .await
        .map_err(|_| "Не удалось прочитать вход Microsoft.")??;
    let mut browser = Browser::for_account(account, visible, browser_name).await?;
    let result = redeem(
        &mut browser,
        account,
        title,
        code,
        timeout,
        visible,
        &session,
    )
    .await;
    browser.close().await;
    result
}
async fn redeem(
    browser: &mut Browser,
    account: &Account,
    title: &str,
    code: &str,
    timeout: u16,
    visible: bool,
    session: &Session,
) -> Result<(), String> {
    browser.restore(session).await?;
    browser
        .navigate(
            "microsoft",
            "https://account.microsoft.com/billing/redeem?lang=en-US",
        )
        .await?;
    let start = Instant::now();
    let mut entered = false;
    let mut next = false;
    let mut confirmed = false;
    let mut product = None;
    while start.elapsed() < Duration::from_secs(timeout as u64) {
        let parent = browser_claim::snapshot(browser, account, "", session).await?;
        if parent.challenge || parent.age_gate || parent.requires_confirmation {
            if visible {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            return Err("Microsoft требует подтверждения пользователя в окне магазина.".into());
        }
        if !parent.signed_in || parent.identity.as_deref() != Some(&session.identity) {
            if start.elapsed() < Duration::from_secs(12) {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            return Err("Microsoft не подтвердил выбранный аккаунт. Подключи его магазин.".into());
        }
        let mut frames = browser
            .frame_values("microsoft", REDEEM_STATE_SCRIPT)
            .await?;
        for (sid, context, value) in frames.drain(..) {
            let Ok(s) = serde_json::from_value::<RedeemState>(value) else {
                continue;
            };
            if !platforms::trusted_url("microsoft", &s.url) {
                continue;
            }
            if s.email
                .as_ref()
                .is_some_and(|e| !e.eq_ignore_ascii_case(&session.identity))
            {
                return Err("Форма Microsoft вошла в другой аккаунт; код не подтверждён.".into());
            }
            if s.refused {
                return Err("Microsoft отклонил код или сообщил, что он уже использован. Проверь библиотеку выбранного аккаунта.".into());
            }
            if s.challenge || s.sensitive {
                if visible {
                    continue;
                }
                return Err("Microsoft требует проверки пользователя.".into());
            }
            if s.code_field && !entered {
                let code = json!(code).to_string();
                let script=format!("(()=>{{const e=document.querySelector('input[name=\"tokenString\"]');if(!e||e.disabled)return false;const set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')?.set;if(!set)return false;set.call(e,{code});e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));return true;}})()");
                entered = browser
                    .evaluate_context(&script, &sid, context)
                    .await?
                    .as_bool()
                    == Some(true);
            } else if entered && s.next && !next {
                next = browser
                    .evaluate_context(NEXT_SCRIPT, &sid, context)
                    .await?
                    .as_bool()
                    == Some(true);
            } else if entered && next && !confirmed && preview_matches(&s, &session.identity, title)
            {
                let id = s.product_id.as_ref().unwrap();
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|_| "Не удалось проверить игру Microsoft.")?;
                let r = client
                    .get(format!(
                        "https://storeedgefd.dsx.mp.microsoft.com/v9.0/products/{id}"
                    ))
                    .query(&[
                        ("market", session.country.as_str()),
                        ("locale", "en-us"),
                        ("deviceFamily", "Windows.Xbox"),
                    ])
                    .send()
                    .await
                    .map_err(|_| "Microsoft не подтвердил игру для ключа.")?;
                if !r.status().is_success() {
                    return Err(
                        "Microsoft не подтвердил метаданные игры; активация остановлена.".into(),
                    );
                }
                let data: Value = r.json().await.map_err(|_| "Неизвестная игра Microsoft.")?;
                let game = store_discovery::microsoft_product(&data, id, title)?;
                if !matches!(
                    data.pointer("/Payload/ProductType").and_then(Value::as_str),
                    Some("Game" | "game" | "Games")
                ) {
                    return Err("Microsoft не подтвердил, что ключ выдаёт полную игру.".into());
                }
                product = Some(format!(
                    "https://www.xbox.com/en-US/games/store/game/{}",
                    game.id
                ));
                let fresh_parent = browser_claim::snapshot(browser, account, "", session).await?;
                if !fresh_parent.signed_in
                    || fresh_parent.identity.as_deref() != Some(&session.identity)
                    || fresh_parent.challenge
                    || fresh_parent.age_gate
                    || fresh_parent.requires_confirmation
                {
                    return Err(
                        "Аккаунт Microsoft изменился перед подтверждением; активация остановлена."
                            .into(),
                    );
                }
                let fresh: RedeemState = serde_json::from_value(
                    browser
                        .evaluate_context(REDEEM_STATE_SCRIPT, &sid, context)
                        .await?,
                )
                .map_err(|_| "Microsoft изменил страницу подтверждения.")?;
                if !preview_matches(&fresh, &session.identity, title)
                    || fresh.product_id != s.product_id
                {
                    return Err(
                        "Microsoft изменил товар перед подтверждением; активация остановлена."
                            .into(),
                    );
                }
                confirmed = browser
                    .evaluate_context(CONFIRM_SCRIPT, &sid, context)
                    .await?
                    .as_bool()
                    == Some(true);
            } else if confirmed && s.done {
                browser
                    .navigate(
                        "microsoft",
                        product
                            .as_ref()
                            .ok_or("Не удалось определить активированную игру.")?,
                    )
                    .await?;
                for _ in 0..8 {
                    let owned = browser_claim::snapshot(browser, account, title, session).await?;
                    if owned.signed_in
                        && owned.identity.as_deref() == Some(&session.identity)
                        && owned.checkout_offer_matches
                        && owned.in_library
                    {
                        return Ok(());
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                return Err("Microsoft принял подтверждение, но владение этой игрой ещё не подтверждено. Проверь выбранный аккаунт.".into());
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("Microsoft не подтвердил получение ожидаемой игры. Повтор возможен только для того же аккаунта.".into())
}
pub const REDEEM_STATE_SCRIPT: &str = r#"(()=>{
 const text=(document.body?.innerText||'').toLowerCase();const visible=e=>!!e&&e.getBoundingClientRect().width>0&&e.getBoundingClientRect().height>0;
 const buttons=[...document.querySelectorAll('button,input[type="submit"]')].filter(e=>visible(e)&&!e.disabled);
 const has=name=>buttons.some(e=>(e.textContent||e.value||'').trim().toLowerCase()===name);
 const field=document.querySelector('input[name="tokenString"]');const item=document.querySelector('[data-testid="redemption-product"],[data-product-id],.redeem-product,.product-details');
 const heading=item?.querySelector('[data-testid="product-title"],.product-title,h1,h2')||document.querySelector('[data-testid="product-title"],.redeem-product-title');
 const link=item?.querySelector('a[href*="/store/"]')||document.querySelector('a[data-product-link]');
 const pid=item?.getAttribute('data-product-id')||link?.href?.match(/\/([a-z0-9]{12})(?:[/?#]|$)/i)?.[1]||null;
 return {url:location.origin+location.pathname,email:document.querySelector('[data-testid="account-email"],#mectrl_currentAccount_secondary,.redeem-account-email')?.textContent?.trim()||null,
 product_title:heading?.textContent?.trim()||'',product_id:pid,code_field:visible(field),next:has('next'),confirm:has('confirm')||has('redeem'),
 done:/added to your account|all set|ready for you|good to go/.test(text),refused:/already (been )?(used|redeemed)|invalid code|code isn't valid|code is not valid|code has been redeemed|couldn't redeem|expired code/.test(text),
 challenge:/verify you are human|checking your browser|security check/.test(text)||/just a moment/i.test(document.title),
 sensitive:!!document.querySelector('input[type="password"],input[autocomplete="one-time-code"],input[name="dateOfBirth"]')||/payment pin|parental control|verify your age/.test(text)||/gift card|account balance|game pass|subscription/.test((heading?.textContent||'').toLowerCase())};
})()"#;
const NEXT_SCRIPT: &str = r#"(()=>{const e=[...document.querySelectorAll('button,input[type="submit"]')].find(e=>!e.disabled&&e.getBoundingClientRect().width>0&&e.getBoundingClientRect().height>0&&(e.textContent||e.value||'').trim().toLowerCase()==='next');if(!e)return false;e.click();return true;})()"#;
const CONFIRM_SCRIPT: &str = r#"(()=>{const e=[...document.querySelectorAll('button,input[type="submit"]')].find(e=>!e.disabled&&e.getBoundingClientRect().width>0&&e.getBoundingClientRect().height>0&&['confirm','redeem'].includes((e.textContent||e.value||'').trim().toLowerCase()));if(!e)return false;e.click();return true;})()"#;
