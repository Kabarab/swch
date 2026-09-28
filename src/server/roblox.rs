use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::launchers::roblox::RobloxManager;
use crate::storage::{RobloxAccountInfo, RobloxPlace};

type Error = (StatusCode, Json<Value>);

fn err(status: StatusCode, message: &str) -> Error {
    (status, Json(json!({ "error": message })))
}

#[derive(Debug, Deserialize)]
pub struct AddAccountRequest {
    pub cookie: String,
}

#[derive(Debug, Deserialize)]
pub struct AccountActionRequest {
    pub account_id: Option<String>,
    pub account_name: Option<String>,
}

impl AccountActionRequest {
    pub fn target_id(&self) -> Option<&str> {
        self.account_id
            .as_deref()
            .or(self.account_name.as_deref())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    }
}

#[derive(Debug, Deserialize)]
pub struct LaunchRobloxRequest {
    pub account_id: Option<String>,
    pub place_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddPlaceRequest {
    pub place_id: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InventoryQuery {
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SettingRequest {
    pub key: String,
    pub value: String,
}

/// GET /api/roblox/library
pub async fn library(
    State(roblox): State<Arc<RobloxManager>>,
) -> Json<crate::launchers::roblox::RobloxLibraryResponse> {
    Json(roblox.get_library_summary().await)
}

/// GET /api/roblox/accounts
pub async fn list_accounts(
    State(roblox): State<Arc<RobloxManager>>,
) -> Result<Json<Vec<RobloxAccountInfo>>, Error> {
    match roblox.db().get_roblox_accounts().await {
        Ok(accounts) => Ok(Json(accounts)),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())),
    }
}

/// POST /api/roblox/accounts/scan
pub async fn scan_accounts(
    State(roblox): State<Arc<RobloxManager>>,
) -> Result<Json<Value>, Error> {
    let imported = roblox.scanner().scan_and_import().await;
    let all = roblox.db().get_roblox_accounts().await.unwrap_or_default();
    Ok(Json(json!({
        "status": "success",
        "newly_discovered": imported.len(),
        "total_accounts": all.len(),
        "accounts": all,
    })))
}

/// POST /api/roblox/accounts/add
pub async fn add_account(
    State(roblox): State<Arc<RobloxManager>>,
    Json(payload): Json<AddAccountRequest>,
) -> Result<Json<RobloxAccountInfo>, Error> {
    let cookie = payload.cookie.trim();
    if cookie.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Cookie cannot be empty"));
    }

    let client = roblox.client();
    let user = client.get_authenticated_user(cookie).await.map_err(|e| {
        err(StatusCode::UNAUTHORIZED, &format!("Не удалось авторизоваться в Roblox: {}", e))
    })?;

    let user_id = user.id;
    let user_id_str = user_id.to_string();
    let account_id = format!("roblox_{}", user_id_str);

    let robux = client.get_user_currency(user_id, cookie).await.unwrap_or(0);
    let is_premium = client.get_user_premium(user_id, cookie).await.unwrap_or(false);
    let (headshot, avatar) = client.get_avatar_thumbnails(user_id).await;
    let (_, rap) = client.get_collectibles(user_id, cookie).await;

    // Sync favorite places
    let favorites = client.get_favorite_places(user_id, cookie).await;
    for fav in favorites {
        let place_id_str = fav.id.to_string();
        let icon = client.get_place_icon(fav.id).await;
        let place = RobloxPlace {
            place_id: place_id_str,
            name: fav.name,
            description: fav.description,
            icon_url: icon,
            creator_name: None,
            is_manual: false,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = roblox.db().save_roblox_place(&place).await;
    }

    let current = roblox.db().get_roblox_accounts().await.unwrap_or_default();
    let is_active = current.is_empty();

    let info = RobloxAccountInfo {
        account_id: account_id.clone(),
        username: user.name,
        display_name: user.display_name,
        user_id: user_id_str,
        cookie: cookie.to_string(),
        is_active,
        is_premium,
        robux_balance: robux,
        avatar_url: avatar,
        headshot_url: headshot,
        rap_value: rap,
        is_banned: false,
        last_validated: chrono::Utc::now().timestamp(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    roblox.db().save_roblox_account(&info).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed to save account: {}", e))
    })?;

    Ok(Json(info))
}

/// POST /api/roblox/accounts/remove
pub async fn remove_account(
    State(roblox): State<Arc<RobloxManager>>,
    Json(payload): Json<AccountActionRequest>,
) -> Result<Json<Value>, Error> {
    let target = payload.target_id().ok_or_else(|| {
        err(StatusCode::BAD_REQUEST, "account_id or account_name is required")
    })?;

    let acc = roblox.db().get_roblox_account(target).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    })?.ok_or_else(|| {
        err(StatusCode::NOT_FOUND, &format!("Account '{}' not found", target))
    })?;

    roblox.db().delete_roblox_account(&acc.account_id).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    })?;

    Ok(Json(json!({
        "status": "success",
        "message": format!("Аккаунт @{} успешно удален", acc.username)
    })))
}

