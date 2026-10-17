use super::{host::Services, process::ProcessGuard, types::*};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{oneshot, watch, Mutex},
};

pub async fn line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    frame(reader, &mut out).await
}

// Keep partial frames outside the read future so select! can safely service host calls.
async fn frame<R: AsyncBufRead + Unpin>(reader: &mut R, out: &mut Vec<u8>) -> Result<Vec<u8>> {
    loop {
        let buf = reader.fill_buf().await.map_err(|e| e.to_string())?;
        if buf.is_empty() {
            return Err(if out.is_empty() {
                "Mod process closed output"
            } else {
                "Incomplete protocol message"
            }
            .into());
        }
        let end = buf.iter().position(|b| *b == b'\n').map(|i| i + 1);
        let n = end.unwrap_or(buf.len());
        out.try_reserve(n).map_err(|e| e.to_string())?;
        out.extend_from_slice(&buf[..n]);
        reader.consume(n);
        if end.is_some() {
            return Ok(std::mem::take(out));
        }
    }
}
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;
pub struct Runtime {
    child: Child,
    input: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    sequence: u64,
    failure: Arc<Mutex<Option<String>>>,
    cancellation: watch::Sender<bool>,
    cancelled: watch::Receiver<bool>,
    group: Arc<ProcessGuard>,
    services: Arc<Services>,
    id: String,
    logs: Arc<Mutex<VecDeque<String>>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
pub fn node_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SWCH_NODE_PATH") {
        return Ok(PathBuf::from(path));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let base = exe.parent().ok_or("Missing executable directory")?;
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    for path in [
        base.join("runtime").join(name),
        base.join("../Resources/runtime").join(name),
    ] {
        if path.is_file() {
            return Ok(path);
        }
    }
    Err("Bundled Node.js missing. Developers can set SWCH_NODE_PATH to Node.js 24.".into())
}

fn node_entry_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        if let Some(Component::Prefix(prefix)) = components.next() {
            let mut plain = match prefix.kind() {
                Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", char::from(drive))),
                Prefix::VerbatimUNC(server, share) => {
                    let mut base = std::ffi::OsString::from("\\\\");
                    base.push(server);
                    base.push("\\");
                    base.push(share);
                    PathBuf::from(base)
                }
                _ => return path.into(),
            };
            for component in components {
                if component != Component::RootDir {
                    plain.push(component.as_os_str());
                }
            }
            return plain;
        }
    }
    path.into()
}

