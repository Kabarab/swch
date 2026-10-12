//! Short-lived capabilities for an explicit browser sign-in. No browser cookies
//! or OAuth credentials are kept here; completed accounts use the normal store.
use std::collections::HashMap;
use std::time::{Duration, Instant};
use serde::Serialize;
use tokio::sync::Mutex;
use super::models::EpicAccount;

pub const LOGIN_TTL: Duration = Duration::from_secs(600);

#[derive(Clone, Serialize)]
pub struct BrowserLoginStatus {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<EpicAccount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
    pub library_status: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn capabilities_are_unique_single_use_and_cancellable() {
        let auth = BrowserAuth::default();
        let first = auth.start().await.unwrap();
        let second = auth.start().await.unwrap();
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
        assert!(auth.status("unknown").await.is_none());
        let (left, right) = tokio::join!(auth.claim(&first), auth.claim(&first));
        assert_ne!(left, right, "Only one concurrent completion may redeem the Epic code");
        assert_eq!(auth.cancel(&first).await.unwrap().status, "authenticating");
        assert_eq!(auth.cancel(&second).await.unwrap().status, "cancelled");
        assert!(!auth.claim(&second).await);
    }

    #[tokio::test]
    async fn expired_sessions_cannot_be_redeemed_and_are_pruned() {
        let auth = BrowserAuth::default();
        let id = auth.start().await.unwrap();
        auth.logins.lock().await.get_mut(&id).unwrap().expires = Instant::now() - Duration::from_secs(1);
        assert_eq!(auth.status(&id).await.unwrap().status, "expired");
        assert!(!auth.claim(&id).await);
        auth.start().await.unwrap();
        assert!(auth.status(&id).await.is_none());
    }
}

struct Login {
    expires: Instant,
    state: BrowserLoginStatus,
}

#[derive(Default)]
pub struct BrowserAuth {
    logins: Mutex<HashMap<String, Login>>,
}

impl BrowserAuth {
    pub async fn start(&self) -> Result<String, &'static str> {
        let mut logins = self.logins.lock().await;
        logins.retain(|_, login| login.expires > Instant::now());
        if logins.len() >= 32 {
            return Err("Слишком много попыток входа. Повтори через несколько минут.");
        }
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).map_err(|_| "Не удалось создать безопасную сессию входа.")?;
        let id = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        logins.insert(id.clone(), Login {
            expires: Instant::now() + LOGIN_TTL,
            state: BrowserLoginStatus {
                status: "pending", account: None, error: None, library_status: "idle",
            },
        });
        Ok(id)
    }

    pub async fn status(&self, id: &str) -> Option<BrowserLoginStatus> {
        let mut logins = self.logins.lock().await;
        let login = logins.get_mut(id)?;
        if login.expires <= Instant::now() && login.state.status == "pending" {
            login.state.status = "expired";
        }
        Some(login.state.clone())
    }

    /// Claim the capability once, before redeeming a one-use Epic code.
    pub async fn claim(&self, id: &str) -> bool {
        let mut logins = self.logins.lock().await;
        let Some(login) = logins.get_mut(id) else { return false; };
        if login.expires <= Instant::now() || login.state.status != "pending" {
            return false;
        }
        login.state.status = "authenticating";
        true
    }

    pub async fn connected(&self, id: &str, account: EpicAccount) {
        if let Some(login) = self.logins.lock().await.get_mut(id) {
            login.state.status = "connected";
            login.state.account = Some(account);
            login.state.library_status = "syncing";
            // Keep the completed result available while the library syncs.
            login.expires = Instant::now() + LOGIN_TTL;
        }
    }

    pub async fn failed(&self, id: &str) {
        if let Some(login) = self.logins.lock().await.get_mut(id) {
            login.state.status = "failed";
            login.state.error = Some("Не удалось подключить аккаунт Epic. Начни вход заново или используй код.");
        }
    }

    pub async fn library_finished(&self, id: &str, success: bool) {
        if let Some(login) = self.logins.lock().await.get_mut(id) {
            login.state.library_status = if success { "ready" } else { "failed" };
        }
    }

    pub async fn cancel(&self, id: &str) -> Option<BrowserLoginStatus> {
        let mut logins = self.logins.lock().await;
        let login = logins.get_mut(id)?;
        // Authentication already in progress must finish: the code may have
        // been redeemed and the account may already have been persisted.
        if login.state.status == "pending" {
            login.state.status = "cancelled";
        }
        Some(login.state.clone())
    }
}
