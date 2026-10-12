//! Browser sessions remain exclusively in the OS credential vault. Windows
//! limits a credential blob to 2560 bytes (passwords additionally use UTF-16),
//! so session JSON is stored as bounded raw UTF-8 parts with an atomic head.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;

const ROOT_SERVICE: &str = "swch:claims-web";
const PART_SERVICE: &str = "swch:claims-web:parts";
const JOURNAL_SERVICE: &str = "swch:claims-web:transactions";
const PART_BYTES: usize = 2400;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_PARTS: usize = MAX_BYTES.div_ceil(PART_BYTES);
const MANIFEST_KIND: &str = "swch-web-parts";
const JOURNAL_KIND: &str = "swch-web-transaction";
static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    kind: String,
    version: u8,
    generation: String,
    parts: usize,
    bytes: usize,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    kind: String,
    version: u8,
    previous: Option<Manifest>,
    next: Manifest,
}

#[derive(Clone)]
struct Key {
    service: &'static str,
    user: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Backend,
    Missing,
    Corrupt,
    TooLarge,
}

trait Vault {
    fn password(&self, key: &Key) -> Result<Option<String>, Failure>;
    fn secret(&self, key: &Key) -> Result<Option<Vec<u8>>, Failure>;
    fn put_password(&self, key: &Key, value: &str) -> Result<(), Failure>;
    fn put_secret(&self, key: &Key, value: &[u8]) -> Result<(), Failure>;
    fn delete(&self, key: &Key) -> Result<(), Failure>;
}

struct OsVault;
impl Vault for OsVault {
    fn password(&self, key: &Key) -> Result<Option<String>, Failure> {
        match keyring::Entry::new(key.service, &key.user).and_then(|entry| entry.get_password()) {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(Failure::Backend),
        }
    }

    fn secret(&self, key: &Key) -> Result<Option<Vec<u8>>, Failure> {
        match keyring::Entry::new(key.service, &key.user).and_then(|entry| entry.get_secret()) {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(Failure::Backend),
        }
    }

    fn put_password(&self, key: &Key, value: &str) -> Result<(), Failure> {
        keyring::Entry::new(key.service, &key.user)
            .and_then(|entry| entry.set_password(value))
            .map_err(|_| Failure::Backend)
    }

    fn put_secret(&self, key: &Key, value: &[u8]) -> Result<(), Failure> {
        keyring::Entry::new(key.service, &key.user)
            .and_then(|entry| entry.set_secret(value))
            .map_err(|_| Failure::Backend)
    }

    fn delete(&self, key: &Key) -> Result<(), Failure> {
        match keyring::Entry::new(key.service, &key.user)
            .and_then(|entry| entry.delete_credential())
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(Failure::Backend),
        }
    }
}

fn account_key(platform: &str, id: &str) -> Result<String, Failure> {
    if super::platforms::platform(platform).is_none()
        || id.is_empty()
        || id.len() > 256
        || id.chars().any(char::is_control)
    {
        return Err(Failure::Corrupt);
    }
    Ok(format!("{platform}:{id}"))
}

fn root_key(account: &str) -> Key {
    Key {
        service: ROOT_SERVICE,
        user: account.into(),
    }
}

fn journal_key(account: &str) -> Key {
    Key {
        service: JOURNAL_SERVICE,
        user: account.into(),
    }
}

fn part_key(account: &str, generation: &str, part: usize) -> Key {
    Key {
        service: PART_SERVICE,
        user: format!("{account}:{generation}:{part}"),
    }
}

