use std::path::{Path, PathBuf};
use serde_json::Value;
use sha2::Digest;
use tracing::{debug, info, warn};

use crate::modules::detector::LauncherDetector;
use crate::storage::models::{MinecraftAccount, MinecraftProfile, MinecraftWorld};
use crate::storage::Database;
use super::nbt::parse_servers_dat;

/// Scanner responsible for discovering and importing accounts, profiles,
/// multiplayer servers, and singleplayer worlds from `.minecraft` and `.tlauncher`.
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

    /// Resolves the .tlauncher data directory on the host system.
    pub fn resolve_tlauncher_dir(&self) -> Option<PathBuf> {
        self.detector.detect_tlauncher_dir()
    }

    /// Runs a full discovery scan across accounts, profiles, servers, and worlds.
    pub async fn scan_and_import(&self) -> (usize, usize, usize, usize) {
        let mc_dir = self.resolve_minecraft_dir();
        let tl_dir = self.resolve_tlauncher_dir();

        let base_dir = match (&mc_dir, &tl_dir) {
            (Some(d), _) if d.exists() => d.clone(),
            (_, Some(d)) if d.exists() => d.clone(),
            _ => {
                debug!("Minecraft/TLauncher data directories not found on system.");
                return (0, 0, 0, 0);
            }
        };

        info!("Scanning Minecraft/TLauncher directories (primary: {})", base_dir.display());

        let acc_count = self.scan_accounts(&base_dir).await;
        let prof_count = self.scan_profiles(&base_dir).await;
        let srv_count = self.scan_servers(&base_dir).await;
        let world_count = self.scan_worlds(&base_dir).await;

        info!(
            "Minecraft scan complete: {} account(s), {} profile(s), {} server(s), {} world(s)",
            acc_count, prof_count, srv_count, world_count
        );

        (acc_count, prof_count, srv_count, world_count)
    }

    /// Scans launcher_accounts.json, TlauncherProfiles.json, and properties for saved accounts.
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

        // 3. TLauncher profiles in mc_dir
        for name in &["TlauncherProfiles.json", "tlauncher_profiles.json", "tlauncher-profiles.json", "tlauncherProfiles.json"] {
            let tl_file = mc_dir.join(name);
            if tl_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&tl_file) {
                    let tl_accs = parse_tlauncher_profiles_accounts(&content);
                    for acc in tl_accs {
                        if !accounts.iter().any(|a: &MinecraftAccount| a.account_id == acc.account_id || a.username.eq_ignore_ascii_case(&acc.username)) {
                            accounts.push(acc);
                        }
                    }
                }
            }
        }

        // 4. TLauncher directory if separate
        if let Some(tl_dir) = self.resolve_tlauncher_dir() {
            if tl_dir.exists() && tl_dir != mc_dir {
                for name in &["TlauncherProfiles.json", "tlauncher_profiles.json", "tlauncher-profiles.json", "tlauncherProfiles.json"] {
                    let tl_sub_file = tl_dir.join(name);
                    if tl_sub_file.exists() {
                        if let Ok(content) = std::fs::read_to_string(&tl_sub_file) {
                            for acc in parse_tlauncher_profiles_accounts(&content) {
                                if !accounts.iter().any(|a: &MinecraftAccount| a.account_id == acc.account_id || a.username.eq_ignore_ascii_case(&acc.username)) {
                                    accounts.push(acc);
                                }
                            }
                        }
                    }
                }

                for prop_name in &["tlauncher-2.0.properties", "tlauncher.properties"] {
                    let prop_path = tl_dir.join(prop_name);
                    if prop_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&prop_path) {
                            for acc in parse_tlauncher_properties(&content) {
                                if !accounts.iter().any(|a: &MinecraftAccount| a.username.eq_ignore_ascii_case(&acc.username)) {
                                    accounts.push(acc);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 5. Properties files in mc_dir
        for prop_name in &["tlauncher-2.0.properties", "tlauncher.properties"] {
            let prop_path = mc_dir.join(prop_name);
            if prop_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&prop_path) {
                    for acc in parse_tlauncher_properties(&content) {
                        if !accounts.iter().any(|a: &MinecraftAccount| a.username.eq_ignore_ascii_case(&acc.username)) {
                            accounts.push(acc);
                        }
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

    /// Scans launcher_profiles.json and TlauncherProfiles.json for Fabric, Forge, Vanilla, and custom profiles.
    pub async fn scan_profiles(&self, mc_dir: &Path) -> usize {
        let mut profiles = Vec::new();

        let profiles_file = mc_dir.join("launcher_profiles.json");
        if profiles_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&profiles_file) {
                profiles.extend(parse_launcher_profiles(&content));
            }
        }

        // Also inspect TLauncher profiles files
        for name in &["TlauncherProfiles.json", "tlauncher_profiles.json", "tlauncher-profiles.json", "tlauncherProfiles.json"] {
            let tl_profiles_file = mc_dir.join(name);
            if tl_profiles_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&tl_profiles_file) {
                    let tl_profs = parse_launcher_profiles(&content);
                    for p in tl_profs {
                        if !profiles.iter().any(|existing: &MinecraftProfile| existing.id == p.id) {
                            profiles.push(p);
                        }
                    }
                }
            }
        }

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

/// Generates a standard offline player UUID based on SHA-256 of "OfflinePlayer:<username>".
pub fn generate_offline_uuid(username: &str) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(format!("OfflinePlayer:{}", username).as_bytes());
    let result = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&result[0..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x30; // version 3
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn sanitize_account_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

/// Parses TlauncherProfiles.json string into a list of MinecraftAccount objects.
pub fn parse_tlauncher_profiles_accounts(json_str: &str) -> Vec<MinecraftAccount> {
    let parsed: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    // Determine active account indicator
    let active_user_account = parsed
        .get("selectedUser")
        .and_then(|u| {
            if let Some(obj) = u.as_object() {
                obj.get("account").or_else(|| obj.get("profile")).and_then(|s| s.as_str())
            } else {
                u.as_str()
            }
        })
        .unwrap_or("");

    let selected_account = parsed
        .get("selected_account")
        .or_else(|| parsed.get("selectedAccount"))
        .or_else(|| parsed.get("activeAccountLocalId"))
        .and_then(|s| s.as_str())
        .unwrap_or("");

    let mut result = Vec::new();

    // 1. Try "accounts" as an object
    if let Some(accounts_obj) = parsed.get("accounts").and_then(|a| a.as_object()) {
        for (key, val) in accounts_obj {
            let username = val
                .get("displayName")
                .or_else(|| val.get("username"))
                .or_else(|| val.get("name"))
                .and_then(|s| s.as_str())
                .unwrap_or("Player")
                .to_string();

            let raw_type = val
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("tlauncher");

            let account_type = if raw_type.eq_ignore_ascii_case("free") || raw_type.eq_ignore_ascii_case("offline") {
                "tlauncher_free".to_string()
            } else if raw_type.eq_ignore_ascii_case("mojang") {
                "tlauncher_mojang".to_string()
            } else if raw_type.eq_ignore_ascii_case("msa") || raw_type.eq_ignore_ascii_case("microsoft") {
                "tlauncher_msa".to_string()
            } else if raw_type.eq_ignore_ascii_case("elyby") {
                "tlauncher_elyby".to_string()
            } else {
                "tlauncher".to_string()
            };

            let uuid = val
                .get("uuid")
                .or_else(|| val.get("userid"))
                .or_else(|| val.get("id"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| generate_offline_uuid(&username));

            let account_id = if key.starts_with("tl_") {
                key.clone()
            } else {
                format!("tl_{}", key)
            };

            let is_explicit_active = val.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let is_active = is_explicit_active
                || (!active_user_account.is_empty()
                    && (key.eq_ignore_ascii_case(active_user_account)
                        || uuid.eq_ignore_ascii_case(active_user_account)
                        || username.eq_ignore_ascii_case(active_user_account)))
                || (!selected_account.is_empty()
                    && (key.eq_ignore_ascii_case(selected_account)
                        || uuid.eq_ignore_ascii_case(selected_account)
                        || username.eq_ignore_ascii_case(selected_account)));

            let access_token = val
                .get("accessToken")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());

            let avatar_url = Some(format!("https://mc-heads.net/avatar/{}/100", username));
            let skin_url = Some(format!("https://mc-heads.net/skin/{}", username));
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
    } else if let Some(accounts_arr) = parsed.get("accounts").and_then(|a| a.as_array()) {
        // 2. Try "accounts" as an array
        for (idx, val) in accounts_arr.iter().enumerate() {
            let username = val
                .get("displayName")
                .or_else(|| val.get("username"))
                .or_else(|| val.get("name"))
                .and_then(|s| s.as_str())
                .unwrap_or("Player")
                .to_string();

            let uuid = val
                .get("uuid")
                .or_else(|| val.get("userid"))
                .or_else(|| val.get("id"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| generate_offline_uuid(&username));

            let account_id = format!("tl_{}", &uuid);
            let raw_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("tlauncher");
            let account_type = if raw_type.eq_ignore_ascii_case("free") || raw_type.eq_ignore_ascii_case("offline") {
                "tlauncher_free".to_string()
            } else {
                "tlauncher".to_string()
            };

            let is_explicit_active = val.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let is_active = is_explicit_active
                || (!active_user_account.is_empty()
                    && (uuid.eq_ignore_ascii_case(active_user_account)
                        || username.eq_ignore_ascii_case(active_user_account)))
                || (!selected_account.is_empty()
                    && (uuid.eq_ignore_ascii_case(selected_account)
                        || username.eq_ignore_ascii_case(selected_account)))
                || (idx == 0 && active_user_account.is_empty() && selected_account.is_empty());

            result.push(MinecraftAccount {
                account_id,
                username: username.clone(),
                account_type,
                uuid,
                avatar_url: Some(format!("https://mc-heads.net/avatar/{}/100", username)),
                skin_url: Some(format!("https://mc-heads.net/skin/{}", username)),
                is_active,
                access_token: val.get("accessToken").and_then(|t| t.as_str()).map(|s| s.to_string()),
                raw_json: Some(val.to_string()),
                last_used: Some(chrono::Utc::now().to_rfc3339()),
                updated_at: chrono::Utc::now().to_rfc3339(),
            });
        }
    }

    // 3. Fallback: "authenticationDatabase" if accounts list was empty
    if result.is_empty() {
        if let Some(auth_db) = parsed.get("authenticationDatabase").and_then(|a| a.as_object()) {
            for (key, val) in auth_db {
                let username = val
                    .get("displayName")
                    .or_else(|| val.get("username"))
                    .or_else(|| {
                        val.get("profiles")
                            .and_then(|p| p.as_object())
                            .and_then(|o| o.values().next())
                            .and_then(|p0| p0.get("displayName"))
                    })
                    .and_then(|s| s.as_str())
                    .unwrap_or("Player")
                    .to_string();

                let uuid = val
                    .get("uuid")
                    .and_then(|s| s.as_str())
                    .unwrap_or(key)
                    .to_string();

                let account_id = format!("tl_{}", &uuid);
                let is_active = (!active_user_account.is_empty()
                    && (key.eq_ignore_ascii_case(active_user_account)
                        || uuid.eq_ignore_ascii_case(active_user_account)
                        || username.eq_ignore_ascii_case(active_user_account)))
                    || (!selected_account.is_empty()
                        && (key.eq_ignore_ascii_case(selected_account)
                            || uuid.eq_ignore_ascii_case(selected_account)
                            || username.eq_ignore_ascii_case(selected_account)));

                result.push(MinecraftAccount {
                    account_id,
                    username: username.clone(),
                    account_type: "tlauncher".to_string(),
                    uuid,
                    avatar_url: Some(format!("https://mc-heads.net/avatar/{}/100", username)),
                    skin_url: Some(format!("https://mc-heads.net/skin/{}", username)),
                    is_active,
                    access_token: val.get("accessToken").and_then(|t| t.as_str()).map(|s| s.to_string()),
                    raw_json: Some(val.to_string()),
                    last_used: Some(chrono::Utc::now().to_rfc3339()),
                    updated_at: chrono::Utc::now().to_rfc3339(),
                });
            }
        }
    }

    result
}

/// Parses tlauncher-2.0.properties string for active selected account.
pub fn parse_tlauncher_properties(props_str: &str) -> Vec<MinecraftAccount> {
    let mut props_map = std::collections::HashMap::new();
    for line in props_str.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim().to_lowercase();
            let val = v.trim();
            if !val.is_empty() {
                props_map.insert(key, val.to_string());
            }
        }
    }

    let priority_keys = ["selected.account", "account.selected", "login.account", "user.login"];
    for key in priority_keys {
        if let Some(username) = props_map.get(key) {
            let uuid = generate_offline_uuid(username);
            let account_id = format!("tl_{}", sanitize_account_id(username));
            return vec![MinecraftAccount {
                account_id,
                username: username.clone(),
                account_type: "tlauncher".to_string(),
                uuid,
                avatar_url: Some(format!("https://mc-heads.net/avatar/{}/100", username)),
                skin_url: Some(format!("https://mc-heads.net/skin/{}", username)),
                is_active: true,
                access_token: None,
                raw_json: None,
                last_used: Some(chrono::Utc::now().to_rfc3339()),
                updated_at: chrono::Utc::now().to_rfc3339(),
            }];
        }
    }

    Vec::new()
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