/// POST /api/roblox/switch-account
pub async fn switch_account(
    State(roblox): State<Arc<RobloxManager>>,
    Json(payload): Json<AccountActionRequest>,
) -> Result<Json<Value>, Error> {
    let target = payload.target_id().ok_or_else(|| {
        err(StatusCode::BAD_REQUEST, "account_id or account_name is required")
    })?;

    let acc = roblox.db().get_roblox_account(target).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    })?.ok_or_else(|| {
        err(StatusCode::NOT_FOUND, &format!("Account '{}' not found", target))
    })?;

    roblox.db().set_active_roblox_account(&acc.account_id).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    })?;

    Ok(Json(json!({
        "status": "success",
        "account_id": acc.account_id,
        "username": acc.username,
        "display_name": acc.display_name,
        "message": format!("Активный профиль переключен на @{} ({})", acc.username, acc.display_name)
    })))
}

/// POST /api/roblox/launch
pub async fn launch(
    State(roblox): State<Arc<RobloxManager>>,
    Json(payload): Json<LaunchRobloxRequest>,
) -> Result<Json<Value>, Error> {
    // 1. Resolve target account: explicit ID or currently active account
    let target_account = if let Some(ref aid) = payload.account_id {
        roblox.db().get_roblox_account(aid).await.map_err(|e| {
            err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        })?
    } else {
        let accounts = roblox.db().get_roblox_accounts().await.map_err(|e| {
            err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        })?;
        accounts.into_iter().find(|a| a.is_active)
    };

    let account = target_account.ok_or_else(|| {
        err(StatusCode::BAD_REQUEST, "Нет активного аккаунта Roblox. Добавьте или выберите аккаунт.")
    })?;

    // 2. Prepare multi-instance / single-instance
    roblox.multi_instance().prepare_for_launch().await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e)
    })?;

    let multi = roblox.multi_instance().is_multi_instance_enabled().await;

    // 3. Launch place or app
    let msg = if let Some(ref pid) = payload.place_id {
        roblox.launcher().launch_place(&account, pid, multi).await.map_err(|e| {
            err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Ошибка запуска плейса: {}", e))
        })?
    } else {
        roblox.launcher().launch_app(&account, multi).await.map_err(|e| {
            err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Ошибка запуска клиента: {}", e))
        })?
    };

    Ok(Json(json!({
        "status": "success",
        "message": msg,
        "account_id": account.account_id,
        "username": account.username,
        "place_id": payload.place_id,
        "multi_instance": multi,
    })))
}

/// GET /api/roblox/places
pub async fn list_places(
    State(roblox): State<Arc<RobloxManager>>,
) -> Result<Json<Vec<RobloxPlace>>, Error> {
    match roblox.db().get_roblox_places().await {
        Ok(places) => Ok(Json(places)),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())),
    }
}

/// POST /api/roblox/places/add
pub async fn add_place(
    State(roblox): State<Arc<RobloxManager>>,
    Json(payload): Json<AddPlaceRequest>,
) -> Result<Json<RobloxPlace>, Error> {
    // Parse Place ID from digits or URL like https://www.roblox.com/games/12345/name
    let clean_id = payload
        .place_id
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>();

    if clean_id.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Invalid Place ID"));
    }

    let pid: i64 = clean_id.parse().map_err(|_| {
        err(StatusCode::BAD_REQUEST, "Place ID must be a numeric string")
    })?;

    let client = roblox.client();
    let detail = client.get_place_details(pid).await.ok();
    let icon = client.get_place_icon(pid).await;

    let name = payload.name.or_else(|| detail.as_ref().map(|d| d.name.clone()))
        .unwrap_or_else(|| format!("Place #{}", clean_id));
    let description = detail.as_ref().and_then(|d| d.description.clone());
    let creator_name = detail.as_ref().and_then(|d| d.builder.clone());

    let place = RobloxPlace {
        place_id: clean_id,
        name,
        description,
        icon_url: icon,
        creator_name,
        is_manual: true,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    roblox.db().save_roblox_place(&place).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed to save place: {}", e))
    })?;

    Ok(Json(place))
}

