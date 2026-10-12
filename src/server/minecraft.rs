use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::launchers::minecraft::{
    LaunchMinecraftRequest, LaunchMinecraftResponse, MinecraftLibraryResponse, MinecraftManager,
};
use crate::storage::models::{MinecraftAccount, MinecraftProfile, MinecraftServer, MinecraftWorld};

type Error = (StatusCode, Json<Value>);

fn err(status: StatusCode, message: &str) -> Error {
    (status, Json(json!({ "error": message })))
}

#[derive(Debug, Deserialize)]
pub struct SwitchAccountRequest {
    pub account_id: Option<String>,
    pub account_name: Option<String>,
    pub id: Option<String>,
}

impl SwitchAccountRequest {
    pub fn target_id(&self) -> Option<&str> {
        self.account_id
            .as_deref()
            .or(self.id.as_deref())
            .or(self.account_name.as_deref())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    }
}

#[derive(Debug, Deserialize)]
pub struct AddServerRequest {
    pub id: Option<String>,
    pub name: String,
    pub ip: String,
    pub icon: Option<String>,
    pub is_favorite: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct AddMinecraftAccountRequest {
    pub username: String,
    pub account_type: Option<String>,
    pub uuid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct OpenLauncherRequest {
    pub account_id: Option<String>,
    #[serde(default, alias = "client", alias = "launcher")]
    pub launcher_client: Option<String>,
}

/// GET /api/minecraft/library
pub async fn library(
    State(mc): State<Arc<MinecraftManager>>,
) -> Json<MinecraftLibraryResponse> {
    Json(mc.get_library_summary().await)
}

/// GET /api/minecraft/accounts
pub async fn list_accounts(
    State(mc): State<Arc<MinecraftManager>>,
) -> Result<Json<Vec<MinecraftAccount>>, Error> {
    match mc.db().get_minecraft_accounts().await {
        Ok(accounts) => Ok(Json(accounts)),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())),
    }
}

/// POST /api/minecraft/accounts/scan
pub async fn scan_accounts(
    State(mc): State<Arc<MinecraftManager>>,
) -> Result<Json<Value>, Error> {
    let (accs, profs, srvs, worlds) = mc.scanner().scan_and_import().await;
    let all = mc.db().get_minecraft_accounts().await.unwrap_or_default();
    Ok(Json(json!({
        "status": "success",
        "accounts_found": accs,
        "profiles_found": profs,
        "servers_found": srvs,
        "worlds_found": worlds,
        "total_accounts": all.len(),
        "accounts": all,
    })))
}

/// POST /api/minecraft/switch-account & POST /api/minecraft/accounts/switch
pub async fn switch_account(
    State(mc): State<Arc<MinecraftManager>>,
    Json(payload): Json<SwitchAccountRequest>,
) -> Result<Json<Value>, Error> {
    let target = payload
        .target_id()
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "account_id обязателен для переключения"))?;

    match mc.hot_swap().switch_account(target).await {
        Ok(message) => Ok(Json(json!({
            "status": "success",
            "message": message,
            "account_id": target,
        }))),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())),
    }
}

/// POST /api/minecraft/accounts/remove
pub async fn remove_account(
    State(mc): State<Arc<MinecraftManager>>,
    Json(payload): Json<SwitchAccountRequest>,
) -> Result<Json<Value>, Error> {
    let target = payload
        .target_id()
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "account_id обязателен для удаления"))?;

    match mc.db().delete_minecraft_account(target).await {
        Ok(_) => Ok(Json(json!({
            "status": "success",
            "message": "Аккаунт Minecraft отключен",
            "account_id": target,
        }))),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())),
    }
}

/// POST /api/minecraft/accounts/add
pub async fn add_account(
    State(mc): State<Arc<MinecraftManager>>,
    Json(payload): Json<AddMinecraftAccountRequest>,
) -> Result<Json<MinecraftAccount>, Error> {
    let username = payload.username.trim();
    if username.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Имя пользователя не может быть пустым"));
    }

    let uuid = payload
        .uuid
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| crate::launchers::minecraft::scanner::generate_offline_uuid(username));

    let raw_type = payload.account_type.as_deref().unwrap_or("tlauncher_free");
    let account_type = if raw_type.is_empty() {
        "tlauncher_free"
    } else {
        raw_type
    };
    let account_id = format!("tl_{}", &uuid);

    let account = MinecraftAccount {
        account_id: account_id.clone(),
        username: username.to_string(),
        account_type: account_type.to_string(),
        uuid: uuid.clone(),
        avatar_url: Some(format!("https://mc-heads.net/avatar/{}/100", username)),
        skin_url: Some(format!("https://mc-heads.net/skin/{}", username)),
        is_active: true,
        access_token: None,
        raw_json: None,
        last_used: Some(chrono::Utc::now().to_rfc3339()),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    mc.db().save_minecraft_account(&account).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    })?;

    // Perform hot-swap to write to TlauncherProfiles.json and make it active
    let _ = mc.hot_swap().switch_account(&account_id).await;

    Ok(Json(account))
}