fn digest(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hex_string(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl Manifest {
    fn validate(&self) -> Result<(), Failure> {
        if self.kind != MANIFEST_KIND
            || self.version != 1
            || !hex_string(&self.generation, 32)
            || !hex_string(&self.sha256, 64)
            || self.bytes == 0
            || self.bytes > MAX_BYTES
            || self.parts == 0
            || self.parts > MAX_PARTS
            || self.parts != self.bytes.div_ceil(PART_BYTES)
        {
            return Err(Failure::Corrupt);
        }
        Ok(())
    }
}

fn parse_manifest(raw: &str) -> Result<Option<Manifest>, Failure> {
    if raw.len() > MAX_BYTES {
        return Err(Failure::Corrupt);
    }
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|_| Failure::Corrupt)?;
    if value.get("kind").is_none() {
        return Ok(None);
    }
    let manifest: Manifest = serde_json::from_value(value).map_err(|_| Failure::Corrupt)?;
    manifest.validate()?;
    Ok(Some(manifest))
}

fn read_journal(vault: &impl Vault, account: &str) -> Result<Option<Journal>, Failure> {
    let Some(raw) = vault.password(&journal_key(account))? else {
        return Ok(None);
    };
    if raw.encode_utf16().count() > 1280 {
        return Err(Failure::Corrupt);
    }
    let journal: Journal = serde_json::from_str(&raw).map_err(|_| Failure::Corrupt)?;
    if journal.kind != JOURNAL_KIND || journal.version != 1 {
        return Err(Failure::Corrupt);
    }
    journal.next.validate()?;
    if let Some(previous) = &journal.previous {
        previous.validate()?;
        if previous.generation == journal.next.generation {
            return Err(Failure::Corrupt);
        }
    }
    Ok(Some(journal))
}

fn delete_parts(vault: &impl Vault, account: &str, manifest: &Manifest) -> Result<(), Failure> {
    manifest.validate()?;
    let mut failed = false;
    for part in 0..manifest.parts {
        failed |= vault
            .delete(&part_key(account, &manifest.generation, part))
            .is_err();
    }
    if failed {
        Err(Failure::Backend)
    } else {
        Ok(())
    }
}

/// A journal contains only bounded generation metadata, never a legacy session
/// or cookie. Keep the committed generation; clean the other one after a crash.
fn recover(vault: &impl Vault, account: &str, current: Option<&Manifest>) -> Result<(), Failure> {
    let Some(journal) = read_journal(vault, account)? else {
        return Ok(());
    };
    if current.is_some_and(|manifest| manifest.generation == journal.next.generation) {
        if let Some(previous) = &journal.previous {
            delete_parts(vault, account, previous)?;
        }
    } else {
        if let Some(current) = current {
            if !journal
                .previous
                .as_ref()
                .is_some_and(|previous| previous.generation == current.generation)
            {
                return Err(Failure::Corrupt);
            }
        }
        delete_parts(vault, account, &journal.next)?;
    }
    vault.delete(&journal_key(account))
}

fn load_from(vault: &impl Vault, account: &str) -> Result<String, Failure> {
    let raw = vault
        .password(&root_key(account))?
        .ok_or(Failure::Missing)?;
    let manifest = parse_manifest(&raw)?;
    if let Err(error) = recover(vault, account, manifest.as_ref()) {
        // A failed cleanup of retired parts must not hide a valid committed
        // session. The journal stays for retry; current parts are still fully
        // length/hash checked below. Invalid transaction metadata fails closed.
        if error != Failure::Backend {
            return Err(error);
        }
    }
    let Some(manifest) = manifest else {
        return Ok(raw);
    };
    let mut payload = Vec::with_capacity(manifest.bytes);
    for part in 0..manifest.parts {
        let bytes = vault
            .secret(&part_key(account, &manifest.generation, part))?
            .ok_or(Failure::Corrupt)?;
        if bytes.len() != PART_BYTES.min(manifest.bytes - part * PART_BYTES) {
            return Err(Failure::Corrupt);
        }
        payload.extend_from_slice(&bytes);
    }
    if digest(&payload) != manifest.sha256 {
        return Err(Failure::Corrupt);
    }
    String::from_utf8(payload).map_err(|_| Failure::Corrupt)
}

fn save_generation(
    vault: &impl Vault,
    account: &str,
    payload: &str,
    generation: String,
) -> Result<(), Failure> {
    if payload.is_empty() || payload.len() > MAX_BYTES {
        return Err(Failure::TooLarge);
    }
    let current_raw = vault.password(&root_key(account))?;
    let previous = current_raw
        .as_deref()
        .map(parse_manifest)
        .transpose()?
        .flatten();
    recover(vault, account, previous.as_ref())?;
    let next = Manifest {
        kind: MANIFEST_KIND.into(),
        version: 1,
        generation,
        parts: payload.len().div_ceil(PART_BYTES),
        bytes: payload.len(),
        sha256: digest(payload.as_bytes()),
    };
    next.validate()?;
    if previous
        .as_ref()
        .is_some_and(|previous| previous.generation == next.generation)
    {
        return Err(Failure::Corrupt);
    }
    let journal = Journal {
        kind: JOURNAL_KIND.into(),
        version: 1,
        previous,
        next,
    };
    let journal_raw = serde_json::to_string(&journal).map_err(|_| Failure::Corrupt)?;
    if journal_raw.encode_utf16().count() > 1280 {
        return Err(Failure::Corrupt);
    }
    vault.put_password(&journal_key(account), &journal_raw)?;
    let write_result = (|| {
        for (part, bytes) in payload.as_bytes().chunks(PART_BYTES).enumerate() {
            vault.put_secret(&part_key(account, &journal.next.generation, part), bytes)?;
        }
        let head = serde_json::to_string(&journal.next).map_err(|_| Failure::Corrupt)?;
        vault.put_password(&root_key(account), &head)
    })();
    if let Err(error) = write_result {
        // Do not alter the previous committed head if any new part or commit fails.
        if delete_parts(vault, account, &journal.next).is_ok() {
            let _ = vault.delete(&journal_key(account));
        }
        return Err(error);
    }
    // The new generation is committed. Retain the journal if cleanup fails so
    // the next load/save/delete can still find and remove the retired parts.
    let _ = recover(vault, account, Some(&journal.next));
    Ok(())
}

fn delete_from(vault: &impl Vault, account: &str) -> Result<(), Failure> {
    let current_raw = vault.password(&root_key(account))?;
    let current = current_raw
        .as_deref()
        .map(parse_manifest)
        .transpose()?
        .flatten();
    let journal = read_journal(vault, account)?;
    let mut failed = false;
    if let Some(current) = &current {
        failed |= delete_parts(vault, account, current).is_err();
    }
    if let Some(journal) = &journal {
        failed |= delete_parts(vault, account, &journal.next).is_err();
        if let Some(previous) = &journal.previous {
            failed |= delete_parts(vault, account, previous).is_err();
        }
    }
    if failed {
        return Err(Failure::Backend);
    }
    vault.delete(&root_key(account))?;
    vault.delete(&journal_key(account))
}

pub(super) fn load(platform: &str, id: &str) -> Result<String, String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Не удалось прочитать хранилище входа.".to_string())?;
    let account =
        account_key(platform, id).map_err(|_| "Некорректный аккаунт магазина.".to_string())?;
    load_from(&OsVault, &account).map_err(|error| {
        match error {
            Failure::Missing => {
                "Подключи браузерный вход этого аккаунта в настройках автоклеймера."
            }
            Failure::Corrupt => "Браузерный вход повреждён; подключи его заново.",
            _ => "Не удалось прочитать браузерный вход из хранилища ОС.",
        }
        .into()
    })
}