/// DELETE /api/roblox/places/:id
pub async fn delete_place(
    State(roblox): State<Arc<RobloxManager>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    roblox.db().delete_roblox_place(&id).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    })?;

    Ok(Json(json!({
        "status": "success",
        "message": format!("Плейс {} удален из библиотеки", id)
    })))
}

/// GET /api/roblox/inventory
pub async fn get_inventory(
    State(roblox): State<Arc<RobloxManager>>,
    Query(query): Query<InventoryQuery>,
) -> Result<Json<Value>, Error> {
    let target_account = if let Some(ref aid) = query.account_id {
        roblox.db().get_roblox_account(aid).await.map_err(|e| {
            err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        })?
    } else {
        let accounts = roblox.db().get_roblox_accounts().await.map_err(|e| {
            err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        })?;
        accounts.into_iter().find(|a| a.is_active)
    };

    let account = target_account.ok_or_else(|| {
        err(StatusCode::BAD_REQUEST, "Account not found")
    })?;

    let cache_key = format!("inv_{}", account.user_id);
    if let Ok(Some((payload, updated_at))) = roblox.db().roblox_cache_get(&cache_key).await {
        if let Ok(mut val) = serde_json::from_str::<Value>(&payload) {
            if let Some(obj) = val.as_object_mut() {
                obj.insert("cached".into(), json!(true));
                obj.insert("cached_at".into(), json!(updated_at));
            }
            return Ok(Json(val));
        }
    }

    // Not cached: refresh on demand
    refresh_inventory_internal(&roblox, &account).await
}

/// POST /api/roblox/inventory/refresh
pub async fn refresh_inventory(
    State(roblox): State<Arc<RobloxManager>>,
    Json(payload): Json<AccountActionRequest>,
) -> Result<Json<Value>, Error> {
    let target = payload.target_id().ok_or_else(|| {
        err(StatusCode::BAD_REQUEST, "account_id is required")
    })?;

    let account = roblox.db().get_roblox_account(target).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    })?.ok_or_else(|| {
        err(StatusCode::NOT_FOUND, &format!("Account '{}' not found", target))
    })?;

    refresh_inventory_internal(&roblox, &account).await
}

async fn refresh_inventory_internal(
    roblox: &RobloxManager,
    account: &RobloxAccountInfo,
) -> Result<Json<Value>, Error> {
    let uid: i64 = account.user_id.parse().unwrap_or(0);
    let client = roblox.client();

    let wearing = client.get_currently_wearing(uid).await;
    let (collectibles, total_rap) = client.get_collectibles(uid, &account.cookie).await;

    // Update RAP on account record
    let mut updated_acc = account.clone();
    updated_acc.rap_value = total_rap;
    updated_acc.updated_at = chrono::Utc::now().to_rfc3339();
    let _ = roblox.db().save_roblox_account(&updated_acc).await;

    let result = json!({
        "account_id": account.account_id,
        "user_id": account.user_id,
        "username": account.username,
        "total_rap": total_rap,
        "wearing_asset_ids": wearing,
        "collectibles": collectibles,
        "cached": false,
        "cached_at": chrono::Utc::now().timestamp(),
    });

    let cache_key = format!("inv_{}", account.user_id);
    let _ = roblox.db().roblox_cache_put(&cache_key, &result.to_string()).await;

    Ok(Json(result))
}

/// GET /api/roblox/settings
pub async fn get_settings(
    State(roblox): State<Arc<RobloxManager>>,
) -> Result<Json<Value>, Error> {
    let multi = roblox.multi_instance().is_multi_instance_enabled().await;
    Ok(Json(json!({
        "multi_instance_enabled": multi,
    })))
}

/// POST /api/roblox/settings
pub async fn update_settings(
    State(roblox): State<Arc<RobloxManager>>,
    Json(payload): Json<SettingRequest>,
) -> Result<Json<Value>, Error> {
    roblox.db().set_roblox_setting(&payload.key, &payload.value).await.map_err(|e| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    })?;

    Ok(Json(json!({
        "status": "success",
        "key": payload.key,
        "value": payload.value,
    })))
}
