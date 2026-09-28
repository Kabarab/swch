//! Secure persistence for Epic Games DeviceAuth credentials.
//!
//! Provides the `CredentialStore` trait with:
//! - `OsKeyringStore`: Production OS Keyring storage (`swch:epic:<accountId>`).
//! - `MemoryCredentialStore`: Thread-safe in-memory storage for CI, tests, and headless environments.
//! - `default_store()`: Runtime probing with automatic graceful fallback.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;

use super::models::{DeviceAuth, EpicError};

/// Default OS Keyring service identifier for Epic Games credentials.
pub const DEFAULT_EPIC_SERVICE: &str = "swch:epic";

/// Errors arising from keyring operations or credential persistence.
#[derive(Error, Debug)]
pub enum KeyringStoreError {
    /// Operating system keyring backend error.
    #[error("Keyring backend error: {0}")]
    Keyring(#[from] keyring::Error),

    /// Serialization or deserialization error of DeviceAuth JSON.
    #[error("DeviceAuth JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Concurrency lock was poisoned.
    #[error("Internal memory lock poisoned: {0}")]
    LockPoisoned(String),

    /// Keyring backend is completely unavailable.
    #[error("Credential store unavailable: {0}")]
    Unavailable(String),
}

impl From<KeyringStoreError> for EpicError {
    fn from(err: KeyringStoreError) -> Self {
        EpicError::Keyring(err.to_string())
    }
}

/// Thread-safe abstraction for persisting and retrieving Epic Games DeviceAuth credentials.
pub trait CredentialStore: Send + Sync {
    /// Retrieves the DeviceAuth for a given Epic account ID.
    ///
    /// Returns:
    /// - `Ok(Some(auth))` if the credential exists and was successfully deserialized.
    /// - `Ok(None)` if no credential exists for the given account ID (not an error).
    /// - `Err(KeyringStoreError)` if the backend storage system failed.
    fn get_device_auth(&self, account_id: &str) -> Result<Option<DeviceAuth>, KeyringStoreError>;

    /// Stores the DeviceAuth credential under service `"swch:epic"` and user `<auth.account_id>`.
    /// Overwrites any pre-existing entry for this account ID.
    fn set_device_auth(&self, auth: &DeviceAuth) -> Result<(), KeyringStoreError>;

    /// Deletes the DeviceAuth for a given Epic account ID.
    /// Idempotent: returns `Ok(())` if the credential does not exist.
    fn delete_device_auth(&self, account_id: &str) -> Result<(), KeyringStoreError>;
}

/// Persistent credential store backed by the native OS Keyring.
pub struct OsKeyringStore {
    service: String,
}

impl Default for OsKeyringStore {
    fn default() -> Self {
        Self::new(DEFAULT_EPIC_SERVICE)
    }
}

impl OsKeyringStore {
    /// Creates a new OS keyring store with a custom service name.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// Internal helper to obtain a keyring Entry.
    fn entry(&self, account_id: &str) -> Result<keyring::Entry, KeyringStoreError> {
        keyring::Entry::new(&self.service, account_id).map_err(KeyringStoreError::Keyring)
    }
}

impl CredentialStore for OsKeyringStore {
    fn get_device_auth(&self, account_id: &str) -> Result<Option<DeviceAuth>, KeyringStoreError> {
        let entry = self.entry(account_id)?;
        match entry.get_password() {
            Ok(json_str) => {
                let auth: DeviceAuth = serde_json::from_str(&json_str)?;
                Ok(Some(auth))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(KeyringStoreError::Keyring(err)),
        }
    }

    fn set_device_auth(&self, auth: &DeviceAuth) -> Result<(), KeyringStoreError> {
        let entry = self.entry(&auth.account_id)?;
        let json_str = serde_json::to_string(auth)?;
        entry.set_password(&json_str).map_err(KeyringStoreError::Keyring)?;
        Ok(())
    }

    fn delete_device_auth(&self, account_id: &str) -> Result<(), KeyringStoreError> {
        let entry = self.entry(account_id)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Idempotent: already absent
            Err(err) => Err(KeyringStoreError::Keyring(err)),
        }
    }
}

/// Thread-safe in-memory credential store for tests, CI runners, and headless environments.
#[derive(Debug, Clone, Default)]
pub struct MemoryCredentialStore {
    storage: Arc<RwLock<HashMap<String, DeviceAuth>>>,
}

impl MemoryCredentialStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self {
            storage: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initializes a store pre-populated with credentials.
    pub fn from_entries(entries: impl IntoIterator<Item = DeviceAuth>) -> Self {
        let store = Self::new();
        if let Ok(mut map) = store.storage.write() {
            for entry in entries {
                map.insert(entry.account_id.clone(), entry);
            }
        }
        store
    }

