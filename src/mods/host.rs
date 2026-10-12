use super::{error, read_json, types::*, write_json};
use crate::storage::Database;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{broadcast, Mutex};

pub struct Services {
    pub root: PathBuf,
    pub db: Database,
    pub gate: Arc<Mutex<()>>,
    pub events: broadcast::Sender<Value>,
}
impl Services {
    pub async fn call(&self, id: &str, method: String, params: Value) -> Result<Value> {
        if !valid_id(id) {
            return Err("Invalid mod ID".into());
        }
        match method.as_str() {
            "library.list" => {
                return serde_json::to_value(self.db.get_games().await.map_err(error)?)
                    .map_err(error)
            }
            "accounts.list" => {
                return serde_json::to_value(self.db.get_accounts().await.map_err(error)?)
                    .map_err(error)
            }
            "event" => {
                let event = params["event"]
                    .as_str()
                    .filter(|e| valid_id(e))
                    .ok_or("Invalid event")?;
                let _ = self
                    .events
                    .send(json!({"event":format!("mod:{id}:{event}"),"data":params["data"]}));
                return Ok(Value::Null);
            }
            _ => {}
        }
        let _gate = self.gate.lock().await;
        let dir = self.root.join("data").join(id);
        if method == "settings.get" {
            return read_json(&dir.join("settings.json"));
        }
        if method == "settings.set" {
            if !params["value"].is_object() {
                return Err("Settings must be an object".into());
            }
            write_json(&dir.join("settings.json"), &params["value"])?;
            return Ok(Value::Null);
        }
        let key = params["key"]
            .as_str()
            .filter(|s| valid_id(s))
            .ok_or("Invalid storage key")?;
        if method.starts_with("secrets.") {
            let entry = keyring::Entry::new(&format!("swch-mod:{id}"), key).map_err(error)?;
            match method.as_str() {
                "secrets.get" => {
                    return match entry.get_password() {
                        Ok(value) => Ok(json!(value)),
                        Err(keyring::Error::NoEntry) => Ok(Value::Null),
                        Err(e) => Err(e.to_string()),
                    }
                }
                "secrets.set" => {
                    let value = params["value"].as_str().ok_or("Secret must be a string")?;
                    let mut keys = read_json(&dir.join("secret-keys.json"))?
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    if !keys.iter().any(|k| k == key) {
                        keys.push(json!(key));
                    }
                    write_json(&dir.join("secret-keys.json"), &keys)?;
                    entry.set_password(value).map_err(error)?;
                    return Ok(Value::Null);
                }
                "secrets.delete" => {
                    match entry.delete_credential() {
                        Ok(()) | Err(keyring::Error::NoEntry) => {}
                        Err(e) => return Err(e.to_string()),
                    }
                    return Ok(Value::Null);
                }
                _ => return Err("Unknown SDK method".into()),
            }
        }
        let path = dir.join("storage.json");
        let mut data = read_json(&path)?;
        if !data.is_object() {
            return Err("Invalid mod storage".into());
        }
        match method.as_str() {
            "storage.get" => Ok(data[key].clone()),
            "storage.set" => {
                data[key] = params["value"].clone();
                if serde_json::to_vec(&data).map_err(error)?.len() > 10 * MAX_MESSAGE {
                    return Err("Mod storage exceeds 10 MiB".into());
                }
                write_json(&path, &data)?;
                Ok(Value::Null)
            }
            "storage.delete" => {
                data.as_object_mut()
                    .ok_or("Invalid mod storage")?
                    .remove(key);
                write_json(&path, &data)?;
                Ok(Value::Null)
            }
            _ => Err("Unknown SDK method".into()),
        }
    }
}