/// GET /api/minecraft/profiles
pub async fn list_profiles(
    State(mc): State<Arc<MinecraftManager>>,
) -> Result<Json<Vec<MinecraftProfile>>, Error> {
    match mc.db().get_minecraft_profiles().await {
        Ok(profiles) => Ok(Json(profiles)),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())),
    }
}

/// GET /api/minecraft/worlds
pub async fn list_worlds(
    State(mc): State<Arc<MinecraftManager>>,
) -> Result<Json<Vec<MinecraftWorld>>, Error> {
    match mc.db().get_minecraft_worlds().await {
        Ok(worlds) => Ok(Json(worlds)),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())),
    }
}

/// GET /api/minecraft/servers
pub async fn list_servers(
    State(mc): State<Arc<MinecraftManager>>,
) -> Result<Json<Vec<MinecraftServer>>, Error> {
    match mc.db().get_minecraft_servers().await {
        Ok(servers) => Ok(Json(servers)),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())),
    }
}

/// POST /api/minecraft/servers/add
pub async fn add_server(
    State(mc): State<Arc<MinecraftManager>>,
    Json(payload): Json<AddServerRequest>,
) -> Result<Json<MinecraftServer>, Error> {
    let name = payload.name.trim();
    let ip = payload.ip.trim();

    if name.is_empty() || ip.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Имя и IP сервера обязательны"));
    }

    let id = payload
        .id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::launchers::minecraft::nbt::sanitize_server_id(ip));

    let server = MinecraftServer {
        id,
        name: name.to_string(),
        ip: ip.to_string(),
        icon: payload.icon,
        is_favorite: payload.is_favorite.unwrap_or(false),
        is_imported: false,
        last_played: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    mc.db().save_minecraft_server(&server).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    })?;

    // Also persist updated servers to .minecraft/servers.dat if directory exists
    if let Some(dir) = mc.scanner().resolve_minecraft_dir() {
        if let Ok(all_servers) = mc.db().get_minecraft_servers().await {
            let bytes = crate::launchers::minecraft::nbt::write_servers_dat(&all_servers);
            let _ = std::fs::write(dir.join("servers.dat"), bytes);
        }
    }

    Ok(Json(server))
}

/// POST /api/minecraft/servers/import
pub async fn import_servers(
    State(mc): State<Arc<MinecraftManager>>,
) -> Result<Json<Value>, Error> {
    if let Some(dir) = mc.scanner().resolve_minecraft_dir() {
        let count = mc.scanner().scan_servers(&dir).await;
        let all = mc.db().get_minecraft_servers().await.unwrap_or_default();
        Ok(Json(json!({
            "status": "success",
            "imported_count": count,
            "total_servers": all.len(),
            "servers": all,
        })))
    } else {
        Err(err(StatusCode::NOT_FOUND, "Папка .minecraft не найдена"))
    }
}

/// DELETE /api/minecraft/servers/:id
pub async fn delete_server(
    State(mc): State<Arc<MinecraftManager>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    mc.db().delete_minecraft_server(&id).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    })?;

    // Also sync deletion to servers.dat
    if let Some(dir) = mc.scanner().resolve_minecraft_dir() {
        if let Ok(all_servers) = mc.db().get_minecraft_servers().await {
            let bytes = crate::launchers::minecraft::nbt::write_servers_dat(&all_servers);
            let _ = std::fs::write(dir.join("servers.dat"), bytes);
        }
    }

    Ok(Json(json!({
        "status": "success",
        "message": format!("Сервер {} удален", id)
    })))
}

/// POST /api/minecraft/launch
pub async fn launch(
    State(mc): State<Arc<MinecraftManager>>,
    Json(payload): Json<LaunchMinecraftRequest>,
) -> Result<Json<LaunchMinecraftResponse>, Error> {
    match mc.launcher().launch(payload).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// POST /api/minecraft/open-launcher
pub async fn open_launcher(
    State(mc): State<Arc<MinecraftManager>>,
    payload: Option<Json<OpenLauncherRequest>>,
) -> Result<Json<LaunchMinecraftResponse>, Error> {
    let (account_id, launcher_client) = match payload {
        Some(Json(p)) => (p.account_id, p.launcher_client),
        None => (None, None),
    };

    let req = LaunchMinecraftRequest {
        account_id,
        profile_id: None,
        mode: Some("official".to_string()),
        launcher_client,
        server_ip: None,
        world_folder: None,
    };
    match mc.launcher().launch(req).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}
