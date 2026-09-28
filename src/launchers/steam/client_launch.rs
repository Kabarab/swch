//! Literal Steam argument vectors. No shell commands and no stored credentials.
use super::{library::valid_app_id, SteamPaths};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SteamCommand {
    pub program: String,
    pub args: Vec<String>,
}
pub fn command(
    paths: &SteamPaths,
    app_id: &str,
    update_only: bool,
    args: &[String],
) -> Result<SteamCommand, String> {
    if !valid_app_id(app_id)
        || args.len() > 64
        || args.iter().any(|a| a.len() > 4096 || a.contains('\0'))
    {
        return Err("Некорректные параметры запуска Steam".into());
    }
    if update_only && !args.is_empty() {
        return Err("Обновление не принимает параметры игры".into());
    }
    #[cfg(target_os = "macos")]
    let program = {
        let candidates = [
            paths
                .steam_dir
                .join("Steam.AppBundle/Steam/Contents/MacOS/steam_osx"),
            std::path::PathBuf::from("/Applications/Steam.app/Contents/MacOS/steam_osx"),
        ];
        candidates
            .into_iter()
            .find(|p| p.is_file())
            .ok_or("Исполняемый файл Steam не найден")?
            .to_string_lossy()
            .into_owned()
    };
    #[cfg(target_os = "windows")]
    let program = paths
        .steam_dir
        .join("steam.exe")
        .to_string_lossy()
        .into_owned();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = {
        let _ = paths;
        "steam".to_string()
    };
    let mut result = vec!["-silent".into()];
    if update_only {
        result.push(format!("steam://install/{app_id}"));
    } else {
        result.extend(["-applaunch".into(), app_id.into()]);
        result.extend_from_slice(args);
    }
    Ok(SteamCommand {
        program,
        args: result,
    })
}
impl SteamCommand {
    pub async fn dispatch(&self) -> Result<(), String> {
        let mut child = tokio::process::Command::new(&self.program)
            .args(&self.args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|_| "Не удалось запустить клиент Steam")?;
        // Reap the forwarding process without waiting for the Steam session to end.
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        Ok(())
    }
}