    /// Returns the number of accounts currently stored.
    pub fn len(&self) -> usize {
        self.storage.read().map(|m| m.len()).unwrap_or(0)
    }

    /// Returns true if no credentials are stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clears all credentials from the store.
    pub fn clear(&self) -> Result<(), KeyringStoreError> {
        let mut map = self
            .storage
            .write()
            .map_err(|e| KeyringStoreError::LockPoisoned(e.to_string()))?;
        map.clear();
        Ok(())
    }

    /// Takes a point-in-time snapshot of stored credentials.
    pub fn snapshot(&self) -> Result<HashMap<String, DeviceAuth>, KeyringStoreError> {
        let map = self
            .storage
            .read()
            .map_err(|e| KeyringStoreError::LockPoisoned(e.to_string()))?;
        Ok(map.clone())
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn get_device_auth(&self, account_id: &str) -> Result<Option<DeviceAuth>, KeyringStoreError> {
        let map = self
            .storage
            .read()
            .map_err(|e| KeyringStoreError::LockPoisoned(e.to_string()))?;
        Ok(map.get(account_id).cloned())
    }

    fn set_device_auth(&self, auth: &DeviceAuth) -> Result<(), KeyringStoreError> {
        let mut map = self
            .storage
            .write()
            .map_err(|e| KeyringStoreError::LockPoisoned(e.to_string()))?;
        map.insert(auth.account_id.clone(), auth.clone());
        Ok(())
    }

    fn delete_device_auth(&self, account_id: &str) -> Result<(), KeyringStoreError> {
        let mut map = self
            .storage
            .write()
            .map_err(|e| KeyringStoreError::LockPoisoned(e.to_string()))?;
        map.remove(account_id);
        Ok(())
    }
}

/// Checks if the operating system keyring is accessible without creating permanent entries.
pub fn probe_os_keyring() -> bool {
    let probe_service = "swch:epic:probe";
    let probe_user = "__swch_probe__";

    let entry = match keyring::Entry::new(probe_service, probe_user) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!("OS Keyring probe failed on Entry::new: {e}");
            return false;
        }
    };

    match entry.get_password() {
        Ok(_) | Err(keyring::Error::NoEntry) => true,
        Err(keyring::Error::NoStorageAccess(err)) => {
            tracing::warn!("OS Keyring probe: No storage access ({err})");
            false
        }
        Err(keyring::Error::PlatformFailure(err)) => {
            tracing::warn!("OS Keyring probe: Platform failure ({err})");
            false
        }
        Err(err) => {
            tracing::warn!("OS Keyring probe: Unexpected error ({err})");
            false
        }
    }
}

