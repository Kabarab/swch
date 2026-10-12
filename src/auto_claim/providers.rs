use super::{
    offers::{self, Offer},
    steam_auth::SteamSession,
};
use crate::{launchers::epic::EpicAuthStore, storage::Account};
use serde_json::Value;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

const EPIC_MOBILE_LAUNCHER_WARNING: &str =
    "Мобильные раздачи Epic требуют браузерного режима; сейчас включён Epic Launcher для PC.";

pub type Call<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claimed {
    Added,
    Owned,
    CodeReceived,
}
/// A bearer token proves OAuth ownership, but a browser also needs an SSO
/// cookie that is actually sent to the store subdomain. Old bootstrap sessions
/// saved the apex Domain as host-only and must be regenerated from the key.
pub fn epic_web_session_restorable(session: &super::web::Session, now: i64) -> bool {
    session.platform == "epic"
        && session.identity == session.account_id
        && ["EPIC_SSO", "EPIC_BEARER_TOKEN"].iter().all(|name| {
            session.cookies.iter().any(|c| {
                c.name == *name
                    && c.domain.eq_ignore_ascii_case(".epicgames.com")
                    && c.path == "/"
                    && c.secure
                    && !c.value.is_empty()
                    && c.expires.is_none_or(|expires| expires == -1.0 || expires > (now + 60) as f64)
            })
        })
}
pub fn epic_owns_offer(
    items: &[crate::launchers::epic::Entitlement],
    account_id: &str,
    offer: &Offer,
) -> bool {
    let namespace = offer
        .namespace
        .strip_prefix("mobile:")
        .and_then(|v| v.split_once(':').map(|(_, ns)| ns))
        .unwrap_or(&offer.namespace);
    !offer.item_ids.is_empty()
        && offer.item_ids.iter().all(|id| {
            items.iter().any(|e| {
                e.active
                    && e.account_id == account_id
                    && e.namespace == namespace
                    && e.catalog_item_id == *id
            })
        })
}

pub trait Provider: Send + Sync {
    fn begin_run<'a>(&'a self, _settings: &'a super::Settings) -> Call<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn warnings<'a>(&'a self) -> Call<'a, Vec<String>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn accounts<'a>(&'a self, accounts: Vec<Account>) -> Call<'a, Vec<Account>> {
        Box::pin(async move { Ok(accounts) })
    }
    fn country<'a>(&'a self, account: &'a Account) -> Call<'a, String>;
    fn discover<'a>(&'a self, launcher: &'a str, country: &'a str) -> Call<'a, Vec<Offer>>;
    fn discover_for<'a>(&'a self, account: &'a Account, country: &'a str) -> Call<'a, Vec<Offer>> {
        self.discover(&account.launcher, country)
    }
    fn claim<'a>(
        &'a self,
        account: &'a Account,
        offer: &'a Offer,
        country: &'a str,
    ) -> Call<'a, Claimed>;
}

