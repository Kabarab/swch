//! Steam Web API and Community inventory. Read-only requests, bounded pages, no credential logging.
use super::library::SteamGame;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct SteamClient {
    client: reqwest::Client,
    api: String,
    community: String,
}
impl Default for SteamClient {
    fn default() -> Self {
        Self::new("https://api.steampowered.com", "https://steamcommunity.com")
    }
}
impl SteamClient {
    pub fn new(api: &str, community: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("SWCH/0.1")
                .build()
                .expect("HTTP client"),
            api: api.trim_end_matches('/').into(),
            community: community.trim_end_matches('/').into(),
        }
    }
    async fn get(&self, url: String, query: &[(&str, String)]) -> Result<Value, String> {
        let response = self
            .client
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(|_| "Steam недоступен: проверь подключение и повтори запрос.".to_string())?;
        match response.status().as_u16() {
            401 | 403 => return Err(
                "Steam ограничил доступ: профиль или инвентарь закрыт, либо авторизация истекла."
                    .into(),
            ),
            429 => {
                return Err(
                    "Steam ограничил частоту запросов. Подожди немного перед обновлением.".into(),
                )
            }
            200 => (),
            status => {
                return Err(format!(
                    "Steam вернул ошибку HTTP {status}. Повтори запрос позже."
                ))
            }
        }
        response
            .json()
            .await
            .map_err(|_| "Steam вернул неподдерживаемый ответ.".into())
    }
    pub async fn owned(&self, id: &str, key: &str) -> Result<Vec<SteamGame>, String> {
        let value = self.get(format!("{}/IPlayerService/GetOwnedGames/v1/", self.api), &[("key",key.into()),("input_json",json!({"steamid":id,"include_appinfo":true,"include_played_free_games":true}).to_string())]).await?;
        parse_owned(&value, id)
    }
    pub async fn family(&self, id: &str, token: &str) -> Result<Vec<SteamGame>, String> {
        let group = self
            .get(
                format!(
                    "{}/IFamilyGroupsService/GetFamilyGroupForUser/v1/",
                    self.api
                ),
                &[
                    ("access_token", token.into()),
                    ("input_json", json!({"steamid":id}).to_string()),
                ],
            )
            .await?;
        if group["response"]["is_not_member_of_any_group"] == true {
            return Ok(vec![]);
        }
        let groupid = string_id(&group["response"]["family_groupid"])
            .filter(|s| s != "0")
            .ok_or(
                "Steam не предоставил семейную группу. Проверь авторизацию выбранного аккаунта.",
            )?;
        let value = self.get(format!("{}/IFamilyGroupsService/GetSharedLibraryApps/v1/",self.api), &[("access_token",token.into()),("input_json",json!({"family_groupid":groupid,"steamid":id,"include_own":true,"include_excluded":false,"language":"russian"}).to_string())]).await?;
        parse_family(&value, id)
    }
    pub async fn inventory(
        &self,
        id: &str,
        appid: &str,
        context: &str,
        cursor: Option<&str>,
    ) -> Result<InventoryPage, String> {
        let mut query = vec![("l", "russian".into()), ("count", "200".into())];
        if let Some(cursor) = cursor {
            query.push(("start_assetid", cursor.into()));
        }
        let value = self
            .get(
                format!("{}/inventory/{id}/{appid}/{context}", self.community),
                &query,
            )
            .await?;
        parse_inventory(&value)
    }
}
fn string_id(v: &Value) -> Option<String> {
    v.as_str()
        .map(String::from)
        .or_else(|| v.as_u64().map(|n| n.to_string()))
}
fn flag(v: &Value) -> bool {
    v == true || v.as_u64() == Some(1) || v.as_str() == Some("1")
}

