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
    #[serde(default, alias = "client", alias = "launcher")]
    pub launcher_client: Option<String>, // "official" or "tlauncher"
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

        // 3. Resolve Mode & Launcher Client
        let is_tl_account = account.as_ref().map_or(false, |a| {
            a.account_type.starts_with("tlauncher") || a.account_type == "offline"
        });

        let client_pref = match req.launcher_client.as_deref().map(|s| s.trim().to_lowercase()) {
            Some(ref s) if s == "auto" || s.is_empty() => None,
            other => other,
        };

        let (launcher_path, mode_used) = match client_pref.as_deref() {
            Some("tlauncher") | Some("tl") => {
                let p = self.detector.detect_tlauncher().ok_or_else(|| {
                    "T-Launcher не найден на этом компьютере. Установите T-Launcher или укажите SWCH_TLAUNCHER_APP / SWCH_TLAUNCHER_PATH.".to_string()
                })?;
                (p, "tlauncher".to_string())
            }
            Some("official") | Some("mojang") | Some("microsoft") => {
                let p = self.detector.detect_minecraft().or_else(|| {
                    LauncherType::Minecraft.default_app_path()
                }).ok_or_else(|| {
                    "Официальный лаунчер Minecraft не найден на этом компьютере. Установите Minecraft или укажите SWCH_MINECRAFT_LAUNCHER.".to_string()
                })?;
                (p, "official".to_string())
            }
            _ => {
                // Auto resolution based on account type with fallback
                if is_tl_account {
                    if let Some(p) = self.detector.detect_tlauncher() {
                        (p, "tlauncher".to_string())
                    } else if let Some(p) = self.detector.detect_minecraft().or_else(|| LauncherType::Minecraft.default_app_path()) {
                        (p, "official".to_string())
                    } else {
                        return Err("Лаунчер Minecraft (T-Launcher или Официальный) не найден на этом компьютере.".to_string());
                    }
                } else {
                    if let Some(p) = self.detector.detect_minecraft().or_else(|| LauncherType::Minecraft.default_app_path()) {
                        (p, "official".to_string())
                    } else if let Some(p) = self.detector.detect_tlauncher() {
                        (p, "tlauncher".to_string())
                    } else {
                        return Err("Лаунчер Minecraft (Официальный или T-Launcher) не найден на этом компьютере.".to_string());
                    }
                }
            }
        };

        info!(
            "Launching Minecraft via {} [client: {}, server: {:?}, world: {:?}]",
            launcher_path.display(),
            mode_used,
            req.server_ip,
            req.world_folder
        );

        let mut args: Vec<String> = Vec::new();

        // If launcher is a .jar file, invoke java -jar <jar_path>
        let is_jar = launcher_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("jar"))
            .unwrap_or(false);

        let target_binary = if is_jar {
            args.push("-jar".to_string());
            args.push(launcher_path.to_string_lossy().to_string());
            resolve_java_binary(&self.detector)
        } else {
            launcher_path.clone()
        };

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
        let config = LaunchConfig::new(&target_binary)
            .args(&args)
            .prefer_open(!is_jar);

        match launch_app(&config) {
            Ok(_) => {
                let acc_username = account.as_ref().map(|a| a.username.clone());
                let client_label = if mode_used == "tlauncher" { "T-Launcher" } else { "Minecraft" };
                let msg = match (&req.server_ip, &req.world_folder) {
                    (Some(ip), _) => format!("{} запускается с подключением к серверу {}", client_label, ip),
                    (_, Some(world)) => format!("{} запускается с миром {}", client_label, world),
                    _ => format!("{} успешно запущен", client_label),
                };

                Ok(LaunchMinecraftResponse {
                    success: true,
                    message: msg,
                    account_used: acc_username,
                    profile_used: profile_name,
                    mode_used,
                })
            }
            Err(e) => Err(format!("Ошибка запуска лаунчера: {}", e)),
        }
    }
}

/// Resolves appropriate Java executable binary (javaw on Windows or java).
pub fn resolve_java_binary(mc_detector: &LauncherDetector) -> std::path::PathBuf {
    if let Ok(jh) = std::env::var("JAVA_HOME") {
        #[cfg(windows)]
        {
            let p_javaw = std::path::PathBuf::from(&jh).join("bin\\javaw.exe");
            if p_javaw.exists() {
                return p_javaw;
            }
            let p_java = std::path::PathBuf::from(&jh).join("bin\\java.exe");
            if p_java.exists() {
                return p_java;
            }
        }
        #[cfg(not(windows))]
        {
            let p_java = std::path::PathBuf::from(&jh).join("bin/java");
            if p_java.exists() {
                return p_java;
            }
        }
    }

    // Check .minecraft/runtime bundled JRE if available
    if let Some(mc_dir) = mc_detector.detect_minecraft_data_dir() {
        let runtime_dir = mc_dir.join("runtime");
        if runtime_dir.exists() {
            #[cfg(windows)]
            let exe_names = ["bin\\javaw.exe", "bin\\java.exe"];
            #[cfg(not(windows))]
            let exe_names = ["bin/java"];

            if let Ok(entries) = std::fs::read_dir(&runtime_dir) {
                for entry in entries.flatten() {
                    let sub = entry.path();
                    if sub.is_dir() {
                        for name in &exe_names {
                            let candidate = sub.join(name);
                            if candidate.exists() {
                                return candidate;
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(windows)]
    {
        std::path::PathBuf::from("javaw")
    }
    #[cfg(not(windows))]
    {
        std::path::PathBuf::from("java")
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
            "launcher_client": "tlauncher",
            "server_ip": "mc.hypixel.net"
        }"#;

        let req: LaunchMinecraftRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.account_id, Some("acc-1".to_string()));
        assert_eq!(req.profile_id, Some("fabric".to_string()));
        assert_eq!(req.launcher_client, Some("tlauncher".to_string()));
        assert_eq!(req.server_ip, Some("mc.hypixel.net".to_string()));
    }
}