pub struct Stores {
    http: reqwest::Client,
    epic: Arc<EpicAuthStore>,
    steam_base: String,
    steam_login_base: String,
    epic_promotions: String,
    catalog_cache:
        tokio::sync::Mutex<Option<Result<Vec<super::catalog::GiveawayCandidate>, String>>>,
    config: tokio::sync::RwLock<super::Settings>,
    region_accounts: tokio::sync::RwLock<std::collections::HashMap<String, Account>>,
    warnings: tokio::sync::Mutex<std::collections::BTreeSet<String>>,
    resolved_catalog:
        tokio::sync::Mutex<std::collections::HashMap<String, Option<(String, String)>>>,
    regular_prices: tokio::sync::Mutex<std::collections::HashMap<(String, String), Option<i64>>>,
    steamdb_cache: tokio::sync::Mutex<Option<Result<Vec<String>, String>>>,
}
impl Stores {
    pub fn new(epic: Arc<EpicAuthStore>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("SWCH/1.2 game-giveaways")
                .build()
                .expect("HTTP client"),
            epic,
            catalog_cache: tokio::sync::Mutex::new(None),
            config: tokio::sync::RwLock::new(super::Settings::default()),
            region_accounts: Default::default(),
            warnings: Default::default(),
            resolved_catalog: Default::default(),
            regular_prices: Default::default(),
            steamdb_cache: Default::default(),
            steam_base: "https://store.steampowered.com".into(),
            steam_login_base: "https://login.steampowered.com".into(),
            epic_promotions:
                "https://store-site-backend-static.ak.epicgames.com/freeGamesPromotions".into(),
        }
    }
    async fn get(
        &self,
        url: &str,
        query: &[(&str, String)],
        session: Option<&SteamSession>,
    ) -> Result<reqwest::Response, String> {
        let mut req = self.http.get(url).query(query);
        if let Some(session) = session {
            req = req.header(reqwest::header::COOKIE, session.cookies());
        }
        let response = req
            .send()
            .await
            .map_err(|_| "Магазин недоступен. Проверка будет повторена.".to_string())?;
        if !response.status().is_success() {
            if response.status().is_redirection()
                && std::env::var("SWCH_NATIVE_AUTH_DIAGNOSTICS").as_deref() == Ok("1")
            {
                if let Some(target) = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| response.url().join(v).ok())
                {
                    eprintln!(
                        "Steam redirect: {}{}",
                        target.origin().ascii_serialization(),
                        target.path()
                    );
                }
            }
            return Err(format!(
                "Магазин вернул HTTP {}. Возможно, нужен повторный вход или проверка в браузере.",
                response.status().as_u16()
            ));
        }
        Ok(response)
    }
    async fn regular_price(&self, title: &str, country: &str) -> Option<i64> {
        let name = super::catalog::canonical_game_title(title);
        let key = (country.to_owned(), name.clone());
        if let Some(value) = self.regular_prices.lock().await.get(&key) {
            return *value;
        }
        let result = async {
            let response = self
                .http
                .get(format!("{}/search/results/", self.steam_base))
                .query(&[
                    ("term", name.as_str()),
                    ("cc", country),
                    ("l", "english"),
                    ("count", "5"),
                    ("json", "1"),
                ])
                .timeout(Duration::from_secs(8))
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?;
            let data: Value = response.json().await.ok()?;
            let ids = data["results_html"]
                .as_str()?
                .split("<a ")
                .skip(1)
                .filter_map(|e| {
                    super::offers::attr(e.split('>').next().unwrap_or_default(), "data-ds-appid")
                })
                .filter(|id| crate::launchers::steam::library::valid_app_id(id))
                .take(5)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            for id in ids {
                let response = self
                    .http
                    .get(format!("{}/api/appdetails", self.steam_base))
                    .query(&[("appids", id.as_str()), ("cc", country), ("l", "english")])
                    .timeout(Duration::from_secs(8))
                    .send()
                    .await
                    .ok()?
                    .error_for_status()
                    .ok()?;
                let data: Value = response.json().await.ok()?;
                let game = &data[&id]["data"];
                if data[&id]["success"] == true
                    && game["type"] == "game"
                    && game["is_free"] == false
                    && game["name"]
                        .as_str()
                        .is_some_and(|s| super::catalog::canonical_game_title(s) == name)
                {
                    return game
                        .pointer("/price_overview/initial")
                        .and_then(Value::as_i64)
                        .filter(|p| *p > 0);
                }
            }
            None
        }
        .await;
        self.regular_prices.lock().await.insert(key, result);
        result
    }
    async fn candidate_offers(
        &self,
        platform: &str,
        country: &str,
        candidates: Vec<super::store_discovery::Candidate>,
    ) -> Vec<Offer> {
        use futures_util::{stream, StreamExt};
        stream::iter(candidates).map(|c|async move{
            let now=chrono::Utc::now().timestamp();
            if !c.ends_at.is_some_and(|e|e>now){self.warnings.lock().await.insert(format!("{platform}: у части полных игр не подтверждён конечный срок раздачи."));return None;}
            let price=match c.regular_price.filter(|p|*p>0){Some(p)=>Some(p),None=>self.regular_price(&c.title,country).await};
            let Some(price)=price else{self.warnings.lock().await.insert(format!("{platform}: у части игр не подтверждена обычная платная цена; оформление пропущено."));return None;};
            Some(Offer{launcher:platform.into(),id:c.id,namespace:format!("native:{platform}"),title:c.title,url:c.url,original_price:Some(price),normally_paid:true,final_price:0,ends_at:c.ends_at,item_ids:vec![]})
        }).buffer_unordered(4).filter_map(|v|async move{v}).collect().await
    }
    async fn discover_ubisoft(&self, country: &str) -> Result<Vec<Offer>, String> {
        let response = self
            .http
            .get("https://www.ubisoft.com/en-us/games/free")
            .timeout(Duration::from_secs(12))
            .send()
            .await
            .map_err(|_| "Каталог Ubisoft временно недоступен.".to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "Каталог Ubisoft ответил HTTP {}.",
                response.status().as_u16()
            ));
        }
        let html = response
            .text()
            .await
            .map_err(|_| "Не удалось прочитать каталог Ubisoft.".to_string())?;
        let candidates = super::store_discovery::ubisoft(&html, chrono::Utc::now().timestamp())?;
        Ok(self.candidate_offers("ubisoft", country, candidates).await)
    }
    async fn discover_steamdb(&self, country: &str) -> Result<Vec<Offer>, String> {
        let ids = {
            let mut cache = self.steamdb_cache.lock().await;
            if cache.is_none() {
                let result = async {
                    let r = self
                        .http
                        .get("https://steamdb.info/upcoming/free/")
                        .timeout(Duration::from_secs(8))
                        .send()
                        .await
                        .map_err(|_| "SteamDB временно недоступен.".to_string())?;
                    if !r.status().is_success() {
                        return Err(format!(
                            "SteamDB ответил HTTP {}; основной поиск Steam продолжается.",
                            r.status().as_u16()
                        ));
                    }
                    let html = r
                        .text()
                        .await
                        .map_err(|_| "SteamDB не прислал список.".to_string())?;
                    Ok(super::store_discovery::steamdb(&html))
                }
                .await;
                *cache = Some(result);
            }
            cache.as_ref().unwrap().clone()?
        };
        use futures_util::{stream, StreamExt};
        Ok(stream::iter(ids)
            .map(|id| async move { self.steam_app(&id, country).await.ok().flatten() })
            .buffer_unordered(4)
            .filter_map(|v| async move { v })
            .collect()
            .await)
    }
    async fn discover_prime(&self, account: &Account, country: &str) -> Result<Vec<Offer>, String> {
        let a = account.clone();
        let session =
            tokio::task::spawn_blocking(move || super::web::Session::load("prime", &a.id))
                .await
                .map_err(|_| "Не удалось прочитать вход Prime.".to_string())??;
        let config = self.config.read().await.clone();
        let mut browser = super::web::Browser::for_account(
            &account,
            config.browser_visible,
            &config.browser_name,
        )
        .await?;
        let result=async{
            browser.restore(&session).await?;browser.navigate("prime","https://luna.amazon.com/claims/home").await?;
            let state=super::browser_claim::snapshot(&mut browser,account,"",&session).await?;
            if state.challenge{return Err("Prime требует проверки безопасности в окне магазина.".into());}
            if !state.signed_in||state.identity.as_deref()!=Some(&session.identity){return Err("Подключи вход нужного аккаунта Prime.".into());}
            if state.requires_subscription{return Err("Для раздач Prime требуется уже действующая подписка.".into());}
            // Selecting the Game tab and scrolling only expose catalog entries.
            browser.evaluate("(()=>{const tab=document.querySelector('button[data-type=\"Game\"]');if(tab)tab.click();return true;})()",None).await?;
            let mut count=0;
            for _ in 0..16{
                let v=browser.evaluate("(()=>{const c=document.querySelector('[data-a-target=\"offer-list-FGWP_FULL\"]');if(!c)return 0;window.scrollTo(0,document.body.scrollHeight);c.scrollTop=c.scrollHeight;return c.children.length;})()",None).await?;
                tokio::time::sleep(Duration::from_millis(300)).await;
                let next=v.as_u64().unwrap_or_default();if next>0&&next==count{break;}count=next;
            }
            let cards=browser.evaluate(super::store_discovery::PRIME_CARDS_SCRIPT,None).await?;
            let mut candidates=super::store_discovery::prime_cards(&cards,chrono::Utc::now().timestamp())?;
            for candidate in &mut candidates{
                if candidate.ends_at.is_none(){browser.navigate("prime",&candidate.url).await?;
                    candidate.ends_at=browser.evaluate(super::store_discovery::PRIME_DETAIL_SCRIPT,None).await?.as_i64();}
            }
            Ok(self.candidate_offers("prime",country,candidates).await)
        }.await;
        browser.close().await;
        result
    }
    async fn microsoft_product(
        &self,
        url: &str,
        title: &str,
        country: &str,
    ) -> Result<super::store_discovery::MicrosoftProduct, String> {
        let id = super::store_discovery::microsoft_id(url)
            .ok_or("Неизвестный товар Microsoft Store.")?;
        let r = self
            .http
            .get(format!(
                "https://storeedgefd.dsx.mp.microsoft.com/v9.0/products/{id}"
            ))
            .query(&[
                ("market", country),
                ("locale", "en-us"),
                ("deviceFamily", "Windows.Xbox"),
            ])
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|_| "Microsoft Store временно недоступен.".to_string())?;
        if !r.status().is_success() {
            return Err(format!(
                "Microsoft Store ответил HTTP {}.",
                r.status().as_u16()
            ));
        }
        let data: Value = r
            .json()
            .await
            .map_err(|_| "Microsoft изменил формат товара.".to_string())?;
        super::store_discovery::microsoft_product(&data, &id, title)
    }
    async fn steam_session(&self, account: &Account) -> Result<SteamSession, String> {
        let id = account.id.clone();
        let cached = tokio::task::spawn_blocking(move || SteamSession::load(&id))
            .await
            .map_err(|_| "Не удалось прочитать вход Steam.".to_string())?;
        if let Ok(session) = cached {
            if self.verify_steam_session(&session).await.is_ok() {
                return Ok(session);
            }
            if session.refresh_token.is_some() {
                if let Ok(refreshed) = self.refresh_steam_session(session).await {
                    self.save_steam_session(refreshed.clone()).await?;
                    return Ok(refreshed);
                }
            }
        }
        self.native_steam_session(account).await
    }
    async fn save_steam_session(&self, session: SteamSession) -> Result<(), String> {
        tokio::task::spawn_blocking(move || session.save())
            .await
            .map_err(|_| "Не удалось сохранить обновлённый вход Steam.".to_string())??;
        Ok(())
    }
    async fn native_steam_session(&self, account: &Account) -> Result<SteamSession, String> {
        let connection = super::native_steam::connect(account).await?;
        let session = connection.session.clone();
        self.verify_steam_session(&session).await?;
        self.save_steam_session(session.clone()).await?;
        drop(connection);
        Ok(session)
    }
    pub async fn automatic_steam_connect(&self, account: &Account) -> Result<(), String> {
        self.native_steam_session(account).await.map(|_| ())
    }
    pub async fn redeem_steam_key(
        &self,
        account: &Account,
        title: &str,
        code: &str,
    ) -> Result<(), String> {
        if account.launcher != "steam" || !super::keys::valid_code(code) {
            return Err("Выбери аккаунт Steam и сохранённый игровой ключ.".into());
        }
        let session = self.steam_session(account).await?;
        self.verify_steam_session(&session).await?;
        self.redeem_steam_with_session(&session, title, code).await
    }
    async fn redeem_steam_with_session(
        &self,
        session: &SteamSession,
        title: &str,
        code: &str,
    ) -> Result<(), String> {
        // This dedicated activation endpoint receives only a product key and
        // CSRF token. It has no checkout, cart, wallet or payment parameters.
        let response=self.http.post(format!("{}/account/ajaxregisterkey/",self.steam_base))
            .header(reqwest::header::COOKIE,session.cookies()).header(reqwest::header::REFERER,format!("{}/account/registerkey",self.steam_base))
            .form(&[("product_key",code),("sessionid",session.session_id.as_str())]).send().await.map_err(|_|"Не удалось проверить результат активации ключа. Повторяй только для того же аккаунта.".to_string())?;
        if !response.status().is_success() {
            return Err("Steam отклонил активацию ключа. Проверь ограничения аккаунта.".into());
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| "Steam не прислал подтверждение активации.".to_string())?;
        let package = super::keys::steam_receipt_proof(&body, title)?;
        let offer = Offer {
            launcher: "steam".into(),
            id: package,
            namespace: String::new(),
            title: title.into(),
            url: String::new(),
            original_price: None,
            normally_paid: true,
            final_price: 0,
            ends_at: None,
            item_ids: vec![],
        };
        for n in 0..3 {
            if n > 0 {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            if self.owns_steam(session, &offer).await? {
                return Ok(());
            }
        }
        Err("Steam принял ключ, но ещё не подтвердил лицензию в библиотеке. Проверь выбранный аккаунт.".into())
    }
    pub async fn automatic_epic_web_connect(&self, account: &Account) -> Result<(), String> {
        let a = account.clone();
        if let Ok(Ok(session)) =
            tokio::task::spawn_blocking(move || super::web::Session::load("epic", &a.id)).await
        {
            if session.identity == account.id
                && epic_web_session_restorable(&session, chrono::Utc::now().timestamp())
                && self
                    .epic
                    .client()
                    .verify_web_identity(&session.cookies, &account.id)
                    .await
                    .is_ok()
            {
                return Ok(());
            }
        }
        let token = self
            .epic
            .get_valid_access_token(&account.id)
            .await
            .map_err(|_| "Повтори вход Epic для этого аккаунта.".to_string())?;
        let (_, cookies) = self
            .epic
            .client()
            .store_web_session(&token, &account.id)
            .await
            .map_err(epic_web_login_error)?;
        let details = self
            .epic
            .client()
            .fetch_account_details(&token, &account.id)
            .await
            .map_err(|_| "Не удалось проверить Epic-аккаунт.".to_string())?;
        if details.id != account.id {
            return Err("Epic подтвердил другой аккаунт.".into());
        }
        let session = super::web::Session {
            platform: "epic".into(),
            account_id: account.id.clone(),
            identity: account.id.clone(),
            country: details
                .country
                .ok_or("Epic не сообщил регион аккаунта.")?
                .to_ascii_uppercase(),
            cookies,
            connected_at: chrono::Utc::now().to_rfc3339(),
            identity_proof: "store_api".into(),
            store_identity: None,
            web_storage: vec![],
        };
        tokio::task::spawn_blocking(move || session.save())
            .await
            .map_err(|_| "Не удалось сохранить Epic-вход.".to_string())??;
        Ok(())
    }
    /// Read-only ownership proof for a manual browser recovery. No checkout or browser login.
    pub async fn epic_library_status(
        &self,
        account: &Account,
    ) -> Result<Vec<(Offer, bool)>, String> {
        if account.launcher != "epic" {
            return Err("Проверка доступна для Epic Games.".into());
        }
        let country = self.country(account).await?;
        let offers = self.discover_epic(&country).await?;
        let token = self
            .epic
            .get_valid_access_token(&account.id)
            .await
            .map_err(|_| "Повтори вход Epic в SWCH для этого аккаунта.")?;
        let items = self
            .epic
            .client()
            .fetch_all_entitlements(&token, &account.id)
            .await
            .map_err(|_| "Не удалось проверить библиотеку Epic. Попробуй позже.")?;
        Ok(offers
            .into_iter()
            .filter(|o| o.eligible(chrono::Utc::now().timestamp()))
            .map(|o| {
                let owned = epic_owns_offer(&items, &account.id, &o);
                (o, owned)
            })
            .collect())
    }
    async fn extra_candidates(
        &self,
        platform: &str,
    ) -> Result<Vec<super::catalog::GiveawayCandidate>, String> {
        let mut cached = self.catalog_cache.lock().await;
        if cached.is_none() {
            *cached = Some(
                async {
                    let response = self
                        .http
                        .get("https://www.gamerpower.com/api/giveaways")
                        .send()
                        .await
                        .map_err(|_| "Дополнительный каталог GamerPower недоступен.".to_string())?;
                    if !response.status().is_success() {
                        return Err("GamerPower не прислал список раздач.".into());
                    }
                    let body = response
                        .json()
                        .await
                        .map_err(|_| "GamerPower вернул неизвестный список раздач.".to_string())?;
                    super::catalog::gamerpower_candidates(&body, chrono::Utc::now().timestamp())
                }
                .await,
            );
        }
        Ok(cached
            .as_ref()
            .unwrap()
            .as_ref()
            .map_err(Clone::clone)?
            .iter()
            .filter(|c| platform.is_empty() || c.platform == platform)
            .cloned()
            .collect())
    }
    pub async fn additional_catalog(
        &self,
    ) -> Result<Vec<super::catalog::GiveawayCandidate>, String> {
        *self.catalog_cache.lock().await = None;
        self.extra_candidates("").await
    }
    async fn resolve_candidate(
        &self,
        candidate: &super::catalog::GiveawayCandidate,
    ) -> Result<Option<(String, String)>, String> {
        if let Some(value) = self
            .resolved_catalog
            .lock()
            .await
            .get(&candidate.source_id)
            .cloned()
        {
            return Ok(value);
        }
        let result = match self.resolve_uncached(candidate).await {
            Ok(value) => value,
            Err(error) => {
                self.warnings
                    .lock()
                    .await
                    .insert(format!("Дополнительная ссылка GamerPower: {error}"));
                None
            }
        };
        self.resolved_catalog
            .lock()
            .await
            .insert(candidate.source_id.clone(), result.clone());
        Ok(result)
    }
    async fn resolve_uncached(
        &self,
        candidate: &super::catalog::GiveawayCandidate,
    ) -> Result<Option<(String, String)>, String> {
        if let Some(url) = &candidate.claim_url {
            return Ok(super::platforms::product_url(&candidate.platform, url)
                .then(|| (candidate.platform.clone(), url.clone())));
        }
        let mut url = reqwest::Url::parse(&candidate.source_url)
            .map_err(|_| "Некорректная ссылка каталога.".to_string())?;
        for _ in 0..5 {
            if let Some((platform, valid)) = super::catalog::trusted_claim_url(url.as_str()) {
                return Ok(
                    super::platforms::product_url(&platform, &valid).then_some((platform, valid))
                );
            }
            if url.scheme() != "https"
                || !matches!(
                    url.host_str(),
                    Some("www.gamerpower.com" | "gamerpower.com")
                )
                || url.port().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Ok(None);
            }
            let response = self
                .http
                .get(url.clone())
                .send()
                .await
                .map_err(|_| "Не удалось прочитать ссылку раздачи.".to_string())?;
            if !response.status().is_redirection() {
                if response.status().is_success() {
                    let html = response
                        .text()
                        .await
                        .map_err(|_| "Не удалось прочитать перенаправление раздачи.")?;
                    if let Some(url) = super::catalog::html_redirect(&html) {
                        return Ok(super::catalog::trusted_claim_url(&url));
                    }
                }
                return Ok(None);
            }
            let Some(next) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| url.join(v).ok())
            else {
                return Ok(None);
            };
            url = next;
        }
        Ok(None)
    }

    async fn refresh_steam_session(
        &self,
        mut session: SteamSession,
    ) -> Result<SteamSession, String> {
        let refresh = session
            .refresh_token
            .as_deref()
            .ok_or("Вход Steam истёк. Подключи магазин заново с включённым «Запомнить меня».")?;
        let response = self
            .http
            .post(format!("{}/jwt/finalizelogin", self.steam_login_base))
            .header(reqwest::header::ORIGIN, "https://store.steampowered.com")
            .header(reqwest::header::REFERER, "https://store.steampowered.com/")
            .form(&[
                ("nonce", refresh),
                ("sessionid", &session.session_id),
                ("redir", "https://store.steampowered.com/"),
            ])
            .send()
            .await
            .map_err(|_| "Не удалось обновить вход Steam. Проверка будет повторена.".to_string())?;
        if !response.status().is_success() {
            return Err("Steam отклонил обновление входа. Подключи магазин заново.".into());
        }
        let rotated = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|v| {
                v.strip_prefix("steamRefresh_steam=")
                    .and_then(|v| v.split(';').next())
            })
            .and_then(|v| v.split_once("%7C%7C").or_else(|| v.split_once("||")))
            .filter(|(id, _)| *id == session.account_id)
            .map(|(_, token)| token.to_string());
        let data: Value = response
            .json()
            .await
            .map_err(|_| "Steam вернул неизвестный формат обновления входа.".to_string())?;
        if data["steamID"].as_str() != Some(&session.account_id)
            || data["error"].as_i64().is_some_and(|v| v != 0)
        {
            return Err(
                "Steam не подтвердил аккаунт при обновлении входа. Подключи его заново.".into(),
            );
        }
        let transfer = data["transfer_info"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| {
                t["url"].as_str().is_some_and(|url| {
                    reqwest::Url::parse(url).is_ok_and(|u| {
                        u.origin().ascii_serialization() == self.steam_base
                            && u.path().trim_end_matches('/') == "/login/settoken"
                    })
                })
            })
            .ok_or("Steam не прислал разрешение на вход в магазин.")?;
        let nonce = transfer["params"]["nonce"]
            .as_str()
            .ok_or("Steam не прислал параметры входа.")?;
        let auth = transfer["params"]["auth"]
            .as_str()
            .ok_or("Steam не прислал параметры входа.")?;
        let response = self
            .http
            .post(format!("{}/login/settoken", self.steam_base))
            .form(&[
                ("steamID", session.account_id.as_str()),
                ("nonce", nonce),
                ("auth", auth),
            ])
            .send()
            .await
            .map_err(|_| "Не удалось обновить вход магазина Steam.".to_string())?;
        if !response.status().is_success() {
            return Err("Steam отклонил обновление сессии магазина.".into());
        }
        let cookie = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|v| {
                v.strip_prefix("steamLoginSecure=")
                    .and_then(|v| v.split(';').next())
            })
            .ok_or("Steam не выдал новую сессию магазина.")?
            .to_string();
        session.login_secure = cookie;
        if let Some(rotated) = rotated {
            session.refresh_token = Some(rotated);
        }
        session.validate()?;
        self.verify_steam_session(&session).await?;
        Ok(session)
    }
    pub async fn verify_steam_session(&self, session: &SteamSession) -> Result<(), String> {
        session.validate()?;
        let html = self
            .get(
                &format!("{}/account/", self.steam_base),
                &[("l", "english".into())],
                Some(session),
            )
            .await?
            .text()
            .await
            .map_err(|_| "Не удалось проверить вход Steam.".to_string())?;
        // Verify the server's identity, rather than trusting the cookie prefix.
        let id = store_identity(&html);
        if std::env::var("SWCH_NATIVE_AUTH_DIAGNOSTICS").as_deref() == Ok("1") {
            eprintln!(
                "{}",
                serde_json::json!({"store_metadata":{"identity_found":id.is_some(),"identity_matches":id.as_deref()==Some(&session.account_id),"identity_occurrences":html.matches("g_steamID").count(),"steamid_keys":html.to_ascii_lowercase().matches("steamid").count(),"expected_id_mentions":html.matches(&session.account_id).count(),"profile_url":html.contains(&format!("steamcommunity.com/profiles/{}",session.account_id)),"account_details":html.contains("account_header_line") || html.contains("Account details"),"login_form":html.contains("login_form") || html.contains("newlogindialog"),"html_bytes":html.len()}})
            );
        }
        if id.as_deref() != Some(&session.account_id) {
            return Err(
                "Вход в магазин Steam истёк или выполнен в другой аккаунт. Подключи его заново."
                    .into(),
            );
        }
        Ok(())
    }
    async fn steam_app(&self, app_id: &str, country: &str) -> Result<Option<Offer>, String> {
        let body: Value = self
            .get(
                &format!("{}/api/appdetails", self.steam_base),
                &[
                    ("appids", app_id.into()),
                    ("cc", country.into()),
                    ("l", "english".into()),
                ],
                None,
            )
            .await?
            .json()
            .await
            .map_err(|_| "Steam вернул неизвестный формат игры.".to_string())?;
        if body[app_id]["success"].as_bool() != Some(true) {
            return Ok(None);
        }
        let html = self
            .get(
                &format!("{}/app/{app_id}/", self.steam_base),
                &[("cc", country.into()), ("l", "english".into())],
                None,
            )
            .await?
            .text()
            .await
            .map_err(|_| "Не удалось прочитать акцию Steam.".to_string())?;
        Ok(offers::steam_offer(
            app_id,
            &body[app_id]["data"],
            &html,
            chrono::Utc::now().timestamp(),
        ))
    }
    async fn discover_steam(&self, country: &str) -> Result<Vec<Offer>, String> {
        let mut start = 0;
        let mut ids = std::collections::BTreeSet::new();
        loop {
            let body: Value = self
                .get(
                    &format!("{}/search/results/", self.steam_base),
                    &[
                        ("maxprice", "free".into()),
                        ("specials", "1".into()),
                        ("category1", "998".into()),
                        ("json", "1".into()),
                        ("count", "100".into()),
                        ("start", start.to_string()),
                        ("cc", country.into()),
                        ("l", "english".into()),
                        ("infinite", "1".into()),
                    ],
                    None,
                )
                .await?
                .json()
                .await
                .map_err(|_| "Steam вернул неизвестный формат поиска раздач.".to_string())?;
            let html = body["results_html"]
                .as_str()
                .ok_or("Steam не прислал результаты поиска.")?;
            ids.extend(offers::search_app_ids(html));
            let total = body["total_count"]
                .as_u64()
                .ok_or("Steam не прислал количество результатов.")?;
            start += 100;
            if start >= total {
                break;
            }
            if html.trim().is_empty() || start > 100_000 {
                return Err(
                    "Steam прислал неполный список раздач; проверка будет повторена.".into(),
                );
            }
        }
        let mut result = vec![];
        for id in ids {
            if let Some(offer) = self.steam_app(&id, country).await? {
                result.push(offer);
            }
        }
        Ok(result)
    }
    async fn discover_epic(&self, country: &str) -> Result<Vec<Offer>, String> {
        let body = self
            .get(
                &self.epic_promotions,
                &[
                    ("country", country.into()),
                    ("allowCountries", country.into()),
                    ("locale", "en-US".into()),
                ],
                None,
            )
            .await?
            .json()
            .await
            .map_err(|_| "Epic вернул неизвестный формат раздач.".to_string())?;
        offers::epic_offers(&body, chrono::Utc::now().timestamp())
    }
    async fn discover_epic_mobile(&self, country: &str) -> Result<Vec<Offer>, String> {
        if self.config.read().await.epic_launcher_claims {
            return Err(EPIC_MOBILE_LAUNCHER_WARNING.into());
        }
        let account = self
            .region_accounts
            .read()
            .await
            .get(country)
            .cloned()
            .ok_or("Нет подтверждённого аккаунта для региона мобильных раздач Epic.")?;
        self.automatic_epic_web_connect(&account).await?;
        let a = account.clone();
        let session = tokio::task::spawn_blocking(move || super::web::Session::load("epic", &a.id))
            .await
            .map_err(|_| "Не удалось прочитать вход мобильного Epic.".to_string())??;
        let config = self.config.read().await.clone();
        let mut browser = super::web::Browser::for_account(
            &account,
            config.browser_visible,
            &config.browser_name,
        )
        .await?;
        let result = self
            .mobile_in_browser(&mut browser, &session, country)
            .await;
        browser.close().await;
        result
    }
    async fn mobile_in_browser(
        &self,
        browser: &mut super::web::Browser,
        session: &super::web::Session,
        country: &str,
    ) -> Result<Vec<Offer>, String> {
        browser.restore(session).await?;
        browser
            .navigate("epic", "https://store.epicgames.com/en-US/free-games")
            .await?;
        let mut output = vec![];
        for platform in ["android", "ios"] {
            let url=format!("https://egs-platform-service.store.epicgames.com/api/v2/public/discover/home?count=100&country={country}&locale=en&platform={platform}&start=0&store=EGS");
            let config = serde_json::json!(url).to_string();
            let script=format!("(async()=>{{if(/please complete a security check|verify you are human|checking your browser/i.test(document.body.innerText)||/just a moment/i.test(document.title))return {{blocked:true}};const r=await fetch({config},{{credentials:'include'}});if(!r.ok)return {{status:r.status}};try{{return {{data:await r.json()}}}}catch{{return {{invalid:true}}}}}})()");
            let response = browser.evaluate(&script, None).await?;
            if response["blocked"] == true || response["status"].as_u64() == Some(403) {
                return Err("Epic Mobile требует проверки безопасности в окне магазина.".into());
            }
            let offers =
                super::mobile::offers(&response["data"], platform, chrono::Utc::now().timestamp())?;
            if offers.is_empty()
                && response["data"]["data"].as_array().is_some_and(|v| {
                    v.iter().any(|s| {
                        s["type"] == "freeGame"
                            && s["offers"].as_array().is_some_and(|a| !a.is_empty())
                    })
                })
            {
                self.warnings.lock().await.insert(format!("Epic Mobile ({platform}): найденные предложения не подтвердили обычную цену, срок раздачи или отдельную лицензию; получение пропущено."));
            }
            output.extend(offers);
        }
        Ok(output)
    }
    async fn owns_steam(&self, session: &SteamSession, offer: &Offer) -> Result<bool, String> {
        let data: Value = self
            .get(
                &format!("{}/dynamicstore/userdata/", self.steam_base),
                &[("t", chrono::Utc::now().timestamp().to_string())],
                Some(session),
            )
            .await?
            .json()
            .await
            .map_err(|_| "Не удалось проверить лицензию Steam.".to_string())?;
        let packages = data["rgOwnedPackages"].as_array();
        Ok(packages.is_some_and(|p| {
            p.iter()
                .any(|v| v.as_u64().map(|n| n.to_string()).as_deref() == Some(&offer.id))
        }))
    }
    async fn claim_steam(
        &self,
        account: &Account,
        offer: &Offer,
        country: &str,
    ) -> Result<Claimed, String> {
        let session = self.steam_session(account).await?;
        if self.owns_steam(&session, offer).await? {
            return Ok(Claimed::Owned);
        }
        let app = offer.item_ids.first().ok_or("Нет AppID акции Steam.")?;
        let current = self
            .steam_app(app, country)
            .await?
            .filter(|v| v.id == offer.id)
            .ok_or("Раздача Steam закончилась или её условия изменились.")?;
        if !current.eligible(chrono::Utc::now().timestamp()) {
            return Err("Раздача уже закончилась.".into());
        }
        // Valve's complimentary license endpoint cannot charge a wallet/card.
        let response = self
            .http
            .post(format!("{}/freelicense/addfreelicense", self.steam_base))
            .header(reqwest::header::COOKIE, session.cookies())
            .header(reqwest::header::REFERER, &offer.url)
            .form(&[
                ("action", "add_to_cart"),
                ("sessionid", &session.session_id),
                ("subid", &offer.id),
            ])
            .send()
            .await
            .map_err(|_| {
                "Не удалось получить лицензию Steam; результат будет проверен при повторе."
                    .to_string()
            })?;
        if !response.status().is_success() {
            return Err("Steam отклонил получение. Проверь вход и ограничения аккаунта.".into());
        }
        let html = response
            .text()
            .await
            .map_err(|_| "Не удалось прочитать результат Steam.".to_string())?;
        if html.contains("<h2>Success!</h2>") || self.owns_steam(&session, offer).await? {
            return Ok(Claimed::Added);
        }
        Err("Steam не подтвердил получение игры. Возможно, требуется проверка в магазине.".into())
    }
    async fn claim_epic(
        &self,
        account: &Account,
        offer: &Offer,
        country: &str,
    ) -> Result<Claimed, String> {
        let token = self
            .epic
            .get_valid_access_token(&account.id)
            .await
            .map_err(|_| "Повтори вход Epic Games для этого аккаунта.".to_string())?;
        let namespace = offer
            .namespace
            .strip_prefix("mobile:")
            .and_then(|v| v.split_once(':').map(|(_, ns)| ns))
            .unwrap_or(&offer.namespace);
        let owns = |items: &[crate::launchers::epic::Entitlement]| {
            epic_owns_offer(items, &account.id, offer)
        };
        let before = self
            .epic
            .client()
            .fetch_all_entitlements(&token, &account.id)
            .await
            .map_err(|_| "Не удалось проверить библиотеку Epic.".to_string())?;
        if owns(&before) {
            return Ok(Claimed::Owned);
        }
        let fresh = if offer.namespace.starts_with("mobile:") {
            self.discover_epic_mobile(country).await?
        } else {
            self.discover_epic(country).await?
        }
        .into_iter()
        .find(|v| v.key() == offer.key())
        .ok_or("Раздача Epic закончилась или стала недоступна в регионе аккаунта.")?;
        if !fresh.eligible(chrono::Utc::now().timestamp()) {
            return Err("Раздача уже закончилась.".into());
        }
        let api_result = self
            .epic
            .client()
            .claim_free_offer(&token, &account.id, namespace, &fresh.id, country)
            .await;
        let mut browser_error = None;
        if let Err(error) = api_result {
            let config = self.config.read().await.clone();
            let can_use_browser = matches!(&error,crate::launchers::epic::EpicError::Api{status,..} if matches!(status.as_u16(),403|404))
                || matches!(&error,crate::launchers::epic::EpicError::Auth(message) if message=="Epic checkout needs browser verification");
            if !can_use_browser {
                return Err(epic_claim_error(error));
            }
            if config.epic_launcher_claims && !fresh.namespace.starts_with("mobile:") {
                if let Err(e) = super::native_epic::claim(
                    &self.epic,
                    account,
                    &fresh,
                    config.browser_operation_timeout_seconds,
                )
                .await
                {
                    browser_error = Some(e);
                }
            } else {
                if !config.browser_enabled {
                    return Err(epic_claim_error(error));
                }
                self.automatic_epic_web_connect(account).await?;
                // Browser confirmation is followed by an independent entitlement
                // check below. A DOM success message alone does not prove a license.
                if let Err(error) = super::browser_claim::claim(
                    account,
                    &fresh,
                    config.browser_operation_timeout_seconds,
                    config.browser_visible,
                    &config.browser_name,
                )
                .await
                {
                    browser_error = Some(error);
                }
            }
        }
        // An HTTP 200 (including REJECTED/CHECKOUT) is never counted as ownership.
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            let after = self
                .epic
                .client()
                .fetch_all_entitlements(&token, &account.id)
                .await
                .map_err(|_| {
                    "Не удалось подтвердить лицензию Epic; проверим при повторе.".to_string()
                })?;
            if owns(&after) {
                if self.epic.sync_library(&account.id, None).await.is_err() {
                    tracing::warn!("Epic giveaway claimed, library sync deferred");
                }
                return Ok(Claimed::Added);
            }
        }
        Err(browser_error.unwrap_or_else(|| {
            "Epic ещё не выдал лицензию. Получение будет проверено при следующем запуске.".into()
        }))
    }
}
fn epic_web_login_error(error: crate::launchers::epic::EpicError) -> String {
    use crate::launchers::epic::EpicError;
    match error {
        EpicError::Api { status, .. } => format!(
            "Epic отклонил автоматический вход в магазин (HTTP {}). Получение игры ещё не запускалось.",
            status.as_u16()
        ),
        EpicError::Reqwest(_) => "Не удалось связаться с сервисом входа Epic. Получение игры ещё не запускалось.".into(),
        _ => "Epic не подтвердил перенос входа в магазин для этого аккаунта. Получение игры ещё не запускалось.".into(),
    }
}
fn epic_claim_error(error: crate::launchers::epic::EpicError) -> String {
    use crate::launchers::epic::EpicError;
    match error {
        EpicError::Api { status, .. } => format!(
            "Epic отклонил оформление раздачи (HTTP {}). Открой её в магазине для проверки.",
            status.as_u16()
        ),
        EpicError::Reqwest(_) => {
            "Сервис оформления Epic недоступен. Проверка будет повторена.".into()
        }
        EpicError::Serialization(_) => {
            "Epic вернул неизвестный формат оформления; лицензия не подтверждена.".into()
        }
        EpicError::Auth(message)
            if message == "Epic checkout is not a zero-price order for this offer" =>
        {
            "Epic не подтвердил нулевую цену, игру или регион; заказ остановлен.".into()
        }
        _ => "Epic не подтвердил получение. Открой раздачу в магазине для проверки.".into(),
    }
}
fn store_identity(html: &str) -> Option<String> {
    let script_identity = html
        .to_ascii_lowercase()
        .match_indices("g_steamid")
        .find_map(|(offset, _)| {
            let value = html[offset + "g_steamID".len()..]
                .trim_start()
                .strip_prefix('=')?
                .trim_start();
            let quote = value.chars().next()?;
            if !matches!(quote, '\'' | '"') {
                return None;
            }
            let id = value[1..].split(quote).next()?;
            crate::launchers::steam::library::valid_steam_id(id).then(|| id.to_string())
        });
    script_identity.or_else(|| {
        // Current account pages render the authenticated user's profile link.
        html.split("steamcommunity.com/profiles/")
            .skip(1)
            .find_map(|tail| {
                let id = tail
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>();
                crate::launchers::steam::library::valid_steam_id(&id).then_some(id)
            })
    })
}
impl Provider for Stores {
    fn begin_run<'a>(&'a self, settings: &'a super::Settings) -> Call<'a, ()> {
        Box::pin(async move {
            *self.config.write().await = settings.clone();
            *self.catalog_cache.lock().await = None;
            self.region_accounts.write().await.clear();
            self.warnings.lock().await.clear();
            self.resolved_catalog.lock().await.clear();
            self.regular_prices.lock().await.clear();
            *self.steamdb_cache.lock().await = None;
            Ok(())
        })
    }
    fn warnings<'a>(&'a self) -> Call<'a, Vec<String>> {
        Box::pin(async { Ok(self.warnings.lock().await.iter().cloned().collect()) })
    }
    fn accounts<'a>(&'a self, accounts: Vec<Account>) -> Call<'a, Vec<Account>> {
        Box::pin(async move {
            let local = tokio::task::spawn_blocking(super::native_steam::accounts)
                .await
                .map_err(|_| "Не удалось прочитать аккаунты Steam.".to_string())?;
            let mut accounts = super::native_steam::merge_accounts(accounts, local);
            if let Some(raw) = self
                .epic
                .database()
                .auto_claim_value("web_accounts")
                .await
                .map_err(|_| "Не удалось прочитать подключённые магазины.".to_string())?
            {
                let extra: Vec<Account> = serde_json::from_str(&raw)
                    .map_err(|_| "Не удалось прочитать подключённые магазины.".to_string())?;
                for a in extra {
                    if !accounts
                        .iter()
                        .any(|b| b.id == a.id && b.launcher == a.launcher)
                    {
                        accounts.push(a);
                    }
                }
            }
            Ok(accounts)
        })
    }
    fn country<'a>(&'a self, account: &'a Account) -> Call<'a, String> {
        Box::pin(async move {
            let country = if account.launcher == "steam" {
                let session = self.steam_session(account).await?;
                session.country
            } else if account.launcher == "epic" {
                let token = self
                    .epic
                    .get_valid_access_token(&account.id)
                    .await
                    .map_err(|_| "Повтори вход Epic Games для этого аккаунта.".to_string())?;
                let details = self
                    .epic
                    .client()
                    .fetch_account_details(&token, &account.id)
                    .await
                    .map_err(|_| "Не удалось определить регион Epic Games.".to_string())?;
                if details.id != account.id {
                    return Err("Epic вернул другой аккаунт.".into());
                }
                details
                    .country
                    .ok_or("Epic не сообщил регион аккаунта.")?
                    .to_ascii_uppercase()
            } else if super::platforms::platform(&account.launcher).is_some() {
                if account.launcher == "alienware" {
                    return Ok("ZZ".into());
                }
                let account = account.clone();
                return tokio::task::spawn_blocking(move || {
                    super::web::Session::load(&account.launcher, &account.id).map(|s| s.country)
                })
                .await
                .map_err(|_| "Не удалось прочитать браузерный вход.".to_string())?;
            } else {
                return Err("Эта платформа не поддерживает игровой автоклеймер.".into());
            };
            if country.len() != 2 || !country.bytes().all(|b| b.is_ascii_uppercase()) {
                return Err("Некорректный регион аккаунта.".into());
            }
            if account.launcher == "epic" {
                self.region_accounts
                    .write()
                    .await
                    .entry(country.clone())
                    .or_insert_with(|| account.clone());
            }
            Ok(country)
        })
    }
    fn discover_for<'a>(&'a self, account: &'a Account, country: &'a str) -> Call<'a, Vec<Offer>> {
        Box::pin(async move {
            if account.launcher != "prime" || !self.config.read().await.browser_enabled {
                return self.discover(&account.launcher, country).await;
            }
            let native = self.discover_prime(account, country).await;
            let extra = self.discover("prime", country).await;
            let mut offers = vec![];
            let mut last_error = None;
            for result in [native, extra] {
                match result {
                    Ok(found) => {
                        for offer in found {
                            if !offers.iter().any(|v: &Offer| v.url == offer.url) {
                                offers.push(offer);
                            }
                        }
                    }
                    Err(error) => {
                        self.warnings.lock().await.insert(error.clone());
                        last_error = Some(error);
                    }
                }
            }
            if offers.is_empty() {
                if let Some(error) = last_error {
                    return Err(error);
                }
            }
            Ok(offers)
        })
    }
    fn discover<'a>(&'a self, launcher: &'a str, country: &'a str) -> Call<'a, Vec<Offer>> {
        Box::pin(async move {
            let mut offers = match launcher {
                "steam" => self.discover_steam(country).await?,
                "epic" => self.discover_epic(country).await?,
                id if super::platforms::platform(id).is_some() => vec![],
                _ => return Err("Неизвестная платформа раздачи.".into()),
            };
            let settings = self.config.read().await.clone();
            if launcher == "steam" && settings.steamdb {
                match self.discover_steamdb(country).await {
                    Ok(extra) => {
                        for o in extra {
                            if !offers.iter().any(|v| v.key() == o.key()) {
                                offers.push(o);
                            }
                        }
                    }
                    Err(e) => {
                        self.warnings.lock().await.insert(e);
                    }
                }
            }
            if launcher == "ubisoft" {
                match self.discover_ubisoft(country).await {
                    Ok(native) => offers.extend(native),
                    Err(e) => {
                        self.warnings.lock().await.insert(e);
                    }
                }
            }
            if launcher == "epic" && settings.epic_mobile {
                if settings.epic_launcher_claims {
                    self.warnings
                        .lock()
                        .await
                        .insert(EPIC_MOBILE_LAUNCHER_WARNING.into());
                } else if settings.browser_enabled {
                    match self.discover_epic_mobile(country).await {
                        Ok(mobile) => offers.extend(mobile),
                        Err(e) => {
                            self.warnings
                                .lock()
                                .await
                                .insert(format!("Дополнительная проверка Epic Mobile: {e}"));
                        }
                    }
                } else {
                    self.warnings
                        .lock()
                        .await
                        .insert("Epic Mobile пропущен: оформление через браузер выключено.".into());
                }
            }
            if self.config.read().await.gamerpower {
                let candidates = match self.extra_candidates("").await {
                    Ok(v) => v,
                    Err(e) if !offers.is_empty() => {
                        tracing::warn!("Optional giveaway catalog unavailable: {}", e);
                        vec![]
                    }
                    Err(e) => return Err(e),
                };
                for candidate in candidates {
                    let Some((source, url)) =
                        self.resolve_candidate(&candidate).await.ok().flatten()
                    else {
                        continue;
                    };
                    if source != launcher {
                        continue;
                    }
                    if launcher == "steam" {
                        if let Some(id) = reqwest::Url::parse(&url)
                            .ok()
                            .and_then(|u| {
                                u.path_segments()
                                    .map(|p| p.map(str::to_string).collect::<Vec<_>>())
                            })
                            .filter(|p| p.first().is_some_and(|p| p == "app"))
                            .and_then(|p| p.get(1).cloned())
                        {
                            if let Ok(Some(offer)) = self.steam_app(&id, country).await {
                                if !offers.iter().any(|o| o.key() == offer.key()) {
                                    offers.push(offer);
                                }
                            }
                        }
                    } else if !offers
                        .iter()
                        .any(|o| super::platforms::same_product_url(launcher, &o.url, &url))
                    {
                        offers.push(Offer {
                            launcher: launcher.into(),
                            id: candidate.source_id,
                            namespace: "gamerpower".into(),
                            title: candidate.title,
                            url,
                            original_price: Some(candidate.worth),
                            normally_paid: true,
                            final_price: 0,
                            ends_at: Some(candidate.ends_at),
                            item_ids: vec![],
                        });
                    }
                }
            }
            Ok(offers)
        })
    }
    fn claim<'a>(
        &'a self,
        account: &'a Account,
        offer: &'a Offer,
        country: &'a str,
    ) -> Call<'a, Claimed> {
        Box::pin(async move {
            if account.launcher != offer.launcher || !offer.eligible(chrono::Utc::now().timestamp())
            {
                return Err("Некорректная или завершённая раздача.".into());
            }
            if account.launcher == "steam" {
                self.claim_steam(account, offer, country).await
            } else if account.launcher == "epic" && offer.namespace != "gamerpower" {
                self.claim_epic(account, offer, country).await
            } else if account.launcher == "alienware" {
                Err(
                    "Alienware Arena: ручное получение ключа, нужны ARP и проверка магазина."
                        .into(),
                )
            } else if super::platforms::platform(&account.launcher).is_some() {
                let config = self.config.read().await.clone();
                if account.launcher == "epic" {
                    let native = self
                        .discover_epic(country)
                        .await?
                        .into_iter()
                        .find(|v| super::platforms::same_product_url("epic", &v.url, &offer.url))
                        .ok_or(
                            "Epic не подтвердил SKU этой раздачи из дополнительного каталога.",
                        )?;
                    return self.claim_epic(account, &native, country).await;
                }
                if !config.browser_enabled {
                    return Err("Браузерное получение выключено в настройках.".into());
                }
                if account.launcher == "gog" {
                    return super::gog_claim::claim(
                        account,
                        offer,
                        config.browser_operation_timeout_seconds,
                        config.browser_visible,
                        &config.browser_name,
                    )
                    .await;
                }
                let mut checked = offer.clone();
                let mut verified_price = false;
                if checked.namespace == "gamerpower"
                    && matches!(account.launcher.as_str(), "prime" | "ubisoft" | "fanatical")
                {
                    let price = self.regular_price(&checked.title, country).await.ok_or(
                        "Не удалось подтвердить обычную платную цену игры; получение пропущено.",
                    )?;
                    checked.original_price = Some(price);
                    verified_price = true;
                }
                if account.launcher == "microsoft" {
                    let product = self
                        .microsoft_product(&offer.url, &offer.title, country)
                        .await?;
                    if product.price != 0 {
                        return Err("Microsoft больше не отдаёт эту игру бесплатно.".into());
                    }
                    checked.title = product.title;
                    checked.original_price = Some(product.regular_price);
                    verified_price = true;
                }
                super::browser_claim::claim_with_proof(
                    account,
                    &checked,
                    config.browser_operation_timeout_seconds,
                    config.browser_visible,
                    verified_price,
                    &config.browser_name,
                )
                .await
            } else {
                Err("Неизвестная платформа раздачи.".into())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        launchers::epic::{EpicClient, MemoryCredentialStore},
        storage::Database,
    };
    use axum::{
        extract::Form,
        http::HeaderMap,
        routing::{get, post},
        Json, Router,
    };
    use base64::Engine;
    use serde_json::json;
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn store_identity_accepts_current_account_pages_and_rejects_guests() {
        let id = "76561197960265729";
        assert_eq!(
            store_identity(&format!(
                "<a href=\"https://steamcommunity.com/profiles/{id}/\">Profile</a>"
            )),
            Some(id.into())
        );
        assert_eq!(
            store_identity(&format!(
                "<script>if (g_steamID) {{}} var g_steamID = '{id}';</script>"
            )),
            Some(id.into())
        );
        assert_eq!(store_identity("<script>var g_steamID = false;</script><a href=\"https://steamcommunity.com/profiles/0/\">Sign in</a>"), None);
        assert_eq!(
            store_identity("<form id=\"login_form\">Sign in</form>"),
            None
        );
    }

    #[test]
    fn checkout_errors_do_not_expose_server_bodies_or_credentials() {
        let message = epic_claim_error(crate::launchers::epic::EpicError::Api {
            status: reqwest::StatusCode::FORBIDDEN,
            code: Some("opaque-secret".into()),
            message: "Bearer private-token".into(),
            numeric: None,
        });
        assert!(message.contains("403"));
        assert!(!message.contains("private-token"));
        assert!(!message.contains("opaque-secret"));
    }

    #[tokio::test]
    async fn epic_launcher_discovery_keeps_pc_offers_without_mobile_web_login() {
        let now = chrono::Utc::now();
        let promotions = json!({"data":{"Catalog":{"searchStore":{"elements":[{
            "id":"pc-offer", "namespace":"pc-namespace", "offerType":"BASE_GAME",
            "title":"Paid PC Game", "items":[{"id":"pc-item"}],
            "catalogNs":{"mappings":[{"pageSlug":"paid-pc-game"}]},
            "price":{"totalPrice":{"originalPrice":1500,"discountPrice":0}},
            "promotions":{"promotionalOffers":[{"promotionalOffers":[{
                "startDate":(now-chrono::Duration::minutes(1)).to_rfc3339(),
                "endDate":(now+chrono::Duration::hours(1)).to_rfc3339(),
                "discountSetting":{"discountType":"PERCENTAGE","discountPercentage":0}
            }]}]}
        }]}}}});
        let router = Router::new().route(
            "/promotions",
            get(move || {
                let data = promotions.clone();
                async move { Json(data) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/promotions", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().await.unwrap();
        let epic = Arc::new(EpicAuthStore::new(
            db,
            Arc::new(MemoryCredentialStore::new()),
            Arc::new(EpicClient::from_router(Router::new())),
        ));
        let mut stores = Stores::new(epic);
        stores.epic_promotions = url;
        // No browser account/session is supplied. A mobile web path would fail
        // with a login error before returning the PC catalogue instead of the
        // intentional native-mode warning.
        for browser_enabled in [true, false] {
            stores
                .begin_run(&super::super::Settings {
                    epic_launcher_claims: true,
                    epic_mobile: true,
                    browser_enabled,
                    gamerpower: false,
                    ..Default::default()
                })
                .await
                .unwrap();
            let found = stores.discover("epic", "BY").await.unwrap();
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].id, "pc-offer");
            assert_eq!(found[0].item_ids, ["pc-item"]);
            assert_eq!(
                stores.warnings().await.unwrap(),
                [EPIC_MOBILE_LAUNCHER_WARNING]
            );
            assert_eq!(
                stores.discover_epic_mobile("BY").await.unwrap_err(),
                EPIC_MOBILE_LAUNCHER_WARNING
            );
        }
        server.abort();
    }

    fn refresh_token(id: &str) -> String {
        let payload = json!({"sub":id,"aud":["web","derive"]}).to_string();
        format!(
            "header.{}.signature",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload)
        )
    }
    async fn fixture(
        malicious: bool,
    ) -> (
        Stores,
        SteamSession,
        Arc<AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let transfers = Arc::new(AtomicUsize::new(0));
        let count = transfers.clone();
        let target = if malicious {
            "https://untrusted.example/login/settoken".into()
        } else {
            format!("{base}/login/settoken")
        };
        let router = Router::new()
            .route("/jwt/finalizelogin",post(move |Form(body): Form<HashMap<String,String>>| {
                let target = target.clone();
                async move {
                    assert_eq!(body["nonce"],refresh_token("76561197960265729"));
                    Json(json!({"steamID":"76561197960265729","transfer_info":[{"url":target,"params":{"nonce":"nonce","auth":"auth"}}]}))
                }
            }))
            .route("/login/settoken",post(move |Form(body): Form<HashMap<String,String>>| {
                let count = count.clone();
                async move {
                    assert_eq!(body["steamID"],"76561197960265729");
                    count.fetch_add(1,Ordering::SeqCst);
                    ([("set-cookie","steamLoginSecure=76561197960265729%7C%7Cfresh_token; Secure; Path=/")],Json(json!({"result":1})))
                }
            }))
            .route("/account/",get(|headers: HeaderMap| async move {
                assert!(headers["cookie"].to_str().unwrap().contains("fresh_token"));
                "<script>var g_steamID = \"76561197960265729\";</script>"
            }));
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().await.unwrap();
        let epic = Arc::new(EpicAuthStore::new(
            db,
            Arc::new(MemoryCredentialStore::new()),
            Arc::new(EpicClient::from_router(Router::new())),
        ));
        let mut stores = Stores::new(epic);
        stores.steam_base = base.clone();
        stores.steam_login_base = base;
        let session = SteamSession {
            account_id: "76561197960265729".into(),
            login_secure: "76561197960265729%7C%7Cold_token".into(),
            session_id: "0123456789abcdef".into(),
            country: "BY".into(),
            refresh_token: Some(refresh_token("76561197960265729")),
            auth_source: "browser".into(),
        };
        (stores, session, transfers, server)
    }
    #[tokio::test]
    async fn expired_steam_cookie_renews_and_verifies_the_same_account() {
        let (stores, session, count, server) = fixture(false).await;
        let refreshed = stores.refresh_steam_session(session).await.unwrap();
        assert!(refreshed.login_secure.ends_with("fresh_token"));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        server.abort();
    }
    #[tokio::test]
    async fn refresh_never_sends_credentials_to_a_foreign_transfer_url() {
        let (stores, mut session, count, server) = fixture(true).await;
        assert!(stores.refresh_steam_session(session.clone()).await.is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
        session.refresh_token = Some(refresh_token("76561197960265730"));
        assert!(session.validate().is_err());
        server.abort();
    }
}