async fn write(input: &Arc<Mutex<ChildStdin>>, value: Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    let mut input = input.lock().await;
    input.write_all(&bytes).await.map_err(|e| e.to_string())?;
    input.flush().await.map_err(|e| e.to_string())
}
impl Runtime {
    pub async fn spawn(
        node: &Path,
        host: &Path,
        backend: &Path,
        logs: Arc<Mutex<VecDeque<String>>>,
        services: Arc<Services>,
        id: String,
        cancellation: watch::Sender<bool>,
        package: &Path,
    ) -> Result<Self> {
        let cancelled = cancellation.subscribe();
        let mut version_command = Command::new(node);
        version_command.arg("--version");
        #[cfg(windows)]
        version_command.creation_flags(0x08000000);
        let version = tokio::time::timeout(Duration::from_secs(10), version_command.output())
            .await
            .map_err(|_| "Node.js version check timed out")?
            .map_err(|e| e.to_string())?;
        if !version.status.success()
            || !String::from_utf8_lossy(&version.stdout)
                .trim()
                .starts_with("v24.")
        {
            return Err("Mods require Node.js 24 LTS".into());
        }
        let mut command = Command::new(node);
        // Games inherit a stable data directory rather than locking the package on Windows.
        let data_directory = services.files.directory(&id, "data")?;
        command
            // Node's CLI realpath resolution rejects Windows verbatim entry paths.
            .arg(node_entry_path(&host.join("runtime.mjs")))
            .arg(backend)
            .current_dir(&data_directory)
            .env("SWCH_MOD_DATA_DIR", &data_directory)
            .env(
                "SWCH_MOD_CACHE_DIR",
                services.files.directory(&id, "cache")?,
            )
            .env("SWCH_MOD_TEMP_DIR", services.files.directory(&id, "temp")?)
            .env(
                "SWCH_MOD_TOOLS_DIR",
                services.files.directory(&id, "tools")?,
            )
            .env("SWCH_MOD_PACKAGE_DIR", package)
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        if let Some(path) = services.db.path() {
            command.env(
                "SWCH_DB_PATH",
                path.canonicalize().map_err(|e| e.to_string())?,
            );
        }
        super::process::configure(&mut command);
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let group = Arc::new(ProcessGuard::attach(
            child.id().ok_or("Missing mod process ID")?,
        )?);
        let input = Arc::new(Mutex::new(child.stdin.take().ok_or("Missing mod stdin")?));
        let mut output = BufReader::new(child.stdout.take().ok_or("Missing mod stdout")?);
        let mut stderr = BufReader::new(child.stderr.take().ok_or("Missing mod stderr")?);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let failure = Arc::new(Mutex::new(None));
        let reader_input = input.clone();
        let reader_pending = pending.clone();
        let reader_failure = failure.clone();
        let mut events = services.events.subscribe();
        let reader_services = services.clone();
        let reader_id = id.clone();
        let reader_group = group.clone();
        let reader = tokio::spawn(async move {
            let mut calls = tokio::task::JoinSet::<Result<()>>::new();
            let mut partial = Vec::new();
            let result: Result<()> = async {
                loop {
                    // The partial frame survives when another host call finishes.
                    let line = tokio::select! {
                        completed = calls.join_next(), if !calls.is_empty() => {
                            completed.ok_or("Missing host call")?.map_err(|e| e.to_string())??;
                            continue;
                        }
                        line = frame(&mut output, &mut partial) => line?,
                    };
                    let message: Value = serde_json::from_slice(&line).map_err(|e| e.to_string())?;
                    if message["jsonrpc"] != "2.0" {
                        return Err("Invalid JSON-RPC version".into());
                    }
                    if let Some(method) = message["method"].as_str() {
                        let request_id = message.get("id").cloned();
                        if request_id.as_ref().is_some_and(|id| !id.as_str().is_some_and(|s| s.starts_with("host:"))) {
                            return Err("Invalid host request ID".into());
                        }
                        let method = method.to_string();
                        let params = message["params"].clone();
                        let services = reader_services.clone();
                        let id = reader_id.clone();
                        let input = reader_input.clone();
                        calls.spawn(async move {
                            let result = services.call_from_runtime(&id, method, params).await;
                            if let Some(request_id) = request_id {
                                let reply = match result {
                                    Ok(result) => json!({"jsonrpc":"2.0","id":request_id,"result":result}),
                                    Err(error) => json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32000,"message":error}}),
                                };
                                write(&input, reply).await?;
                            }
                            Ok(())
                        });
                    } else {
                        let response_id = message["id"].as_u64().ok_or("Invalid response ID")?;
                        let sender = reader_pending.lock().await.remove(&response_id).ok_or("Unexpected response ID")?;
                        let result = if let Some(error) = message.get("error") {
                            Err(format!("Mod operation failed: {}", error["message"].as_str().unwrap_or("Mod request failed")))
                        } else {
                            message.get("result").cloned().ok_or("Missing mod result".into())
                        };
                        let _ = sender.send(result);
                    }
                }
            }.await;
            calls.abort_all();
            reader_group.terminate();
            reader_services.tools.stop_mod(&reader_id).await;
            let error = result.unwrap_err();
            *reader_failure.lock().await = Some(error.clone());
            for (_, sender) in reader_pending.lock().await.drain() {
                let _ = sender.send(Err(error.clone()));
            }
        });
        let event_input = input.clone();
        let event_id = id.clone();
        let event_task = tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        if write(
                            &event_input,
                            json!({"jsonrpc":"2.0","method":"event","params":event}),
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if write(&event_input, json!({"jsonrpc":"2.0","method":"event","params":{"event":format!("mod:{event_id}:jobs.reconcile")}})).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let runtime_logs = logs.clone();
        let log_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buffer = [0u8; 4096];
            let mut tail = String::new();
            while let Ok(n) = stderr.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
                tail.push_str(&String::from_utf8_lossy(&buffer[..n]));
                while let Some(end) = tail.find('\n').or_else(|| {
                    if tail.len() > 16000 {
                        Some(tail.char_indices().nth(8000).map_or(tail.len(), |x| x.0))
                    } else {
                        None
                    }
                }) {
                    let chunk = tail[..end].to_string();
                    tail = tail[end + usize::from(tail.as_bytes().get(end) == Some(&b'\n'))..]
                        .to_string();
                    let mut logs = logs.lock().await;
                    if logs.len() >= 200 {
                        logs.pop_front();
                    }
                    logs.push_back(chunk);
                }
            }
            if !tail.is_empty() {
                let mut logs = logs.lock().await;
                if logs.len() >= 200 {
                    logs.pop_front();
                }
                logs.push_back(tail);
            }
        });
        Ok(Self {
            child,
            input,
            pending,
            sequence: 0,
            failure,
            cancellation,
            cancelled,
            group,
            services,
            id,
            logs: runtime_logs,
            tasks: vec![reader, event_task, log_task],
        })
    }
    pub fn exited(&mut self) -> bool {
        self.tasks[0].is_finished() || !matches!(self.child.try_wait(), Ok(None))
    }
    pub fn cancellation(&self) -> watch::Sender<bool> {
        self.cancellation.clone()
    }
    pub async fn stop(&mut self) {
        // Give mods a chance to release native workers and package file handles.
        let graceful = async {
            write(&self.input, json!({"jsonrpc":"2.0","method":"shutdown"})).await?;
            self.child.wait().await.map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        };
        if !matches!(
            tokio::time::timeout(Duration::from_secs(10), graceful).await,
            Ok(Ok(()))
        ) {
            let _ = self.child.kill().await;
        }
        self.group.terminate();
        self.services.tools.stop_mod(&self.id).await;
        for task in &self.tasks {
            task.abort();
        }
    }
    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        if let Some(error) = self.failure.lock().await.clone() {
            return Err(error);
        }
        if *self.cancelled.borrow() {
            self.stop().await;
            return Err("Mod operation cancelled".into());
        }
        self.sequence += 1;
        let id = self.sequence;
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);
        let exchange = async {
            write(
                &self.input,
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            )
            .await?;
            receiver
                .await
                .map_err(|_| "Mod reader closed".to_string())?
        };
        let mut result = tokio::select! {
            biased;
            _ = self.cancelled.wait_for(|cancelled| *cancelled) => {
                Err("Mod operation cancelled".into())
            },
            result = exchange => result,
        };
        if result.is_err() {
            self.pending.lock().await.remove(&id);
        }
        if let Err(error) = &mut result {
            if !error.starts_with("Mod operation failed: ") && error != "Mod operation cancelled" {
                // On process/protocol failure, keep stderr before stop() aborts its reader.
                let _ = tokio::time::timeout(Duration::from_millis(100), &mut self.tasks[2]).await;
                if let Ok(Some(status)) = self.child.try_wait() {
                    error.push_str(&format!(" (process {status})"));
                }
                let logs = self.logs.lock().await;
                let tail = logs
                    .iter()
                    .rev()
                    .take(4)
                    .rev()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");
                if !tail.is_empty() {
                    error.push_str(&format!(": {tail}"));
                }
            }
        }
        if *self.cancelled.borrow() {
            self.stop().await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{frame, line};
    use tokio::io::{AsyncWriteExt, BufReader};

    #[cfg(windows)]
    #[test]
    fn node_entries_use_plain_drive_and_unc_paths() {
        use super::node_entry_path;
        use std::path::Path;
        assert_eq!(
            node_entry_path(Path::new(r"\\?\E:\mods\host\runtime.mjs")),
            Path::new(r"E:\mods\host\runtime.mjs")
        );
        assert_eq!(
            node_entry_path(Path::new(r"\\?\UNC\server\share\host\runtime.mjs")),
            Path::new(r"\\server\share\host\runtime.mjs")
        );
        assert_eq!(
            node_entry_path(Path::new(r"E:\mods\host\runtime.mjs")),
            Path::new(r"E:\mods\host\runtime.mjs")
        );
    }

    #[tokio::test]
    async fn protocol_lines_can_exceed_one_mebibyte_and_keep_message_boundaries() {
        let mut message = vec![b'x'; 2 * 1024 * 1024];
        message.push(b'\n');
        let mut input = message.clone();
        input.extend_from_slice(b"next\n");
        let mut reader = BufReader::with_capacity(97, input.as_slice());
        assert_eq!(line(&mut reader).await.unwrap(), message);
        assert_eq!(line(&mut reader).await.unwrap(), b"next\n");
        assert_eq!(
            line(&mut reader).await.unwrap_err(),
            "Mod process closed output"
        );
    }

    #[tokio::test]
    async fn protocol_still_requires_complete_messages() {
        let mut reader = BufReader::new(&b"incomplete"[..]);
        assert_eq!(
            line(&mut reader).await.unwrap_err(),
            "Incomplete protocol message"
        );
    }

    #[tokio::test]
    async fn partial_protocol_frames_survive_interleaved_host_call_completion() {
        let (mut writer, reader) = tokio::io::duplex(128);
        let mut reader = BufReader::new(reader);
        let mut partial = Vec::new();
        writer.write_all(b"partial").await.unwrap();
        tokio::select! {
            result = frame(&mut reader, &mut partial) => panic!("Unexpected frame: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {}
        }
        writer.write_all(b" message\n").await.unwrap();
        assert_eq!(
            frame(&mut reader, &mut partial).await.unwrap(),
            b"partial message\n"
        );
        assert!(partial.is_empty());
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.group.terminate();
        for task in &self.tasks {
            task.abort();
        }
    }
}
