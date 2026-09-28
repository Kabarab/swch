//! Read-only local discovery. A cached app is NOT proof of ownership.
use super::{SteamError, SteamPaths};
use crate::parsers::vdf::{VdfDocument, VdfTable};
use crate::storage::{Account, Game};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
};

pub const STEAM_ID_BASE: u64 = 76561197960265728;
pub fn valid_steam_id(id: &str) -> bool {
    id.len() == 17
        && id
            .parse::<u64>()
            .is_ok_and(|n| n > STEAM_ID_BASE && n <= STEAM_ID_BASE + u32::MAX as u64)
}
pub fn valid_app_id(id: &str) -> bool {
    id.parse::<u32>().is_ok_and(|n| n > 0) && id.bytes().all(|b| b.is_ascii_digit())
}
pub fn is_ignored_steam_app(app_id: &str, name: &str) -> bool {
    let trimmed_id = app_id.trim();
    if matches!(trimmed_id, "7" | "760" | "2371090") {
        return true;
    }
    let lower_name = name.trim().to_lowercase();
    matches!(
        lower_name.as_str(),
        "steam client" | "steam screenshots" | "steam game notes"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamGame {
    #[serde(flatten)]
    pub game: Game,
    pub account_id: Option<String>,
    #[serde(default)]
    pub is_free: Option<bool>,
    #[serde(default)]
    pub has_inventory: Option<bool>,
    #[serde(default)]
    pub update: Option<UpdateState>,
    pub access: String,
    pub owner_steamids: Vec<String>,
    pub playtime_minutes: Option<u64>,
    pub playtime_2weeks: Option<u64>,
    pub last_played: Option<u64>,
    pub source: String,
    pub updated_at: String,
}
impl SteamGame {
    pub fn new(appid: &str, account_id: Option<String>) -> Self {
        Self {
            game: Game {
                id: format!("steam_{appid}"),
                launcher: "steam".into(),
                app_id: appid.into(),
                name: format!("Steam #{appid}"),
                install_path: None,
                executable_path: None,
                size_bytes: 0,
                is_installed: false,
                banner_url: Some(format!(
                    "https://cdn.akamai.steamstatic.com/steam/apps/{appid}/library_600x900.jpg"
                )),
                launch_uri: Some(format!("steam://run/{appid}")),
                namespace: None,
            },
            account_id,
            is_free: None,
            has_inventory: None,
            update: None,
            access: "local".into(),
            owner_steamids: vec![],
            playtime_minutes: None,
            playtime_2weeks: None,
            last_played: None,
            source: "local".into(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}
/// A read-only snapshot of the local Steam manifest, not an online version check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateState {
    pub required: bool,
    pub phase: String,
    pub downloaded: u64,
    pub download_total: u64,
    pub staged: u64,
    pub stage_total: u64,
}
impl UpdateState {
    fn from_manifest(state: &VdfTable) -> Self {
        let flags = number(state, "StateFlags").unwrap_or(0);
        let required = flags & 2 != 0;
        let phase = if flags & 512 != 0 {
            "paused"
        } else if flags & 1024 != 0 {
            "downloading"
        } else if flags & (131072 | 262144) != 0 {
            "installing"
        } else if flags & 128 != 0 {
            "verifying"
        } else if required {
            "required"
        } else if flags & 4 != 0 {
            "ready"
        } else {
            "unknown"
        };
        Self {
            required,
            phase: phase.into(),
            downloaded: number(state, "BytesDownloaded").unwrap_or(0),
            download_total: number(state, "BytesToDownload").unwrap_or(0),
            staged: number(state, "BytesStaged").unwrap_or(0),
            stage_total: number(state, "BytesToStage").unwrap_or(0),
        }
    }
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SteamLibrary {
    pub accounts: Vec<Account>,
    pub games: Vec<SteamGame>,
    pub warnings: Vec<String>,
    pub api_key_configured: bool,
    #[serde(default)]
    pub synced_sources: std::collections::BTreeMap<String, Vec<String>>,
}
fn number(t: &VdfTable, key: &str) -> Option<u64> {
    t.get_str(key)?.parse().ok()
}

#[derive(Debug, Clone, Default)]
pub struct FamilyGroupInfo {
    pub group_id: String,
    pub name: String,
    pub members: HashSet<String>,
}

impl FamilyGroupInfo {
    pub fn is_member(&self, steam_id: &str) -> bool {
        self.members.contains(steam_id)
    }

    pub fn has_other_members(&self, my_steam_id: &str) -> bool {
        !self.group_id.is_empty()
            && self.group_id != "0"
            && self.members.iter().any(|id| id != my_steam_id)
    }
}

pub fn parse_family_group(doc: &VdfDocument, account_id: &str) -> Option<FamilyGroupInfo> {
    let fg = doc.find_obj_path(&["UserLocalConfigStore", "FamilyGroup"])?;
    let group_id = fg.get_str("groupid").unwrap_or("").trim().to_string();
    let name = fg.get_str("name").unwrap_or("").trim().to_string();
    let mut members = HashSet::new();
    if let Some(members_table) = fg.get_obj("members") {
        for (key, val) in members_table.iter() {
            let acc_str = val
                .as_obj()
                .and_then(|t| t.get_str("accountid"))
                .or_else(|| {
                    if key.bytes().all(|b| b.is_ascii_digit()) && key.len() >= 4 {
                        Some(key.as_str())
                    } else {
                        None
                    }
                });
            if let Some(s) = acc_str {
                if let Ok(num) = s.parse::<u64>() {
                    let steam64 = if num > STEAM_ID_BASE {
                        num.to_string()
                    } else {
                        (num + STEAM_ID_BASE).to_string()
                    };
                    members.insert(steam64);
                }
            }
        }
    }
    if !group_id.is_empty() && group_id != "0" {
        members.insert(account_id.to_string());
    }
    Some(FamilyGroupInfo {
        group_id,
        name,
        members,
    })
}

fn parse_preferred_owners(steam_dir: &std::path::Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let log_path = steam_dir.join("logs/librarysharing_log.txt");
    let Ok(content) = fs::read_to_string(log_path) else {
        return map;
    };
    for line in content.lines() {
        if let Some(app_idx) = line.find("app ") {
            let after_app = &line[app_idx + 4..];
            let app_id_str: String = after_app.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !app_id_str.is_empty() {
                if let Some(pref_idx) = line.find("preferred owner ") {
                    let after_pref = &line[pref_idx + 16..];
                    let owner_str: String =
                        after_pref.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(owner32) = owner_str.parse::<u64>() {
                        let owner64 = if owner32 > STEAM_ID_BASE {
                            owner32.to_string()
                        } else {
                            (owner32 + STEAM_ID_BASE).to_string()
                        };
                        map.insert(app_id_str, owner64);
                    }
                }
            }
        }
    }
    map
}

fn extract_cloud_apps(key: &str, val: Option<&serde_json::Value>, apps: &mut HashSet<String>) {
    if let Some(aid) = key.strip_prefix("NewContentRollup_") {
        if valid_app_id(aid) && !is_ignored_steam_app(aid, "") {
            apps.insert(aid.to_string());
        }
    } else if key.starts_with("user-collections") {
        if let Some(val_obj) = val {
            let val_str = val_obj
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if let Ok(coll) = serde_json::from_str::<serde_json::Value>(val_str) {
                if let Some(added) = coll.get("added").and_then(|v| v.as_array()) {
                    for elem in added {
                        let id_str = elem
                            .as_u64()
                            .map(|n| n.to_string())
                            .or_else(|| elem.as_str().map(|s| s.to_string()));
                        if let Some(id) = id_str {
                            if valid_app_id(&id) && !is_ignored_steam_app(&id, "") {
                                apps.insert(id);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn read_cloud_and_cache_apps(user_dir: &std::path::Path) -> HashSet<String> {
    let mut apps = HashSet::new();

    let cloud_file = user_dir.join("config/cloudstorage/cloud-storage-namespace-1.json");
    if let Ok(data) = fs::read(&cloud_file) {
        if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&data) {
            if let Some(items) = parsed.as_array() {
                for item in items {
                    if let Some(pair) = item.as_array() {
                        if let Some(key) = pair.first().and_then(|v| v.as_str()) {
                            extract_cloud_apps(key, pair.get(1), &mut apps);
                        }
                    }
                }
            } else if let Some(map) = parsed.as_object() {
                for (key, val) in map {
                    extract_cloud_apps(key, Some(val), &mut apps);
                }
            }
        }
    }

    let librarycache_dir = user_dir.join("config/librarycache");
    if let Ok(entries) = fs::read_dir(librarycache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if valid_app_id(stem) && !is_ignored_steam_app(stem, "") {
                        apps.insert(stem.to_string());
                    }
                }
            }
        }
    }

    apps
}

pub fn scan(paths: &SteamPaths) -> Result<SteamLibrary, SteamError> {
    let mut result = SteamLibrary::default();
    if paths.loginusers_path().exists() {
        let doc = VdfDocument::from_file(paths.loginusers_path())?;
        let users = doc.get_obj("users").ok_or(SteamError::InvalidLoginUsers)?;
        for (id, value) in users.iter() {
            if !valid_steam_id(id) {
                continue;
            }
            let Some(user) = value.as_obj() else { continue };
            result.accounts.push(Account {
                id: id.clone(),
                launcher: "steam".into(),
                username: user.get_str("AccountName").unwrap_or(id).into(),
                display_name: user.get_str("PersonaName").map(String::from),
                email: None,
                is_active: user.get_str("MostRecent") == Some("1"),
                last_used: number(user, "Timestamp")
                    .and_then(|n| chrono::DateTime::from_timestamp(n as i64, 0))
                    .map(|d| d.to_rfc3339()),
            });
        }
        if !result.accounts.iter().any(|a| a.is_active) {
            if let Some(active_id) = crate::launchers::steam::detect_active_steam_user(paths, users) {
                for acc in &mut result.accounts {
                    if acc.id == active_id || acc.username.eq_ignore_ascii_case(&active_id) {
                        acc.is_active = true;
                        break;
                    }
                }
            }
        }
    }
    let mut roots = vec![paths.steam_dir.clone()];
    for path in [
        paths.steam_dir.join("steamapps/libraryfolders.vdf"),
        paths.config_dir().join("libraryfolders.vdf"),
    ] {
        if let Ok(doc) = VdfDocument::from_file(path) {
            if let Some(libraries) = doc.get_obj("libraryfolders") {
                for (key, value) in libraries.iter() {
                    if key.parse::<u32>().is_err() {
                        continue;
                    }
                    if let Some(path) = value.as_str().or_else(|| value.as_obj()?.get_str("path")) {
                        roots.push(path.into());
                    }
                }
            }
        }
    }
    roots.sort();
    roots.dedup();
    let mut installed = BTreeMap::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root.join("steamapps")) else {
            continue;
        };
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().into_owned();
            let Some(appid) = filename
                .strip_prefix("appmanifest_")
                .and_then(|s| s.strip_suffix(".acf"))
                .filter(|s| valid_app_id(s) && !is_ignored_steam_app(s, ""))
            else {
                continue;
            };
            let doc = match VdfDocument::from_file(entry.path()) {
                Ok(d) => d,
                Err(_) => {
                    result
                        .warnings
                        .push(format!("Не удалось прочитать манифест игры {appid}."));
                    continue;
                }
            };
            let Some(state) = doc.get_obj("AppState") else {
                continue;
            };
            let mut g = SteamGame::new(appid, None);
            g.game.name = state.get_str("name").unwrap_or(&g.game.name).into();
            g.game.size_bytes = number(state, "SizeOnDisk")
                .unwrap_or(0)
                .min(i64::MAX as u64) as i64;
            // FullyInstalled bit; downloading manifests must not enable Play.
            g.game.is_installed = number(state, "StateFlags").is_some_and(|f| f & 4 != 0);
            g.game.install_path = state.get_str("installdir").map(|s| {
                root.join("steamapps/common")
                    .join(s)
                    .to_string_lossy()
                    .into_owned()
            });
            g.update = Some(UpdateState::from_manifest(state));
            let last_owner = state.get_str("LastOwner").filter(|s| valid_steam_id(s)).map(String::from);
            installed.insert(appid.to_owned(), (g, last_owner));
        }
    }
    let preferred_owners = parse_preferred_owners(&paths.steam_dir);
    let mut family_groups: HashMap<String, FamilyGroupInfo> = HashMap::new();

    for account in &result.accounts {
        let account32 = account.id.parse::<u64>().unwrap() - STEAM_ID_BASE;
        let user_dir = paths
            .steam_dir
            .join("userdata")
            .join(account32.to_string());
        let config = user_dir.join("config/localconfig.vdf");
        if !config.exists() {
            continue;
        }
        let doc = match VdfDocument::from_file(&config) {
            Ok(d) => d,
            Err(_) => {
                result.warnings.push(format!(
                    "Не удалось прочитать локальную статистику {}.",
                    account.username
                ));
                continue;
            }
        };

        let family_info = parse_family_group(&doc, &account.id);
        if let Some(fi) = &family_info {
            family_groups.insert(account.id.clone(), fi.clone());
        }

        if let Some(apps) =
            doc.find_obj_path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"])
        {
            for (id, value) in apps.iter() {
                if !valid_app_id(id) || is_ignored_steam_app(id, "") {
                    continue;
                }
                let Some(app) = value.as_obj() else { continue };
                let (mut g, manifest_owner) = installed
                    .get(id)
                    .map(|(game, owner)| (game.clone(), owner.clone()))
                    .unwrap_or_else(|| (SteamGame::new(id, None), None));
                g.account_id = Some(account.id.clone());
                if let Some(owner) = manifest_owner {
                    if owner == account.id {
                        g.access = "owned".into();
                        g.owner_steamids = vec![account.id.clone()];
                    } else if family_info.as_ref().is_some_and(|f| f.is_member(&owner)) {
                        g.access = "family".into();
                        g.source = "steam_family".into();
                        g.owner_steamids = vec![owner];
                    } else {
                        g.access = "local".into();
                        g.owner_steamids = vec![];
                    }
                } else {
                    g.access = "local".into();
                    g.owner_steamids.clear();
                }
                if g.game.name.starts_with("Steam #") {
                    if let Some(name) = app.get_str("name") {
                        g.game.name = name.into();
                    }
                }
                g.playtime_minutes = number(app, "Playtime");
                g.playtime_2weeks = number(app, "Playtime2weeks");
                g.last_played = number(app, "LastPlayed");
                result.games.push(g);
            }
        }

        // If the account belongs to a Family Group with other members, discover family games
        // from cloud storage, library cache, and sharing log.
        if let Some(fi) = &family_info {
            if fi.has_other_members(&account.id) {
                let mut extra_apps = read_cloud_and_cache_apps(&user_dir);
                for (aid, owner) in &preferred_owners {
                    if fi.is_member(owner) && owner != &account.id {
                        extra_apps.insert(aid.clone());
                    }
                }
                for id in extra_apps {
                    if !valid_app_id(&id) || is_ignored_steam_app(&id, "") {
                        continue;
                    }
                    if result
                        .games
                        .iter()
                        .any(|x| x.game.app_id == id && x.account_id.as_deref() == Some(&account.id))
                    {
                        continue;
                    }
                    let (mut g, manifest_owner) = installed
                        .get(&id)
                        .map(|(game, owner)| (game.clone(), owner.clone()))
                        .unwrap_or_else(|| (SteamGame::new(&id, None), None));
                    g.account_id = Some(account.id.clone());
                    g.access = "family".into();
                    g.source = "steam_family".into();
                    if let Some(owner) =
                        manifest_owner.filter(|o| fi.is_member(o) && o != &account.id)
                    {
                        g.owner_steamids = vec![owner];
                    } else if let Some(owner) =
                        preferred_owners.get(&id).filter(|o| fi.is_member(o) && *o != &account.id)
                    {
                        g.owner_steamids = vec![owner.clone()];
                    } else {
                        let mut other_members: Vec<String> = fi
                            .members
                            .iter()
                            .filter(|m| *m != &account.id)
                            .cloned()
                            .collect();
                        other_members.sort();
                        g.owner_steamids = other_members;
                    }
                    result.games.push(g);
                }
            }
        }
    }
    // Ensure every installed game with LastOwner is owned by its owner
    for (id, (game, owner_opt)) in &installed {
        if let Some(owner) = owner_opt {
            if result.accounts.iter().any(|a| &a.id == owner) {
                if !result
                    .games
                    .iter()
                    .any(|x| &x.game.app_id == id && x.account_id.as_deref() == Some(owner))
                {
                    let mut owner_game = game.clone();
                    owner_game.account_id = Some(owner.clone());
                    owner_game.access = "owned".into();
                    owner_game.owner_steamids = vec![owner.clone()];
                    result.games.push(owner_game);
                }
            }
        }
    }
    // Share installed games belonging to other accounts ONLY via verified family sharing
    for account in &result.accounts {
        let acc_fam = family_groups.get(&account.id);
        let has_family = acc_fam
            .map(|f| f.has_other_members(&account.id))
            .unwrap_or(false);
        if !has_family {
            continue;
        }
        let acc_fam = acc_fam.unwrap();
        for (id, (game, owner_opt)) in &installed {
            if let Some(owner) = owner_opt {
                if owner != &account.id && acc_fam.is_member(owner) {
                    if !result
                        .games
                        .iter()
                        .any(|x| &x.game.app_id == id && x.account_id.as_deref() == Some(&account.id))
                    {
                        let mut shared = game.clone();
                        shared.account_id = Some(account.id.clone());
                        shared.access = "family".into();
                        shared.source = "steam_family".into();
                        shared.owner_steamids = vec![owner.clone()];
                        result.games.push(shared);
                    }
                }
            }
        }
    }
    for (id, (mut g, owner_opt)) in installed {
        if !result.games.iter().any(|x| x.game.app_id == id) {
            if let Some(owner) = owner_opt {
                if result.accounts.iter().any(|a| a.id == owner) {
                    g.account_id = Some(owner.clone());
                    g.access = "owned".into();
                    g.owner_steamids = vec![owner];
                }
            }
            result.games.push(g);
        }
    }
    let appinfo_path = paths.steam_dir.join("appcache/appinfo.vdf");
    if fs::metadata(&appinfo_path).is_ok_and(|m| m.len() <= 64 * 1024 * 1024) {
        if let Ok(bytes) = fs::read(appinfo_path) {
            let names = super::appinfo::metadata(&bytes);
            for g in &mut result.games {
                if let Some(meta) = names.get(&g.game.app_id) {
                    if let Some(name) = &meta.name {
                        g.game.name = name.clone();
                    }
                    g.is_free = Some(meta.is_free);
                    g.has_inventory = Some(meta.has_inventory);
                }
            }
        }
    }
    result.games.retain(|g| !is_ignored_steam_app(&g.game.app_id, &g.game.name));
    result.warnings.push("Локальная история не подтверждает владение игрой. Обнови онлайн-данные для полной библиотеки и семейного доступа.".into());
    Ok(result)
}

/// Overlay one authoritative source for one account. Local playtime remains a fallback.
pub fn merge_remote(library: &mut SteamLibrary, mut remote: Vec<SteamGame>) {
    remote.retain(|g| !is_ignored_steam_app(&g.game.app_id, &g.game.name));
    for mut game in remote {
        if let Some(local) = library
            .games
            .iter()
            .find(|g| g.game.app_id == game.game.app_id)
        {
            game.is_free = game.is_free.or(local.is_free);
            game.has_inventory = game.has_inventory.or(local.has_inventory);
        }
        if let Some(local) = library
            .games
            .iter()
            .find(|g| g.game.app_id == game.game.app_id && g.account_id == game.account_id)
        {
            game.playtime_minutes = game.playtime_minutes.or(local.playtime_minutes);
            game.last_played = game.last_played.or(local.last_played);
        }
        if let Some(local) = library
            .games
            .iter()
            .find(|g| g.game.app_id == game.game.app_id && g.game.install_path.is_some())
        {
            game.game.is_installed = local.game.is_installed;
            game.game.install_path = local.game.install_path.clone();
            game.game.size_bytes = local.game.size_bytes;
            game.update = local.update.clone();
        }
        library.games.retain(|g| {
            !(g.game.app_id == game.game.app_id
                && (g.account_id == game.account_id || g.account_id.is_none()))
        });
        library.games.push(game);
    }
}
