//! Read/reveal only the matching order item. Browser auth remains inside the
//! selected account's store context and the OS vault, never in the database.
use super::{catalog::canonical_game_title, keys, offers::Offer, providers::Claimed, web::Browser};
use crate::storage::Account;
use serde_json::{json, Value};

pub fn order_item(data: &Value, slug: &str, title: &str) -> Result<Option<Value>, String> {
    let orders = data
        .as_array()
        .ok_or("Fanatical не подтвердил список заказов.")?;
    let mut matches = vec![];
    for order in orders {
        for item in order["items"].as_array().into_iter().flatten() {
            if !item["name"]
                .as_str()
                .is_some_and(|name| canonical_game_title(name) == canonical_game_title(title))
            {
                continue;
            }
            if ["slug", "productSlug", "urlSlug"]
                .iter()
                .filter_map(|k| item[*k].as_str())
                .any(|s| s != slug)
            {
                continue;
            }
            let Some(oid) = order["_id"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            let Some(pid) = item["_id"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            matches.push(json!({"oid":oid,"pid":pid,"serialId":item["serialId"],"iid":item["iid"],"bid":item["bid"],"item":item}));
        }
    }
    if matches.len() > 1 {
        return Err(
            "Найдено несколько заказов этой игры; выбери нужный ключ в Fanatical вручную.".into(),
        );
    }
    Ok(matches.pop())
}
pub fn item_key(item: &Value) -> Option<String> {
    let mut found = vec![];
    for name in ["key", "serial", "code", "serialKey"] {
        if let Some(value) = item[name]
            .as_str()
            .map(str::trim)
            .filter(|s| keys::valid_code(s))
        {
            found.push(value.to_owned());
        }
    }
    found.sort();
    found.dedup();
    if found.len() == 1 {
        found.pop()
    } else {
        None
    }
}
pub fn item_platform(item: &Value) -> Option<String> {
    let mut platforms = vec![];
    for name in ["drm", "platform", "platforms"] {
        if let Some(value) = item[name].as_str() {
            platforms.push(value.to_lowercase());
        }
        if let Some(list) = item[name].as_array() {
            platforms.extend(list.iter().filter_map(Value::as_str).map(str::to_lowercase));
        }
    }
    platforms.sort();
    platforms.dedup();
    if platforms.len() != 1 {
        return None;
    }
    match platforms[0].as_str() {
        "steam" => Some("steam".into()),
        "gog" | "gog.com" => Some("gog".into()),
        "microsoft" | "xbox" => Some("microsoft".into()),
        _ => None,
    }
}
pub async fn receive(
    browser: &mut Browser,
    account: &Account,
    offer: &Offer,
) -> Result<Option<Claimed>, String> {
    let result = browser.evaluate(ORDERS_SCRIPT, None).await?;
    if result["status"] != 200 {
        return Err("Fanatical не подтвердил доступ к заказам. Подключи магазин заново.".into());
    }
    let url = reqwest::Url::parse(&offer.url).map_err(|_| "Некорректная ссылка Fanatical.")?;
    let slug = url
        .path_segments()
        .and_then(|mut p| {
            while let Some(part) = p.next() {
                if matches!(part, "game" | "giveaway" | "bundle") {
                    return p.next().map(str::to_owned);
                }
            }
            None
        })
        .ok_or("Неизвестный товар Fanatical.")?;
    let Some(found) = order_item(&result["orders"], &slug, &offer.title)? else {
        return Ok(None);
    };
    let item = &found["item"];
    let key = if let Some(key) = item_key(item) {
        key
    } else {
        let mut payload = found.clone();
        payload.as_object_mut().unwrap().remove("item");
        if payload["serialId"].is_null() || payload["iid"].is_null() {
            return Err(
                "Fanatical не прислал идентификатор выдачи ключа. Открой заказ вручную.".into(),
            );
        }
        let script = REVEAL_SCRIPT.replace(
            "__PAYLOAD__",
            &serde_json::to_string(&payload).map_err(|_| "Не удалось подготовить выдачу ключа.")?,
        );
        let result = browser.evaluate(&script, None).await?;
        if result["status"] != 200 {
            return Err("Fanatical не подтвердил выдачу выбранного ключа.".into());
        }
        item_key(&result["data"])
            .or_else(|| {
                result["data"]
                    .as_str()
                    .filter(|s| keys::valid_code(s))
                    .map(str::to_owned)
            })
            .ok_or("Fanatical не выдал однозначный игровой ключ.")?
    };
    let account = account.clone();
    let offer = offer.clone();
    let target = item_platform(item);
    tokio::task::spawn_blocking(move || keys::receive(&account, &offer, key, target))
        .await
        .map_err(|_| "Не удалось сохранить ключ Fanatical.".to_string())??;
    Ok(Some(Claimed::CodeReceived))
}
const ORDERS_SCRIPT: &str = r#"(async()=>{
 if(!['https://www.fanatical.com','https://fanatical.com'].includes(location.origin))return {status:0};
 try{const auth=JSON.parse(localStorage.getItem('bsauth')||'{}');if(typeof auth.token!=='string'||!auth.token)return {status:401};
 const r=await fetch('/api/user/orders',{headers:{authorization:auth.token,accept:'application/json'},signal:AbortSignal.timeout(8000)});
 return {status:r.status,orders:r.ok?await r.json():null};}catch{return {status:0};}
})()"#;
const REVEAL_SCRIPT: &str = r#"(async()=>{
 if(!['https://www.fanatical.com','https://fanatical.com'].includes(location.origin))return {status:0};
 try{const auth=JSON.parse(localStorage.getItem('bsauth')||'{}');if(typeof auth.token!=='string'||!auth.token)return {status:401};
 const atok=JSON.parse(localStorage.getItem('bsatok')||'{}').value||'';
 const r=await fetch('/api/user/orders/redeem',{method:'POST',headers:{authorization:auth.token,'content-type':'application/json',accept:'application/json'},
 body:JSON.stringify({...__PAYLOAD__,atok}),signal:AbortSignal.timeout(8000)});
 return {status:r.status,data:r.ok?await r.json():null};}catch{return {status:0};}
})()"#;
