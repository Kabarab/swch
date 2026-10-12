use super::{game, json_id, product_game, valid_id, GogError};
use crate::storage::{Account, Database};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

// Public desktop OAuth application used by GALAXY-compatible clients (gogdl).
const CLIENT_ID: &str = "46899977096215655";
const CLIENT_SECRET: &str = "9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9";
const REDIRECT: &str = "https://embed.gog.com/on_login_success?origin=client";

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub expires_at: i64,
}

pub trait CredentialStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<Credentials>, GogError>;
    fn set(&self, id: &str, value: &Credentials) -> Result<(), GogError>;
    fn delete(&self, id: &str) -> Result<(), GogError>;
}

pub struct OsCredentialStore;
impl CredentialStore for OsCredentialStore {
    fn get(&self, id: &str) -> Result<Option<Credentials>, GogError> {
        match keyring::Entry::new("swch:gog", id)?.get_password() {
            Ok(value) => serde_json::from_str(&value)
                .map(Some)
                .map_err(|_| GogError::Response),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn set(&self, id: &str, value: &Credentials) -> Result<(), GogError> {
        let value = serde_json::to_string(value).map_err(|_| GogError::Response)?;
        keyring::Entry::new("swch:gog", id)?.set_password(&value)?;
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), GogError> {
        match keyring::Entry::new("swch:gog", id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Clone)]
pub struct Endpoints {
    pub auth: String,
    pub users: String,
    pub library: String,
    pub products: String,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            auth: "https://auth.gog.com".into(),
            users: "https://users.gog.com".into(),
            library: "https://galaxy-library.gog.com".into(),
            products: "https://api.gog.com".into(),
        }
    }
}

pub struct GogStore {
    client: reqwest::Client,
    endpoints: Endpoints,
    credentials: Arc<dyn CredentialStore>,
    galaxy: Arc<dyn super::galaxy::GalaxyClient>,
    pub operation: tokio::sync::Mutex<()>,
}
impl Default for GogStore {
    fn default() -> Self {
        Self::new(Endpoints::default(), Arc::new(OsCredentialStore))
    }
}

pub fn auth_url() -> String {
    let mut url = reqwest::Url::parse("https://auth.gog.com/auth").expect("fixed GOG URL");
    url.query_pairs_mut().extend_pairs([
        ("client_id", CLIENT_ID),
        ("redirect_uri", REDIRECT),
        ("response_type", "code"),
        ("layout", "galaxy"),
    ]);
    url.into()
}

/// Accept the final GOG redirect or a raw code, never fetch a user-supplied URL.
pub fn authorization_code(input: &str) -> Result<String, GogError> {
    let input = input.trim();
    if input.len() > 8192 {
        return Err(GogError::Invalid("Слишком длинный код GOG.".into()));
    }
    let code = if input.contains("://") {
        let url = reqwest::Url::parse(input)
            .map_err(|_| GogError::Invalid("Некорректный адрес GOG.".into()))?;
        if url.scheme() != "https"
            || !matches!(url.host_str(), Some("embed.gog.com" | "www.gog.com"))
            || url.path() != "/on_login_success"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return Err(GogError::Invalid(
                "Вставь адрес страницы on_login_success после входа GOG.".into(),
            ));
        }
        url.query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned())
            .unwrap_or_default()
    } else {
        input.into()
    };
    if code.is_empty()
        || code.len() > 4096
        || !code
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.~".contains(&c))
    {
        return Err(GogError::Invalid(
            "Вставь код или полный адрес после входа GOG.".into(),
        ));
    }
    Ok(code)
}

impl GogStore {
    /// Shared with the claimer so OAuth refreshes use the same operation lock
    /// as account removal and library sync in the application's GOG router.
    pub fn shared()->Arc<Self> {
        static STORE:std::sync::OnceLock<Arc<GogStore>>=std::sync::OnceLock::new();
        STORE.get_or_init(||Arc::new(Self::default())).clone()
    }
    pub async fn owns_product(&self,account:&str,product:&str)->Result<bool,GogError> {
        if !valid_id(product){return Err(GogError::Response);}
        let _guard=self.operation.lock().await;
        let token=self.access(account).await?;
        let ids=match self.owned_ids(&token).await {
            Err(GogError::Unauthorized)=>{let refreshed=self.refresh_access(account,&token).await?;self.owned_ids(&refreshed).await?},
            value=>value?,
        };
        Ok(ids.iter().any(|id|id==product))
    }
    pub async fn describe_product(&self,id:&str)->Result<Value,GogError> {
        if !valid_id(id){return Err(GogError::Response);}
        self.json(self.client.get(format!("{}/products/{id}",self.endpoints.products))).await
    }
    pub fn with_galaxy(mut self, galaxy: Arc<dyn super::galaxy::GalaxyClient>) -> Self {
        self.galaxy = galaxy;
        self
    }

