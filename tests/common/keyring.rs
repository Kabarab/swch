#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;

use super::models::DeviceAuth;

pub const KEYRING_SERVICE: &str = "swch:epic";

#[derive(Error, Debug)]
pub enum KeyringError {
    #[error("Keyring OS error: {0}")]
    OsError(String),
    #[error("Credential not found for account: {0}")]
    NotFound(String),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Lock poisoned")]
    LockError,
}

pub trait CredentialStore: Send + Sync {
    fn get_device_auth(&self, account_id: &str) -> Result<Option<DeviceAuth>, KeyringError>;
    fn set_device_auth(&self, auth: &DeviceAuth) -> Result<(), KeyringError>;
    fn delete_device_auth(&self, account_id: &str) -> Result<(), KeyringError>;
    fn list_accounts(&self) -> Result<Vec<String>, KeyringError>;
}

/// Thread-safe in-memory credential store for tests and CI
#[derive(Debug, Default, Clone)]
pub struct MemoryCredentialStore {
    storage: Arc<RwLock<HashMap<String, String>>>,
}

impl MemoryCredentialStore {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn make_key(account_id: &str) -> String {
        format!("{}:{}", KEYRING_SERVICE, account_id)
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn get_device_auth(&self, account_id: &str) -> Result<Option<DeviceAuth>, KeyringError> {
        let key = Self::make_key(account_id);
        let guard = self.storage.read().map_err(|_| KeyringError::LockError)?;
        if let Some(json) = guard.get(&key) {
            let auth: DeviceAuth = serde_json::from_str(json)?;
            Ok(Some(auth))
        } else {
            Ok(None)
        }
    }

    fn set_device_auth(&self, auth: &DeviceAuth) -> Result<(), KeyringError> {
        let key = Self::make_key(&auth.account_id);
        let json = serde_json::to_string(auth)?;
        let mut guard = self.storage.write().map_err(|_| KeyringError::LockError)?;
        guard.insert(key, json);
        Ok(())
    }

    fn delete_device_auth(&self, account_id: &str) -> Result<(), KeyringError> {
        let key = Self::make_key(account_id);
        let mut guard = self.storage.write().map_err(|_| KeyringError::LockError)?;
        guard.remove(&key);
        Ok(())
    }

    fn list_accounts(&self) -> Result<Vec<String>, KeyringError> {
        let guard = self.storage.read().map_err(|_| KeyringError::LockError)?;
        let prefix = format!("{}:", KEYRING_SERVICE);
        let accounts = guard
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix).map(|s| s.to_string()))
            .collect();
        Ok(accounts)
    }
}
