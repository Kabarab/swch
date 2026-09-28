use std::path::{Path, PathBuf};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::modules::detector::LauncherDetector;
use crate::storage::models::{MinecraftAccount, MinecraftProfile, MinecraftWorld};
use crate::storage::Database;
use super::nbt::parse_servers_dat;

/// Scanner responsible for discovering and importing accounts, profiles,
/// multiplayer servers, and singleplayer worlds from `.minecraft`.
#[derive(Clone)]
pub struct MinecraftScanner {
    db: Database,
    detector: LauncherDetector,
}

impl MinecraftScanner {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            detector: LauncherDetector::new(),
        }
    }

    /// Resolves the .minecraft data directory on the host system.
    pub fn resolve_minecraft_dir(&self) -> Option<PathBuf> {
        self.detector.detect_minecraft_data_dir()
    }

    /// Runs a full discovery scan across accounts, profiles, servers, and worlds.
    pub async fn scan_and_import(&self) -> (usize, usize, usize, usize) {
        let mc_dir = match self.resolve_minecraft_dir() {
            Some(d) if d.exists() => d,
            _ => {
                debug!("Minecraft data directory (.minecraft) not found on system.");
                return (0, 0, 0, 0);
            }
        };

        info!("Scanning Minecraft directory at {}", mc_dir.display());

        let acc_count = self.scan_accounts(&mc_dir).await;
        let prof_count = self.scan_profiles(&mc_dir).await;
        let srv_count = self.scan_servers(&mc_dir).await;
        let world_count = self.scan_worlds(&mc_dir).await;

        info!(
            "Minecraft scan complete: {} account(s), {} profile(s), {} server(s), {} world(s)",
            acc_count, prof_count, srv_count, world_count
        );

        (acc_count, prof_count, srv_count, world_count)
    }

    /// Scans launcher_accounts.json (and MS store variation) for saved accounts.
    pub async fn scan_accounts(&self, mc_dir: &Path) -> usize {
        let mut accounts = Vec::new();

        // 1. Standard launcher_accounts.json
        let standard_file = mc_dir.join("launcher_accounts.json");
        if standard_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&standard_file) {
                accounts.extend(parse_launcher_accounts(&content));
            }
        }

        // 2. Microsoft store launcher_accounts_microsoft_store.json
        let ms_file = mc_dir.join("launcher_accounts_microsoft_store.json");
        if ms_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&ms_file) {
                let ms_accs = parse_launcher_accounts(&content);
                for acc in ms_accs {
                    if !accounts.iter().any(|a: &MinecraftAccount| a.account_id == acc.account_id) {
                        accounts.push(acc);
                    }
                }
            }
        }

        let count = accounts.len();
        for acc in &accounts {
            if let Err(e) = self.db.save_minecraft_account(acc).await {
                warn!("Failed to save Minecraft account {}: {}", acc.username, e);
            }
        }

        count
    }

    /// Scans launcher_profiles.json for Fabric, Forge, Vanilla, and custom profiles.
    pub async fn scan_profiles(&self, mc_dir: &Path) -> usize {
        let profiles_file = mc_dir.join("launcher_profiles.json");
        if !profiles_file.exists() {
            return 0;
        }

        let content = match std::fs::read_to_string(&profiles_file) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read launcher_profiles.json: {}", e);
                return 0;
            }
        };

        let profiles = parse_launcher_profiles(&content);
        let count = profiles.len();
        if let Err(e) = self.db.save_minecraft_profiles(&profiles).await {
            warn!("Failed to save Minecraft profiles: {}", e);
        }

        count
    }

    /// Scans servers.dat NBT file for multiplayer servers.
    pub async fn scan_servers(&self, mc_dir: &Path) -> usize {
        let servers_file = mc_dir.join("servers.dat");
        if !servers_file.exists() {
            return 0;
        }

        let data = match std::fs::read(&servers_file) {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to read servers.dat: {}", e);
                return 0;
            }
        };

        let servers = parse_servers_dat(&data);
        let count = servers.len();
        for server in &servers {
            if let Err(e) = self.db.save_minecraft_server(server).await {
                warn!("Failed to save Minecraft server {}: {}", server.name, e);
            }
        }

        count
    }

    /// Scans .minecraft/saves for singleplayer worlds and their icons.
    pub async fn scan_worlds(&self, mc_dir: &Path) -> usize {
        let saves_dir = mc_dir.join("saves");
        if !saves_dir.exists() || !saves_dir.is_dir() {
            return 0;
        }

        let entries = match std::fs::read_dir(&saves_dir) {
            Ok(e) => e,
            Err(_) => return 0,
        };

        let mut worlds = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let folder_name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(name) => name.to_string(),
                    None => continue,
                };

                let icon_path = path.join("icon.png");
                let icon_base64 = if icon_path.exists() {
                    std::fs::read(&icon_path)
                        .ok()
                        .map(|bytes| {
                            let b64 = base64_simd_encode(&bytes);
                            format!("data:image/png;base64,{}", b64)
                        })
                } else {
                    None
                };

                let last_played = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);

                let level_name = folder_name.clone();

                worlds.push(MinecraftWorld {
                    folder_name,
                    level_name,
                    last_played,
                    game_mode: "Survival".to_string(),
                    hardcore: false,
                    version_name: None,
                    icon_base64,
                    size_bytes: 0,
                });
            }
        }

        let count = worlds.len();
        if let Err(e) = self.db.save_minecraft_worlds(&worlds).await {
            warn!("Failed to save Minecraft worlds: {}", e);
        }

        count
    }
}

