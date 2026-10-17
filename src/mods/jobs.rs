use super::types::{valid_id, Result};
use crate::storage::atomic::atomic_write_json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use tokio::sync::{broadcast, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    pub input: Value,
    pub progress: Value,
    pub checkpoint: Value,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub attempt: u64,
}

impl Job {
    fn terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "cancelled")
    }
}

pub struct JobStore {
    root: PathBuf,
    events: broadcast::Sender<Value>,
    gate: Mutex<()>,
}

impl JobStore {
    pub fn new(root: PathBuf, events: broadcast::Sender<Value>) -> Self {
        Self {
            root,
            events,
            gate: Mutex::new(()),
        }
    }

    fn directory(&self, id: &str) -> PathBuf {
        self.root.join("data").join(id).join("jobs")
    }

    fn save(&self, mod_id: &str, job: &mut Job) -> Result<Value> {
        job.updated_at = chrono::Utc::now().to_rfc3339();
        atomic_write_json(self.directory(mod_id).join(format!("{}.json", job.id)), job)
            .map_err(|e| e.to_string())?;
        let value = serde_json::to_value(job).map_err(|e| e.to_string())?;
        let _ = self
            .events
            .send(json!({"event":format!("mod:{mod_id}:jobs.changed"),"data":value}));
        Ok(value)
    }