    pub fn launcher_account_id(&self) -> Option<String> {
        let state = self.galaxy.state().ok()?;
        state.refresh_token?;
        state.user_id.map(|id| format!("gog_{id}"))
    }

    pub async fn activate_launcher(&self, db: &Database, id: &str) -> Result<(), GogError> {
        let _guard = self.operation.lock().await;
        if !db
            .get_accounts()
            .await?
            .iter()
            .any(|a| a.launcher == "gog" && a.id == id)
        {
            return Err(GogError::Invalid("Аккаунт GOG не подключён.".into()));
        }
        if !self.galaxy.available() {
            return Err(GogError::Invalid(
                "Для переключения аккаунта установи GOG GALAXY на Windows.".into(),
            ));
        }
        let client = self.galaxy.clone();
        let target = id
            .strip_prefix("gog_")
            .filter(|id| super::valid_id(id))
            .ok_or_else(|| GogError::Invalid("Некорректный аккаунт GOG.".into()))?
            .to_string();
        let initial = client.state()?;
        if initial.user_id.as_deref() == Some(&target)
            && initial.refresh_token.is_some()
            && client.running()
        {
            db.set_active_gog_account(id).await?;
            return Ok(());
        }
        // Validate and durably rotate the session before touching a running client.
        let token = self.access(id).await?;
        let token = self.refresh_access(id, &token).await?;
        let profile = self
            .json(
                self.client
                    .get(format!("{}/users/{}", self.endpoints.users, target))
                    .bearer_auth(&token.access_token),
            )
            .await?;
        if super::json_id(&profile["id"]).as_deref() != Some(&target) {
            return Err(GogError::Unauthorized);
        }
        let closing = client.clone();
        let previous = tokio::task::spawn_blocking(move || {
            closing.close()?;
            let state = closing.state()?;
            if let Err(e) = closing.backup(&state) {
                let _ = closing.open();
                return Err(e);
            }
            Ok::<_, GogError>(state)
        })
        .await
        .map_err(|_| GogError::Response)??;
        if let (Some(user), Some(refresh)) = (&previous.user_id, &previous.refresh_token) {
            let previous_id = format!("gog_{user}");
            if let Some(mut saved) = self.credentials.get(&previous_id)? {
                if saved.user_id == *user {
                    saved.refresh_token = refresh.clone();
                    if let Err(e) = self.credentials.set(&previous_id, &saved) {
                        let _ = client.open();
                        return Err(e);
                    }
                }
            }
        }
        let next = super::galaxy::LoginState {
            user_id: None,
            username: None,
            refresh_token: Some(token.refresh_token.clone()),
        };
        let result = async {
            client.write(&next)?;
            client.open()?;
            let deadline = tokio::time::Instant::now() + client.timeout();
            loop {
                // The Windows client must report a new successful authentication after restart.
                if client.confirmed(&target,&token.refresh_token)? { break; }
                if tokio::time::Instant::now()>=deadline {
                    return Err(GogError::Invalid("GALAXY не подтвердил вход в выбранный аккаунт. Повтори подключение аккаунта GOG.".into()));
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            let mut persisted = token.clone();
            persisted.refresh_token = client.state()?.refresh_token.ok_or(GogError::Unauthorized)?;
            self.credentials.set(id,&persisted)?;
            db.set_active_gog_account(id).await?;
            Ok::<_,GogError>(())
        }.await;
        if let Err(original) = result {
            let restore = client.clone();
            tokio::task::spawn_blocking(move || {
                restore.close()?;
                restore.write(&previous)?;
                if previous.refresh_token.is_some() {
                    restore.open()?;
                }
                Ok::<_, GogError>(())
            })
            .await
            .map_err(|_| GogError::Response)??;
            return Err(original);
        }
        Ok(())
    }

    pub fn new(endpoints: Endpoints, credentials: Arc<dyn CredentialStore>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("SWCH/1.2 GOG integration")
            .build()
            .expect("GOG HTTP client");
        Self {
            client,
            endpoints,
            credentials,
            galaxy: Arc::new(super::galaxy::WindowsGalaxy::default()),
            operation: tokio::sync::Mutex::new(()),
        }
    }

    async fn json(&self, request: reqwest::RequestBuilder) -> Result<Value, GogError> {
        // reqwest errors contain OAuth query parameters; never expose them in API errors/logs.
        let response = request.send().await.map_err(|_| GogError::Network)?;
        let status = response.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(GogError::Unauthorized);
        }
        if !status.is_success() {
            return Err(GogError::Http(status.as_u16()));
        }
        response.json().await.map_err(|_| GogError::Response)
    }

    async fn token(
        &self,
        grant: &str,
        parameter: &str,
        value: &str,
    ) -> Result<Credentials, GogError> {
        let data = self
            .json(
                self.client
                    .get(format!("{}/token", self.endpoints.auth))
                    .query(&[
                        ("client_id", CLIENT_ID),
                        ("client_secret", CLIENT_SECRET),
                        ("grant_type", grant),
                        ("redirect_uri", REDIRECT),
                        (parameter, value),
                    ]),
            )
            .await
            .map_err(|e| match e {
                GogError::Http(400) if grant == "refresh_token" => GogError::Unauthorized,
                GogError::Http(400) => GogError::Invalid(
                    "Код входа GOG истёк или уже использован. Повтори вход в браузере.".into(),
                ),
                e => e,
            })?;
        Ok(Credentials {
            access_token: data["access_token"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or(GogError::Response)?
                .into(),
            refresh_token: data["refresh_token"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or(GogError::Response)?
                .into(),
            user_id: json_id(&data["user_id"]).ok_or(GogError::Response)?,
            expires_at: chrono::Utc::now().timestamp().saturating_add(
                data["expires_in"]
                    .as_i64()
                    .filter(|s| *s > 0 && *s <= 31_536_000)
                    .ok_or(GogError::Response)?,
            ),
        })
    }

    pub async fn login(&self, db: &Database, code: &str) -> Result<Account, GogError> {
        let _guard = self.operation.lock().await;
        let token = self
            .token("authorization_code", "code", &authorization_code(code)?)
            .await?;
        let user = self
            .json(
                self.client
                    .get(format!("{}/users/{}", self.endpoints.users, token.user_id))
                    .bearer_auth(&token.access_token),
            )
            .await?;
        if json_id(&user["id"]).as_deref() != Some(&token.user_id) {
            return Err(GogError::Response);
        }
        let username = user["username"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or(GogError::Response)?;
        let account = Account {
            id: format!("gog_{}", token.user_id),
            launcher: "gog".into(),
            username: username.into(),
            display_name: Some(username.into()),
            email: None,
            is_active: true,
            last_used: Some(chrono::Utc::now().to_rfc3339()),
        };
        self.credentials.set(&account.id, &token)?;
        db.upsert_account(&account).await?;
        db.set_active_gog_account(&account.id).await?;
        Ok(account)
    }

    async fn access(&self, id: &str) -> Result<Credentials, GogError> {
        let user_id = id
            .strip_prefix("gog_")
            .filter(|s| valid_id(s))
            .ok_or_else(|| GogError::Invalid("Некорректный аккаунт GOG.".into()))?;
        let mut token = self.credentials.get(id)?.ok_or(GogError::Unauthorized)?;
        if token.user_id != user_id {
            return Err(GogError::Unauthorized);
        }
        if let Ok(native) = self.galaxy.state() {
            if native.user_id.as_deref() == Some(user_id) {
                if let Some(refresh) = native.refresh_token {
                    token.refresh_token = refresh;
                }
            }
        }
        if token.expires_at <= chrono::Utc::now().timestamp() + 60 {
            token = self.refresh_access(id, &token).await?;
        }
        Ok(token)
    }

    async fn refresh_access(
        &self,
        id: &str,
        previous: &Credentials,
    ) -> Result<Credentials, GogError> {
        let token = self
            .token("refresh_token", "refresh_token", &previous.refresh_token)
            .await?;
        if token.user_id != previous.user_id || id != format!("gog_{}", token.user_id) {
            return Err(GogError::Unauthorized);
        }
        self.credentials.set(id, &token)?;
        Ok(token)
    }

    async fn owned_ids(&self, token: &Credentials) -> Result<Vec<String>, GogError> {
        let mut ids = HashSet::new();
        let mut pages = HashSet::new();
        let mut next = String::new();
        loop {
            if !pages.insert(next.clone()) || pages.len() > 1000 {
                return Err(GogError::Response);
            }
            let mut request = self
                .client
                .get(format!(
                    "{}/users/{}/releases",
                    self.endpoints.library, token.user_id
                ))
                .bearer_auth(&token.access_token);
            if !next.is_empty() {
                request = request.query(&[("page_token", &next)]);
            }
            let data = self.json(request).await?;
            let items = data["items"].as_array().ok_or(GogError::Response)?;
            for item in items {
                // Other platforms connected to GALAXY are already handled by their SWCH modules.
                if item["platform_id"].as_str() != Some("gog") {
                    continue;
                }
                if item["owned"].as_bool() == Some(false) {
                    continue;
                }
                ids.insert(json_id(&item["external_id"]).ok_or(GogError::Response)?);
            }
            match &data["next_page_token"] {
                Value::Null => break,
                Value::String(s) if s.is_empty() => break,
                Value::String(s) => next = s.clone(),
                _ => return Err(GogError::Response),
            }
        }
        let mut ids: Vec<_> = ids.into_iter().collect();
        ids.sort();
        Ok(ids)
    }

    pub async fn sync(&self, db: &Database, id: &str) -> Result<(usize, Vec<String>), GogError> {
        let _guard = self.operation.lock().await;
        if !db
            .get_accounts()
            .await?
            .iter()
            .any(|a| a.launcher == "gog" && a.id == id)
        {
            return Err(GogError::Invalid("Аккаунт GOG не подключён.".into()));
        }
        let token = self.access(id).await?;
        let (token, ids) = match self.owned_ids(&token).await {
            Ok(ids) => (token, ids),
            Err(GogError::Unauthorized) => {
                let token = self.refresh_access(id, &token).await?;
                let ids = self.owned_ids(&token).await?;
                (token, ids)
            }
            Err(e) => return Err(e),
        };
        let old: BTreeMap<_, _> = db
            .get_games()
            .await?
            .into_iter()
            .filter(|g| g.launcher == "gog")
            .map(|g| (g.app_id.clone(), g))
            .collect();
        let mut games = Vec::new();
        let mut warnings = Vec::new();
        // Bound concurrent metadata requests; cache ownership only after every page has loaded.
        for chunk in ids.chunks(8) {
            let mut jobs = tokio::task::JoinSet::new();
            for app in chunk {
                let app = app.clone();
                let client = self.client.clone();
                let url = format!("{}/products/{app}", self.endpoints.products);
                let access = token.access_token.clone();
                jobs.spawn(async move {
                    let result = client.get(url).bearer_auth(access).send().await;
                    let product = match result {
                        Ok(response) if response.status().is_success() => {
                            response.json::<Value>().await.ok()
                        }
                        _ => None,
                    };
                    let result = product
                        .as_ref()
                        .ok_or(GogError::Network)
                        .and_then(|p| product_game(&app, p));
                    (app, result)
                });
            }
            while let Some(result) = jobs.join_next().await {
                let (app, result) = result.map_err(|_| GogError::Response)?;
                match result {
                    Ok(Some(game)) => games.push(game),
                    Ok(None) => {}
                    Err(_) => {
                        warnings.push(format!("Не загрузились сведения об игре GOG {app}."));
                        games.push(
                            old.get(&app)
                                .cloned()
                                .unwrap_or_else(|| game(&app, format!("GOG #{app}"), None)),
                        );
                    }
                }
            }
        }
        games.sort_by(|a, b| a.app_id.cmp(&b.app_id));
        db.replace_gog_library(id, &games).await?;
        Ok((games.len(), warnings))
    }

    pub async fn remove(&self, db: &Database, id: &str) -> Result<(), GogError> {
        let _guard = self.operation.lock().await;
        if !db
            .get_accounts()
            .await?
            .iter()
            .any(|a| a.launcher == "gog" && a.id == id)
        {
            return Err(GogError::Invalid("Аккаунт GOG не найден.".into()));
        }
        self.credentials.delete(id)?;
        db.delete_gog_account(id).await?;
        Ok(())
    }
}
