use super::{error, read_json, types::*, write_json};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

pub struct FileStore {
    root: PathBuf,
    gate: Mutex<()>,
}
const KINDS: [&str; 4] = ["data", "cache", "temp", "tools"];
impl FileStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            gate: Mutex::new(()),
        }
    }
    fn config(&self, id: &str) -> PathBuf {
        self.root.join("file-locations").join(format!("{id}.json"))
    }
    pub fn directory(&self, id: &str, kind: &str) -> Result<PathBuf> {
        if !valid_id(id) || !KINDS.contains(&kind) {
            return Err("Invalid file storage kind or mod ID".into());
        }
        let config = read_json(&self.config(id))?;
        self.directory_with_config(id, kind, &config)
    }
    fn directory_with_config(&self, id: &str, kind: &str, config: &Value) -> Result<PathBuf> {
        let base = if let Some(parent) = config[kind].as_str() {
            PathBuf::from(parent)
                .canonicalize()
                .map_err(error)?
                .join("SWCH-Mods")
        } else {
            fs::create_dir_all(self.root.join("files")).map_err(error)?;
            self.root.join("files").canonicalize().map_err(error)?
        };
        let mut path = base;
        for part in [id, kind] {
            // Never follow a link/junction while creating or cleaning an owned directory.
            if path.exists()
                && fs::symlink_metadata(&path)
                    .map_err(error)?
                    .file_type()
                    .is_symlink()
            {
                return Err("File storage directory must not be a link".into());
            }
            fs::create_dir_all(&path).map_err(error)?;
            path = path.join(part);
        }
        if path.exists()
            && fs::symlink_metadata(&path)
                .map_err(error)?
                .file_type()
                .is_symlink()
        {
            return Err("File storage directory must not be a link".into());
        }
        let new_directory = !path.exists();
        fs::create_dir_all(&path).map_err(error)?;
        if new_directory && kind == "data" && config[kind].is_null() {
            let legacy = self.root.join("data").join(id);
            if let Ok(entries) = fs::read_dir(&legacy) {
                for entry in entries {
                    let entry = entry.map_err(error)?;
                    if matches!(
                        entry.file_name().to_str(),
                        Some("settings.json" | "storage.json" | "secret-keys.json" | "jobs")
                    ) {
                        continue;
                    }
                    let destination = path.join(entry.file_name());
                    if !destination.exists() {
                        fs::rename(entry.path(), destination).map_err(error)?;
                    }
                }
            }
        }
        Ok(path)
    }
    fn paths(&self, id: &str) -> Result<Value> {
        let mut paths = json!({});
        for kind in KINDS {
            paths[kind] = json!(self.directory(id, kind)?);
        }
        Ok(paths)
    }
    pub async fn call(self: &Arc<Self>, id: &str, method: &str, params: Value) -> Result<Value> {
        let _gate = self.gate.lock().await;
        let store = self.clone();
        let id = id.to_owned();
        let method = method.to_owned();
        tokio::task::spawn_blocking(move || store.execute(&id, &method, params))
            .await
            .map_err(error)?
    }
    fn execute(&self, id: &str, method: &str, params: Value) -> Result<Value> {
        if !valid_id(id) {
            return Err("Invalid mod ID".into());
        }
        match method {
            "files.paths" => self.paths(id),
            "files.usage" => {
                let mut usage = json!({});
                for kind in KINDS {
                    usage[kind] = json!(size(&self.directory(id, kind)?)?);
                }
                Ok(usage)
            }
            "files.clearCache" => {
                let cache = self.directory(id, "cache")?;
                for entry in fs::read_dir(&cache).map_err(error)? {
                    let entry = entry.map_err(error)?;
                    let path = entry.path();
                    if entry.file_type().map_err(error)?.is_dir() {
                        fs::remove_dir_all(path).map_err(error)?;
                    } else {
                        fs::remove_file(path).map_err(error)?;
                    }
                }
                Ok(Value::Null)
            }
            "files.configure" => {
                let kind = params["kind"].as_str().ok_or("Missing storage kind")?;
                if !KINDS.contains(&kind) {
                    return Err("Invalid storage kind".into());
                }
                let parent = PathBuf::from(
                    params["directory"]
                        .as_str()
                        .ok_or("Missing destination directory")?,
                )
                .canonicalize()
                .map_err(error)?;
                if !parent.is_dir() {
                    return Err("Destination must be an existing directory".into());
                }
                let source = self.directory(id, kind)?;
                let mut config = read_json(&self.config(id))?;
                config[kind] = json!(parent);
                let destination = self.directory_with_config(id, kind, &config)?;
                if source == destination {
                    return self.paths(id);
                }
                if destination.starts_with(&source) || source.starts_with(&destination) {
                    return Err("Storage locations must not overlap".into());
                }
                if fs::read_dir(&destination).map_err(error)?.next().is_some() {
                    return Err("Destination storage is not empty".into());
                }
                let staging = tempfile::tempdir_in(
                    destination.parent().ok_or("Invalid storage destination")?,
                )
                .map_err(error)?;
                for item in walkdir::WalkDir::new(&source)
                    .min_depth(1)
                    .follow_links(false)
                {
                    let item = item.map_err(error)?;
                    if item.file_type().is_symlink() {
                        return Err("Cannot migrate linked storage files".into());
                    }
                    let output = staging
                        .path()
                        .join(item.path().strip_prefix(&source).map_err(error)?);
                    if item.file_type().is_dir() {
                        fs::create_dir_all(&output).map_err(error)?;
                    } else {
                        fs::copy(item.path(), &output).map_err(error)?;
                    }
                }
                fs::remove_dir(&destination).map_err(error)?;
                fs::rename(staging.path(), &destination).map_err(error)?;
                write_json(&self.config(id), &config)?;
                // Both paths are resolved owned subdirectories; data is copied before removal.
                fs::remove_dir_all(&source).map_err(error)?;
                self.paths(id)
            }
            _ => Err("Unknown files method".into()),
        }
    }
    pub async fn remove(self: &Arc<Self>, id: &str) -> Result<()> {
        let _gate = self.gate.lock().await;
        let paths = KINDS
            .into_iter()
            .map(|kind| self.directory(id, kind))
            .collect::<Result<Vec<_>>>()?;
        let config = self.config(id);
        tokio::task::spawn_blocking(move || -> Result<()> {
            for path in paths {
                fs::remove_dir_all(path).map_err(error)?;
            }
            match fs::remove_file(config) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        })
        .await
        .map_err(error)?
    }
}
fn size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for item in walkdir::WalkDir::new(path).follow_links(false) {
        let item = item.map_err(error)?;
        if item.file_type().is_file() {
            total = total
                .checked_add(item.metadata().map_err(error)?.len())
                .ok_or("Storage size overflow")?;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn separated_files_migrate_and_cache_cleanup_preserves_data() {
        let root = tempfile::tempdir().unwrap();
        let disk = tempfile::tempdir().unwrap();
        let files = Arc::new(FileStore::new(root.path().join("mods")));
        fs::write(
            files.directory("alice.mod", "data").unwrap().join("backup"),
            b"save",
        )
        .unwrap();
        fs::write(
            files
                .directory("alice.mod", "cache")
                .unwrap()
                .join("preview"),
            b"cache",
        )
        .unwrap();
        assert_eq!(
            files
                .call("alice.mod", "files.usage", json!({}))
                .await
                .unwrap()["data"],
            4
        );
        let paths = files
            .call(
                "alice.mod",
                "files.configure",
                json!({"kind":"data","directory":disk.path()}),
            )
            .await
            .unwrap();
        assert_eq!(
            fs::read(Path::new(paths["data"].as_str().unwrap()).join("backup")).unwrap(),
            b"save"
        );
        files
            .call("alice.mod", "files.clearCache", json!({}))
            .await
            .unwrap();
        let usage = files
            .call("alice.mod", "files.usage", json!({}))
            .await
            .unwrap();
        assert_eq!(usage["data"], 4);
        assert_eq!(usage["cache"], 0);
        let reopened = FileStore::new(root.path().join("mods"));
        assert_eq!(
            reopened.directory("alice.mod", "data").unwrap(),
            PathBuf::from(paths["data"].as_str().unwrap())
        );
    }
}
