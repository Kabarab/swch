pub mod catalog;
mod host;
pub mod package;
pub mod runtime;
pub mod types;

use crate::storage::{atomic::atomic_write_json, Account, Database, Game, GameAccount};
use package::read_manifest;
use runtime::Runtime;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{broadcast, Mutex, RwLock};
use types::*;

struct Plugin {
    manifest: Manifest,
    directory: PathBuf,
    saved: SavedState,
    runtime: Option<Runtime>,
    contributions: Contributions,
    error: Option<String>,
    logs: Arc<Mutex<VecDeque<String>>>,
}

pub struct ModManager {
    pub root: PathBuf,
    db: Database,
    node: Option<PathBuf>,
    entries: RwLock<HashMap<String, Arc<Mutex<Plugin>>>>,
    mutation: Mutex<()>,
    data_gate: Arc<Mutex<()>>,
    host: Arc<host::Services>,
    pub events: broadcast::Sender<Value>,
}

fn read_json(path: &Path) -> Result<Value> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.to_string()),
    }
}
fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    atomic_write_json(path, value).map_err(|e| e.to_string())
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl ModManager {
    pub fn default_root() -> PathBuf {
        std::env::var_os("SWCH_MODS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .unwrap_or_else(|| PathBuf::from(".swch"))
                    .join("swch")
                    .join("mods")
            })
    }
    pub fn new(db: Database) -> Self {
        Self::with_dir(db, Self::default_root(), None)
    }
    pub fn with_dir(db: Database, root: PathBuf, node: Option<PathBuf>) -> Self {
        let (events, _) = broadcast::channel(100);
        let mut entries = HashMap::new();
        if let Ok(states) = fs::read_dir(root.join("state")) {
            for file in states.flatten() {
                let Ok(saved) = fs::read(file.path()).and_then(|b| {
                    serde_json::from_slice::<SavedState>(&b).map_err(std::io::Error::other)
                }) else {
                    continue;
                };
                let Some(id) = file
                    .path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_owned)
                else {
                    continue;
                };
                if !valid_id(&id) {
                    continue;
                }
                let dir = saved
                    .development
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| root.join("packages").join(&id).join("current"));
                match read_manifest(&dir, Some(target())) {
                    Ok(manifest) if manifest.id == id => {
                        entries.insert(
                            id,
                            Arc::new(Mutex::new(Plugin {
                                manifest,
                                directory: dir,
                                saved,
                                runtime: None,
                                contributions: Contributions::default(),
                                error: None,
                                logs: Arc::new(Mutex::new(VecDeque::new())),
                            })),
                        );
                    }
                    _ => {
                        tracing::warn!("Cannot load saved mod {id}");
                    }
                }
            }
        }
        let data_gate = Arc::new(Mutex::new(()));
        let host = Arc::new(host::Services {
            root: root.clone(),
            db: db.clone(),
            gate: data_gate.clone(),
            events: events.clone(),
        });
        Self {
            root,
            db,
            node,
            entries: RwLock::new(entries),
            mutation: Mutex::new(()),
            data_gate,
            host,
            events,
        }
    }
    fn state_file(&self, id: &str) -> PathBuf {
        self.root.join("state").join(format!("{id}.json"))
    }
    async fn entry(&self, id: &str) -> Result<Arc<Mutex<Plugin>>> {
        self.entries
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or("Mod not installed".into())
    }
    pub async fn initialize(&self) {
        let entries: Vec<_> = self.entries.read().await.values().cloned().collect();
        for entry in entries {
            let mut p = entry.lock().await;
            if p.saved.enabled {
                let _ = self.start(&mut p).await;
            }
        }
    }
    /// Bundled ZIPs are first-install defaults. A durable marker preserves removal.
    pub async fn install_bundled(&self) {
        let Ok(exe) = std::env::current_exe() else { return; };
        let Some(base) = exe.parent() else { return; };
        for directory in [base.join("bundled-mods"), base.join("../Resources/bundled-mods")] {
            let Ok(files) = fs::read_dir(directory) else { continue; };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|v| v.to_str()) != Some("zip") { continue; }
                let Some(id) = path.file_stem().and_then(|v| v.to_str()) else { continue; };
                if !valid_id(id) { continue; }
                let marker = self.root.join("bundled").join(format!("{id}.json"));
                if marker.exists() { continue; }
                let result: Result<()> = async {
                    if !self.entries.read().await.contains_key(id) {
                        if fs::metadata(&path).map_err(error)?.len() > MAX_PACKAGE { return Err("Bundled package too large".into()); }
                        let bytes = fs::read(&path).map_err(error)?;
                        let installed = self.install_bytes(&bytes, &package::digest(&bytes), None).await?;
                        if installed != id { return Err("Bundled package filename must match mod ID".into()); }
                    }
                    write_json(&marker, &json!({"installed":true}))
                }.await;
                if let Err(error) = result { tracing::warn!("Cannot install bundled mod {id}: {error}"); }
            }
        }
    }
    async fn prepare_host(&self) -> Result<PathBuf> {
        let dir = self.root.join("host");
        fs::create_dir_all(&dir).map_err(error)?;
        crate::storage::atomic::atomic_write_str(
            dir.join("runtime.mjs"),
            include_str!("../../mod-sdk/runtime.mjs"),
        )
        .map_err(error)?;
        crate::storage::atomic::atomic_write_str(
            dir.join("index.mjs"),
            include_str!("../../mod-sdk/index.mjs"),
        )
        .map_err(error)?;
        dir.canonicalize().map_err(error)
    }
    async fn start(&self, p: &mut Plugin) -> Result<()> {
        if !p.saved.enabled {
            return Err("Mod disabled".into());
        }
        if p.runtime.as_mut().is_some_and(|r| r.exited()) {
            if let Some(mut runtime) = p.runtime.take() {
                runtime.stop().await;
            }
            p.error = Some("Mod process exited; use Restart".into());
        }
        if p.error.is_some() {
            return Err(p.error.clone().unwrap());
        }
        if p.runtime.is_some() {
            return Ok(());
        }
        let result: Result<()> = async {
            let mut contributions = Contributions::default();
            if p.manifest.ui.is_some() {
                contributions.tabs.push(Contribution {
                    id: "main".into(),
                    name: p.manifest.name.clone(),
                });
            }
            if let Some(backend) = &p.manifest.backend {
                let node = match &self.node {
                    Some(n) => n.clone(),
                    None => runtime::node_path()?,
                };
                let host = self.prepare_host().await?;
                p.runtime = Some(
                    Runtime::spawn(
                        &node,
                        &host,
                        &p.directory.join(backend).canonicalize().map_err(error)?,
                        p.logs.clone(),
                        self.host.clone(),
                        p.manifest.id.clone(),
                    )
                    .await?,
                );
                let response = p
                    .runtime
                    .as_mut()
                    .unwrap()
                    .request("initialize", json!({"api":API_VERSION,"id":p.manifest.id}))
                    .await?;
                let registered: Contributions = serde_json::from_value(response).map_err(error)?;
                let mut seen = HashSet::new();
                for item in registered
                    .launchers
                    .iter()
                    .map(|x| (&x.id, &x.name))
                    .chain(registered.tabs.iter().map(|x| (&x.id, &x.name)))
                    .chain(registered.actions.iter().map(|x| (&x.id, &x.name)))
                {
                    if !valid_id(item.0)
                        || item.1.trim().is_empty()
                        || item.1.len() > 120
                        || !seen.insert(item.0.clone())
                    {
                        return Err("Invalid or duplicate contribution ID".into());
                    }
                }
                if registered.launchers.len() + registered.tabs.len() + registered.actions.len()
                    > 100
                {
                    return Err("Too many contributions".into());
                }
                for platform in &registered.launchers {
                    if platform.capabilities.iter().any(|c| {
                        !matches!(
                            c.as_str(),
                            "library"
                                | "sync"
                                | "accounts"
                                | "connect"
                                | "disconnect"
                                | "switch"
                                | "launch"
                        )
                    }) {
                        return Err("Unknown launcher capability".into());
                    }
                }
                contributions = registered;
                if p.manifest.ui.is_some() && contributions.tabs.is_empty() {
                    contributions.tabs.push(Contribution {
                        id: "main".into(),
                        name: p.manifest.name.clone(),
                    });
                }
            }
            p.contributions = contributions;
            Ok(())
        }
        .await;
        if let Err(e) = &result {
            if let Some(mut runtime) = p.runtime.take() {
                runtime.stop().await;
            }
            p.error = Some(e.clone());
        }
        result
    }
    async fn request(&self, p: &mut Plugin, method: &str, params: Value) -> Result<Value> {
        self.start(p).await?;
        let result = p
            .runtime
            .as_mut()
            .ok_or("Mod has no backend")?
            .request(method, params)
            .await;
        if let Err(e) = &result {
            // An operation error is recoverable; process/protocol failures require restart.
            if !e.starts_with("Mod operation failed: ")
                || p.runtime.as_mut().is_some_and(|r| r.exited())
            {
                if let Some(mut runtime) = p.runtime.take() {
                    runtime.stop().await;
                }
                p.error = Some(e.clone());
            }
        }
        result
    }
    pub async fn list(&self) -> Vec<ModInfo> {
        let entries: Vec<_> = self.entries.read().await.values().cloned().collect();
        let mut info = Vec::new();
        for entry in entries {
            let mut p = entry.lock().await;
            if p.saved.enabled {
                let _ = self.start(&mut p).await;
            }
            info.push(ModInfo {
                manifest: p.manifest.clone(),
                games: p.saved.games.clone().unwrap_or_else(|| p.manifest.games.clone()),
                enabled: p.saved.enabled,
                development: p.saved.development.is_some(),
                status: if !p.saved.enabled {
                    "disabled"
                } else if p.error.is_some() {
                    "error"
                } else {
                    "active"
                }
                .into(),
                error: p.error.clone(),
                contributions: p.contributions.clone(),
                source: p.saved.source.clone(),
            });
        }
        info.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        info
    }
    pub async fn platforms(&self) -> Vec<Platform> {
        let mut out = Vec::new();
        for p in self.list().await {
            if p.enabled && p.status == "active" {
                for mut platform in p.contributions.launchers {
                    platform.id = format!("mod:{}:{}", p.manifest.id, platform.id);
                    platform.mod_id = p.manifest.id.clone();
                    out.push(platform);
                }
            }
        }
        out
    }
    pub async fn toggle(&self, id: &str, enabled: bool) -> Result<()> {
        let entry = self.entry(id).await?;
        let mut p = entry.lock().await;
        let mut saved = p.saved.clone();
        saved.enabled = enabled;
        write_json(&self.state_file(id), &saved)?;
        if let Some(mut runtime) = p.runtime.take() {
            runtime.stop().await;
        }
        p.error = None;
        p.saved = saved;
        p.contributions = Contributions::default();
        if enabled {
            self.start(&mut p).await?;
        }
        let _ = self.events.send(json!({"event":"mods.changed"}));
        Ok(())
    }
    pub async fn bind_games(&self, id: &str, games: Option<Vec<GameTarget>>) -> Result<()> {
        let _gate = self.mutation.lock().await;
        if let Some(games) = &games {
            validate_game_targets(games)?;
        }
        let entry = self.entry(id).await?;
        let mut p = entry.lock().await;
        let mut saved = p.saved.clone();
        saved.games = games;
        write_json(&self.state_file(id), &saved)?;
        p.saved = saved;
        let _ = self.events.send(json!({"event":"mods.changed"}));
        Ok(())
    }
    pub async fn reload(&self, id: &str) -> Result<()> {
        let entry = self.entry(id).await?;
        let mut p = entry.lock().await;
        if let Some(mut runtime) = p.runtime.take() {
            runtime.stop().await;
        }
        let manifest = read_manifest(&p.directory, Some(target()))?;
        if manifest.id != id {
            return Err("Mod ID changed".into());
        }
        p.manifest = manifest;
        p.error = None;
        if p.saved.enabled {
            self.start(&mut p).await?;
        }
        let _ = self.events.send(json!({"event":"mods.changed"}));
        Ok(())
    }
    pub async fn development(&self, dir: PathBuf) -> Result<String> {
        let _gate = self.mutation.lock().await;
        let dir = dir.canonicalize().map_err(error)?;
        let manifest = read_manifest(&dir, Some(target()))?;
        let id = manifest.id.clone();
        if self.entries.read().await.contains_key(&id) {
            return Err("Mod ID already installed; use Reload".into());
        }
        let saved = SavedState {
            enabled: true,
            games: None,
            development: Some(dir.to_string_lossy().into()),
            source: None,
        };
        let mut p = Plugin {
            manifest,
            directory: dir,
            saved,
            runtime: None,
            contributions: Contributions::default(),
            error: None,
            logs: Arc::new(Mutex::new(VecDeque::new())),
        };
        let _ = self.start(&mut p).await;
        write_json(&self.state_file(&id), &p.saved)?;
        self.entries
            .write()
            .await
            .insert(id.clone(), Arc::new(Mutex::new(p)));
        let _ = self.events.send(json!({"event":"mods.changed"}));
        Ok(id)
    }
    pub async fn install_bytes(
        &self,
        bytes: &[u8],
        hash: &str,
        source: Option<CatalogEntry>,
    ) -> Result<String> {
        let _gate = self.mutation.lock().await;
        fs::create_dir_all(self.root.join("packages")).map_err(error)?;
        let staging = tempfile::tempdir_in(self.root.join("packages")).map_err(error)?;
        let manifest = package::extract(bytes, hash, staging.path(), Some(target()))?;
        if let Some(source) = &source {
            if source.sha256.to_ascii_lowercase() != hash.to_ascii_lowercase()
                || serde_json::to_value(&source.manifest).map_err(error)?
                    != serde_json::to_value(&manifest).map_err(error)?
            {
                return Err("Catalog manifest differs from package".into());
            }
        }
        let id = manifest.id.clone();
        let existing = self.entries.read().await.get(&id).cloned();
        let mut old = if let Some(entry) = &existing {
            Some(entry.lock().await)
        } else {
            None
        };
        if let Some(old) = &old {
            if old.saved.development.is_some() {
                return Err("Remove development mod before installing package".into());
            }
            if let Some(previous) = &old.saved.source {
                if source.as_ref().is_none_or(|s| {
                    s.repository_id != previous.repository_id || s.owner_id != previous.owner_id
                }) {
                    return Err("Mod repository identity changed".into());
                }
            }
            if semver::Version::parse(&manifest.version).map_err(error)?
                <= semver::Version::parse(&old.manifest.version).map_err(error)?
            {
                return Err("Update must have a newer version".into());
            }
        }
        let base = self.root.join("packages").join(&id);
        fs::create_dir_all(&base).map_err(error)?;
        let current = base.join("current");
        let previous = base.join("previous");
        if previous.exists() {
            fs::remove_dir_all(&previous).map_err(error)?;
        }
        if let Some(old) = old.as_mut() {
            if let Some(mut runtime) = old.runtime.take() {
                runtime.stop().await;
            }
        }
        if current.exists() {
            fs::rename(&current, &previous).map_err(error)?;
        }
        if let Err(e) = fs::rename(staging.path(), &current) {
            if previous.exists() {
                let _ = fs::rename(&previous, &current);
            }
            return Err(e.to_string());
        }
        let saved = SavedState {
            enabled: old.as_ref().map_or(true, |p| p.saved.enabled),
            games: old.as_ref().and_then(|p| p.saved.games.clone()),
            development: None,
            source,
        };
        let mut plugin = Plugin {
            manifest,
            directory: current.clone(),
            saved,
            runtime: None,
            contributions: Contributions::default(),
            error: None,
            logs: Arc::new(Mutex::new(VecDeque::new())),
        };
        let activated = if plugin.saved.enabled {
            self.start(&mut plugin).await
        } else {
            Ok(())
        };
        if let Err(e) = activated.and_then(|_| write_json(&self.state_file(&id), &plugin.saved)) {
            if let Some(mut runtime) = plugin.runtime.take() {
                runtime.stop().await;
            }
            fs::remove_dir_all(&current).map_err(error)?;
            if previous.exists() {
                fs::rename(&previous, &current).map_err(error)?;
            }
            if let Some(old) = old.as_mut() {
                old.error = None;
                let _ = self.start(old).await;
            }
            return Err(format!("Installation rolled back: {e}"));
        }
        if let Some(old) = old.as_mut() {
            **old = plugin;
        } else {
            self.entries
                .write()
                .await
                .insert(id.clone(), Arc::new(Mutex::new(plugin)));
        }
        let _ = self.events.send(json!({"event":"mods.changed"}));
        Ok(id)
    }
    pub async fn uninstall(&self, id: &str) -> Result<()> {
        let _gate = self.mutation.lock().await;
        let entry = self.entry(id).await?;
        let mut p = entry.lock().await;
        if let Some(mut runtime) = p.runtime.take() {
            runtime.stop().await;
        }
        // Development sources are never removed.
        let package = self.root.join("packages").join(id);
        if package.exists() {
            fs::remove_dir_all(package).map_err(error)?;
        }
        let state = self.state_file(id);
        if state.exists() {
            fs::remove_file(state).map_err(error)?;
        }
        self.entries.write().await.remove(id);
        let _ = self.events.send(json!({"event":"mods.changed"}));
        Ok(())
    }
    pub async fn clear_data(&self, id: &str) -> Result<()> {
        let _mutation = self.mutation.lock().await;
        if !valid_id(id) {
            return Err("Invalid mod ID".into());
        }
        if self.entries.read().await.contains_key(id) {
            return Err("Uninstall mod before clearing its data".into());
        }
        let _gate = self.data_gate.lock().await;
        let dir = self.root.join("data").join(id);
        let keys = read_json(&dir.join("secret-keys.json"))?;
        for key in keys
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            match keyring::Entry::new(&format!("swch-mod:{id}"), key)
                .map_err(error)?
                .delete_credential()
            {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        self.db.clear_mod_data(id).await.map_err(error)?;
        if dir.exists() {
            fs::remove_dir_all(dir).map_err(error)?;
        }
        Ok(())
    }
    pub async fn logs(&self, id: &str) -> Result<Vec<String>> {
        let entry = self.entry(id).await?;
        let p = entry.lock().await;
        let logs = p.logs.lock().await.iter().cloned().collect();
        Ok(logs)
    }
    pub async fn ui(&self, id: &str) -> Result<String> {
        let entry = self.entry(id).await?;
        let p = entry.lock().await;
        if !p.saved.enabled || p.error.is_some() {
            return Err("Mod not active".into());
        }
        let path = p.manifest.ui.as_ref().ok_or("No mod interface")?;
        let bytes = fs::read(p.directory.join(path)).map_err(error)?;
        if bytes.len() > MAX_MESSAGE {
            return Err("Interface too large".into());
        }
        String::from_utf8(bytes).map_err(error)
    }
    pub async fn theme(&self, id: Option<&str>) -> Result<Value> {
        if let Some(id) = id {
            let entry = self.entry(id).await?;
            let p = entry.lock().await;
            if !p.saved.enabled {
                return Err("Mod disabled".into());
            }
            let path = p.manifest.theme.as_ref().ok_or("No theme")?;
            let value = read_json(&p.directory.join(path))?;
            theme_tokens(&value)?;
            write_json(&self.root.join("theme.json"), &json!({"id":id}))?;
            let _ = self.events.send(json!({"event":"mods.changed"}));
            return Ok(value);
        }
        write_json(&self.root.join("theme.json"), &json!({}))?;
        let _ = self.events.send(json!({"event":"mods.changed"}));
        Ok(json!({}))
    }
    pub async fn active_theme(&self) -> Value {
        if let Ok(value) = read_json(&self.root.join("theme.json")) {
            if let Some(id) = value["id"].as_str() {
                if let Ok(entry) = self.entry(id).await {
                    let p = entry.lock().await;
                    if p.saved.enabled {
                        if let Some(path) = &p.manifest.theme {
                            if let Ok(value) = read_json(&p.directory.join(path)) {
                                if theme_tokens(&value).is_ok() {
                                    return value;
                                }
                            }
                        }
                    }
                }
            }
        }
        json!({})
    }
    pub async fn action(&self, id: &str, action: &str, input: Value) -> Result<Value> {
        let entry = self.entry(id).await?;
        let mut p = entry.lock().await;
        self.start(&mut p).await?;
        if !p.contributions.actions.iter().any(|a| a.id == action) {
            return Err("Unknown action".into());
        }
        self.request(&mut p, "action", json!({"id":action,"input":input}))
            .await
    }
    pub async fn sdk_call(&self, id: &str, method: String, params: Value) -> Result<Value> {
        let entry = self.entry(id).await?;
        {
            let p = entry.lock().await;
            if !p.saved.enabled || p.error.is_some() {
                return Err("Mod not active".into());
            }
        }
        if method == "action" {
            return self
                .action(
                    id,
                    params["id"].as_str().ok_or("Missing action ID")?,
                    params["input"].clone(),
                )
                .await;
        }
        self.host_call(id, method, params).await
    }

    pub async fn platform_operation(
        &self,
        platform: &str,
        operation: &str,
        mut input: Value,
    ) -> Result<Value> {
        let parts: Vec<_> = platform.split(':').collect();
        if parts.len() != 3 || parts[0] != "mod" {
            return Err("Unknown platform".into());
        }
        let (id, local) = (parts[1], parts[2]);
        let entry = self.entry(id).await?;
        let mut p = entry.lock().await;
        self.start(&mut p).await?;
        let provider = p
            .contributions
            .launchers
            .iter()
            .find(|x| x.id == local)
            .ok_or("Unknown launcher")?;
        if !provider.capabilities.iter().any(|c| c == operation) {
            return Err("Unsupported launcher operation".into());
        }
        // IDs passed to authors are local, never another mod's identifiers.
        for (field, kind) in [("game_id", "game"), ("account_id", "account")] {
            if let Some(value) = input.get(field).and_then(Value::as_str) {
                let prefix = format!("{platform}:{kind}:");
                let local = value
                    .strip_prefix(&prefix)
                    .ok_or("ID belongs to another platform")?
                    .to_string();
                let exists = if kind == "game" {
                    self.db
                        .get_games()
                        .await
                        .map_err(error)?
                        .iter()
                        .any(|g| g.id == value)
                } else {
                    self.db
                        .get_accounts()
                        .await
                        .map_err(error)?
                        .iter()
                        .any(|a| a.id == value)
                };
                if !exists {
                    return Err("Game or account not found".into());
                }
                input[field] = json!(local);
            }
        }
        if matches!(operation, "launch" | "switch" | "disconnect")
            && input[if operation == "launch" {
                "game_id"
            } else {
                "account_id"
            }]
            .as_str()
            .is_none()
        {
            return Err("Missing operation target".into());
        }
        let _switch = if matches!(operation, "launch" | "switch") {
            Some(crate::sys::process::LAUNCHER_SWITCH_LOCK.lock().await)
        } else {
            None
        };
        let value = self
            .request(
                &mut p,
                "launcher",
                json!({"id":local,"operation":operation,"input":input}),
            )
            .await?;
        match operation {
            "library" | "sync" => {
                self.save_games(platform, value.clone()).await?;
            }
            "accounts" => {
                self.save_accounts(platform, value.clone()).await?;
            }
            "connect" | "disconnect" | "switch" => {
                if p.contributions
                    .launchers
                    .iter()
                    .find(|x| x.id == local)
                    .is_some_and(|x| x.capabilities.iter().any(|c| c == "accounts"))
                {
                    let accounts = self
                        .request(
                            &mut p,
                            "launcher",
                            json!({"id":local,"operation":"accounts","input":{}}),
                        )
                        .await?;
                    self.save_accounts(platform, accounts).await?;
                }
            }
            _ => {}
        }
        let _ = self
            .events
            .send(json!({"event":"library.changed","platform":platform}));
        Ok(value)
    }
    async fn save_games(&self, platform: &str, value: Value) -> Result<()> {
        let rows = value.as_array().ok_or("Library must return an array")?;
        let mut games = Vec::new();
        let mut links = Vec::new();
        let mut ids = HashSet::new();
        for row in rows {
            let local = row["id"]
                .as_str()
                .filter(|x| valid_id(x))
                .ok_or("Invalid game ID")?;
            let id = format!("{platform}:game:{local}");
            if !ids.insert(id.clone()) {
                return Err("Duplicate game ID".into());
            }
            let game = Game {
                id: id.clone(),
                launcher: platform.into(),
                app_id: row["app_id"].as_str().unwrap_or(local).into(),
                name: row["name"]
                    .as_str()
                    .filter(|x| !x.trim().is_empty())
                    .ok_or("Missing game name")?
                    .into(),
                install_path: row["install_path"].as_str().map(str::to_owned),
                executable_path: row["executable_path"].as_str().map(str::to_owned),
                size_bytes: row["size_bytes"].as_i64().unwrap_or(0).max(0),
                is_installed: row["is_installed"].as_bool().unwrap_or(false),
                banner_url: row["banner_url"].as_str().map(str::to_owned),
                launch_uri: row["launch_uri"].as_str().map(str::to_owned),
                namespace: Some(platform.into()),
            };
            for owner in row["owner_ids"].as_array().into_iter().flatten() {
                let owner = owner
                    .as_str()
                    .filter(|x| valid_id(x))
                    .ok_or("Invalid owner ID")?;
                links.push(GameAccount {
                    game_id: id.clone(),
                    account_id: format!("{platform}:account:{owner}"),
                    launcher: platform.into(),
                });
            }
            games.push(game);
        }
        self.db
            .replace_mod_games(platform, &games, &links)
            .await
            .map_err(error)
    }
    async fn save_accounts(&self, platform: &str, value: Value) -> Result<()> {
        let rows = value.as_array().ok_or("Accounts must return an array")?;
        let mut accounts = Vec::new();
        let mut ids = HashSet::new();
        let mut active = false;
        for row in rows {
            let local = row["id"]
                .as_str()
                .filter(|x| valid_id(x))
                .ok_or("Invalid account ID")?;
            let id = format!("{platform}:account:{local}");
            if !ids.insert(id.clone()) {
                return Err("Duplicate account ID".into());
            }
            let is_active = row["is_active"].as_bool().unwrap_or(false);
            if is_active && active {
                return Err("Multiple active accounts".into());
            }
            active |= is_active;
            accounts.push(Account {
                id,
                launcher: platform.into(),
                username: row["username"]
                    .as_str()
                    .filter(|x| !x.trim().is_empty())
                    .ok_or("Missing account username")?
                    .into(),
                display_name: row["display_name"].as_str().map(str::to_owned),
                email: row["email"].as_str().map(str::to_owned),
                is_active,
                last_used: row["last_used"].as_str().map(str::to_owned),
            });
        }
        self.db
            .replace_mod_accounts(platform, &accounts)
            .await
            .map_err(error)
    }
    pub async fn library(&self) -> Result<Value> {
        let platforms = self.platforms().await;
        let active: HashSet<_> = platforms.iter().map(|p| p.id.as_str()).collect();
        let games = self
            .db
            .get_games()
            .await
            .map_err(error)?
            .into_iter()
            .filter(|g| active.contains(g.launcher.as_str()))
            .collect::<Vec<_>>();
        let accounts = self
            .db
            .get_accounts()
            .await
            .map_err(error)?
            .into_iter()
            .filter(|g| active.contains(g.launcher.as_str()))
            .collect::<Vec<_>>();
        let links = self.db.get_game_accounts().await.map_err(error)?;
        let mut games = serde_json::to_value(games).map_err(error)?;
        for game in games.as_array_mut().unwrap() {
            game["owner_ids"] = json!(links
                .iter()
                .filter(|l| l.game_id == game["id"].as_str().unwrap())
                .map(|l| l.account_id.clone())
                .collect::<Vec<_>>());
        }
        Ok(json!({"platforms":platforms,"games":games,"accounts":accounts}))
    }
    pub async fn host_call(&self, id: &str, method: String, params: Value) -> Result<Value> {
        self.host.call(id, method, params).await
    }
    pub async fn shutdown(&self) {
        let entries: Vec<_> = self.entries.read().await.values().cloned().collect();
        for entry in entries {
            let mut p = entry.lock().await;
            if let Some(mut runtime) = p.runtime.take() {
                runtime.stop().await;
            }
        }
    }
}