/// Parses launcher_accounts.json string into a list of MinecraftAccount objects.
pub fn parse_launcher_accounts(json_str: &str) -> Vec<MinecraftAccount> {
    let parsed: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let active_id = parsed
        .get("activeAccountLocalId")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let accounts_obj = match parsed.get("accounts").and_then(|v| v.as_object()) {
        Some(o) => o,
        None => return Vec::new(),
    };

    let mut result = Vec::new();

    for (key, val) in accounts_obj {
        let account_id = val
            .get("localId")
            .and_then(|v| v.as_str())
            .unwrap_or(key)
            .to_string();

        let prof = val.get("minecraftProfile");
        let username = prof
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .or_else(|| val.get("username").and_then(|u| u.as_str()))
            .unwrap_or("Player")
            .to_string();

        let uuid = prof
            .and_then(|p| p.get("id"))
            .and_then(|i| i.as_str())
            .unwrap_or(&account_id)
            .to_string();

        let account_type = val
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("msa")
            .to_string();

        let access_token = val
            .get("accessToken")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());

        let is_active = account_id == active_id || key == active_id;

        // Player Head Avatars using mc-heads.net and crafatar
        let avatar_url = Some(format!("https://mc-heads.net/avatar/{}/100", uuid));
        let skin_url = Some(format!("https://mc-heads.net/skin/{}", uuid));

        let raw_json = Some(val.to_string());

        result.push(MinecraftAccount {
            account_id,
            username,
            account_type,
            uuid,
            avatar_url,
            skin_url,
            is_active,
            access_token,
            raw_json,
            last_used: Some(chrono::Utc::now().to_rfc3339()),
            updated_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    result
}

/// Parses launcher_profiles.json string into a list of MinecraftProfile objects.
pub fn parse_launcher_profiles(json_str: &str) -> Vec<MinecraftProfile> {
    let parsed: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let profiles_obj = match parsed.get("profiles").and_then(|v| v.as_object()) {
        Some(o) => o,
        None => return Vec::new(),
    };

    let mut list = Vec::new();

    for (key, val) in profiles_obj {
        let name = val
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(key)
            .to_string();

        let profile_type = val
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("custom")
            .to_string();

        let version_id = val
            .get("lastVersionId")
            .and_then(|v| v.as_str())
            .unwrap_or("latest-release")
            .to_string();

        let icon = val
            .get("icon")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let game_dir = val
            .get("gameDir")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let java_dir = val
            .get("javaDir")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let java_args = val
            .get("javaArgs")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let last_used = val
            .get("lastUsed")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let created = val
            .get("created")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        list.push(MinecraftProfile {
            id: key.clone(),
            name,
            profile_type,
            version_id: version_id.clone(),
            icon,
            game_dir,
            java_dir,
            java_args,
            last_used,
            last_version_id: Some(version_id),
            created,
        });
    }

    list
}

/// Simple standard base64 encoder without external dependencies.
fn base64_simd_encode(bytes: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        out.push(CHARSET[(b0 >> 2) as usize] as char);
        out.push(CHARSET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);

        if chunk.len() > 1 {
            out.push(CHARSET[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }

        if chunk.len() > 2 {
            out.push(CHARSET[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_launcher_accounts_sample() {
        let sample = r#"{
            "activeAccountLocalId": "acc-1234",
            "accounts": {
                "acc-1234": {
                    "localId": "acc-1234",
                    "username": "steve@example.com",
                    "type": "msa",
                    "minecraftProfile": {
                        "id": "c06f8906473e4b39b4a110279b4f4001",
                        "name": "Steve_Pro"
                    }
                },
                "acc-5678": {
                    "localId": "acc-5678",
                    "username": "alex@example.com",
                    "type": "mojang",
                    "minecraftProfile": {
                        "id": "550e8400e29b41d4a716446655440000",
                        "name": "Alex_Queen"
                    }
                }
            }
        }"#;

        let accounts = parse_launcher_accounts(sample);
        assert_eq!(accounts.len(), 2);

        let steve = accounts.iter().find(|a| a.username == "Steve_Pro").unwrap();
        assert_eq!(steve.account_id, "acc-1234");
        assert!(steve.is_active);
        assert_eq!(steve.uuid, "c06f8906473e4b39b4a110279b4f4001");
        assert!(steve.avatar_url.as_ref().unwrap().contains("c06f8906473e4b39b4a110279b4f4001"));

        let alex = accounts.iter().find(|a| a.username == "Alex_Queen").unwrap();
        assert_eq!(alex.account_id, "acc-5678");
        assert!(!alex.is_active);
    }

    #[test]
    fn test_parse_launcher_profiles_sample() {
        let sample = r#"{
            "profiles": {
                "fabric-1.20.4": {
                    "name": "Fabric 1.20.4 (Performance)",
                    "type": "custom",
                    "lastVersionId": "fabric-loader-0.15.7-1.20.4",
                    "icon": "Furnace"
                },
                "(Default)": {
                    "name": "Latest release",
                    "type": "latest-release",
                    "lastVersionId": "1.20.4",
                    "icon": "Grass"
                }
            }
        }"#;

        let profiles = parse_launcher_profiles(sample);
        assert_eq!(profiles.len(), 2);

        let fabric = profiles.iter().find(|p| p.id == "fabric-1.20.4").unwrap();
        assert_eq!(fabric.name, "Fabric 1.20.4 (Performance)");
        assert_eq!(fabric.version_id, "fabric-loader-0.15.7-1.20.4");
        assert_eq!(fabric.icon, Some("Furnace".to_string()));
    }
}
