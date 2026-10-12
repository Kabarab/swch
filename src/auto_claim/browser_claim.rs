use super::{
    offers::Offer,
    platforms,
    providers::Claimed,
    web::{Browser, Session},
    web_rules::{self, Decision, Snapshot},
};
use crate::storage::Account;
use serde_json::json;
use std::time::{Duration, Instant};

pub fn expected_identity(account: &Account) -> String {
    if account.launcher == "epic"
        || account.launcher == "steam"
        || account.launcher == "gog" && account.id.starts_with("gog_")
    {
        account.id.clone()
    } else {
        account
            .email
            .clone()
            .unwrap_or_else(|| account.username.clone())
    }
}
pub(crate) async fn snapshot(
    browser: &mut Browser,
    account: &Account,
    title: &str,
    verified: &Session,
) -> Result<Snapshot, String> {
    let value = browser
        .evaluate(
            &web_rules::snapshot_script_for_identity(
                &account.launcher,
                title,
                &expected_identity(account),
            ),
            None,
        )
        .await?;
    let mut state: Snapshot = serde_json::from_value(value)
        .map_err(|_| "Не удалось прочитать состояние раздачи.".to_string())?;
    if !platforms::trusted_url(&account.launcher, &state.url) {
        return Err("Магазин перенаправил на неподдерживаемый адрес.".into());
    }
    // The OS-vault binding was verified when the account was connected. A
    // different identity reported by the current page always wins and rejects.
    if state.signed_in
        && verified.identity_proof == "login_form"
        && state.identity == verified.store_identity
    {
        state.identity = Some(verified.identity.clone());
    }
    if state.signed_in
        && state.identity.is_none()
        && !(account.launcher == "gog" && account.id.starts_with("gog_"))
    {
        state.identity = Some(verified.identity.clone());
    }
    Ok(state)
}
pub async fn claim(
    account: &Account,
    offer: &Offer,
    timeout_seconds: u16,
    visible: bool,
    browser_name: &str,
) -> Result<Claimed, String> {
    claim_with_proof(
        account,
        offer,
        timeout_seconds,
        visible,
        false,
        browser_name,
    )
    .await
}
pub async fn claim_with_proof(
    account: &Account,
    offer: &Offer,
    timeout_seconds: u16,
    visible: bool,
    verified_regular_price: bool,
    browser_name: &str,
) -> Result<Claimed, String> {
    let a = account.clone();
    let session = tokio::task::spawn_blocking(move || Session::load(&a.launcher, &a.id))
        .await
        .map_err(|_| "Не удалось прочитать браузерный вход.".to_string())??;
    let mut browser = Browser::for_account(account, visible, browser_name).await?;
    let outcome = claim_in_browser(
        &mut browser,
        account,
        offer,
        timeout_seconds,
        visible,
        verified_regular_price,
        &session,
    )
    .await;
    browser.close().await;
    outcome
}
async fn claim_in_browser(
    browser: &mut Browser,
    account: &Account,
    offer: &Offer,
    timeout_seconds: u16,
    visible: bool,
    verified_regular_price: bool,
    session: &Session,
) -> Result<Claimed, String> {
    browser.restore(&session).await?;
    if !platforms::product_url(&account.launcher, &offer.url) {
        return Err("Ссылка не ведёт на игру выбранного магазина.".into());
    }
    browser.navigate(&account.launcher, &offer.url).await?;
    let start = Instant::now();
    let mut acted = false;
    let mut last_action: Option<(String, String)> = None;
    let mut confirmed = false;
    let mut fanatical_poll = None;
    let mut session_refreshed = false;
    let mut paid_verified =
        verified_regular_price || offer.normally_paid && offer.namespace != "gamerpower";
    let mut finite_verified = (verified_regular_price || offer.namespace != "gamerpower")
        && offer
            .ends_at
            .is_some_and(|e| e > chrono::Utc::now().timestamp());
    while start.elapsed() < Duration::from_secs(timeout_seconds as u64) {
        if !offer.eligible(chrono::Utc::now().timestamp()) {
            return Err("Раздача закончилась; оформление остановлено.".into());
        }
        let mut state = snapshot(browser, account, &offer.title, &session).await?;
        let mut frame = None;
        if acted
            && !state.challenge
            && state.signed_in
            && state.identity.as_deref() == Some(&session.identity)
        {
            for (sid, context, value) in browser
                .frame_values(
                    &account.launcher,
                    &web_rules::snapshot_script(&account.launcher, &offer.title),
                )
                .await?
            {
                if let Ok(mut child) = serde_json::from_value::<Snapshot>(value) {
                    if child.checkout_offer_matches
                        && platforms::trusted_url(&account.launcher, &child.url)
                        && child.identity.as_ref().is_none_or(|id| {
                            id == &session.identity || Some(id) == session.store_identity.as_ref()
                        })
                        && (child.code.is_some()
                            || child.claim_label.as_deref().is_some_and(|v| {
                                v.eq_ignore_ascii_case("place order")
                                    || v.eq_ignore_ascii_case("add to library")
                            }))
                    {
                        child.identity = state.identity.clone();
                        child.signed_in = true;
                        state = child;
                        frame = Some((sid, context));
                        break;
                    }
                    if account.launcher == "ubisoft"
                        && state.checkout_offer_matches
                        && child.signed_in
                        && !child.challenge
                        && !child.requires_confirmation
                        && child
                            .claim_label
                            .as_deref()
                            .is_some_and(|l| l.eq_ignore_ascii_case("continue"))
                        && child.identity.as_ref().is_some_and(|id| {
                            id == &session.identity || Some(id) == session.store_identity.as_ref()
                        })
                    {
                        child.identity = Some(session.identity.clone());
                        child.checkout_offer_matches = true;
                        child.price_zero = true;
                        state = child;
                        frame = Some((sid, context));
                        break;
                    }
                }
            }
        }
        let stage = web_rules::decision(&state, &session.identity, paid_verified, finite_verified);
        if account.launcher == "fanatical"
            && state.signed_in
            && state.identity.as_deref() == Some(&session.identity)
            && state.checkout_offer_matches
            && !state.challenge
            && !state.age_gate
            && !state.requires_confirmation
            && (paid_verified || state.original_price_positive)
            && (finite_verified || state.finite_promotion)
            && fanatical_poll.is_none_or(|t: Instant| t.elapsed() > Duration::from_secs(3))
        {
            fanatical_poll = Some(Instant::now());
            if let Some(claimed) = super::fanatical::receive(browser, account, offer).await? {
                return Ok(claimed);
            }
        }
        match stage {
            Decision::Owned=>return Ok(if acted{Claimed::Added}else{Claimed::Owned}),
            Decision::Get|Decision::Confirm=>{
                if account.launcher=="epic" {
                    let cookies=browser.cookies("epic").await?;
                    crate::launchers::epic::EpicClient::new().verify_web_identity(&cookies,&account.id).await.map_err(|_|"Epic не подтвердил владельца текущего браузерного входа. Подключи нужный аккаунт.".to_string())?;
                }
                if !session_refreshed{
                    let mut fresh=session.clone();fresh.cookies=browser.cookies(&account.launcher).await?;fresh.web_storage=browser.web_storage(&account.launcher).await?;
                    if !tokio::task::spawn_blocking(move||fresh.save()).await.is_ok_and(|r|r.is_ok()){tracing::warn!("Store session refresh was not persisted");}
                    session_refreshed=true;
                }
                let selector=state.claim_selector.ok_or("Не удалось определить кнопку раздачи.")?;
                let label=state.claim_label.unwrap_or_default();
                let scope=frame.as_ref().map(|(s,id)|format!("{s}:{id}")).unwrap_or_else(||browser.target_session.clone());
                let signature=(scope,label.clone());
                if confirmed||last_action.as_ref()==Some(&signature) {tokio::time::sleep(Duration::from_secs(1)).await;continue;}
                let raw_selector=selector.clone();let raw_label=label.clone();
                let selector=json!(selector).to_string();let label=json!(label).to_string();
                let expression=format!("(()=>{{const e=document.querySelector({selector});if(!e||e.disabled||e.textContent.trim()!=={label})return false;if(e.tagName==='A'&&e.target==='_blank')return {{navigate:e.href}};e.click();return true;}})()");
                let mut clicked=if let Some((sid,context))=&frame {browser.evaluate_context(&expression,sid,*context).await?} else {browser.click_control(&raw_selector,&raw_label).await?};
                if let Some(target)=clicked.get("navigate").and_then(serde_json::Value::as_str){if !platforms::product_url(&account.launcher,target){return Err("Кнопка ведёт за пределы раздачи магазина.".into());}browser.navigate(&account.launcher,target).await?;clicked=json!(true);}
                if clicked.as_bool()==Some(true){acted=true;paid_verified|=state.original_price_positive;finite_verified|=state.finite_promotion;confirmed=stage==Decision::Confirm;last_action=Some(signature);}
            }
            Decision::Challenge | Decision::NeedsConfirmation if visible => {},
            Decision::Challenge=>return Err("В отдельном окне автоклеймера магазин показал проверку безопасности. Получение ещё не отправлено; открой проверку этого аккаунта в SWCH.".into()),
            Decision::NeedsConfirmation=>return Err("Подтверждение возраста, лицензии или родительского доступа требуется в окне магазина.".into()),
            Decision::NeedsLogin if start.elapsed()<Duration::from_secs(15)=>{},
            Decision::NeedsLogin=>return Err("Браузерный вход истёк. Подключи этот аккаунт заново.".into()),
            Decision::NeedsSubscription=>return Err("Для этой раздачи требуется уже действующая подписка Prime. Подписка автоматически не оформляется.".into()),
            Decision::DownloadOnly=>return Err("Это только скачивание файла; игровая лицензия не выдаётся.".into()),
            Decision::ClaimCode=>{
                let a=account.clone();let o=offer.clone();let code=state.code.ok_or("Магазин не показал игровой ключ.")?;let target=state.code_platform;
                tokio::task::spawn_blocking(move||super::keys::receive(&a,&o,code,target)).await.map_err(|_|"Не удалось сохранить ключ.".to_string())??;
                return Ok(Claimed::CodeReceived);
            },
            Decision::Reject=>{},
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("Магазин не подтвердил аккаунт, нулевую сумму или получение игры. Повтор не создаёт запись об успехе.".into())
}

/// First connection is an explicit application action. The user handles login,
/// MFA/age/challenge dialogs in the application's own visible browser window.
pub async fn connect(
    account: &Account,
    country: &str,
    timeout_seconds: u16,
    browser_name: &str,
) -> Result<(), String> {
    connect_with_session(account, country, timeout_seconds, None, browser_name).await
}
pub async fn connect_with_session(
    account: &Account,
    country: &str,
    timeout_seconds: u16,
    existing: Option<Session>,
    browser_name: &str,
) -> Result<(), String> {
    let mut browser = Browser::for_account(account, true, browser_name).await?;
    let outcome =
        connect_in_browser(&mut browser, account, country, timeout_seconds, existing).await;
    browser.close().await;
    outcome
}

fn connection_entry_url(platform: &str, restoring: bool) -> Result<String, String> {
    let spec = platforms::platform(platform).ok_or("Неизвестный магазин.")?;
    if restoring {
        Ok(spec.giveaways_url.into())
    } else {
        platforms::connection_url(platform, spec.giveaways_url)
    }
}

async fn connect_in_browser(
    browser: &mut Browser,
    account: &Account,
    country: &str,
    timeout_seconds: u16,
    existing: Option<Session>,
) -> Result<(), String> {
    let spec = platforms::platform(&account.launcher).ok_or("Неизвестный магазин.")?;
    if let Some(session) = &existing {
        if session.account_id != account.id
            || (matches!(account.launcher.as_str(), "steam" | "epic")
                && session.identity != account.id)
        {
            return Err("Вход относится к другому аккаунту.".into());
        }
        browser.restore(session).await?;
    }
    browser.watch_login(&account.launcher).await?;
    let login_url = platforms::connection_url(&account.launcher, spec.giveaways_url)?;
    let entry_url = connection_entry_url(&account.launcher, existing.is_some())?;
    browser.navigate(&account.launcher, &entry_url).await?;
    let expected = existing
        .as_ref()
        .map(|s| s.identity.clone())
        .unwrap_or_else(|| expected_identity(account));
    let start = Instant::now();
    let mut saw_challenge = false;
    let mut login_opened = existing.is_none();
    while start.elapsed() < Duration::from_secs(timeout_seconds as u64) {
        browser.watch_login_popups().await?;
        let value = browser
            .evaluate(
                &web_rules::snapshot_script_for_identity(&account.launcher, "", &expected),
                None,
            )
            .await?;
        let mut state: Snapshot =
            serde_json::from_value(value).map_err(|_| "Не удалось прочитать вход магазина.")?;
        saw_challenge |= state.challenge;
        // An imported SSO session gets a chance to authenticate the store first.
        // Open login only when its fully visible navigation reports sign-out;
        // loading pages and security checks must not force a login restart.
        if account.launcher == "epic"
            && !login_opened
            && !state.signed_in
            && !state.challenge
            && start.elapsed() >= Duration::from_secs(5)
            && browser
                .evaluate(
                    r#"(()=>{if(location.hostname!=='store.epicgames.com')return false;const visible=e=>!!e&&e.getBoundingClientRect().width>0&&e.getBoundingClientRect().height>0&&getComputedStyle(e).visibility!=='hidden'&&getComputedStyle(e).display!=='none';const nav=document.querySelector('egs-navigation');if(visible(nav)&&nav.getAttribute('isloggedin')==='false')return true;return [...document.querySelectorAll('header button,header a,nav button,nav a')].some(e=>visible(e)&&/^sign in$/i.test(e.textContent.trim()));})()"#,
                    None,
                )
                .await?
                .as_bool()
                == Some(true)
        {
            browser.navigate(&account.launcher, &login_url).await?;
            login_opened = true;
            continue;
        }
        if state.signed_in
            && state.identity.is_none()
            && !(account.launcher == "gog" && account.id.starts_with("gog_"))
        {
            if let Some(session) = &existing {
                state.identity = Some(session.identity.clone());
            }
        }
        if state.signed_in {
            if let Some(session) = &existing {
                if session.identity_proof == "login_form"
                    && state.identity == session.store_identity
                {
                    state.identity = Some(session.identity.clone());
                }
            }
        }
        let submitted = browser
            .submitted_login()
            .is_some_and(|v| v.eq_ignore_ascii_case(&expected));
        let profile_matches = state.identity.as_deref() == Some(&expected);
        let can_bind_login = !matches!(account.launcher.as_str(), "steam" | "epic")
            && !(account.launcher == "gog" && account.id.starts_with("gog_"))
            && submitted
            && !state.identity.as_ref().is_some_and(|id| {
                id.contains('@') && expected.contains('@') && !id.eq_ignore_ascii_case(&expected)
            });
        if state.signed_in
            && (profile_matches || can_bind_login)
            && !state.challenge
            && !state.age_gate
            && !state.requires_confirmation
            && platforms::trusted_url(&account.launcher, &state.url)
        {
            let cookies = browser.cookies(&account.launcher).await?;
            if account.launcher == "epic" {
                crate::launchers::epic::EpicClient::new()
                    .verify_web_identity(&cookies, &account.id)
                    .await
                    .map_err(|_| {
                        "Окно магазина вошло в другой Epic-аккаунт; подключение не сохранено."
                            .to_string()
                    })?;
            }
            let session = Session {
                platform: account.launcher.clone(),
                account_id: account.id.clone(),
                identity: expected.clone(),
                country: country.into(),
                cookies,
                connected_at: chrono::Utc::now().to_rfc3339(),
                identity_proof: if account.launcher == "epic" {
                    "store_page".into()
                } else if profile_matches {
                    existing
                        .as_ref()
                        .map(|s| s.identity_proof.clone())
                        .filter(|v| !v.is_empty())
                        .unwrap_or("store_page".into())
                } else {
                    "login_form".into()
                },
                store_identity: if profile_matches {
                    existing.as_ref().and_then(|s| s.store_identity.clone())
                } else {
                    state.identity.clone()
                },
                web_storage: browser.web_storage(&account.launcher).await?,
            };
            tokio::task::spawn_blocking(move || session.save())
                .await
                .map_err(|_| "Не удалось сохранить браузерный вход.".to_string())??;
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(if saw_challenge {
        "Проверка безопасности магазина не завершилась, вход не подтверждён. Если галочка появляется снова, используй «Открыть обычный браузер» для этого аккаунта; после получения нажми «Проверить библиотеку»."
    } else {
        "Магазин не подтвердил вход этого аккаунта. Войди на странице авторизации; если вход не проходит, используй «Открыть обычный браузер»."
    }.into())
}

#[cfg(test)]
mod connection_tests {
    use super::*;

    #[test]
    fn restored_epic_connection_starts_at_store_instead_of_login_form() {
        let restored = reqwest::Url::parse(&connection_entry_url("epic", true).unwrap()).unwrap();
        assert_eq!(restored.host_str(), Some("store.epicgames.com"));
        assert_eq!(restored.path(), "/en-US/free-games");
        assert!(restored.query().is_none());

        let first = reqwest::Url::parse(&connection_entry_url("epic", false).unwrap()).unwrap();
        assert_eq!(first.host_str(), Some("www.epicgames.com"));
        assert_eq!(first.path(), "/id/login");
        assert_eq!(
            first
                .query_pairs()
                .find(|(name, _)| name == "redirectUrl")
                .unwrap()
                .1,
            restored.as_str()
        );
    }
}