    fn list(&self, mod_id: &str) -> Result<Vec<Job>> {
        let entries = match fs::read_dir(self.directory(mod_id)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) => return Err(error.to_string()),
        };
        let mut jobs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let job: Job =
                serde_json::from_slice(&fs::read(entry.path()).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if !valid_id(&job.id) || !valid_id(&job.kind) {
                return Err("Invalid persisted job identity".into());
            }
            jobs.push(job);
        }
        jobs.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok(jobs)
    }

    fn get(&self, mod_id: &str, params: &Value) -> Result<Job> {
        let id = params["id"]
            .as_str()
            .filter(|id| valid_id(id))
            .ok_or("Invalid job ID")?;
        let bytes = fs::read(self.directory(mod_id).join(format!("{id}.json")))
            .map_err(|e| e.to_string())?;
        let job: Job = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if job.id != id || !valid_id(&job.kind) {
            return Err("Invalid persisted job identity".into());
        }
        Ok(job)
    }

    pub async fn call(&self, mod_id: &str, method: &str, params: Value) -> Result<Value> {
        if !valid_id(mod_id) {
            return Err("Invalid mod ID".into());
        }
        let _guard = self.gate.lock().await;
        match method {
            "jobs.start" => {
                let kind = params["type"]
                    .as_str()
                    .filter(|kind| valid_id(kind))
                    .ok_or("Invalid job type")?;
                let mut random = [0u8; 16];
                getrandom::getrandom(&mut random).map_err(|e| e.to_string())?;
                let id = random.iter().map(|byte| format!("{byte:02x}")).collect();
                let now = chrono::Utc::now().to_rfc3339();
                let mut job = Job {
                    id,
                    kind: kind.into(),
                    status: "queued".into(),
                    input: params["input"].clone(),
                    progress: Value::Null,
                    checkpoint: Value::Null,
                    result: None,
                    error: None,
                    created_at: now.clone(),
                    updated_at: now,
                    attempt: 0,
                };
                self.save(mod_id, &mut job)
            }
            "jobs.list" => serde_json::to_value(self.list(mod_id)?).map_err(|e| e.to_string()),
            "jobs.get" => {
                serde_json::to_value(self.get(mod_id, &params)?).map_err(|e| e.to_string())
            }
            "jobs.cancel" => {
                let mut job = self.get(mod_id, &params)?;
                if !job.terminal() {
                    job.status = "cancelled".into();
                    return self.save(mod_id, &mut job);
                }
                serde_json::to_value(job).map_err(|e| e.to_string())
            }
            "jobs.update" => {
                let mut job = self.get(mod_id, &params)?;
                if job.terminal() {
                    return serde_json::to_value(job).map_err(|e| e.to_string());
                }
                if let Some(status) = params.get("status") {
                    let status = status
                        .as_str()
                        .filter(|status| matches!(*status, "running" | "completed" | "failed"))
                        .ok_or("Invalid job status")?;
                    if status == "running" && job.status != "running" {
                        job.attempt += 1;
                    }
                    job.status = status.into();
                }
                if let Some(progress) = params.get("progress") {
                    job.progress = progress.clone();
                }
                if let Some(checkpoint) = params.get("checkpoint") {
                    job.checkpoint = checkpoint.clone();
                }
                if let Some(result) = params.get("result") {
                    job.result = Some(result.clone());
                }
                if let Some(error) = params.get("error") {
                    job.error = Some(error.as_str().ok_or("Job error must be a string")?.into());
                }
                self.save(mod_id, &mut job)
            }
            "jobs.recover" => {
                let mut jobs = self.list(mod_id)?;
                for job in &mut jobs {
                    if job.status == "running" {
                        job.status = "queued".into();
                        self.save(mod_id, job)?;
                    }
                }
                serde_json::to_value(
                    jobs.into_iter()
                        .filter(|job| job.status == "queued")
                        .collect::<Vec<_>>(),
                )
                .map_err(|e| e.to_string())
            }
            _ => Err("Unknown jobs method".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn jobs_persist_progress_checkpoint_result_and_resume_after_reopening() {
        let directory = tempfile::tempdir().unwrap();
        let (events, mut receiver) = broadcast::channel(32);
        let store = JobStore::new(directory.path().into(), events.clone());
        let created = store
            .call(
                "demo.mod",
                "jobs.start",
                json!({"type":"download","input":{"url":"https://example.com/file"}}),
            )
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap();
        assert_eq!(created["status"], "queued");
        assert_eq!(
            receiver.recv().await.unwrap()["event"],
            "mod:demo.mod:jobs.changed"
        );
        store.call("demo.mod", "jobs.update", json!({"id":id,"status":"running","progress":{"bytes":42},"checkpoint":{"offset":42}})).await.unwrap();
        drop(store);
        let store = JobStore::new(directory.path().into(), events);
        let recovered = store
            .call("demo.mod", "jobs.recover", json!({}))
            .await
            .unwrap();
        assert_eq!(recovered[0]["id"], id);
        assert_eq!(recovered[0]["progress"]["bytes"], 42);
        assert_eq!(recovered[0]["checkpoint"]["offset"], 42);
        assert_eq!(recovered[0]["input"], created["input"]);
        let complete = store
            .call(
                "demo.mod",
                "jobs.update",
                json!({"id":id,"status":"completed","result":{"file":"output.bin"}}),
            )
            .await
            .unwrap();
        assert_eq!(complete["result"]["file"], "output.bin");
        assert_eq!(
            store
                .call("demo.mod", "jobs.get", json!({"id":id}))
                .await
                .unwrap(),
            complete
        );
        assert!(store
            .call("demo.mod", "jobs.recover", json!({}))
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
        assert!(store
            .call("other.mod", "jobs.get", json!({"id":id}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn cancellation_is_final_and_paths_and_states_remain_validated() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path().into(), broadcast::channel(16).0);
        let job = store
            .call("demo.mod", "jobs.start", json!({"type":"download"}))
            .await
            .unwrap();
        let id = job["id"].as_str().unwrap();
        let cancelled = store
            .call("demo.mod", "jobs.cancel", json!({"id":id}))
            .await
            .unwrap();
        assert_eq!(cancelled["status"], "cancelled");
        assert_eq!(
            store
                .call(
                    "demo.mod",
                    "jobs.update",
                    json!({"id":id,"status":"completed","result":42})
                )
                .await
                .unwrap(),
            cancelled
        );
        for invalid in ["../outside", "", "CON"] {
            assert!(store
                .call("demo.mod", "jobs.get", json!({"id":invalid}))
                .await
                .is_err());
            assert!(store
                .call("demo.mod", "jobs.start", json!({"type":invalid}))
                .await
                .is_err());
        }
    }
}