pub fn parse_owned(value: &Value, id: &str) -> Result<Vec<SteamGame>, String> {
    let response = &value["response"];
    if response["game_count"].as_u64() == Some(0) {
        return Ok(vec![]);
    }
    let games = response["games"]
        .as_array()
        .ok_or("Steam не открыл список игр. Проверь видимость «Данные об играх» в профиле.")?;
    let mut out = Vec::new();
    for app in games {
        let appid = app["appid"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= u32::MAX as u64)
            .ok_or("Некорректный AppID в ответе Steam")?
            .to_string();
        let name = app["name"].as_str().unwrap_or("");
        if super::library::is_ignored_steam_app(&appid, name) {
            continue;
        }
        let mut game = SteamGame::new(&appid, Some(id.into()));
        game.game.name = if name.is_empty() { &game.game.name } else { name }.into();
        game.access = "owned".into();
        game.owner_steamids = vec![id.into()];
        game.source = "steam_api".into();
        game.playtime_minutes = app["playtime_forever"].as_u64();
        game.playtime_2weeks = app["playtime_2weeks"].as_u64();
        game.last_played = app["rtime_last_played"].as_u64();
        out.push(game);
    }
    Ok(out)
}
pub fn parse_family(value: &Value, id: &str) -> Result<Vec<SteamGame>, String> {
    let response = value
        .get("response")
        .filter(|v| v.is_object())
        .ok_or("Steam не предоставил семейную библиотеку.")?;
    if response.get("apps").is_none() && string_id(&response["owner_steamid"]).is_none() {
        return Err("Steam не предоставил семейную библиотеку. Проверь токен веб-сессии.".into());
    }
    let empty = vec![];
    let apps = response
        .get("apps")
        .map(|v| v.as_array().ok_or("Некорректная семейная библиотека Steam"))
        .transpose()?
        .unwrap_or(&empty);
    let mut out = Vec::new();
    for app in apps {
        if app["exclude_reason"].as_u64().unwrap_or(0) != 0 {
            continue;
        }
        let appid = app["appid"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= u32::MAX as u64)
            .ok_or("Некорректный AppID в семейной библиотеке")?
            .to_string();
        let name = app["name"].as_str().unwrap_or("");
        if super::library::is_ignored_steam_app(&appid, name) {
            continue;
        }
        let owners: Vec<_> = app["owner_steamids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(string_id)
            .collect();
        if owners.is_empty() {
            continue;
        }
        let mut g = SteamGame::new(&appid, Some(id.into()));
        g.game.name = app["name"].as_str().unwrap_or(&g.game.name).into();
        g.access = if owners.iter().any(|o| o == id) {
            "owned"
        } else {
            "family"
        }
        .into();
        g.owner_steamids = owners;
        g.source = "steam_family".into();
        // rt_playtime is not treated as this account's lifetime playtime.
        out.push(g);
    }
    Ok(out)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryItem {
    pub asset_id: String,
    pub amount: u64,
    pub name: String,
    pub market_hash_name: Option<String>,
    pub item_type: String,
    pub icon_url: Option<String>,
    pub tradable: bool,
    pub marketable: bool,
    pub tags: Vec<String>,
    #[serde(default)]
    pub tag_details: Vec<InventoryTag>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryTag {
    pub category: String,
    pub category_name: String,
    pub value: String,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryContext {
    pub app_id: String,
    pub context_id: String,
    pub name: String,
    pub count: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryPage {
    pub items: Vec<InventoryItem>,
    pub total_count: u64,
    pub next_cursor: Option<String>,
    pub updated_at: String,
    pub cached: bool,
    pub warning: Option<String>,
}
pub fn parse_inventory(value: &Value) -> Result<InventoryPage, String> {
    if !flag(&value["success"]) {
        return Err(
            "Инвентарь недоступен. Проверь его видимость и поддержку Steam Inventory этой игрой."
                .into(),
        );
    }
    let total = value["total_inventory_count"]
        .as_u64()
        .ok_or("Steam не сообщил размер инвентаря.")?;
    let descriptions = value["descriptions"].as_array();
    let mut items = Vec::new();
    if total > 0 && !value["assets"].is_array() {
        return Err("Steam вернул неполный инвентарь. Повтори запрос.".into());
    }
    for asset in value["assets"].as_array().into_iter().flatten() {
        let desc = descriptions
            .into_iter()
            .flatten()
            .find(|d| d["classid"] == asset["classid"] && d["instanceid"] == asset["instanceid"]);
        let asset_id =
            string_id(&asset["assetid"]).ok_or("Steam вернул предмет без идентификатора.")?;
        let amount = string_id(&asset["amount"])
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let name = desc
            .and_then(|d| d["name"].as_str())
            .unwrap_or("Предмет без описания")
            .into();
        let icon_url = desc
            .and_then(|d| d["icon_url"].as_str())
            .filter(|s| {
                s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-/".contains(&b))
            })
            .map(|s| {
                format!("https://community.akamai.steamstatic.com/economy/image/{s}/256fx256f")
            });
        items.push(InventoryItem {
            asset_id,
            amount,
            name,
            icon_url,
            market_hash_name: desc
                .and_then(|d| d["market_hash_name"].as_str())
                .map(String::from),
            item_type: desc.and_then(|d| d["type"].as_str()).unwrap_or("").into(),
            tradable: desc.is_some_and(|d| flag(&d["tradable"])),
            marketable: desc.is_some_and(|d| flag(&d["marketable"])),
            tag_details: desc
                .and_then(|d| d["tags"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|t| {
                    Some(InventoryTag {
                        category: t["category"].as_str()?.into(),
                        category_name: t["localized_category_name"]
                            .as_str()
                            .or(t["category"].as_str())?
                            .into(),
                        value: t["internal_name"].as_str()?.into(),
                        name: t["localized_tag_name"]
                            .as_str()
                            .or(t["internal_name"].as_str())?
                            .into(),
                    })
                })
                .collect(),
            tags: desc
                .and_then(|d| d["tags"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|t| t["localized_tag_name"].as_str())
                .map(String::from)
                .collect(),
        });
    }
    let next_cursor = if flag(&value["more_items"]) {
        Some(
            string_id(&value["last_assetid"])
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .ok_or("Steam не предоставил следующую страницу инвентаря.")?,
        )
    } else {
        None
    };
    Ok(InventoryPage {
        items,
        total_count: total,
        next_cursor,
        updated_at: chrono::Utc::now().to_rfc3339(),
        cached: false,
        warning: None,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceQuote {
    pub unit_price_cents: Option<u64>,
    pub currency: String,
    pub updated_at: String,
    pub cached: bool,
    pub warning: Option<String>,
}
/// Steam is explicitly queried in USD/English. Reject ambiguous/localized currencies.
pub fn usd_cents(text: &str) -> Option<u64> {
    let text = text.trim().strip_prefix('$')?.trim();
    let text = text.strip_suffix(" USD").unwrap_or(text);
    let mut parts = text.split('.');
    let whole = parts.next()?;
    let fraction = parts.next()?;
    if parts.next().is_some()
        || fraction.len() != 2
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let groups: Vec<_> = whole.split(',').collect();
    if groups
        .iter()
        .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
        || (groups.len() > 1 && (groups[0].len() > 3 || groups[1..].iter().any(|s| s.len() != 3)))
    {
        return None;
    }
    groups
        .concat()
        .parse::<u64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(fraction.parse().ok()?)
}
impl SteamClient {
    pub async fn price(&self, appid: &str, name: &str) -> Result<PriceQuote, String> {
        let value = self
            .get(
                format!("{}/market/priceoverview/", self.community),
                &[
                    ("appid", appid.into()),
                    ("market_hash_name", name.into()),
                    ("currency", "1".into()),
                    ("country", "US".into()),
                    ("l", "english".into()),
                ],
            )
            .await?;
        if !flag(&value["success"]) {
            return Err("Steam не предоставил цену предмета. Повтори оценку позже.".into());
        }
        let price = match value["lowest_price"].as_str() {
            Some(text) => Some(usd_cents(text).ok_or("Не удалось разобрать цену Steam в USD.")?),
            None => None,
        };
        Ok(PriceQuote {
            unit_price_cents: price,
            currency: "USD".into(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            cached: false,
            warning: None,
        })
    }
    pub async fn avatar(&self, id: &str) -> Result<String, String> {
        let response = self
            .client
            .get(format!("{}/profiles/{id}/", self.community))
            .query(&[("xml", "1")])
            .send()
            .await
            .map_err(|_| "Аватар Steam недоступен")?;
        if !response.status().is_success() {
            return Err("Аватар Steam недоступен".into());
        }
        let xml = response
            .text()
            .await
            .map_err(|_| "Аватар Steam недоступен")?;
        avatar_from_xml(&xml).ok_or("Steam не предоставил аватар".into())
    }
    pub async fn app_details(&self, appid: &str) -> Result<StoreAppDetails, String> {
        let value = self
            .get(
                "https://store.steampowered.com/api/appdetails".into(),
                &[
                    ("appids", appid.into()),
                    ("filters", "basic".into()),
                    ("l", "russian".into()),
                ],
            )
            .await?;
        parse_app_details(&value, appid)
    }
    pub async fn is_free(&self, appid: &str) -> Result<bool, String> {
        self.app_details(appid)
            .await
            .and_then(|d| d.is_free.ok_or_else(|| "Steam не предоставил сведения об игре".into()))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreAppDetails {
    pub name: Option<String>,
    pub is_free: Option<bool>,
}
pub fn parse_app_details(value: &Value, appid: &str) -> Result<StoreAppDetails, String> {
    let entry = &value[appid];
    if entry["success"] != true {
        return Err("Steam не предоставил сведения об игре".into());
    }
    let data = &entry["data"];
    let name = data["name"].as_str().map(String::from);
    let is_free = data["is_free"].as_bool();
    Ok(StoreAppDetails { name, is_free })
}
pub fn avatar_from_xml(xml: &str) -> Option<String> {
    let raw = xml
        .split_once("<avatarFull>")?
        .1
        .split_once("</avatarFull>")?
        .0
        .trim();
    let url = raw
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
        .unwrap_or(raw)
        .trim();
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if parsed.scheme() != "https"
        || !(host.ends_with(".steamstatic.com")
            || host.ends_with(".steamusercontent.com")
            || host == "steamcdn-a.akamaihd.net")
    {
        return None;
    }
    Some(parsed.to_string())
}

impl SteamClient {
    pub async fn inventory_apps(&self) -> Result<Vec<String>, String> {
        let response = self
            .client
            .get(format!("{}/market/", self.community))
            .send()
            .await
            .map_err(|_| "Список инвентарей Steam недоступен")?;
        if !response.status().is_success() {
            return Err("Список инвентарей Steam недоступен".into());
        }
        let html = response
            .text()
            .await
            .map_err(|_| "Список инвентарей Steam недоступен")?;
        inventory_apps_from_html(&html).ok_or("Steam изменил формат списка инвентарей".into())
    }
}
pub fn inventory_apps_from_html(html: &str) -> Option<Vec<String>> {
    let start = html.split_once("g_rgAppContextData = ")?.1;
    let value = serde_json::Deserializer::from_str(start)
        .into_iter::<Value>()
        .next()?
        .ok()?;
    let obj = value.as_object()?;
    Some(
        obj.keys()
            .filter(|s| super::library::valid_app_id(s) && !super::library::is_ignored_steam_app(s, ""))
            .cloned()
            .collect(),
    )
}

impl SteamClient {
    pub async fn inventory_contexts(&self, id: &str) -> Result<Vec<InventoryContext>, String> {
        let response = self
            .client
            .get(format!(
                "{}/profiles/{id}/inventory/?l=russian",
                self.community
            ))
            .send()
            .await
            .map_err(|_| "Не удалось получить список инвентарей аккаунта")?;
        if !response.status().is_success() {
            return Err("Список инвентарей аккаунта недоступен".into());
        }
        let html = response
            .text()
            .await
            .map_err(|_| "Список инвентарей аккаунта недоступен")?;
        inventory_contexts_from_html(&html)
            .ok_or("Steam не предоставил список инвентарей. Возможно, профиль закрыт.".into())
    }
}
pub fn inventory_contexts_from_html(html: &str) -> Option<Vec<InventoryContext>> {
    if html
        .split_once("g_bInventoryIsPrivate")
        .is_some_and(|(_, tail)| {
            tail.trim_start()
                .strip_prefix('=')
                .is_some_and(|v| v.trim_start().starts_with("true"))
        })
    {
        return None;
    }
    let tail = html
        .split_once("g_rgAppContextData")?
        .1
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    let value = serde_json::Deserializer::from_str(tail)
        .into_iter::<Value>()
        .next()?
        .ok()?;
    let mut result = vec![];
    for (app, data) in value.as_object()? {
        let name = data["name"].as_str().unwrap_or(app);
        if !super::library::valid_app_id(app) || super::library::is_ignored_steam_app(app, name) {
            continue;
        }
        for (context, detail) in data["rgContexts"].as_object()? {
            if !context.parse::<u64>().is_ok_and(|n| n > 0) {
                continue;
            }
            result.push(InventoryContext {
                app_id: app.clone(),
                context_id: context.clone(),
                name: data["name"].as_str().unwrap_or(app).into(),
                count: string_id(&detail["asset_count"]).and_then(|s| s.parse().ok()),
            });
        }
    }
    Some(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamePrice {
    pub app_id: String,
    pub currency: String,
    pub region: String,
    pub price_cents: Option<u64>,
    pub regular_price_cents: Option<u64>,
    pub is_free: Option<bool>,
    pub updated_at: String,
    pub cached: bool,
    pub warning: Option<String>,
}
pub fn parse_game_price(value: &Value, app_id: &str) -> Result<GamePrice, String> {
    parse_game_price_region(value, app_id, "US", Some("USD"))
}

pub fn parse_game_price_region(value: &Value, app_id: &str, region: &str, expected_currency: Option<&str>) -> Result<GamePrice, String> {
    let response = &value[app_id];
    if !response.is_object() {
        return Err("Steam вернул неполные сведения о цене игры".into());
    }
    let data = &response["data"];
    let available = response["success"] == true;
    let is_free = if available {
        data["is_free"].as_bool()
    } else {
        None
    };
    let price = &data["price_overview"];
    let raw_currency = price["currency"].as_str().unwrap_or("USD").to_uppercase();
    let currency_match = match expected_currency {
        Some(exp) => raw_currency.eq_ignore_ascii_case(exp),
        None => true,
    };
    let (current, regular, currency_str) = if is_free == Some(true) {
        (Some(0), Some(0), expected_currency.unwrap_or("USD").to_string())
    } else if available && currency_match && price["final"].as_u64().is_some() {
        (price["final"].as_u64(), price["initial"].as_u64(), raw_currency)
    } else {
        (None, None, raw_currency)
    };
    let region_upper = region.to_uppercase();
    Ok(GamePrice {
        app_id: app_id.into(),
        currency: currency_str,
        region: region_upper.clone(),
        price_cents: current,
        regular_price_cents: regular,
        is_free,
        updated_at: chrono::Utc::now().to_rfc3339(),
        cached: false,
        warning: if current.is_none() {
            Some(format!("Нет отдельной цены в регионе {region_upper}: игра может быть недоступна в этом регионе, снята с продажи или доступна только в комплекте."))
        } else {
            None
        },
    })
}
impl SteamClient {
    pub async fn game_price(&self, app_id: &str, cc: Option<&str>) -> Result<GamePrice, String> {
        let region = cc.unwrap_or("US").to_lowercase();
        let value = self
            .get(
                "https://store.steampowered.com/api/appdetails".into(),
                &[
                    ("appids", app_id.into()),
                    ("cc", region.clone().into()),
                    ("l", "english".into()),
                    ("filters", "basic,price_overview".into()),
                ],
            )
            .await?;
        parse_game_price_region(&value, app_id, &region, None)
    }
}
