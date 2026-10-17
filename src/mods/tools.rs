use super::{error, package, process, types::*};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::BufReader,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

#[derive(Clone)]
struct Installed {
    metadata: NativeTool,
    root: PathBuf,
}
struct Running {
    child: Child,
    group: process::ProcessGuard,
    tool_id: String,
}
pub struct ToolManager {
    installed: Mutex<HashMap<String, Vec<Installed>>>,
    processes: Mutex<HashMap<String, HashMap<u32, Running>>>,
}
impl ToolManager {
    pub fn new() -> Self {
        Self {
            installed: Mutex::new(HashMap::new()),
            processes: Mutex::new(HashMap::new()),
        }
    }
    pub async fn prepare(
        &self,
        id: &str,
        manifest: &Manifest,
        package_dir: &Path,
        directory: &Path,
    ) -> Result<()> {
        let definitions: Vec<_> = manifest
            .native_tools
            .iter()
            .filter(|tool| tool.platform == target())
            .cloned()
            .collect();
        if self.installed.lock().await.get(id).is_some_and(|current| {
            current.len() == definitions.len()
                && current.iter().zip(&definitions).all(|(old, new)| {
                    old.root == directory.join(&new.id).join(&new.version)
                        && new.files.iter().all(|file|old.root.join(&file.path).is_file())
                        && serde_json::to_value(&old.metadata).ok()
                            == serde_json::to_value(new).ok()
                })
        }) {
            return Ok(());
        }
        let package_dir = package_dir.to_owned();
        let directory = directory.to_owned();
        let installed = tokio::task::spawn_blocking(move || -> Result<Vec<Installed>> {
            fs::create_dir_all(&directory).map_err(error)?;
            let mut result = Vec::new();
            for tool in definitions {
                let base = directory.join(&tool.id);
                if base.exists()
                    && fs::symlink_metadata(&base)
                        .map_err(error)?
                        .file_type()
                        .is_symlink()
                {
                    return Err("Native tool directory must not be a link".into());
                }
                fs::create_dir_all(&base).map_err(error)?;
                let destination = base.join(&tool.version);
                let stage = tempfile::tempdir_in(&base).map_err(error)?;
                for file in &tool.files {
                    let source = package_dir.join(safe_relative(&file.path)?);
                    let root = package_dir.canonicalize().map_err(error)?;
                    if !source.canonicalize().map_err(error)?.starts_with(root) {
                        return Err("Tool file outside package".into());
                    }
                    if package::digest_reader(&mut BufReader::new(
                        fs::File::open(&source).map_err(error)?,
                    ))? != file.sha256.to_ascii_lowercase()
                    {
                        return Err("Native tool SHA-256 mismatch".into());
                    }
                    let output = stage.path().join(&file.path);
                    fs::create_dir_all(output.parent().ok_or("Invalid tool path")?)
                        .map_err(error)?;
                    fs::copy(source, &output).map_err(error)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(
                            &output,
                            fs::Permissions::from_mode(if file.executable { 0o755 } else { 0o644 }),
                        )
                        .map_err(error)?;
                    }
                }
                // Only this component's validated, owned version directory is replaced.
                if destination.exists() {
                    if fs::symlink_metadata(&destination)
                        .map_err(error)?
                        .file_type()
                        .is_symlink()
                    {
                        return Err("Installed tool directory must not be a link".into());
                    }
                    fs::remove_dir_all(&destination).map_err(error)?;
                }
                fs::rename(stage.path(), &destination).map_err(error)?;
                result.push(Installed {
                    metadata: tool,
                    root: destination,
                });
            }
            Ok(result)
        })
        .await
        .map_err(error)??;
        self.installed.lock().await.insert(id.into(), installed);
        Ok(())
    }
    pub async fn relocate(&self, id: &str, directory: &Path) {
        if let Some(tools) = self.installed.lock().await.get_mut(id) {
            for tool in tools {
                tool.root = directory
                    .join(&tool.metadata.id)
                    .join(&tool.metadata.version);
            }
        }
    }
    pub async fn stop_mod(&self, id: &str) {
        let running = self.processes.lock().await.remove(id).unwrap_or_default();
        for (_, mut process) in running {
            process.group.terminate();
            let _ = process.child.kill().await;
            let _ = process.child.wait().await;
        }
    }
    pub async fn call(
        &self,
        id: &str,
        method: &str,
        params: Value,
        data_dir: &Path,
    ) -> Result<Value> {
        match method {
            "tools.list" => Ok(json!(self.installed.lock().await.get(id).into_iter().flatten().map(|tool|json!({"id":tool.metadata.id,"version":tool.metadata.version,"license":tool.metadata.license,"platform":tool.metadata.platform,"files":tool.metadata.files,"directory":tool.root})).collect::<Vec<_>>())),
            "tools.run" => {
                let installed=self.installed.lock().await;
                let tool=installed.get(id).into_iter().flatten().find(|t|Some(t.metadata.id.as_str())==params["id"].as_str()).ok_or("Native tool is not installed for this platform")?;
                let executable=tool.metadata.files.iter().find(|f|f.executable && params["executable"].as_str().is_none_or(|p|p==f.path)).ok_or("Executable is not declared by native tool")?;
                let path=tool.root.join(&executable.path).canonicalize().map_err(error)?;
                if !path.starts_with(tool.root.canonicalize().map_err(error)?) { return Err("Tool executable outside installation".into()); }
                if package::digest_reader(&mut BufReader::new(fs::File::open(&path).map_err(error)?))?!=executable.sha256.to_ascii_lowercase() { return Err("Installed executable SHA-256 mismatch".into()); }
                let args=params["args"].as_array().ok_or("Tool args must be an array")?.iter().map(|v|v.as_str().map(str::to_owned).ok_or_else(||"Tool arguments must be strings".to_owned())).collect::<Result<Vec<_>>>()?;
                let mut command=Command::new(path);
                command.args(args).current_dir(data_dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
                process::configure(&mut command);
                let mut child=command.spawn().map_err(error)?;
                let pid=child.id().ok_or("Tool exited before ownership could be established")?;
                let group=match process::ProcessGuard::attach(pid) {Ok(group)=>group,Err(e)=>{let _=child.kill().await;return Err(e);}};
                self.processes.lock().await.entry(id.into()).or_default().insert(pid,Running{child,group,tool_id:tool.metadata.id.clone()});
                Ok(json!({"process_id":pid}))
            }
            "tools.status" => {
                let mut processes=self.processes.lock().await;
                let mut out=Vec::new();
                for (&pid,process) in processes.entry(id.into()).or_default() {
                    let status=process.child.try_wait().map_err(error)?;
                    if status.is_some(){process.group.terminate();}
                    out.push(json!({"process_id":pid,"tool_id":process.tool_id,"running":status.is_none(),"exit_code":status.and_then(|s|s.code())}));
                }
                Ok(json!(out))
            }
            "tools.stop" => {
                let pid=u32::try_from(params["process_id"].as_u64().ok_or("Missing process ID")?).map_err(error)?;
                let mut process=self.processes.lock().await.entry(id.into()).or_default().remove(&pid).ok_or("Process does not belong to this mod")?;
                process.group.terminate(); let _=process.child.kill().await; let _=process.child.wait().await;
                Ok(Value::Null)
            }
            _=>Err("Unknown tools method".into()),
        }
    }
}
