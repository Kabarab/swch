//! Conservative browser state machine; source discovery never authorizes a paid
//! checkout. Account identity and the exact product are checked at every step.
use serde::Deserialize;
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub title: String,
    pub url: String,
    pub identity: Option<String>,
    pub signed_in: bool,
    pub in_library: bool,
    pub price_zero: bool,
    pub original_price_positive: bool,
    pub discount_100: bool,
    pub finite_promotion: bool,
    pub claim_label: Option<String>,
    pub claim_selector: Option<String>,
    pub checkout_total_zero: bool,
    pub checkout_offer_matches: bool,
    pub checkout_item_count: Option<u32>,
    pub challenge: bool,
    pub age_gate: bool,
    pub requires_confirmation: bool,
    pub requires_subscription: bool,
    pub download_only: bool,
    pub code: Option<String>,
    pub code_platform: Option<String>,
    pub product_id: Option<String>,
    pub regular_price_positive: bool,
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Decision {
    Owned,
    Get,
    Confirm,
    ClaimCode,
    NeedsLogin,
    Challenge,
    NeedsConfirmation,
    NeedsSubscription,
    DownloadOnly,
    Reject,
}
pub fn decision(s: &Snapshot, expected_identity: &str, paid: bool, finite: bool) -> Decision {
    if s.challenge {
        return Decision::Challenge;
    }
    if s.age_gate || s.requires_confirmation {
        return Decision::NeedsConfirmation;
    }
    if !s.signed_in {
        return Decision::NeedsLogin;
    }
    if s.identity.as_deref() != Some(expected_identity) || !s.checkout_offer_matches {
        return Decision::Reject;
    }
    if s.in_library && s.code.is_none() {
        return Decision::Owned;
    }
    if s.requires_subscription {
        return Decision::NeedsSubscription;
    }
    if s.download_only {
        return Decision::DownloadOnly;
    }
    if !(paid || s.original_price_positive) || !(finite || s.finite_promotion) {
        return Decision::Reject;
    }
    let label = s
        .claim_label
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if s.code.is_some() {
        return Decision::ClaimCode;
    }
    if matches!(
        label.as_str(),
        "add to library"
            | "place order"
            | "claim game"
            | "claim now"
            | "get game"
            | "claim"
            | "add to account"
            | "claim free game"
            | "claim this game"
            | "claim code"
            | "continue"
    ) {
        return if if label == "place order" {
            s.checkout_total_zero && s.checkout_item_count == Some(1)
        } else {
            s.checkout_total_zero || s.price_zero
        } {
            Decision::Confirm
        } else {
            Decision::Reject
        };
    }
    if matches!(
        label.as_str(),
        "get" | "download" | "buy now" | "redeem" | "claim game" | "pc"
    ) && s.price_zero
    {
        return Decision::Get;
    }
    Decision::Reject
}
pub fn snapshot_script(platform: &str, title: &str) -> String {
    snapshot_script_for_identity(platform, title, "")
}
pub fn snapshot_script_for_identity(platform: &str, title: &str, identity: &str) -> String {
    let config =
        serde_json::json!({"platform":platform,"title":title,"identity":identity}).to_string();
    format!("(async function(config) {{ {} }})({config})", SCRIPT)
}
const SCRIPT: &str = r#"
const visible=e=>!!e&&e.getBoundingClientRect().width>0&&e.getBoundingClientRect().height>0&&getComputedStyle(e).visibility!=='hidden'&&getComputedStyle(e).display!=='none'&&e.getAttribute('aria-hidden')!=='true';
const norm=s=>String(s||'').normalize('NFKC').replace(/[®™©]/g,'').replace(/\s+/g,' ').trim().toLowerCase().replace(/\s*\((?:pc|steam|gog|epic games)\)\s*/g,' ').replace(/\s+(?:steam |gog )?key giveaway$/,'').replace(/\s+giveaway$/,'').trim();
const dialog=Array.from(document.querySelectorAll('[role="dialog"],dialog')).find(visible);
let root=dialog||document.querySelector('main,[role="main"],#webPurchaseContainer,.payment')||document.body;
let primeCard=null;
if(config.platform==='prime'&&config.title){primeCard=[...document.querySelectorAll('[data-a-target="offer-list-FGWP_FULL"] [data-a-target="item-card"],[data-a-target="offer-list-FGWP_FULL"] .item-card,[data-a-target="offer-list-FGWP_FULL"] .item-card__action')].map(c=>{for(let i=0;i<5&&c&&!c.querySelector('.item-card-details__body__primary,[data-a-target="item-title"],h2,h3');i++)c=c.parentElement;return c;}).filter(Boolean).find(c=>norm(c.querySelector('.item-card-details__body__primary,[data-a-target="item-title"],h2,h3')?.textContent)===norm(config.title));if(primeCard&&!dialog)root=primeCard;}
const name=root.querySelector('[data-testid="product-title"],.payment__offer-title,.productcard-basics__title,h1.game_title,.game_info h2,.item-card-details__body__primary,[data-a-target="item-title"],[data-a-target="DescriptionItemDetails"] h1,h1')||document.querySelector('h1');
let currentTitle=name?.textContent?.trim()||'';
if(config.platform==='ubisoft') {const named=document.querySelector('[data-game-title],.sitegen-game-title');const meta=document.querySelector('meta[property="og:title"]');currentTitle=named?.getAttribute('data-game-title')||named?.textContent||meta?.content?.split(/\s+[|–—]\s+/)[0]||currentTitle;}
const text=(root?.innerText||'').toLowerCase();
const priceZero=s=>/^(free|free with prime|[$€£₽]?\s*0([.,]0{1,2})?\s*(usd|eur|gbp|rub|byn)?|0([.,]0{1,2})?\s*[$€£₽])$/i.test(String(s||'').trim());
const labels=/^(get|add to library|place order|claim game|claim this game|claim now|claim|claim code|claim free game|get game|add to account|download|buy now|redeem|in library|owned|already claimed|in your library)$/i;
let cta=root.querySelector('button[data-testid="purchase-cta-button"],button[data-testid="claim-button"],.giveaway__button');
if(!visible(cta)) cta=Array.from(root.querySelectorAll('button,a')).find(e=>visible(e)&&labels.test(e.textContent.trim())&&!e.disabled);
if(config.platform==='ubisoft'&&/get your free game/i.test(text)&&!/try now for free|free weekend|free trial/i.test(text))cta=Array.from(root.querySelectorAll('.sitegen-platform-button')).find(e=>visible(e)&&/^pc$/i.test(e.textContent.trim()))||cta;
if(config.platform==='ubisoft'&&!document.querySelector('#AuthEmail,#AuthPassword,input[type="password"]')&&/welcome back/i.test(text))cta=Array.from(root.querySelectorAll('button')).find(e=>visible(e)&&/^continue$/i.test(e.textContent.trim()))||cta;
let area=cta?.closest('aside,[data-testid="purchase-area"],.product-actions,.payment,.giveaway,.purchase-options')||cta?.parentElement;
for(let i=0;i<4&&area&&!area.querySelector('[data-testid*="price"],.price,.product-price,[class*="total-price"]');i++)area=area.parentElement;
const prices=Array.from(area?.querySelectorAll('[data-testid*="price"],.price,.product-price,[class*="total-price"]')||[]).filter(e=>visible(e)&&!e.closest('s,del,[data-testid="original-price"]'));
const crossed=area?.querySelector('s,del,[data-testid="original-price"],.original_price');
const positive=!!crossed&&/[1-9]/.test(crossed.textContent);
let zero=prices.length===1&&priceZero(prices[0].textContent)||prices.length===0&&positive&&!!area&&/-100\s*%/.test(area.innerText)&&Array.from(area.querySelectorAll('span')).filter(visible).some(e=>/^Free$/.test(e.textContent.trim()))||!!dialog&&/^this is free\. add it to your library to get started\./i.test(dialog.innerText.trim());
if(config.platform==='prime'&&primeCard&&cta?.matches('[data-a-target="FGWPOffer"]')&&!/join prime|start.*trial|subscribe|link.*account/i.test(text))zero=true;
if(config.platform==='prime'&&!primeCard&&/free with prime|included with prime/i.test(text)&&!/join prime|start.*trial|subscribe|link.*account/i.test(text))zero=true;
if(config.platform==='ubisoft'&&/get your free game/i.test(text)&&!/try now for free|free weekend|free trial/i.test(text)&&/^pc$/i.test(cta?.textContent.trim()||''))zero=true;
const items=Array.from(root.querySelectorAll('[data-testid="order-item"],.order-summary__item,.cart-item'));
const itemTitle=e=>(e.querySelector('[data-testid="product-title"],.order-item-title,.product-title,h3')||e).textContent;
const matches=!!config.title&&(items.length?items.length===1&&norm(itemTitle(items[0]))===norm(config.title):norm(currentTitle)===norm(config.title));
const identityEl=document.querySelector('header [data-account-id],header [data-user-id],[data-testid="account-email"],#mectrl_currentAccount_secondary,input[readonly][type="email"],a[href*="/profile/"][data-current-user]');
let identity=identityEl?.getAttribute('data-account-id')||identityEl?.getAttribute('data-user-id')||identityEl?.value||identityEl?.textContent?.trim()||null;
const nav=document.querySelector('egs-navigation');
let signedIn=nav?nav.getAttribute('isloggedin')==='true':!!identity;
const authPage=/(^|\/)(login|signin|sign-in|password|two_factor)(\/|$)/i.test(location.pathname)||['login.live.com','login.microsoftonline.com'].includes(location.hostname)||location.pathname.startsWith('/ap/signin');
if(authPage)signedIn=false;
if(config.platform==='prime')signedIn=!!document.querySelector('[data-a-target="user-dropdown-first-name-text"]')?.textContent?.trim();
if(config.platform==='fanatical'&&!authPage){try{const auth=JSON.parse(localStorage.getItem('bsauth')||'{}');const email=auth.email||auth.user?.email||auth.profile?.email;if(typeof email==='string'&&typeof auth.token==='string'&&auth.token){identity=email;signedIn=true;}}catch{}}
if(config.platform==='ubisoft'){const user=document.querySelector('[data-testid="account-username"],.welcome-back-username,[data-user-name]');if(user){identity=user.getAttribute('data-user-name')||user.textContent.trim();if(!document.querySelector('#AuthEmail,#AuthPassword,input[type="password"]')&&/welcome back/i.test(text))signedIn=true;}}
if(config.platform==='itchio') {const user=document.querySelector('.user_panel .username,.user_panel a[href*="/profile/"]');if(user){identity=user.textContent.trim();signedIn=!!identity&&!authPage;}}
if(!signedIn&&!authPage) {signedIn=Array.from(document.querySelectorAll('header a[href*="logout"],nav a[href*="logout"],header button,nav button')).some(e=>visible(e)&&(/\/logout(\?|$)/.test(e.getAttribute('href')||'')||/^log out$|^sign out$/i.test(e.textContent.trim())));}
if(config.platform==='gog'&&/(^|\.)gog\.com$/.test(location.hostname)) {
 try {const r=await fetch('https://menu.gog.com/v1/account/basic',{credentials:'include',cache:'no-store',signal:AbortSignal.timeout(5000)});if(r.ok){const raw=await r.text(),j=JSON.parse(raw);signedIn=j.isLoggedIn===true;const digits=raw.match(/"userId"\s*:\s*"?(\d+)"?/)?.[1];const uid=digits?'gog_'+digits:null;const values=[j.username,j.email,uid].filter(v=>typeof v==='string'&&v);identity=/^gog_\d+$/.test(config.identity)?uid:values.includes(config.identity)?config.identity:values[0]||null;}}catch{signedIn=false;identity=null;}
}
const primary=cta?.textContent?.trim()||'';
document.querySelectorAll('[data-swch-claim-target]').forEach(e=>e.removeAttribute('data-swch-claim-target'));
if(cta&&!cta.id)cta.setAttribute('data-swch-claim-target','1');
const total=root.querySelector('[data-testid="order-total"],.payment__total,.order-summary__total,[class*="total-price"]');
const challengeFrame=Array.from(document.querySelectorAll('iframe[src*="hcaptcha"],iframe[src*="recaptcha"]')).some(e=>{
 if(!visible(e))return false;const src=e.getAttribute('src')||'';const rect=e.getBoundingClientRect();
 if(/size=invisible|frame=checkbox/.test(src)&&rect.height<90)return false;
 return /\/bframe|frame=challenge/.test(src)&&rect.height>90||/\/anchor/.test(src)&&!src.includes('size=invisible')&&rect.height>60;
});
const captcha=challengeFrame||Array.from(document.querySelectorAll('#challenge-running,#challenge-stage,#challenge-form,.h_captcha_challenge')).some(visible)||!cta&&(/verify you are human|checking your browser|please complete a security check/.test(text)||/just a moment/i.test(document.title));
let codeElement=root.querySelector('[data-a-target="ClaimStateClaimCodeContent"],[data-testid="game-key"],.key-redeem input[readonly],input[readonly][name="game-key"]');
if(config.platform==='prime'&&!codeElement){const copy=[...root.querySelectorAll('button')].find(e=>visible(e)&&/^(copy|copy code|claim code)$/i.test(e.textContent.trim()));const previous=copy?.previousElementSibling;if(previous&&/^[A-Z0-9-]{8,128}$/i.test(previous.textContent.trim()))codeElement=previous;}
const codeText=codeElement?.value||codeElement?.textContent?.replace(/^Your code:\s*/i,'').trim()||null;
const code=codeText&&/^[A-Z0-9-]{8,128}$/i.test(codeText)&&(/\d/.test(codeText)||/^(?:[A-Z0-9]{5}-){2}(?:[A-Z0-9]{5}|[A-Z0-9]{5}-[A-Z0-9]{5}-[A-Z0-9]{5})$/i.test(codeText))?codeText:null;
const instruction=text.match(/\bredeem (?:your|this) (?:product )?code (?:on|at) (gog|steam|microsoft|xbox)\b/i)?.[1];
const codePlatform=root.querySelector('[data-redemption-platform]')?.getAttribute('data-redemption-platform')||(instruction==='xbox'?'microsoft':instruction)||null;
const product=document.querySelector('#productcard [data-product-id],main [data-product-id],[data-game-id],[data-gog-product-id]');
const productId=product?.getAttribute('data-product-id')||product?.getAttribute('data-game-id')||product?.getAttribute('data-gog-product-id')||null;
return {title:currentTitle,url:location.origin+location.pathname,identity,signed_in:signedIn,
in_library:/^(in library|owned|already claimed|in your library)$/i.test(primary)||config.platform==='prime'&&!!primeCard&&Array.from(primeCard.querySelectorAll('p')).some(p=>/^collected$/i.test(p.textContent.trim()))||config.platform==='ubisoft'&&/the game has been successfully added.{0,100}library/i.test(text)||config.platform==='microsoft'&&!!root.querySelector('[data-testid="ownership-indicator"],[aria-label="You own this"]'),price_zero:zero,
original_price_positive:positive,discount_100:!!area&&/-100\s*%/.test(area.innerText),finite_promotion:!!area&&/sale ends|offer ends|giveaway ends|ends in/.test(area.innerText.toLowerCase()),
claim_label:primary||null,claim_selector:cta?(cta.id?'#'+CSS.escape(cta.id):'[data-swch-claim-target="1"]'):null,
checkout_total_zero:!!total&&priceZero(total.textContent),checkout_offer_matches:matches,checkout_item_count:items.length||null,
challenge:captcha,age_gate:!!document.querySelector('[data-testid="AgeSelect"],input[name="dateOfBirth"],.age-verification,input[name="birthdate"]'),
requires_confirmation:/enter your date of birth|verify your age|parental control pin|payment pin/.test(text)||!!document.querySelector('input[autocomplete="one-time-code"],input[name="otpCode"],#auth-mfa-otpcode'),
requires_subscription:/join prime|start your free trial|subscribe to claim/.test(text),download_only:/no account required|no claim to library/.test(text),code,code_platform:codePlatform,product_id:productId,regular_price_positive:positive||prices.some(e=>/[$€£₽]\s*[1-9]|[1-9]\d*[.,]\d\d/.test(e.textContent))};
"#;