pub(super) fn save(platform: &str, id: &str, payload: &str) -> Result<(), String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Не удалось открыть хранилище входа.".to_string())?;
    let account =
        account_key(platform, id).map_err(|_| "Некорректный аккаунт магазина.".to_string())?;
    let mut random = [0_u8; 16];
    getrandom::getrandom(&mut random)
        .map_err(|_| "Не удалось подготовить защищённое сохранение.".to_string())?;
    let generation = random.iter().map(|byte| format!("{byte:02x}")).collect();
    save_generation(&OsVault, &account, payload, generation).map_err(|error| {
        if error == Failure::TooLarge {
            "Браузерный вход слишком большой для безопасного сохранения. Подключи его заново."
        } else {
            "Не удалось сохранить браузерный вход в хранилище ОС."
        }
        .into()
    })
}

pub(super) fn delete(platform: &str, id: &str) -> Result<(), String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "Не удалось открыть хранилище входа.".to_string())?;
    let account =
        account_key(platform, id).map_err(|_| "Некорректный аккаунт магазина.".to_string())?;
    delete_from(&OsVault, &account)
        .map_err(|_| "Не удалось удалить браузерный вход из хранилища ОС.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};

    #[derive(Default)]
    struct MemoryVault {
        entries: RefCell<HashMap<(String, String), Vec<u8>>>,
        failed_write: RefCell<Option<(String, String)>>,
        failed_delete: RefCell<Option<(String, String)>>,
        failed_read: RefCell<Option<(String, String)>>,
    }

    fn identity(key: &Key) -> (String, String) {
        (key.service.into(), key.user.clone())
    }

    impl Vault for MemoryVault {
        fn password(&self, key: &Key) -> Result<Option<String>, Failure> {
            self.secret(key)?
                .map(|bytes| String::from_utf8(bytes).map_err(|_| Failure::Corrupt))
                .transpose()
        }
        fn secret(&self, key: &Key) -> Result<Option<Vec<u8>>, Failure> {
            if self.failed_read.borrow().as_ref() == Some(&identity(key)) {
                return Err(Failure::Backend);
            }
            Ok(self.entries.borrow().get(&identity(key)).cloned())
        }
        fn put_password(&self, key: &Key, value: &str) -> Result<(), Failure> {
            if value.encode_utf16().count() > 1280 {
                return Err(Failure::TooLarge);
            }
            self.put_secret(key, value.as_bytes())
        }
        fn put_secret(&self, key: &Key, value: &[u8]) -> Result<(), Failure> {
            if value.len() > 2560 {
                return Err(Failure::TooLarge);
            }
            if self.failed_write.borrow().as_ref() == Some(&identity(key)) {
                return Err(Failure::Backend);
            }
            self.entries
                .borrow_mut()
                .insert(identity(key), value.to_vec());
            Ok(())
        }
        fn delete(&self, key: &Key) -> Result<(), Failure> {
            if self.failed_delete.borrow().as_ref() == Some(&identity(key)) {
                return Err(Failure::Backend);
            }
            self.entries.borrow_mut().remove(&identity(key));
            Ok(())
        }
    }

    fn generation(n: u8) -> String {
        format!("{n:032x}")
    }
    fn manifest(vault: &MemoryVault, account: &str) -> Manifest {
        parse_manifest(&vault.password(&root_key(account)).unwrap().unwrap())
            .unwrap()
            .unwrap()
    }

    #[test]
    fn web_vault_round_trips_large_utf8_and_deletes_every_part() {
        let vault = MemoryVault::default();
        let payload = serde_json::json!({"fixture": "Игры🎮".repeat(4000)}).to_string();
        save_generation(&vault, "epic:fixture", &payload, generation(1)).unwrap();
        assert!(manifest(&vault, "epic:fixture").parts > 1);
        assert_eq!(load_from(&vault, "epic:fixture").unwrap(), payload);
        assert!(vault
            .entries
            .borrow()
            .values()
            .all(|bytes| bytes.len() <= 2560));
        delete_from(&vault, "epic:fixture").unwrap();
        delete_from(&vault, "epic:fixture").unwrap();
        assert!(vault.entries.borrow().is_empty());
    }

    #[test]
    fn web_vault_failed_part_or_commit_preserves_old_legacy_session() {
        for failed in [
            part_key("epic:fixture", &generation(2), 1),
            root_key("epic:fixture"),
        ] {
            let vault = MemoryVault::default();
            vault
                .put_password(&root_key("epic:fixture"), r#"{"legacy":"old"}"#)
                .unwrap();
            *vault.failed_write.borrow_mut() = Some(identity(&failed));
            assert_eq!(
                save_generation(&vault, "epic:fixture", &"x".repeat(5000), generation(2)),
                Err(Failure::Backend)
            );
            assert_eq!(
                load_from(&vault, "epic:fixture").unwrap(),
                r#"{"legacy":"old"}"#
            );
            assert_eq!(vault.entries.borrow().len(), 1);
        }
    }

    #[test]
    fn web_vault_failed_replacement_keeps_committed_generation_and_cleans_new_parts() {
        let vault = MemoryVault::default();
        let old = "o".repeat(5000);
        save_generation(&vault, "epic:fixture", &old, generation(1)).unwrap();
        *vault.failed_write.borrow_mut() =
            Some(identity(&part_key("epic:fixture", &generation(2), 1)));
        assert_eq!(
            save_generation(&vault, "epic:fixture", &"n".repeat(7000), generation(2)),
            Err(Failure::Backend)
        );
        assert_eq!(load_from(&vault, "epic:fixture").unwrap(), old);
        assert_eq!(vault.entries.borrow().len(), 4);
    }

    #[test]
    fn web_vault_journal_recovers_precommit_and_postcommit_interruption() {
        let vault = MemoryVault::default();
        save_generation(&vault, "epic:fixture", &"o".repeat(5000), generation(1)).unwrap();
        let previous = manifest(&vault, "epic:fixture");
        let payload = "n".repeat(7000);
        let next = Manifest {
            kind: MANIFEST_KIND.into(),
            version: 1,
            generation: generation(2),
            parts: 3,
            bytes: payload.len(),
            sha256: digest(payload.as_bytes()),
        };
        let journal = Journal {
            kind: JOURNAL_KIND.into(),
            version: 1,
            previous: Some(previous.clone()),
            next: next.clone(),
        };
        let raw = serde_json::to_string(&journal).unwrap();
        vault
            .put_password(&journal_key("epic:fixture"), &raw)
            .unwrap();
        vault
            .put_secret(
                &part_key("epic:fixture", &next.generation, 0),
                &payload.as_bytes()[..PART_BYTES],
            )
            .unwrap();
        assert_eq!(load_from(&vault, "epic:fixture").unwrap(), "o".repeat(5000));
        assert!(vault
            .password(&journal_key("epic:fixture"))
            .unwrap()
            .is_none());
        assert!(vault
            .secret(&part_key("epic:fixture", &next.generation, 0))
            .unwrap()
            .is_none());

        vault
            .put_password(&journal_key("epic:fixture"), &raw)
            .unwrap();
        for (part, bytes) in payload.as_bytes().chunks(PART_BYTES).enumerate() {
            vault
                .put_secret(&part_key("epic:fixture", &next.generation, part), bytes)
                .unwrap();
        }
        vault
            .put_password(
                &root_key("epic:fixture"),
                &serde_json::to_string(&next).unwrap(),
            )
            .unwrap();
        assert_eq!(load_from(&vault, "epic:fixture").unwrap(), payload);
        assert!(vault
            .secret(&part_key("epic:fixture", &previous.generation, 0))
            .unwrap()
            .is_none());
        assert_eq!(vault.entries.borrow().len(), 4);
    }

    #[test]
    fn web_vault_retains_cleanup_journal_until_all_secrets_can_be_deleted() {
        let vault = MemoryVault::default();
        save_generation(&vault, "epic:fixture", &"o".repeat(5000), generation(1)).unwrap();
        let old_part = part_key("epic:fixture", &generation(1), 0);
        *vault.failed_delete.borrow_mut() = Some(identity(&old_part));
        save_generation(&vault, "epic:fixture", &"n".repeat(7000), generation(2)).unwrap();
        assert!(vault
            .password(&journal_key("epic:fixture"))
            .unwrap()
            .is_some());
        assert_eq!(load_from(&vault, "epic:fixture").unwrap(), "n".repeat(7000));
        assert_eq!(delete_from(&vault, "epic:fixture"), Err(Failure::Backend));
        assert!(vault.password(&root_key("epic:fixture")).unwrap().is_some());
        assert!(vault
            .password(&journal_key("epic:fixture"))
            .unwrap()
            .is_some());
        *vault.failed_delete.borrow_mut() = None;
        delete_from(&vault, "epic:fixture").unwrap();
        assert!(vault.entries.borrow().is_empty());
    }

    #[test]
    fn web_vault_rejects_invalid_generation_checksum_and_read_errors() {
        let vault = MemoryVault::default();
        save_generation(&vault, "epic:fixture", &"x".repeat(5000), generation(1)).unwrap();
        let mut head = manifest(&vault, "epic:fixture");
        let part = part_key("epic:fixture", &head.generation, 0);
        *vault.failed_read.borrow_mut() = Some(identity(&part));
        assert_eq!(load_from(&vault, "epic:fixture"), Err(Failure::Backend));
        *vault.failed_read.borrow_mut() = None;
        vault
            .entries
            .borrow_mut()
            .get_mut(&identity(&part))
            .unwrap()[0] = b'z';
        assert_eq!(load_from(&vault, "epic:fixture"), Err(Failure::Corrupt));
        head.generation = "../../other-account".into();
        vault
            .put_password(
                &root_key("epic:fixture"),
                &serde_json::to_string(&head).unwrap(),
            )
            .unwrap();
        assert_eq!(load_from(&vault, "epic:fixture"), Err(Failure::Corrupt));
        assert_eq!(delete_from(&vault, "epic:fixture"), Err(Failure::Corrupt));
        assert_eq!(
            save_generation(
                &vault,
                "epic:fixture",
                &"x".repeat(MAX_BYTES + 1),
                generation(2)
            ),
            Err(Failure::TooLarge)
        );
    }
}
