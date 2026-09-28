use std::path::Path;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::modules::detector::LauncherDetector;
use crate::storage::models::MinecraftAccount;
use crate::storage::Database;
use crate::sys::process::{launch_app, LaunchConfig, LauncherType};
use super::hot_swap::MinecraftHotSwap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchMinecraftRequest {
    pub account_id: Option<String>,
    pub profile_id: Option<String>,
    pub mode: Option<String>, // "official" (default) or "direct"
    pub server_ip: Option<String>,
    pub world_folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchMinecraftResponse {
    pub success: bool,
    pub message: String,
    pub account_used: Option<String>,
    pub profile_used: Option<String>,
    pub mode_used: String,
}

#[derive(Clone)]
pub struct MinecraftLauncher {
    db: Database,
    detector: LauncherDetector,
    hot_swap: MinecraftHotSwap,
}

impl MinecraftLauncher {
    pub fn new(db: Database) -> Self {
        let detector = LauncherDetector::new();
        let hot_swap = MinecraftHotSwap::new(db.clone());
        Self {
            db,
            detector,
            hot_swap,
        }
    }

    /// Launches Minecraft Java Edition according to request parameters.
    pub async fn launch(&self, req: LaunchMinecraftRequest) -> Result<LaunchMinecraftResponse, String> {
        // 1. Resolve Account
        let account: Option<MinecraftAccount> = if let Some(ref aid) = req.account_id {
            // Switch to requested account if specified
            let _ = self.hot_swap.switch_account(aid).await.map_err(|e| e.to_string())?;
            self.db.get_minecraft_account(aid).await.map_err(|e| e.to_string())?
        } else {
            // Use active account or first available
            let accounts = self.db.get_minecraft_accounts().await.map_err(|e| e.to_string())?;
            accounts.into_iter().find(|a| a.is_active)
        };

        // 2. Resolve Profile if specified
        let mut profile_name = None;
        if let Some(ref pid) = req.profile_id {
            if let Ok(profiles) = self.db.get_minecraft_profiles().await {
                if let Some(p) = profiles.into_iter().find(|p| &p.id == pid || &p.name == pid) {
                    profile_name = Some(p.name);
                    // Update launcher_profiles.json selected profile if file exists
                    if let Some(mc_dir) = self.detector.detect_minecraft_data_dir() {
                        let prof_file = mc_dir.join("launcher_profiles.json");
                        let _ = set_selected_profile(&prof_file, pid);
                    }
                }
            }
        }

        // 3. Resolve Mode
        let mode = req.mode.unwrap_or_else(|| "official".to_string());

        // 4. Resolve launcher executable
        let launcher_path = self.detector.detect_minecraft().or_else(|| {
            LauncherType::Minecraft.default_app_path()
        }).ok_or_else(|| {
            "Официальный лаунчер Minecraft не найден на этом компьютере. Установите Minecraft или укажите SWCH_MINECRAFT_LAUNCHER.".to_string()
        })?;

        info!(
            "Launching Minecraft via {} [mode: {}, server: {:?}, world: {:?}]",
            launcher_path.display(),
            mode,
            req.server_ip,
            req.world_folder
        );

        let mut args: Vec<String> = Vec::new();

        // Server quick-play argument
        if let Some(ref ip) = req.server_ip {
            args.push("--quickPlayMultiplayer".to_string());
            args.push(ip.clone());
        }

        // World quick-play argument
        if let Some(ref world) = req.world_folder {
            args.push("--quickPlaySingleplayer".to_string());
            args.push(world.clone());
        }

        // Execute launch
        let config = LaunchConfig::new(&launcher_path)
            .args(&args)
            .prefer_open(true);

        match launch_app(&config) {
            Ok(_) => {
                let acc_username = account.as_ref().map(|a| a.username.clone());
                let msg = match (&req.server_ip, &req.world_folder) {
                    (Some(ip), _) => format!("Minecraft запускается с подключением к серверу {}", ip),
                    (_, Some(world)) => format!("Minecraft запускается с миром {}", world),
                    _ => "Minecraft: Java Edition успешно запущен".to_string(),
                };

                Ok(LaunchMinecraftResponse {
                    success: true,
                    message: msg,
                    account_used: acc_username,
                    profile_used: profile_name,
                    mode_used: mode,
                })
            }
            Err(e) => Err(format!("Ошибка запуска лаунчера: {}", e)),
        }
    }
}

/// Helper to set selected profile in launcher_profiles.json
fn set_selected_profile(path: &Path, profile_id: &str) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(path)?;
    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) {
        if let Some(obj) = json.as_object_mut() {
            obj.insert(
                "selectedProfile".to_string(),
                serde_json::Value::String(profile_id.to_string()),
            );
            if let Ok(updated) = serde_json::to_string_pretty(&json) {
                let _ = std::fs::write(path, updated);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launch_request_deserialization() {
        let json = r#"{
            "account_id": "acc-1",
            "profile_id": "fabric",
            "mode": "official",
            "server_ip": "mc.hypixel.net"
        }"#;

        let req: LaunchMinecraftRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.account_id, Some("acc-1".to_string()));
        assert_eq!(req.profile_id, Some("fabric".to_string()));
        assert_eq!(req.server_ip, Some("mc.hypixel.net".to_string()));
    }
}
