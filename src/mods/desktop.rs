use super::{error, types::*};
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::{io::AsyncWriteExt, process::Command};

pub async fn call(method: &str, params: Value) -> Result<Value> {
    if method == "shell.openFolder" {
        let path = std::path::PathBuf::from(params["path"].as_str().ok_or("Missing folder path")?)
            .canonicalize()
            .map_err(error)?;
        if !path.is_dir() {
            return Err("Path must be a folder".into());
        }
        tokio::task::spawn_blocking(move || open::that(path))
            .await
            .map_err(error)?
            .map_err(error)?;
        return Ok(Value::Null);
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-STA",
                "-Command",
                include_str!("desktop.ps1"),
            ])
            .creation_flags(0x08000000);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(error)?;
        let mut input = child.stdin.take().ok_or("Missing desktop input")?;
        input
            .write_all(
                serde_json::to_string(&json!({"method":method,"params":params}))
                    .map_err(error)?
                    .as_bytes(),
            )
            .await
            .map_err(error)?;
        drop(input);
        let output = child.wait_with_output().await.map_err(error)?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().into());
        }
        return serde_json::from_slice(&output.stdout).map_err(error);
    }
    #[cfg(target_os = "macos")]
    {
        if method == "clipboard.readText" {
            let output = Command::new("pbpaste").output().await.map_err(error)?;
            if !output.status.success() {
                return Err("Cannot read clipboard".into());
            }
            return String::from_utf8(output.stdout)
                .map(|s| json!(s))
                .map_err(error);
        }
        if method == "clipboard.writeText" {
            let mut child = Command::new("pbcopy")
                .stdin(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(error)?;
            child
                .stdin
                .take()
                .ok_or("Missing clipboard input")?
                .write_all(
                    params["text"]
                        .as_str()
                        .ok_or("Text must be a string")?
                        .as_bytes(),
                )
                .await
                .map_err(error)?;
            if !child.wait().await.map_err(error)?.success() {
                return Err("Cannot write clipboard".into());
            }
            return Ok(Value::Null);
        }
        let script = r#"function run(args){const r=JSON.parse(args[0]),a=Application.currentApplication();a.includeStandardAdditions=true;try{if(r.method==='dialogs.openFolder')return JSON.stringify(a.chooseFolder({withPrompt:r.params.title||'Select folder'}).toString());if(r.method==='dialogs.openFile'){const o={withPrompt:r.params.title||'Select files',multipleSelectionsAllowed:!!r.params.multiple};if(r.params.extensions?.length)o.ofType=r.params.extensions;let f=a.chooseFile(o);return JSON.stringify((Array.isArray(f)?f:[f]).map(x=>x.toString()));}throw Error('Unknown desktop method');}catch(e){if(e.errorNumber===-128)return JSON.stringify(r.method==='dialogs.openFile'?[]:null);throw e;}}"#;
        let output = Command::new("osascript")
            .args([
                "-l",
                "JavaScript",
                "-e",
                script,
                &json!({"method":method,"params":params}).to_string(),
            ])
            .output()
            .await
            .map_err(error)?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().into());
        }
        return serde_json::from_slice(&output.stdout).map_err(error);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    Err("Desktop dialogs and clipboard are available on Windows and macOS".into())
}
