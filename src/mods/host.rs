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
    pub jobs: Arc<super::jobs::JobStore>,
    pub files: Arc<super::files::FileStore>,
    pub tools: Arc<super::tools::ToolManager>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mod_storage_can_exceed_ten_mebibytes() {
        let directory = tempfile::tempdir().unwrap();
        let events = broadcast::channel(8).0;
        let services = Services {
            root: directory.path().to_path_buf(),
            db: Database::open_in_memory().unwrap(),
            gate: Arc::new(Mutex::new(())),
            jobs: Arc::new(super::super::jobs::JobStore::new(directory.path().to_path_buf(),events.clone())),
            files: Arc::new(super::super::files::FileStore::new(directory.path().to_path_buf())),
            tools: Arc::new(super::super::tools::ToolManager::new()),
            events,
        };
        let value = "x".repeat(11 * 1024 * 1024);
        services
            .call(
                "large.mod",
                "storage.set".into(),
                json!({"key":"payload","value":value}),
            )
            .await
            .unwrap();
        assert_eq!(
            services
                .call("large.mod", "storage.get".into(), json!({"key":"payload"}))
                .await
                .unwrap(),
            value
        );
        services
            .call(
                "large.mod",
                "storage.delete".into(),
                json!({"key":"payload"}),
            )
            .await
            .unwrap();
        assert_eq!(
            services
                .call("large.mod", "storage.get".into(), json!({"key":"payload"}))
                .await
                .unwrap(),
            Value::Null
        );
    }
}
impl Services {
    pub async fn call_from_runtime(&self,id:&str,method:String,params:Value)->Result<Value> {
        if method=="files.configure" { return Err("Change file locations through SWCH mod settings or the UI bridge so the mod can be stopped and restarted safely".into()); }
        self.call(id,method,params).await
    }
    pub async fn call(&self, id: &str, method: String, params: Value) -> Result<Value> {
        if !valid_id(id) {
            return Err("Invalid mod ID".into());
        }
        if method.starts_with("jobs.") { return self.jobs.call(id,&method,params).await; }
        if method.starts_with("files.") {
            if method=="files.configure" && params["kind"]=="tools" { self.tools.stop_mod(id).await; }
            let value=self.files.call(id,&method,params).await?;
            if method=="files.configure" { self.tools.relocate(id,&self.files.directory(id,"tools")?).await; }
            return Ok(value);
        }
        if method.starts_with("tools.") { return self.tools.call(id,&method,params,&self.files.directory(id,"data")?).await; }
        if method.starts_with("dialogs.") || method.starts_with("clipboard.") || method=="shell.openFolder" {
            return super::desktop::call(&method,params).await;
        }
        match method.as_str() {
            "library.refreshSteam" => {
                let result = if let Some(account_id)=params["account_id"].as_str() {
                    crate::server::steam::sync_library(axum::extract::State(self.db.clone()),axum::Json(crate::server::steam::SyncRequest{account_id:account_id.into(),api_key:None,access_token:None})).await
                } else { crate::server::steam::get_library(axum::extract::State(self.db.clone())).await };
                let value=result.map_err(|(_,axum::Json(body))|body.error)?;
                let _=self.events.send(json!({"event":"library.changed"}));
                return serde_json::to_value(value.0).map_err(error);
            }
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
