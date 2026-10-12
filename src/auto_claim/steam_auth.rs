use crate::launchers::steam::library::valid_steam_id;
use serde::{Deserialize, Serialize};

// Deliberately no Debug or Serialize on API responses containing these secrets.
#[derive(Clone, Serialize, Deserialize)]
pub struct SteamSession {
    pub account_id: String,
    pub login_secure: String,
    pub session_id: String,
    pub country: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub auth_source: String,
}
impl SteamSession {
    pub fn validate(&self) -> Result<(), String> {
        let prefix = format!("{}%7C%7C", self.account_id);
        let raw_prefix = format!("{}||", self.account_id);
        if !valid_steam_id(&self.account_id)
            || !(self.login_secure.starts_with(&prefix)
                || self.login_secure.starts_with(&raw_prefix))
            || self.login_secure.len() > 8192
            || self.login_secure.len() <= prefix.len()
            || !self
                .login_secure
                .bytes()
                .all(|c| c.is_ascii_graphic() && c != b';' && c != b'"')
            || !(8..=128).contains(&self.session_id.len())
            || !self.session_id.bytes().all(|c| c.is_ascii_alphanumeric())
            || self.country.len() != 2
            || !self.country.bytes().all(|c| c.is_ascii_uppercase())
        {
            return Err("Некорректная сессия магазина Steam или выбран другой аккаунт.".into());
        }
        if let Some(token) = &self.refresh_token {
            use base64::Engine;
            let payload = token
                .split('.')
                .nth(1)
                .and_then(|v| {
                    base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(v)
                        .ok()
                })
                .and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok());
            if token.len() > 16384
                || !token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || !payload.is_some_and(|v| {
                    v["sub"].as_str() == Some(&self.account_id)
                        && v["aud"].as_array().is_some_and(|a| {
                            a.iter().any(|v| v.as_str() == Some("web"))
                                && a.iter().any(|v| v.as_str() == Some("derive"))
                        })
                })
            {
                return Err(
                    "Данные обновления Steam относятся к другому аккаунту или повреждены.".into(),
                );
            }
        }
        Ok(())
    }
    pub fn cookies(&self) -> String {
        format!(
            "steamLoginSecure={}; sessionid={}; clientsessionid={}; Steam_Language=english",
            self.login_secure,
            self.session_id,
            &self.session_id[..self.session_id.len().min(16)]
        )
    }
    pub fn save(&self) -> Result<(), String> {
        self.validate()?;
        keyring::Entry::new("swch:steam-store", &self.account_id)
            .and_then(|entry| entry.set_password(&serde_json::to_string(self).unwrap()))
            .map_err(|_| "Не удалось сохранить вход Steam в хранилище учётных данных ОС.".into())
    }
    pub fn load(account_id: &str) -> Result<Self, String> {
        let value = keyring::Entry::new("swch:steam-store", account_id)
            .and_then(|entry| entry.get_password())
            .map_err(|_| {
                "Войди в официальный Steam с «Запомнить меня»; SWCH подключит магазин автоматически.".to_string()
            })?;
        let session: Self = serde_json::from_str(&value)
            .map_err(|_| "Повтори подключение магазина Steam.".to_string())?;
        session.validate()?;
        if session.account_id != account_id {
            return Err("Сессия Steam относится к другому аккаунту.".into());
        }
        Ok(session)
    }
    pub fn delete(account_id: &str) -> Result<(), String> {
        match keyring::Entry::new("swch:steam-store", account_id)
            .and_then(|entry| entry.delete_credential())
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Не удалось удалить сохранённый вход Steam.".into()),
        }
    }
}
