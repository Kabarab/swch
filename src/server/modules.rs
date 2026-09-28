use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::str::FromStr;

use crate::modules::{ModuleId, ModuleInfo, ModuleManager, ScanReport};
use crate::server::handlers::ErrorResponse;

#[derive(Debug, Serialize)]
pub struct ModuleListResponse {
    pub modules: Vec<ModuleInfo>,
    pub total_ram_bytes: u64,
}

#[derive(Debug, Deserialize)]
pub struct ToggleModuleRequest {
    pub active: bool,
}

#[derive(Debug, Serialize)]
pub struct ModuleActionResponse {
    pub status: &'static str,
    pub message: String,
}

/// Handler for GET /api/modules
pub async fn list_modules(
    State(module_manager): State<Arc<ModuleManager>>,
) -> Json<ModuleListResponse> {
    let modules = module_manager.get_status_all().await;
    let total_ram_bytes = modules.iter().map(|m| m.ram_bytes).sum();
    Json(ModuleListResponse {
        modules,
        total_ram_bytes,
    })
}

/// Handler for POST /api/modules/scan
pub async fn scan_modules(
    State(module_manager): State<Arc<ModuleManager>>,
) -> Result<Json<ScanReport>, (StatusCode, Json<ErrorResponse>)> {
    match module_manager.scan_and_sync().await {
        Ok(report) => Ok(Json(report)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to scan system for launchers: {}", e),
            }),
        )),
    }
}

/// Handler for POST /api/modules/:id/install
pub async fn install_module(
    State(module_manager): State<Arc<ModuleManager>>,
    Path(id): Path<String>,
) -> Result<Json<ModuleActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let module_id = ModuleId::from_str(&id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: e }),
        )
    })?;

    match module_manager.install_module(module_id).await {
        Ok(_) => Ok(Json(ModuleActionResponse {
            status: "success",
            message: format!("Модуль {} успешно установлен и активирован", module_id.display_name()),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Не удалось установить модуль: {}", e),
            }),
        )),
    }
}

/// Handler for POST /api/modules/:id/toggle
pub async fn toggle_module(
    State(module_manager): State<Arc<ModuleManager>>,
    Path(id): Path<String>,
    Json(payload): Json<ToggleModuleRequest>,
) -> Result<Json<ModuleActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let module_id = ModuleId::from_str(&id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: e }),
        )
    })?;

    match module_manager.toggle_module(module_id, payload.active).await {
        Ok(_) => Ok(Json(ModuleActionResponse {
            status: "success",
            message: if payload.active {
                format!("Модуль {} активирован", module_id.display_name())
            } else {
                format!("Модуль {} отключен, память освобождена", module_id.display_name())
            },
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Не удалось изменить статус модуля: {}", e),
            }),
        )),
    }
}

/// Handler for POST /api/modules/:id/purge
pub async fn purge_module(
    State(module_manager): State<Arc<ModuleManager>>,
    Path(id): Path<String>,
) -> Result<Json<ModuleActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let module_id = ModuleId::from_str(&id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: e }),
        )
    })?;

    match module_manager.purge_module(module_id).await {
        Ok(_) => Ok(Json(ModuleActionResponse {
            status: "success",
            message: format!("Файлы и кэш модуля {} удалены с диска", module_id.display_name()),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Не удалось очистить модуль: {}", e),
            }),
        )),
    }
}
