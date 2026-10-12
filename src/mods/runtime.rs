use super::{host::Services, types::*};
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
    sync::{oneshot, Mutex},
};

pub async fn line<R: AsyncBufRead + Unpin>(reader: &mut R, max: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
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
        if out.len() + n > max {
            return Err("Mod message exceeds limit".into());
        }
        out.extend_from_slice(&buf[..n]);
        reader.consume(n);
        if end.is_some() {
            return Ok(out);
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
async fn write(input: &Arc<Mutex<ChildStdin>>, value: Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_MESSAGE {
        return Err("Message too large".into());
    }
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
    ) -> Result<Self> {
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
        let data_directory = services.root.join("data").join(&id);
        std::fs::create_dir_all(&data_directory).map_err(|e| e.to_string())?;
        command
            .arg(host.join("runtime.mjs"))
            .arg(backend)
            .current_dir(&data_directory)
            .env("SWCH_MOD_DATA_DIR", &data_directory)
            .env("SWCH_MOD_PACKAGE_DIR", backend.parent().unwrap())
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let input = Arc::new(Mutex::new(child.stdin.take().ok_or("Missing mod stdin")?));
        let mut output = BufReader::new(child.stdout.take().ok_or("Missing mod stdout")?);
        let mut stderr = BufReader::new(child.stderr.take().ok_or("Missing mod stderr")?);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let failure = Arc::new(Mutex::new(None));
        let reader_input = input.clone();
        let reader_pending = pending.clone();
        let reader_failure = failure.clone();
        let mut events = services.events.subscribe();
        let reader = tokio::spawn(async move {
            let result:Result<()>=async {loop {
                let message:Value=serde_json::from_slice(&line(&mut output,MAX_MESSAGE).await?).map_err(|e|e.to_string())?;
                if message["jsonrpc"]!="2.0"{return Err("Invalid JSON-RPC version".into());}
                if let Some(method)=message["method"].as_str(){
                    let result=services.call(&id,method.into(),message["params"].clone()).await;
                    if let Some(request_id)=message.get("id"){
                        if !request_id.as_str().is_some_and(|s|s.starts_with("host:")){return Err("Invalid host request ID".into());}
                        let reply=match result{Ok(result)=>json!({"jsonrpc":"2.0","id":request_id,"result":result}),Err(error)=>json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32000,"message":error}})};write(&reader_input,reply).await?;
                    }
                }else{
                    let response_id=message["id"].as_u64().ok_or("Invalid response ID")?;let sender=reader_pending.lock().await.remove(&response_id).ok_or("Unexpected response ID")?;
                    let result=if let Some(error)=message.get("error"){Err(format!("Mod operation failed: {}",error["message"].as_str().unwrap_or("Mod request failed")))}else{message.get("result").cloned().ok_or("Missing mod result".into())};let _=sender.send(result);
                }
            }}.await;
            let error = result.unwrap_err();
            *reader_failure.lock().await = Some(error.clone());
            for (_, sender) in reader_pending.lock().await.drain() {
                let _ = sender.send(Err(error.clone()));
            }
        });
        let event_input = input.clone();
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
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
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
            tasks: vec![reader, event_task, log_task],
        })
    }
    pub fn exited(&mut self) -> bool {
        self.tasks[0].is_finished() || !matches!(self.child.try_wait(), Ok(None))
    }
    pub async fn stop(&mut self) {
        let _ = self.child.kill().await;
        for task in &self.tasks {
            task.abort();
        }
    }
    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        if let Some(error) = self.failure.lock().await.clone() {
            return Err(error);
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
        match tokio::time::timeout(Duration::from_secs(30), exchange).await {
            Ok(result) => result,
            Err(_) => {
                self.pending.lock().await.remove(&id);
                self.stop().await;
                Err("Mod request timed out".into())
            }
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
