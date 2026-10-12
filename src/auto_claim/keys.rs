//! Key material lives in the OS vault. Public receipts contain no redeemable
//! code and never imply that the destination account owns the game.
use super::offers::Offer;
use crate::storage::{Account, Database};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Receipt {
    pub id: String,
    pub source_platform: String,
    pub source_account: String,
    pub offer_id: String,
    pub title: String,
    pub url: String,
    pub target_platform: Option<String>,
    pub target_account: Option<String>,
    pub status: String,
    pub received_at: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Secret {
    receipt: Receipt,
    code: String,
}
pub fn receipt_id(platform: &str, account: &str, offer: &str) -> String {
    let input = serde_json::to_vec(&(platform, account, offer)).unwrap();
    format!("{:x}", Sha256::digest(input))
}
pub fn valid_code(code: &str) -> bool {
    (8..=128).contains(&code.len())
        && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && (code.bytes().any(|b| b.is_ascii_digit()) || {
            let parts = code.split('-').collect::<Vec<_>>();
            matches!(parts.len(), 3 | 5) && parts.iter().all(|p| p.len() == 5)
        })
}
pub fn receive(
    account: &Account,
    offer: &Offer,
    code: String,
    target: Option<String>,
) -> Result<Receipt, String> {
    if !valid_code(&code) {
        return Err("Магазин не выдал распознаваемый игровой ключ.".into());
    }
    let id = receipt_id(&account.launcher, &account.id, &offer.key());
    let receipt = Receipt {
        id: id.clone(),
        source_platform: account.launcher.clone(),
        source_account: account.id.clone(),
        offer_id: offer.key(),
        title: super::catalog::canonical_game_title(&offer.title),
        url: offer.url.clone(),
        target_platform: target
            .filter(|p| matches!(p.as_str(), "steam" | "gog" | "microsoft" | "epic")),
        target_account: None,
        status: "code_received".into(),
        received_at: chrono::Utc::now().to_rfc3339(),
    };
    let entry = keyring::Entry::new("swch:claim-keys", &id)
        .map_err(|_| "Не удалось открыть хранилище ключей.".to_string())?;
    match entry.get_password() {
        Ok(value) => {
            let existing: Secret = serde_json::from_str(&value)
                .map_err(|_| "Сохранённый ключ повреждён.".to_string())?;
            if existing.receipt.id != id || existing.code != code {
                return Err(
                    "Магазин показал другой ключ; сохранённый ключ оставлен в хранилище ОС.".into(),
                );
            }
            return Ok(existing.receipt);
        }
        Err(keyring::Error::NoEntry) => {}
        Err(_) => return Err("Не удалось прочитать сохранённый ключ.".into()),
    }
    entry
        .set_password(
            &serde_json::to_string(&Secret {
                receipt: receipt.clone(),
                code,
            })
            .map_err(|_| "Не удалось сохранить ключ.".to_string())?,
        )
        .map_err(|_| "Не удалось сохранить игровой ключ в хранилище ОС.".to_string())?;
    Ok(receipt)
}
pub fn read_receipt(id: &str) -> Result<Receipt, String> {
    Ok(read_secret(id)?.receipt)
}
fn read_secret(id: &str) -> Result<Secret, String> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Некорректный номер ключа.".into());
    }
    let raw = keyring::Entry::new("swch:claim-keys", id)
        .and_then(|e| e.get_password())
        .map_err(|_| "Ключ недоступен в хранилище ОС.".to_string())?;
    let secret: Secret =
        serde_json::from_str(&raw).map_err(|_| "Сохранённый ключ повреждён.".to_string())?;
    if secret.receipt.id != id || !valid_code(&secret.code) {
        return Err("Ключ относится к другой раздаче.".into());
    }
    Ok(secret)
}
pub fn reveal(id: &str) -> Result<String, String> {
    Ok(read_secret(id)?.code)
}
pub fn steam_receipt_proof(data: &serde_json::Value, title: &str) -> Result<String, String> {
    let same_title = |a: &str, b: &str| {
        a.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
            == b.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
    };
    if data["success"].as_u64() != Some(1) || data["purchase_result_details"].as_u64() != Some(0) {
        return Err(
            "Steam не подтвердил активацию ключа. Проверь библиотеку выбранного аккаунта.".into(),
        );
    }
    let receipt = &data["purchase_receipt_info"];
    let items = receipt["line_items"]
        .as_array()
        .ok_or("Steam не прислал подтверждение игры для ключа.")?;
    if items.len() != 1
        || !items[0]["line_item_description"]
            .as_str()
            .is_some_and(|name| same_title(name, title))
    {
        return Err("Steam не подтвердил название игры из очереди ключей; проверь библиотеку выбранного аккаунта.".into());
    }
    items[0]["packageid"]
        .as_u64()
        .filter(|v| *v > 0)
        .map(|v| v.to_string())
        .ok_or("Steam не сообщил номер активированной лицензии.".into())
}
pub async fn list(db: &Database) -> Result<Vec<Receipt>, String> {
    db.auto_claim_values("key-receipt:")
        .await
        .map_err(|_| "Не удалось прочитать очередь ключей.".to_string())?
        .iter()
        .map(|v| serde_json::from_str(v).map_err(|_| "Очередь ключей повреждена.".to_string()))
        .collect()
}
pub async fn record(db: &Database, receipt: Receipt) -> Result<(), String> {
    db.record_auto_claim_key(
        &receipt.id,
        &serde_json::to_string(&receipt)
            .map_err(|_| "Не удалось сохранить очередь ключей.".to_string())?,
    )
    .await
    .map_err(|_| {
        "Ключ сохранён в ОС, но очередь недоступна; повторная проверка восстановит запись.".into()
    })
}
