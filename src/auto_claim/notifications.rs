//! External destinations are explicitly configured by the user in the OS vault.
//! Status/export never returns credentials or destination URLs.
use super::Run;
use crate::storage::Database;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub discord: Option<String>,
    pub webhook: Option<String>,
    pub telegram: Option<Telegram>,
    pub email: Option<Email>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Telegram {
    pub token: String,
    pub chat_id: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Email {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
    pub to: String,
}
pub fn valid_webhook(value: &str, discord: bool) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    let host = url.host_str().unwrap();
    if host == "localhost" || host.ends_with(".local") || host.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    !discord
        || (matches!(host, "discord.com" | "discordapp.com")
            && url.path().starts_with("/api/webhooks/"))
}
impl Config {
    pub fn channels(&self) -> Vec<&'static str> {
        let mut out = vec![];
        if self.discord.is_some() {
            out.push("Discord");
        }
        if self.webhook.is_some() {
            out.push("Webhook");
        }
        if self.telegram.is_some() {
            out.push("Telegram");
        }
        if self.email.is_some() {
            out.push("Email");
        }
        out
    }
    pub fn validate(&self) -> Result<(), String> {
        if self
            .discord
            .as_ref()
            .is_some_and(|s| !valid_webhook(s, true))
            || self
                .webhook
                .as_ref()
                .is_some_and(|s| !valid_webhook(s, false))
        {
            return Err("Нужен HTTPS webhook без логина и пароля в адресе.".into());
        }
        if let Some(t) = &self.telegram {
            if t.token.len() > 256
                || !t.token.split_once(':').is_some_and(|(id, key)| {
                    !id.is_empty()
                        && id.bytes().all(|c| c.is_ascii_digit())
                        && !key.is_empty()
                        && key
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                })
                || t.chat_id.len() > 128
                || t.chat_id.is_empty()
                || t.chat_id.chars().any(char::is_control)
            {
                return Err("Некорректные настройки Telegram.".into());
            }
        }
        if let Some(e) = &self.email {
            if !matches!(e.port, 465 | 587)
                || e.host.len() > 256
                || !e.host.contains('.')
                || e.host
                    .bytes()
                    .any(|c| !(c.is_ascii_alphanumeric() || b".-".contains(&c)))
                || e.username.is_empty()
                || e.password.is_empty()
                || e.from.parse::<lettre::message::Mailbox>().is_err()
                || e.to.parse::<lettre::message::Mailbox>().is_err()
            {
                return Err("Для Email нужны SMTP, порт 465/587, учётные данные и адреса отправителя/получателя.".into());
            }
        }
        if serde_json::to_string(self)
            .map_err(|_| "Не удалось сохранить настройки уведомлений.")?
            .len()
            > 2400
        {
            return Err("Настройки уведомлений слишком большие для хранилища ОС.".into());
        }
        Ok(())
    }
    pub fn load() -> Result<Self, String> {
        let entry = keyring::Entry::new("swch:claim-notifications", "destinations")
            .map_err(|_| "Не удалось открыть хранилище уведомлений.")?;
        match entry.get_password() {
            Ok(raw) => {
                serde_json::from_str(&raw).map_err(|_| "Настройки уведомлений повреждены.".into())
            }
            Err(keyring::Error::NoEntry) => Ok(Self::default()),
            Err(_) => Err("Не удалось прочитать настройки уведомлений.".into()),
        }
    }
    pub fn save(&self) -> Result<(), String> {
        self.validate()?;
        keyring::Entry::new("swch:claim-notifications", "destinations")
            .and_then(|entry| entry.set_password(&serde_json::to_string(self).unwrap()))
            .map_err(|_| "Не удалось сохранить уведомления в хранилище ОС.".into())
    }
    pub fn clear() -> Result<(), String> {
        match keyring::Entry::new("swch:claim-notifications", "destinations")
            .and_then(|e| e.delete_credential())
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Не удалось удалить настройки уведомлений.".into()),
        }
    }
}
pub fn summary(run: &Run) -> Option<(String, String)> {
    let rows = run
        .results
        .iter()
        .filter(|r| {
            matches!(
                r.status.as_str(),
                "attention" | "error" | "needs_login" | "code_received"
            ) || r.status == "claimed" && r.message == "Игра добавлена в библиотеку."
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return None;
    }
    let added = rows.iter().filter(|r| r.status == "claimed").count();
    let codes = rows.iter().filter(|r| r.status == "code_received").count();
    let facts = rows
        .iter()
        .map(|r| {
            (
                &r.launcher,
                &r.account_id,
                &r.offer_id,
                &r.status,
                &r.message,
            )
        })
        .collect::<Vec<_>>();
    let signature = format!("{:x}", Sha256::digest(serde_json::to_vec(&facts).ok()?));
    Some((format!("SWCH: проверка раздач завершена. Игр получено: {added}; ключей: {codes}; нужно внимание: {}. Подробности в SWCH.",rows.len()-added-codes),signature))
}
pub async fn deliver(db: &Database, run: &Run) -> Result<(), String> {
    let Some((text, signature)) = summary(run) else {
        return Ok(());
    };
    let config = tokio::task::spawn_blocking(Config::load)
        .await
        .map_err(|_| "Не удалось прочитать каналы уведомлений.")??;
    config.validate()?;
    let channels = config.channels();
    if channels.is_empty() {
        return Ok(());
    }
    let signature = format!("{signature}:{:?}", channels);
    if db
        .auto_claim_value("notification-signature")
        .await
        .ok()
        .flatten()
        .as_deref()
        == Some(&signature)
    {
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Не удалось создать отправитель уведомлений.")?;
    let mut errors = vec![];
    for (name, url, body) in [
        config.discord.as_ref().map(|url| {
            (
                "Discord",
                url.clone(),
                json!({"content":text,"allowed_mentions":{"parse":[]}}),
            )
        }),
        config.webhook.as_ref().map(|url| {
            (
                "Webhook",
                url.clone(),
                json!({"event":"free_games.finished","text":text}),
            )
        }),
        config.telegram.as_ref().map(|t| {
            (
                "Telegram",
                format!("https://api.telegram.org/bot{}/sendMessage", t.token),
                json!({"chat_id":t.chat_id,"text":text,"disable_web_page_preview":true}),
            )
        }),
    ]
    .into_iter()
    .flatten()
    {
        if !client
            .post(url)
            .json(&body)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            errors.push(name);
        }
    }
    if let Some(email) = config.email {
        use lettre::{
            transport::smtp::authentication::Credentials, AsyncSmtpTransport, AsyncTransport,
            Message, Tokio1Executor,
        };
        let result = async {
            let msg = Message::builder()
                .from(email.from.parse().map_err(|_| ())?)
                .to(email.to.parse().map_err(|_| ())?)
                .subject("SWCH — раздачи игр")
                .body(text)
                .map_err(|_| ())?;
            let builder = if email.port == 465 {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&email.host)
            } else {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&email.host)
            }
            .map_err(|_| ())?;
            builder
                .port(email.port)
                .timeout(Some(Duration::from_secs(5)))
                .credentials(Credentials::new(email.username, email.password))
                .build()
                .send(msg)
                .await
                .map_err(|_| ())?;
            Ok::<_, ()>(())
        };
        if !tokio::time::timeout(Duration::from_secs(8), result)
            .await
            .is_ok_and(|r| r.is_ok())
        {
            errors.push("Email");
        }
    }
    if !errors.is_empty() {
        return Err(format!(
            "Не доставлены уведомления: {}. Результаты сохранены в SWCH.",
            errors.join(", ")
        ));
    }
    db.set_auto_claim_value("notification-signature", &signature)
        .await
        .map_err(|_| "Не удалось сохранить состояние уведомлений.".into())
}