/// Resolves the default credential store for swch2.
///
/// Priority:
/// 1. `SWCH_KEYRING_BACKEND=memory` -> Forces `MemoryCredentialStore`.
/// 2. `SWCH_KEYRING_BACKEND=os` -> Forces `OsKeyringStore`.
/// 3. Runtime probe of OS Keyring -> If accessible, returns `OsKeyringStore`.
/// 4. Fallback -> Returns `MemoryCredentialStore` with a warning log.
pub fn default_store() -> Arc<dyn CredentialStore> {
    if let Ok(backend) = std::env::var("SWCH_KEYRING_BACKEND") {
        if backend.eq_ignore_ascii_case("memory") {
            tracing::info!("CredentialStore: Using MemoryCredentialStore (SWCH_KEYRING_BACKEND=memory)");
            return Arc::new(MemoryCredentialStore::new());
        }
        if backend.eq_ignore_ascii_case("os") {
            tracing::info!("CredentialStore: Using OsKeyringStore (SWCH_KEYRING_BACKEND=os)");
            return Arc::new(OsKeyringStore::default());
        }
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, unsigned/dev builds hang or trigger modal OS SecurityAgent prompts
        // on SecKeychain operations. All credentials are fully and durably stored in SQLite (epic_credentials),
        // so MemoryCredentialStore provides fast, non-blocking in-memory access unless SWCH_KEYRING_BACKEND=os is forced.
        tracing::info!("CredentialStore: macOS detected; using MemoryCredentialStore with SQLite persistence");
        Arc::new(MemoryCredentialStore::new())
    }

    #[cfg(not(target_os = "macos"))]
    {
        if probe_os_keyring() {
            tracing::info!("CredentialStore: OS Keyring is accessible; using OsKeyringStore");
            Arc::new(OsKeyringStore::default())
        } else {
            tracing::warn!(
                "CredentialStore: OS Keyring is unavailable or denied access. Falling back to MemoryCredentialStore."
            );
            Arc::new(MemoryCredentialStore::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_auth(id: &str) -> DeviceAuth {
        DeviceAuth {
            account_id: id.to_string(),
            device_id: format!("dev_{id}"),
            secret: format!("sec_{id}"),
            user_agent: Some("swch_test".to_string()),
            created: None,
            ..Default::default()
        }
    }

    #[test]
    fn test_memory_store_crud_lifecycle() {
        let store = MemoryCredentialStore::new();
        assert!(store.is_empty());

        let auth1 = sample_auth("acc_1");
        let auth2 = sample_auth("acc_2");

        // Insert
        store.set_device_auth(&auth1).unwrap();
        store.set_device_auth(&auth2).unwrap();
        assert_eq!(store.len(), 2);

        // Retrieve
        let retrieved1 = store.get_device_auth("acc_1").unwrap().unwrap();
        assert_eq!(retrieved1, auth1);

        // Non-existent key returns None, not error
        assert!(store.get_device_auth("non_existent").unwrap().is_none());

        // Idempotent delete
        store.delete_device_auth("acc_1").unwrap();
        assert!(store.get_device_auth("acc_1").unwrap().is_none());
        assert_eq!(store.len(), 1);

        store.delete_device_auth("acc_1").unwrap(); // Second delete should succeed
        assert_eq!(store.len(), 1);

        // Clear
        store.clear().unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn test_memory_store_thread_safety() {
        let store = Arc::new(MemoryCredentialStore::new());
        let mut handles = Vec::new();

        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            handles.push(std::thread::spawn(move || {
                let id = format!("thread_acc_{i}");
                let auth = sample_auth(&id);
                store_clone.set_device_auth(&auth).unwrap();
                let read_back = store_clone.get_device_auth(&id).unwrap().unwrap();
                assert_eq!(read_back.account_id, id);
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(store.len(), 10);
    }

    #[test]
    fn test_device_auth_serialization_roundtrip() {
        let auth = sample_auth("epic_acc_42");
        let json = serde_json::to_string(&auth).unwrap();

        let deserialized: DeviceAuth = serde_json::from_str(&json).unwrap();
        assert_eq!(auth, deserialized);
    }

    #[test]
    fn test_device_auth_debug_redaction() {
        let auth = sample_auth("acc_secure");
        let debug_str = format!("{auth:?}");
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("sec_acc_secure"));
    }

    #[test]
    fn test_default_store_env_override() {
        std::env::set_var("SWCH_KEYRING_BACKEND", "memory");
        let store = default_store();
        let auth = sample_auth("env_acc");
        store.set_device_auth(&auth).unwrap();
        assert_eq!(store.get_device_auth("env_acc").unwrap().unwrap(), auth);
        std::env::remove_var("SWCH_KEYRING_BACKEND");
    }
}
