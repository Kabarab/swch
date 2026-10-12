//! GOG's giveaway lives on the home banner, while the product can retain its
//! retail price. A complimentary endpoint and GALAXY ownership confirm it.
use super::{
    offers::Offer,
    providers::Claimed,
    web::{Browser, Session},
    web_rules::{self, Snapshot},
};
use crate::{
    launchers::gog::{auth::GogStore, product_game},
    storage::Account,
};
use serde_json::json;
use std::time::Duration;
pub async fn claim(
    account: &Account,
    offer: &Offer,
    timeout: u16,
    visible: bool,
    browser_name: &str,
) -> Result<Claimed, String> {
    if !account.id.starts_with("gog_") {
        return Err("Для подтверждения лицензий добавь этот аккаунт через модуль GOG в SWCH и подключи его магазин.".into());
    }
    let a = account.clone();
    let session = tokio::task::spawn_blocking(move || Session::load("gog", &a.id))
        .await
        .map_err(|_| "Не удалось прочитать вход GOG.".to_string())??;
    let mut browser = Browser::for_account(account, visible, browser_name).await?;
    let result = tokio::time::timeout(
        Duration::from_secs(u64::from(timeout)),
        run(&mut browser, account, offer, &session),
    )
    .await
    .unwrap_or_else(|_| {
        Err("GOG не подтвердил получение вовремя. Проверь библиотеку этого аккаунта.".into())
    });
    browser.close().await;
    result
}
async fn state(browser: &mut Browser, account: &Account, title: &str) -> Result<Snapshot, String> {
    let script = web_rules::snapshot_script_for_identity("gog", title, &account.id);
    let v = browser.evaluate(&script, None).await?;
    serde_json::from_value(v).map_err(|_| "Не удалось прочитать магазин GOG.".into())
}
fn identity(s: &Snapshot, account: &Account) -> Result<(), String> {
    if s.challenge {
        return Err("GOG требует проверки безопасности в окне магазина.".into());
    }
    if s.age_gate || s.requires_confirmation {
        return Err("GOG требует подтверждения в окне магазина.".into());
    }
    if !s.signed_in || s.identity.as_deref() != Some(&account.id) {
        return Err("Подключи вход именно этого аккаунта GOG.".into());
    }
    Ok(())
}
async fn run(
    browser: &mut Browser,
    account: &Account,
    offer: &Offer,
    session: &Session,
) -> Result<Claimed, String> {
    browser.restore(session).await?;
    browser.navigate("gog", &offer.url).await?;
    let mut product = state(browser, account, &offer.title).await?;
    for _ in 0..10 {
        if product.checkout_offer_matches && product.product_id.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        product = state(browser, account, &offer.title).await?;
    }
    identity(&product, account)?;
    if !product.checkout_offer_matches {
        return Err("GOG не подтвердил название игры; получение пропущено.".into());
    }
    let id = product.product_id.ok_or("GOG не сообщил SKU игры.")?;
    let store = GogStore::shared();
    let data = store
        .describe_product(&id)
        .await
        .map_err(|_| "Не удалось проверить SKU GOG.".to_string())?;
    if !matches!(data["game_type"].as_str(), Some("game" | "pack")) {
        return Err("GOG не подтвердил полную игру для этого SKU.".into());
    }
    let game = product_game(&id, &data)
        .map_err(|_| "GOG не подтвердил SKU игры.".to_string())?
        .ok_or("Это не полная игра GOG.")?;
    if super::catalog::canonical_game_title(&game.name).to_lowercase()
        != super::catalog::canonical_game_title(&offer.title).to_lowercase()
    {
        return Err("Название SKU GOG отличается от раздачи.".into());
    }
    if store
        .owns_product(&account.id, &id)
        .await
        .map_err(|_| "Не удалось проверить библиотеку GOG.".to_string())?
    {
        return Ok(Claimed::Owned);
    }
    if !product.regular_price_positive {
        return Err("GOG не подтвердил обычную цену игры; получение пропущено.".into());
    }
    browser.navigate("gog", "https://www.gog.com/en").await?;
    let page = state(browser, account, "").await?;
    identity(&page, account)?;
    let expected = json!(offer.url).to_string();
    let script=format!("(()=>{{const g=document.querySelector('#giveaway,.giveaway');const a=g?.querySelector('.giveaway__overlay-link,[selenium-id=\"giveawayOverlayLink\"],a[href*=\"/game/\"]');const b=g?.querySelector('button,.giveaway__button');if(!g||!a||!b||b.disabled)return false;const key=u=>new URL(u).pathname.replace(/^\\/[a-z]{{2}}\\//,'/').replace(/\\/$/,'');const host=new URL(a.href).hostname;return key(a.href)===key({expected})&&(host==='gog.com'||host.endsWith('.gog.com'));}})()");
    if browser.evaluate(&script, None).await?.as_bool() != Some(true)
        || !offer.eligible(chrono::Utc::now().timestamp())
    {
        return Err("GOG не подтвердил активную раздачу именно этой игры.".into());
    }
    // GOG's complimentary giveaway endpoint accepts no cart/payment data.
    browser
        .navigate("gog", "https://www.gog.com/giveaway/claim")
        .await?;
    for n in 0..4 {
        if n > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        if store
            .owns_product(&account.id, &id)
            .await
            .map_err(|_| "Не удалось подтвердить лицензию GOG.".to_string())?
        {
            return Ok(Claimed::Added);
        }
    }
    Err("GOG ещё не подтвердил лицензию. Ответ страницы не записан как успех.".into())
}
pub async fn redeem_key(
    account: &Account,
    title: &str,
    code: &str,
    timeout: u16,
    visible: bool,
    browser_name: &str,
) -> Result<(), String> {
    if !account.id.starts_with("gog_") || !super::keys::valid_code(code) {
        return Err("Для активации выбери аккаунт, добавленный через модуль GOG.".into());
    }
    let a = account.clone();
    let session = tokio::task::spawn_blocking(move || Session::load("gog", &a.id))
        .await
        .map_err(|_| "Не удалось прочитать вход GOG.".to_string())??;
    let mut browser = Browser::for_account(account, visible, browser_name).await?;
    let result = tokio::time::timeout(
        Duration::from_secs(u64::from(timeout)),
        redeem_in_browser(&mut browser, account, title, code, &session),
    )
    .await
    .unwrap_or_else(|_| {
        Err("GOG не подтвердил активацию вовремя. Проверь библиотеку выбранного аккаунта.".into())
    });
    browser.close().await;
    result
}
async fn redeem_in_browser(
    browser: &mut Browser,
    account: &Account,
    title: &str,
    code: &str,
    session: &Session,
) -> Result<(), String> {
    browser.restore(session).await?;
    browser
        .navigate("gog", "https://www.gog.com/en/redeem")
        .await?;
    identity(&state(browser, account, title).await?, account)?;
    let value = json!(code).to_string();
    let script=format!("(()=>{{if(!location.pathname.includes('/redeem'))return false;const e=document.querySelector('input#code,input[name=\"code\"],input[name=\"redeem_code\"]');if(!e||!['text','search'].includes(e.type)||!e.getBoundingClientRect().width)return false;Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,{value});e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));const form=e.closest('form')||document.querySelector('.redeem');const b=Array.from(form?.querySelectorAll('button,input[type=submit]')||[]).find(b=>/^continue$|^kontynuuj$|^weiter$|^continuer$|^continuar$/i.test((b.textContent||b.value).trim()));if(!b||b.disabled)return false;b.click();return true;}})()");
    if browser.evaluate(&script, None).await?.as_bool() != Some(true) {
        return Err("GOG не показал форму проверки ключа. Открой активацию вручную.".into());
    }
    let mut product = state(browser, account, title).await?;
    for _ in 0..20 {
        if product.product_id.is_some() && product.checkout_offer_matches {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        product = state(browser, account, title).await?;
    }
    identity(&product, account)?;
    if !product.checkout_offer_matches {
        return Err(
            "GOG не подтвердил игру для этого ключа. Ключ не активирован автоматически.".into(),
        );
    }
    let id = product.product_id.ok_or("GOG не сообщил SKU ключа.")?;
    let store = GogStore::shared();
    let data = store
        .describe_product(&id)
        .await
        .map_err(|_| "Не удалось проверить SKU ключа GOG.".to_string())?;
    if !matches!(data["game_type"].as_str(), Some("game" | "pack")) {
        return Err("GOG не подтвердил полную игру для ключа.".into());
    }
    let game = product_game(&id, &data)
        .map_err(|_| "GOG не подтвердил игру для ключа.".to_string())?
        .ok_or("Это не полная игра GOG.")?;
    if super::catalog::canonical_game_title(&game.name).to_lowercase()
        != super::catalog::canonical_game_title(title).to_lowercase()
    {
        return Err("Игра в GOG отличается от очереди ключей; активация остановлена.".into());
    }
    if store
        .owns_product(&account.id, &id)
        .await
        .map_err(|_| "Не удалось проверить библиотеку GOG.".to_string())?
    {
        return Err(
            "Эта игра уже есть на выбранном GOG-аккаунте; выбери другое назначение вручную.".into(),
        );
    }
    identity(&state(browser, account, title).await?, account)?;
    let submitted=browser.evaluate("(()=>{if(!location.pathname.includes('/redeem'))return false;const root=document.querySelector('.redeem,main')||document.body;const b=Array.from(root.querySelectorAll('button,input[type=submit]')).find(b=>/^activate$|^redeem$|^aktywuj$|^odbierz$|^einlösen$|^canjear$/i.test((b.textContent||b.value).trim()));if(!b||b.disabled||root.querySelector('input[name=dateOfBirth],input[name=otpCode]'))return false;b.click();return true;})()",None).await?;
    if submitted.as_bool() != Some(true) {
        return Err("GOG не подтвердил кнопку активации; проверь ключ в магазине.".into());
    }
    for n in 0..4 {
        if n > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        if store
            .owns_product(&account.id, &id)
            .await
            .map_err(|_| "Не удалось подтвердить лицензию GOG после ключа.".to_string())?
        {
            return Ok(());
        }
    }
    Err(
        "GOG не подтвердил лицензию после ключа. Повтор допускается только на том же аккаунте."
            .into(),
    )
}
