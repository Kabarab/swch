//! Store discovery is read-only. Every candidate needs a dated giveaway and a
//! positive regular price from an actual store before becoming an Offer.
use super::{catalog::canonical_game_title, platforms};
use chrono::{DateTime, NaiveDateTime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Deserialize, Serialize)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub url: String,
    pub ends_at: Option<i64>,
    pub regular_price: Option<i64>,
}
pub fn time(value: &Value) -> Option<i64> {
    let s = value.as_str()?;
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.timestamp())
        .or_else(|| {
            NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|t| t.and_utc().timestamp())
        })
}
pub fn full_game(title: &str) -> bool {
    let text = title.to_lowercase().replace('-', " ");
    ![
        "demo",
        "trial",
        "beta",
        "free week",
        "free-to-play",
        "free to play",
        "dlc",
        "playtest",
        "early access",
        "game pass",
        "subscription",
    ]
    .iter()
    .any(|word| text.contains(word))
}
pub fn ubisoft(html: &str, now: i64) -> Result<Vec<Candidate>, String> {
    let start = html
        .find("window.__PRELOADED_STATE__")
        .ok_or("Ubisoft не прислал каталог freeevents.")?;
    let tail = &html[start..];
    let json = tail
        .split_once('=')
        .ok_or("Неизвестный формат каталога Ubisoft.")?
        .1
        .trim_start();
    let state: Value = serde_json::Deserializer::from_str(json)
        .into_iter::<Value>()
        .next()
        .ok_or("Пустой каталог Ubisoft.")?
        .map_err(|_| "Неизвестный формат каталога Ubisoft.")?;
    let blocks = state["NewsPersonalization"]
        .as_object()
        .ok_or("Ubisoft изменил формат freeevents.")?;
    let mut seen = BTreeSet::new();
    let mut out = vec![];
    for entry in blocks.values().flat_map(|b| {
        b.pointer("/data/news")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
    }) {
        if entry["placement"] != "freeevents"
            || !matches!(
                entry["type"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .as_str(),
                "gametrial" | "giveaway"
            )
        {
            continue;
        }
        let Some(title) = entry["title"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() < 1024 && full_game(s))
        else {
            continue;
        };
        let Some(end) =
            time(&entry["expirationDate"]).filter(|e| *e > now && *e - now <= 90 * 86400)
        else {
            continue;
        };
        if time(&entry["publicationDate"]).is_some_and(|s| s > now) {
            continue;
        }
        let Some(url) = entry["links"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l["param"].as_str())
            .find(|url| {
                platforms::product_url("ubisoft", url)
                    && reqwest::Url::parse(url)
                        .is_ok_and(|u| u.host_str() == Some("register.ubisoft.com"))
            })
        else {
            continue;
        };
        let u = reqwest::Url::parse(url).map_err(|_| "Некорректный адрес Ubisoft.")?;
        let Some(id) = u
            .path_segments()
            .and_then(|mut p| p.find(|s| !s.is_empty()))
            .filter(|s| full_game(s))
        else {
            continue;
        };
        if !seen.insert(id.to_owned()) {
            continue;
        }
        out.push(Candidate {
            id: id.into(),
            title: title.trim().into(),
            url: url.into(),
            ends_at: Some(end),
            regular_price: None,
        });
    }
    Ok(out)
}
pub fn steamdb(html: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for card in html.split("class=\"span4 panel-sale").skip(1) {
        if !card.contains("cat-free-to-keep") {
            continue;
        }
        for link in card.split("<a ").skip(1) {
            let tag = link.split('>').next().unwrap_or_default();
            let Some(href) = super::offers::attr(tag, "href") else {
                continue;
            };
            if !platforms::product_url("steam", href) {
                continue;
            }
            let Ok(url) = reqwest::Url::parse(href) else {
                continue;
            };
            let parts = url
                .path_segments()
                .map(|p| p.collect::<Vec<_>>())
                .unwrap_or_default();
            if parts.first() == Some(&"app") {
                if let Some(id) = parts
                    .get(1)
                    .filter(|s| crate::launchers::steam::library::valid_app_id(s))
                {
                    ids.insert((*id).to_owned());
                }
            }
        }
    }
    ids.into_iter().collect()
}
pub fn microsoft_id(url: &str) -> Option<String> {
    if !platforms::product_url("microsoft", url) {
        return None;
    }
    let url = reqwest::Url::parse(url).ok()?;
    url.path_segments()?
        .rev()
        .find(|s| s.len() == 12 && s.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(|s| s.to_ascii_uppercase())
}
#[derive(Clone)]
pub struct MicrosoftProduct {
    pub id: String,
    pub title: String,
    pub regular_price: i64,
    pub price: i64,
}
pub fn microsoft_product(data: &Value, id: &str, title: &str) -> Result<MicrosoftProduct, String> {
    let p = &data["Payload"];
    let actual = p["ProductId"]
        .as_str()
        .ok_or("Microsoft не подтвердил товар.")?;
    let name = p["Title"]
        .as_str()
        .ok_or("Microsoft не прислал название товара.")?;
    if !actual.eq_ignore_ascii_case(id)
        || canonical_game_title(name) != canonical_game_title(title)
        || !full_game(name)
    {
        return Err("Microsoft вернул другой товар или неполную игру.".into());
    }
    if p["DisplayPrice"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("included"))
        || p["IsFreeToPlay"] == true
    {
        return Err("Это постоянная бесплатная игра или доступ Game Pass.".into());
    }
    if let Some(kind) = p["ProductType"].as_str() {
        if !matches!(kind, "Game" | "game" | "Games") {
            return Err("Товар Microsoft не является полной игрой.".into());
        }
    }
    let price = |v: &Value| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .filter(|p| p.is_finite() && *p >= 0. && *p <= 1_000_000.)
            .map(|p| {
                if p == 0. {
                    0
                } else {
                    ((p * 100.).round() as i64).max(1)
                }
            })
    };
    let regular = price(&p["MSRP"])
        .or_else(|| {
            p["SkusSummary"]
                .as_array()
                .filter(|v| v.len() == 1)
                .and_then(|v| price(&v[0]["MSRP"]))
        })
        .filter(|p| *p > 0)
        .ok_or("Microsoft не подтвердил обычную платную цену.")?;
    let final_price = price(&p["Price"]).ok_or("Microsoft не подтвердил текущую цену.")?;
    Ok(MicrosoftProduct {
        id: actual.into(),
        title: name.into(),
        regular_price: regular,
        price: final_price,
    })
}
pub fn prime_cards(data: &Value, now: i64) -> Result<Vec<Candidate>, String> {
    let list = data
        .as_array()
        .ok_or("Prime не прислал список полных игр.")?;
    let mut seen = BTreeSet::new();
    let mut out = vec![];
    for raw in list {
        let Ok(c) = serde_json::from_value::<Candidate>(raw.clone()) else {
            continue;
        };
        if c.id.is_empty()
            || c.id.len() > 256
            || c.title.is_empty()
            || !full_game(&c.title)
            || !platforms::product_url("prime", &c.url)
            || c.ends_at.is_some_and(|e| e <= now || e - now > 366 * 86400)
        {
            continue;
        }
        if seen.insert(c.id.clone()) {
            out.push(c);
        }
    }
    Ok(out)
}
pub const PRIME_CARDS_SCRIPT: &str = r#"(()=>{
 const root=document.querySelector('[data-a-target="offer-list-FGWP_FULL"]');if(!root)return null;
 const reactEnd=(card,id)=>{
  const read=(o,depth,bound=false)=>{if(!o||typeof o!=='object'||Array.isArray(o)||depth>4)return null;
   const ids=[o.asin,o.offerId,o.id].filter(v=>typeof v==='string');if(ids.length&&ids.some(v=>v===id))bound=true;
   if(bound&&typeof o.endTime==='string'){const n=Date.parse(o.endTime);if(Number.isFinite(n))return n;}
   for(const k of ['offer','offerData','item','data','metadata','availability']){const n=read(o[k],depth+1,bound);if(n)return n;}return null;};
  for(const element of [card,...card.querySelectorAll('button,a')]){const key=Object.keys(element).find(k=>k.startsWith('__reactFiber$'));let fiber=key?element[key]:null;
   for(let i=0;fiber&&i<8;i++,fiber=fiber.return){const n=read(fiber.memoizedProps,0);if(n)return n;}}
  return NaN;
 };
 const cards=[...root.querySelectorAll('[data-a-target="item-card"],.item-card,.item-card__action')].map(c=>{for(let i=0;i<5&&c&&!c.querySelector('.item-card-details__body__primary,[data-a-target="item-title"],h2,h3');i++)c=c.parentElement;return c;}).filter(c=>c&&c!==root&&!c.contains(root));
 const scan=cards.length?cards:[...root.children];
 return scan.map(card=>{
  const title=card.querySelector('.item-card-details__body__primary,[data-a-target="item-title"],h2,h3')?.textContent?.trim();
  const link=card.querySelector('a[href*="/claims/"][href*="/dp/"],a[data-a-target="FGWPOffer"],a[data-a-target="item-how-to-play"]')||card.closest('a[href*="/claims/"]');
  if(!title||!link)return null;const url=new URL(link.getAttribute('href'),location.href);const id=url.pathname.match(/\/dp\/([^/]+)/)?.[1];
  const deadline=card.querySelector('[data-end-date],[data-end-time],[data-expiration-date],time[datetime]');
  const date=deadline?.getAttribute('data-end-date')||deadline?.getAttribute('data-end-time')||deadline?.getAttribute('data-expiration-date')||deadline?.getAttribute('datetime');
  const ends=date?Date.parse(date):reactEnd(card,id);
  const price=card.querySelector('[data-original-price],s,del')?.textContent?.match(/[0-9]+(?:[.,][0-9]{1,2})?/);
  return id?{id,title,url:url.href,ends_at:Number.isFinite(ends)?Math.floor(ends/1000):null,regular_price:price?Math.round(Number(price[0].replace(',','.'))*100):null}:null;
 }).filter(Boolean);
})()"#;
pub const PRIME_DETAIL_SCRIPT: &str = r#"(()=>{
 const el=document.querySelector('[data-end-date],[data-end-time],[data-expiration-date],time[datetime]');
 const date=el?.getAttribute('data-end-date')||el?.getAttribute('data-end-time')||el?.getAttribute('data-expiration-date')||el?.getAttribute('datetime');
 let end=date?Date.parse(date):NaN;
 for(const script of document.querySelectorAll('script[type="application/ld+json"]')){try{const j=JSON.parse(script.textContent);const d=j.offers?.availabilityEnds||j.offers?.priceValidUntil;if(d&&!Number.isFinite(end))end=Date.parse(d);}catch{}}
 return Number.isFinite(end)?Math.floor(end/1000):null;
})()"#;
