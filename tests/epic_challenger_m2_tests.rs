//! Adversarial Empirical Challenger Test Suite for Milestone 2
//!
//! Rigorous stress-testing of:
//! 1. Pagination Loop Robustness (`fetch_all_entitlements`):
//!    - Empty libraries (0 items) -> 1 request termination
//!    - Exact multiples of batch size (1000, 2000, 3000)
//!    - Partial batches (1500, 2500)
//!    - Boundaries (1, 999, 1001)
//!    - Invalid / extreme offsets without overflow or panic
//!    - Mid-flight API errors (500, 401, corrupted body) terminating cleanly
//! 2. Bulk Catalog Responses Wire Formats:
//!    - Map format (`{"id": {...}}`) vs List format (`[{...}]`)
//!    - Empty Map `{}` and Empty List `[]`
//!    - Missing optional fields and null field values
//!    - Extraneous unknown JSON fields
//!    - Unreal Engine asset vs base game classification
//! 3. Manifest Parser Hostile Inputs (`src/parsers/epic_manifest.rs`):
//!    - Empty string, whitespace-only strings
//!    - Truncated JSON
//!    - Missing `AppName` vs empty `AppName: ""` vs whitespace `AppName: "   "`
//!    - UTF-8 BOM prefix (`\u{FEFF}`) with and without leading spaces
//!    - Invalid UTF-8 raw byte stream
//!    - Incomplete install flags (`bIsIncompleteInstall` true, false, omitted)
//!    - Empty `InstallLocation`
//!    - DLC classification variations
//!    - Directory scanner on non-existent paths, files, and mixed chaos folders
//! 4. macOS `.app` Bundle Mach-O Resolution:
//!    - Non-existent `.app` on disk
//!    - Existing `.app` with valid `Contents/Info.plist`
//!    - Existing `.app` with whitespace in Info.plist `<string>`
//!    - Existing `.app` with missing/corrupt Info.plist (fallback to stem / first file)
//!    - Existing `.app` with empty `Contents/MacOS` directory
//!    - Manifest with empty `LaunchExecutable` pointing directly to `.app`
//! 5. Library Aggregator Sync & SQLite Invariants:
//!    - Sync with empty cloud and local manifests
//!    - Sync large library (exact multiple)
//!    - Uninstalled vs installed flags
//!    - Cloud DLC separation from base games
//!    - Multi-account isolation
//!    - Repeated sync idempotency

mod common;

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use axum::extract::{Path as AxPath, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use tempfile::tempdir;

use std::sync::Arc;

use common::mock_server::MockEpicServer;
use common::models::Entitlement as MockEntitlement;
use swch::launchers::epic::models::*;
use swch::launchers::epic::{EpicAuthStore, EpicClient, MemoryCredentialStore};
use swch::parsers::epic_manifest::*;
use swch::storage::Database;

async fn setup_challenger_env() -> (MockEpicServer, Database, EpicAuthStore) {
    let mock = MockEpicServer::start().await;
    let db = Database::open_in_memory().expect("Failed to create in-memory DB");
    db.run_migrations().await.expect("Migrations failed");

    let client = EpicClient::from_router(mock.router());
    let store = EpicAuthStore::new(
        db.clone(),
        Arc::new(MemoryCredentialStore::new()),
        Arc::new(client),
    );
    (mock, db, store)
}

fn create_mock_entitlement(acc: &str, i: usize, is_exec: bool) -> MockEntitlement {
    MockEntitlement {
        id: format!("ent_{}_{}", acc, i),
        entitlement_name: format!("Game_{}_{}", acc, i),
        namespace: "ns_challenger".to_string(),
        catalog_item_id: format!("cat_item_{}_{}", acc, i),
        account_id: acc.to_string(),
        identity_id: None,
        entitlement_type: if is_exec {
            "EXECUTABLE".to_string()
        } else {
            "ENTITLEMENT".to_string()
        },
        grant_date: None,
        consumable: None,
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    }
}

// ============================================================================
// 1. Pagination Loop Robustness (`fetch_all_entitlements`)
// ============================================================================

#[tokio::test]
async fn test_adv_pagination_empty_library() {
    let (mock, _, _) = setup_challenger_env().await;
    let client = EpicClient::from_router(mock.router());

    mock.set_entitlements("acc_empty", vec![]).await;
    mock.clear_request_log().await;

    let res = client
        .fetch_all_entitlements("valid_token", "acc_empty")
        .await
        .expect("Fetch empty entitlements failed");

    assert!(res.is_empty(), "Empty library must return empty vector");
}

#[tokio::test]
async fn test_adv_pagination_single_item() {
    let (mock, _, _) = setup_challenger_env().await;
    let client = EpicClient::from_router(mock.router());

    mock.set_entitlements("acc_1", vec![create_mock_entitlement("acc_1", 0, true)])
        .await;

    let res = client
        .fetch_all_entitlements("valid_token", "acc_1")
        .await
        .expect("Fetch single item failed");

    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "ent_acc_1_0");
}

#[tokio::test]
async fn test_adv_pagination_boundary_999_items() {
    let (mock, _, _) = setup_challenger_env().await;
    let client = EpicClient::from_router(mock.router());

    let items: Vec<MockEntitlement> = (0..999)
        .map(|i| create_mock_entitlement("acc_999", i, true))
        .collect();
    mock.set_entitlements("acc_999", items).await;

    let res = client
        .fetch_all_entitlements("valid_token", "acc_999")
        .await
        .expect("Fetch 999 items failed");

    assert_eq!(res.len(), 999);
}

#[tokio::test]
async fn test_adv_pagination_exact_1000_items() {
    let (mock, _, _) = setup_challenger_env().await;
    let client = EpicClient::from_router(mock.router());

    let items: Vec<MockEntitlement> = (0..1000)
        .map(|i| create_mock_entitlement("acc_1000", i, true))
        .collect();
    mock.set_entitlements("acc_1000", items).await;

    let res = client
        .fetch_all_entitlements("valid_token", "acc_1000")
        .await
        .expect("Fetch 1000 items failed");

    assert_eq!(res.len(), 1000);
    assert_eq!(res[0].id, "ent_acc_1000_0");
    assert_eq!(res[999].id, "ent_acc_1000_999");
}

#[tokio::test]
async fn test_adv_pagination_boundary_1001_items() {
    let (mock, _, _) = setup_challenger_env().await;
    let client = EpicClient::from_router(mock.router());

    let items: Vec<MockEntitlement> = (0..1001)
        .map(|i| create_mock_entitlement("acc_1001", i, true))
        .collect();
    mock.set_entitlements("acc_1001", items).await;

    let res = client
        .fetch_all_entitlements("valid_token", "acc_1001")
        .await
        .expect("Fetch 1001 items failed");

    assert_eq!(res.len(), 1001);
    assert_eq!(res[1000].id, "ent_acc_1001_1000");
}

#[tokio::test]
async fn test_adv_pagination_partial_1500_items() {
    let (mock, _, _) = setup_challenger_env().await;
    let client = EpicClient::from_router(mock.router());

    let items: Vec<MockEntitlement> = (0..1500)
        .map(|i| create_mock_entitlement("acc_1500", i, true))
        .collect();
    mock.set_entitlements("acc_1500", items).await;

    let res = client
        .fetch_all_entitlements("valid_token", "acc_1500")
        .await
        .expect("Fetch 1500 items failed");

    assert_eq!(res.len(), 1500);
}

#[tokio::test]
async fn test_adv_pagination_exact_2000_items() {
    let (mock, _, _) = setup_challenger_env().await;
    let client = EpicClient::from_router(mock.router());

    let items: Vec<MockEntitlement> = (0..2000)
        .map(|i| create_mock_entitlement("acc_2000", i, true))
        .collect();
    mock.set_entitlements("acc_2000", items).await;

    let res = client
        .fetch_all_entitlements("valid_token", "acc_2000")
        .await
        .expect("Fetch 2000 items failed");

    assert_eq!(res.len(), 2000);
    assert_eq!(res[1999].id, "ent_acc_2000_1999");
}

#[tokio::test]
async fn test_adv_pagination_partial_2500_items() {
    let (mock, _, _) = setup_challenger_env().await;
    let client = EpicClient::from_router(mock.router());

    let items: Vec<MockEntitlement> = (0..2500)
        .map(|i| create_mock_entitlement("acc_2500", i, true))
        .collect();
    mock.set_entitlements("acc_2500", items).await;

    let res = client
        .fetch_all_entitlements("valid_token", "acc_2500")
        .await
        .expect("Fetch 2500 items failed");

    assert_eq!(res.len(), 2500);
}

#[tokio::test]
async fn test_adv_pagination_exact_3000_items() {
    let (mock, _, _) = setup_challenger_env().await;
    let client = EpicClient::from_router(mock.router());

    let items: Vec<MockEntitlement> = (0..3000)
        .map(|i| create_mock_entitlement("acc_3000", i, true))
        .collect();
    mock.set_entitlements("acc_3000", items).await;

    let res = client
        .fetch_all_entitlements("valid_token", "acc_3000")
        .await
        .expect("Fetch 3000 items failed");

    assert_eq!(res.len(), 3000);
    assert_eq!(res[2999].id, "ent_acc_3000_2999");
}

#[tokio::test]
async fn test_adv_pagination_extreme_offset() {
    let (mock, _, _) = setup_challenger_env().await;
    let client = EpicClient::from_router(mock.router());

    let items = client
        .fetch_entitlements("valid_token", "acc_any", 1_000_000, 1000)
        .await
        .expect("Huge offset query failed");

    assert!(items.is_empty(), "Offset beyond available items must return empty vector");
}

#[tokio::test]
async fn test_adv_pagination_mid_flight_server_error() {
    // Mock router that returns 1000 items on page 1, but HTTP 500 on page 2
    #[derive(Deserialize)]
    struct QueryParams {
        #[serde(default)]
        start: usize,
    }

    async fn custom_entitlements(
        AxPath(_account_id): AxPath<String>,
        Query(q): Query<QueryParams>,
    ) -> Response {
        if q.start == 0 {
            let items: Vec<Entitlement> = (0..1000)
                .map(|i| {
                    let m = create_mock_entitlement("acc_err", i, true);
                    Entitlement {
                        id: m.id,
                        entitlement_name: m.entitlement_name,
                        namespace: m.namespace,
                        catalog_item_id: m.catalog_item_id,
                        account_id: m.account_id,
                        identity_id: m.identity_id,
                        entitlement_type: m.entitlement_type,
                        grant_date: m.grant_date,
                        consumable: m.consumable,
                        status: m.status,
                        active: m.active,
                        use_count: m.use_count,
                        created: m.created,
                        updated: m.updated,
                    }
                })
                .collect();
            (StatusCode::OK, Json(items)).into_response()
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "errorCode": "errors.com.epicgames.common.server_error",
                    "errorMessage": "Simulated mid-flight crash",
                    "numericErrorCode": 50000
                })),
            )
                .into_response()
        }
    }

    let router = Router::new().route(
        "/entitlement/api/account/:account_id/entitlements",
        get(custom_entitlements),
    );

    let client = EpicClient::from_router(router);
    let res = client.fetch_all_entitlements("valid_token", "acc_err").await;

    assert!(res.is_err(), "Mid-flight error must fail the fetch operation immediately");
    match res.unwrap_err() {
        EpicError::Api { status, message, .. } => {
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert!(message.contains("Simulated mid-flight crash"));
        }
        other => panic!("Expected Api error, got {:?}", other),
    }
}

#[tokio::test]
async fn test_adv_pagination_mid_flight_unauthorized() {
    #[derive(Deserialize)]
    struct QueryParams {
        #[serde(default)]
        start: usize,
    }

    async fn custom_entitlements(
        AxPath(_account_id): AxPath<String>,
        Query(q): Query<QueryParams>,
    ) -> Response {
        if q.start == 0 {
            let items: Vec<Entitlement> = (0..1000)
                .map(|i| {
                    let m = create_mock_entitlement("acc_auth_err", i, true);
                    Entitlement {
                        id: m.id,
                        entitlement_name: m.entitlement_name,
                        namespace: m.namespace,
                        catalog_item_id: m.catalog_item_id,
                        account_id: m.account_id,
                        identity_id: m.identity_id,
                        entitlement_type: m.entitlement_type,
                        grant_date: m.grant_date,
                        consumable: m.consumable,
                        status: m.status,
                        active: m.active,
                        use_count: m.use_count,
                        created: m.created,
                        updated: m.updated,
                    }
                })
                .collect();
            (StatusCode::OK, Json(items)).into_response()
        } else {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "errorCode": "errors.com.epicgames.common.authentication.token_expired",
                    "errorMessage": "Token expired on page 2",
                    "numericErrorCode": 1014
                })),
            )
                .into_response()
        }
    }

    let router = Router::new().route(
        "/entitlement/api/account/:account_id/entitlements",
        get(custom_entitlements),
    );

    let client = EpicClient::from_router(router);
    let res = client.fetch_all_entitlements("valid_token", "acc_auth_err").await;

    assert!(res.is_err());
    assert!(matches!(res.unwrap_err(), EpicError::Api { status: StatusCode::UNAUTHORIZED, .. }));
}

// ============================================================================
// 2. Bulk Catalog Responses Wire Formats & Edge Cases
// ============================================================================

#[tokio::test]
async fn test_adv_catalog_map_wire_format() {
    async fn custom_catalog() -> Response {
        let json = serde_json::json!({
            "cat_1": {
                "id": "cat_1",
                "title": "Cyberpunk 2077",
                "namespace": "cyberpunk_ns"
            },
            "cat_2": {
                "id": "cat_2",
                "title": "Witcher 3",
                "namespace": "witcher_ns"
            }
        });
        (StatusCode::OK, Json(json)).into_response()
    }

    let router = Router::new().route(
        "/catalog/api/shared/namespace/:namespace/bulk/items",
        get(custom_catalog),
    );

    let client = EpicClient::from_router(router);
    let res = client
        .fetch_catalog_items("valid_token", "ns", &["cat_1", "cat_2"])
        .await
        .expect("Fetch map wire format failed");

    assert_eq!(res.len(), 2);
    assert_eq!(res.get("cat_1").unwrap().title, "Cyberpunk 2077");
    assert_eq!(res.get("cat_2").unwrap().title, "Witcher 3");
}

#[tokio::test]
async fn test_adv_catalog_list_wire_format() {
    async fn custom_catalog() -> Response {
        let json = serde_json::json!([
            {
                "id": "cat_list_1",
                "title": "Hades",
                "namespace": "supergiant_ns"
            },
            {
                "id": "cat_list_2",
                "title": "Hades II",
                "namespace": "supergiant_ns"
            }
        ]);
        (StatusCode::OK, Json(json)).into_response()
    }

    let router = Router::new().route(
        "/catalog/api/shared/namespace/:namespace/bulk/items",
        get(custom_catalog),
    );

    let client = EpicClient::from_router(router);
    let res = client
        .fetch_catalog_items("valid_token", "supergiant_ns", &["cat_list_1"])
        .await
        .expect("Fetch list wire format failed");

    assert_eq!(res.len(), 2);
    assert!(res.contains_key("cat_list_1"));
    assert_eq!(res.get("cat_list_1").unwrap().title, "Hades");
    assert!(res.contains_key("cat_list_2"));
}

#[tokio::test]
async fn test_adv_catalog_empty_map_and_empty_list() {
    async fn empty_map_catalog() -> Response {
        (StatusCode::OK, Json(serde_json::json!({}))).into_response()
    }
    let router_map = Router::new().route(
        "/catalog/api/shared/namespace/:namespace/bulk/items",
        get(empty_map_catalog),
    );
    let client_map = EpicClient::from_router(router_map);
    let res_map = client_map
        .fetch_catalog_items("token", "ns", &[])
        .await
        .expect("Empty map catalog failed");
    assert!(res_map.is_empty());

    async fn empty_list_catalog() -> Response {
        (StatusCode::OK, Json(serde_json::json!([]))).into_response()
    }
    let router_list = Router::new().route(
        "/catalog/api/shared/namespace/:namespace/bulk/items",
        get(empty_list_catalog),
    );
    let client_list = EpicClient::from_router(router_list);
    let res_list = client_list
        .fetch_catalog_items("token", "ns", &[])
        .await
        .expect("Empty list catalog failed");
    assert!(res_list.is_empty());
}

#[test]
fn test_adv_catalog_item_missing_optional_fields() {
    let json = r#"{
        "id": "minimal_item",
        "title": "Minimal Game",
        "namespace": "minimal_ns"
    }"#;

    let item: CatalogItem = serde_json::from_str(json).expect("Minimal catalog item failed to parse");
    assert_eq!(item.id, "minimal_item");
    assert_eq!(item.title, "Minimal Game");
    assert!(item.description.is_none());
    assert!(item.key_images.is_none());
    assert!(item.categories.is_none());
    assert!(item.custom_attributes.is_none());
    assert!(item.main_game_item.is_none());

    // Helper safety checks
    assert!(item.is_base_game());
    assert!(!item.is_dlc());
    assert!(!item.is_engine_asset());
    assert!(item.tall_box_art().is_none());
    assert!(item.landscape_box_art().is_none());
    assert!(item.get_custom_attribute("CanRunOffline").is_none());
}

#[test]
fn test_adv_catalog_item_null_and_extra_fields() {
    let json = r#"{
        "id": "extra_item",
        "title": "Extra Game",
        "namespace": "extra_ns",
        "description": null,
        "keyImages": null,
        "categories": null,
        "customAttributes": null,
        "mainGameItem": null,
        "unknown_super_field_123": { "nested": [1, 2, 3] },
        "futureEpicLauncherField": "version99"
    }"#;

    let item: CatalogItem = serde_json::from_str(json).expect("Catalog item with nulls/extra fields failed");
    assert_eq!(item.id, "extra_item");
    assert!(item.is_base_game());
}

#[test]
fn test_adv_catalog_engine_asset_classification() {
    // 1. Namespace 'ue'
    let ue_json = r#"{
        "id": "ue_plugin",
        "title": "Unreal Engine Bridge",
        "namespace": "ue"
    }"#;
    let ue_item: CatalogItem = serde_json::from_str(ue_json).unwrap();
    assert!(ue_item.is_engine_asset());

    // 2. Category path 'assets'
    let asset_json = r#"{
        "id": "mesh_pack",
        "title": "SciFi Meshes",
        "namespace": "fn",
        "categories": [
            { "path": "games" },
            { "path": "assets" }
        ]
    }"#;
    let asset_item: CatalogItem = serde_json::from_str(asset_json).unwrap();
    assert!(asset_item.is_engine_asset());

    // 3. Normal game
    let game_json = r#"{
        "id": "normal_game",
        "title": "Space Shooter",
        "namespace": "space_studios",
        "categories": [
            { "path": "games" }
        ]
    }"#;
    let game_item: CatalogItem = serde_json::from_str(game_json).unwrap();
    assert!(!game_item.is_engine_asset());
}

// ============================================================================
// 3. Manifest Parser Hostile Inputs (`src/parsers/epic_manifest.rs`)
// ============================================================================

#[test]
fn test_adv_manifest_empty_and_whitespace_inputs() {
    assert!(matches!(parse_manifest_from_str(""), Err(ManifestError::Invalid(_))));
    assert!(matches!(parse_manifest_from_str("     "), Err(ManifestError::Invalid(_))));
    assert!(matches!(parse_manifest_from_str("\n\t\r\n  \t"), Err(ManifestError::Invalid(_))));
}

#[test]
fn test_adv_manifest_truncated_json() {
    assert!(matches!(
        parse_manifest_from_str(r#"{"AppName": "Game", "CatalogItemId": "#),
        Err(ManifestError::Json(_))
    ));
    assert!(matches!(
        parse_manifest_from_str(r#"{"AppName":"#),
        Err(ManifestError::Json(_))
    ));
    assert!(matches!(
        parse_manifest_from_str("{"),
        Err(ManifestError::Json(_))
    ));
}

#[test]
fn test_adv_manifest_missing_and_empty_app_name() {
    // Missing AppName completely
    let missing_app_name = r#"{
        "CatalogItemId": "cat123",
        "CatalogNamespace": "ns123",
        "DisplayName": "Game Without AppName",
        "InstallationGuid": "guid123",
        "InstallLocation": "/Games/Game"
    }"#;
    assert!(matches!(parse_manifest_from_str(missing_app_name), Err(ManifestError::Json(_))));

    // AppName is empty string
    let empty_app_name = r#"{
        "AppName": "",
        "CatalogItemId": "cat123",
        "CatalogNamespace": "ns123",
        "DisplayName": "Game With Empty AppName",
        "InstallationGuid": "guid123",
        "InstallLocation": "/Games/Game"
    }"#;
    assert!(matches!(parse_manifest_from_str(empty_app_name), Err(ManifestError::Invalid(_))));

    // AppName is whitespace only
    let whitespace_app_name = r#"{
        "AppName": "    \t\n  ",
        "CatalogItemId": "cat123",
        "CatalogNamespace": "ns123",
        "DisplayName": "Game With Whitespace AppName",
        "InstallationGuid": "guid123",
        "InstallLocation": "/Games/Game"
    }"#;
    assert!(matches!(parse_manifest_from_str(whitespace_app_name), Err(ManifestError::Invalid(_))));
}

#[test]
fn test_adv_manifest_utf8_bom_prefix() {
    let raw = "\u{FEFF}{\"AppName\":\"BOMGame\",\"CatalogItemId\":\"c1\",\"CatalogNamespace\":\"ns\",\"DisplayName\":\"BOM Game\",\"InstallationGuid\":\"g1\",\"InstallLocation\":\"/Games/BOM\"}";
    let item = parse_manifest_from_str(raw).expect("Failed to parse BOM prefixed manifest");
    assert_eq!(item.app_name, "BOMGame");
    assert_eq!(item.display_name, "BOM Game");

    let raw_spaces = "  \u{FEFF}  {\"AppName\":\"BOMSpaces\",\"CatalogItemId\":\"c2\",\"CatalogNamespace\":\"ns\",\"DisplayName\":\"BOM Spaces\",\"InstallationGuid\":\"g2\",\"InstallLocation\":\"/Games/BOM\"}";
    let item2 = parse_manifest_from_str(raw_spaces).expect("Failed to parse BOM with leading spaces");
    assert_eq!(item2.app_name, "BOMSpaces");
}

#[test]
fn test_adv_manifest_invalid_utf8_file() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("invalid_utf8.item");
    let mut f = File::create(&file_path).unwrap();
    f.write_all(&[0xFF, 0xFE, 0xFD, 0x80, 0x81]).unwrap();

    let res = parse_manifest_file(&file_path);
    assert!(matches!(res, Err(ManifestError::Io(_))));
}

#[test]
fn test_adv_manifest_incomplete_install_and_empty_location() {
    // bIsIncompleteInstall = true
    let item_incomplete = EpicManifestItem {
        format_version: None,
        app_name: "IncompleteApp".to_string(),
        catalog_item_id: "c_inc".to_string(),
        catalog_namespace: "ns_inc".to_string(),
        display_name: "Incomplete".to_string(),
        installation_guid: "g_inc".to_string(),
        install_location: "/Games/Incomplete".to_string(),
        install_size: 100,
        launch_executable: "game.exe".to_string(),
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: Some(true),
        can_run_offline: None,
        requires_auth: None,
        app_version_string: None,
    };
    assert!(!item_incomplete.is_valid_installation());

    // Empty install_location
    let item_no_location = EpicManifestItem {
        install_location: "   ".to_string(),
        is_incomplete_install: Some(false),
        ..item_incomplete
    };
    assert!(!item_no_location.is_valid_installation());
}

#[test]
fn test_adv_manifest_dlc_classification_matrix() {
    let mut item = EpicManifestItem {
        format_version: None,
        app_name: "BaseGame".to_string(),
        catalog_item_id: "c_base".to_string(),
        catalog_namespace: "ns".to_string(),
        display_name: "Base Game".to_string(),
        installation_guid: "g".to_string(),
        install_location: "/loc".to_string(),
        install_size: 100,
        launch_executable: "".to_string(),
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: None,
        can_run_offline: None,
        requires_auth: None,
        app_version_string: None,
    };

    // None -> base game
    assert!(!item.is_dlc());

    // Empty string -> base game
    item.main_game_app_name = Some("".to_string());
    assert!(!item.is_dlc());

    // Same as AppName -> base game
    item.main_game_app_name = Some("BaseGame".to_string());
    assert!(!item.is_dlc());

    // Different from AppName -> DLC
    item.main_game_app_name = Some("ParentGame".to_string());
    assert!(item.is_dlc());
}

#[test]
fn test_adv_manifest_scan_directory_chaos() {
    let tmp = tempdir().unwrap();

    // 1. Valid .item file
    let valid_json = r#"{
        "AppName": "ValidGame",
        "CatalogItemId": "val_1",
        "CatalogNamespace": "ns",
        "DisplayName": "Valid Game",
        "InstallationGuid": "guid",
        "InstallLocation": "/Games/ValidGame"
    }"#;
    fs::write(tmp.path().join("game1.item"), valid_json).unwrap();

    // 2. Corrupt .item file
    fs::write(tmp.path().join("broken.item"), b"{{corrupted json").unwrap();

    // 3. 0-byte .item file
    fs::write(tmp.path().join("empty.item"), b"").unwrap();

    // 4. Missing AppName .item file
    let missing_name_json = r#"{
        "CatalogItemId": "val_2",
        "CatalogNamespace": "ns",
        "DisplayName": "No App Name",
        "InstallationGuid": "guid",
        "InstallLocation": "/Games/NoName"
    }"#;
    fs::write(tmp.path().join("missing_name.item"), missing_name_json).unwrap();

    // 5. Non-.item file
    fs::write(tmp.path().join("readme.txt"), b"documentation").unwrap();

    // 6. Subdirectory ending in .item
    fs::create_dir(tmp.path().join("fake_folder.item")).unwrap();

    // Scan must succeed, ignore the noise and return only the 1 valid game
    let items = scan_manifest_directory(tmp.path()).expect("Scan chaos directory failed");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].app_name, "ValidGame");

    // Non-existent directory returns empty
    let non_existent = scan_manifest_directory(tmp.path().join("does_not_exist")).unwrap();
    assert!(non_existent.is_empty());

    // File passed as directory returns empty
    let file_as_dir = scan_manifest_directory(tmp.path().join("game1.item")).unwrap();
    assert!(file_as_dir.is_empty());
}

// ============================================================================
// 4. macOS `.app` Bundle Mach-O Resolution Edge Cases
// ============================================================================

#[test]
fn test_adv_macos_bundle_nonexistent_on_disk() {
    let manifest = EpicManifestItem {
        format_version: None,
        app_name: "MacGame".to_string(),
        catalog_item_id: "c_mac".to_string(),
        catalog_namespace: "ns".to_string(),
        display_name: "Mac Game".to_string(),
        installation_guid: "guid".to_string(),
        install_location: "/Applications".to_string(),
        install_size: 100,
        launch_executable: "ImaginaryGame.app".to_string(),
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: None,
        can_run_offline: None,
        requires_auth: None,
        app_version_string: None,
    };

    let resolved = manifest.resolve_executable_path();
    assert_eq!(resolved, PathBuf::from("/Applications/ImaginaryGame.app"));
}

#[test]
fn test_adv_macos_bundle_info_plist_resolution() {
    let tmp = tempdir().unwrap();
    let app_dir = tmp.path().join("Civ6.app");
    let macos_dir = app_dir.join("Contents/MacOS");
    fs::create_dir_all(&macos_dir).unwrap();

    let binary = macos_dir.join("Civ6_Binary");
    fs::write(&binary, b"mach-o binary header").unwrap();

    let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Civilization VI</string>
    <key>CFBundleExecutable</key>
    <string>  Civ6_Binary  </string>
</dict>
</plist>"#;
    fs::write(app_dir.join("Contents/Info.plist"), plist).unwrap();

    let manifest = EpicManifestItem {
        format_version: None,
        app_name: "Civ6".to_string(),
        catalog_item_id: "c_civ6".to_string(),
        catalog_namespace: "ns".to_string(),
        display_name: "Civilization VI".to_string(),
        installation_guid: "guid".to_string(),
        install_location: tmp.path().to_string_lossy().to_string(),
        install_size: 1000,
        launch_executable: "Civ6.app".to_string(),
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: None,
        can_run_offline: None,
        requires_auth: None,
        app_version_string: None,
    };

    let resolved = manifest.resolve_executable_path();
    assert_eq!(resolved, binary);
}

#[test]
fn test_adv_macos_bundle_missing_info_plist_stem_fallback() {
    let tmp = tempdir().unwrap();
    let app_dir = tmp.path().join("FallGuys.app");
    let macos_dir = app_dir.join("Contents/MacOS");
    fs::create_dir_all(&macos_dir).unwrap();

    // No Info.plist, but executable matches bundle stem "FallGuys"
    let binary = macos_dir.join("FallGuys");
    fs::write(&binary, b"mach-o content").unwrap();

    let manifest = EpicManifestItem {
        format_version: None,
        app_name: "FallGuys".to_string(),
        catalog_item_id: "c_fg".to_string(),
        catalog_namespace: "ns".to_string(),
        display_name: "Fall Guys".to_string(),
        installation_guid: "guid".to_string(),
        install_location: tmp.path().to_string_lossy().to_string(),
        install_size: 1000,
        launch_executable: "FallGuys.app".to_string(),
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: None,
        can_run_offline: None,
        requires_auth: None,
        app_version_string: None,
    };

    let resolved = manifest.resolve_executable_path();
    assert_eq!(resolved, binary);
}

#[test]
fn test_adv_macos_bundle_corrupt_info_plist_first_file_fallback() {
    let tmp = tempdir().unwrap();
    let app_dir = tmp.path().join("CustomGame.app");
    let macos_dir = app_dir.join("Contents/MacOS");
    fs::create_dir_all(&macos_dir).unwrap();

    // Corrupted Info.plist
    fs::write(app_dir.join("Contents/Info.plist"), b"<dict>corrupt xml...").unwrap();

    // Non-stem binary name
    let binary = macos_dir.join("launcher_entrypoint");
    fs::write(&binary, b"mach-o content").unwrap();

    let manifest = EpicManifestItem {
        format_version: None,
        app_name: "CustomGame".to_string(),
        catalog_item_id: "c_custom".to_string(),
        catalog_namespace: "ns".to_string(),
        display_name: "Custom Game".to_string(),
        installation_guid: "guid".to_string(),
        install_location: tmp.path().to_string_lossy().to_string(),
        install_size: 1000,
        launch_executable: "CustomGame.app".to_string(),
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: None,
        can_run_offline: None,
        requires_auth: None,
        app_version_string: None,
    };

    let resolved = manifest.resolve_executable_path();
    assert_eq!(resolved, binary);
}

#[test]
fn test_adv_macos_bundle_empty_launch_executable_resolution() {
    let tmp = tempdir().unwrap();
    let app_dir = tmp.path().join("GameDirect.app");
    let macos_dir = app_dir.join("Contents/MacOS");
    fs::create_dir_all(&macos_dir).unwrap();

    let binary = macos_dir.join("GameDirect");
    fs::write(&binary, b"direct bin").unwrap();

    let manifest = EpicManifestItem {
        format_version: None,
        app_name: "GameDirect".to_string(),
        catalog_item_id: "c_direct".to_string(),
        catalog_namespace: "ns".to_string(),
        display_name: "Game Direct".to_string(),
        installation_guid: "guid".to_string(),
        install_location: app_dir.to_string_lossy().to_string(),
        install_size: 1000,
        launch_executable: "".to_string(), // Empty executable string
        launch_command: None,
        main_game_app_name: None,
        mandatory_app_folder_name: None,
        is_incomplete_install: None,
        can_run_offline: None,
        requires_auth: None,
        app_version_string: None,
    };

    let resolved = manifest.resolve_executable_path();
    assert_eq!(resolved, binary);
}

// ============================================================================
// 5. Library Aggregator Sync & SQLite Invariants
// ============================================================================

#[tokio::test]
async fn test_adv_sync_empty_cloud_and_empty_manifests() {
    let (mock, db, store) = setup_challenger_env().await;
    let acc_id = "acc_sync_empty";
    let code = "0123456789abcdef0123456789abcdef";
    mock.register_auth_code(code, acc_id, "Empty Sync")
        .await;
    mock.set_entitlements(acc_id, vec![]).await;

    store
        .login_with_code(code)
        .await
        .unwrap();

    let tmp = tempdir().unwrap();
    let summary = store.sync_library(acc_id, Some(tmp.path())).await.unwrap();

    assert_eq!(summary.total_entitlements, 0);
    assert_eq!(summary.base_games_count, 0);
    assert_eq!(summary.dlc_count, 0);
    assert_eq!(summary.installed_count, 0);

    let games = db.get_games().await.unwrap();
    assert!(games.is_empty());
}

#[tokio::test]
async fn test_adv_sync_large_library_exact_multiple_batch() {
    let (mock, db, store) = setup_challenger_env().await;
    let acc_id = "acc_sync_2000";
    let code = "11112222333344445555666677778888";
    mock.register_auth_code(code, acc_id, "Large Sync")
        .await;

    // 2000 base games
    let entitlements: Vec<MockEntitlement> = (0..2000)
        .map(|i| create_mock_entitlement(acc_id, i, true))
        .collect();
    mock.set_entitlements(acc_id, entitlements).await;

    store
        .login_with_code(code)
        .await
        .unwrap();

    let tmp = tempdir().unwrap();
    let summary = store.sync_library(acc_id, Some(tmp.path())).await.unwrap();

    assert_eq!(summary.total_entitlements, 2000);
    assert_eq!(summary.base_games_count, 2000);
    assert_eq!(summary.dlc_count, 0);
    assert_eq!(summary.installed_count, 0);

    let games = db.get_games().await.unwrap();
    assert_eq!(games.len(), 2000);
}

#[tokio::test]
async fn test_adv_sync_cloud_dlc_separation() {
    let (mock, db, store) = setup_challenger_env().await;
    let acc_id = "acc_sync_dlc";
    let code = "aaaabbbbccccddddeeeeffff00001111";
    mock.register_auth_code(code, acc_id, "DLC Player")
        .await;

    let mut list = Vec::new();
    // 5 Base games
    for i in 0..5 {
        list.push(create_mock_entitlement(acc_id, i, true));
    }
    // 10 DLC items
    for i in 5..15 {
        list.push(create_mock_entitlement(acc_id, i, false));
    }
    mock.set_entitlements(acc_id, list).await;

    store
        .login_with_code(code)
        .await
        .unwrap();

    let tmp = tempdir().unwrap();
    let summary = store.sync_library(acc_id, Some(tmp.path())).await.unwrap();

    assert_eq!(summary.total_entitlements, 15);
    assert_eq!(summary.base_games_count, 5);
    assert_eq!(summary.dlc_count, 10);

    let games = db.get_games().await.unwrap();
    assert_eq!(games.len(), 5, "Only base games should be persisted to games table");
}

#[tokio::test]
async fn test_adv_sync_repeated_idempotency() {
    let (mock, db, store) = setup_challenger_env().await;
    let acc_id = "acc_idempotent";
    let code = "fedcba9876543210fedcba9876543210";
    mock.register_auth_code(code, acc_id, "Idempotent Player")
        .await;

    let items = vec![
        create_mock_entitlement(acc_id, 0, true),
        create_mock_entitlement(acc_id, 1, true),
        create_mock_entitlement(acc_id, 2, false),
    ];
    mock.set_entitlements(acc_id, items).await;

    store
        .login_with_code(code)
        .await
        .unwrap();

    let tmp = tempdir().unwrap();

    // Sync 1
    let s1 = store.sync_library(acc_id, Some(tmp.path())).await.unwrap();
    // Sync 2
    let s2 = store.sync_library(acc_id, Some(tmp.path())).await.unwrap();
    // Sync 3
    let s3 = store.sync_library(acc_id, Some(tmp.path())).await.unwrap();

    assert_eq!(s1, s2);
    assert_eq!(s2, s3);

    let games = db.get_games().await.unwrap();
    assert_eq!(games.len(), 2, "Repeated sync must not create duplicate game records");
}

#[tokio::test]
async fn test_adv_sync_multi_account_isolation() {
    let (mock, db, store) = setup_challenger_env().await;

    // Account 1
    let acc1 = "acc_user_1";
    let code1 = "12341234123412341234123412341234";
    mock.register_auth_code(code1, acc1, "User One").await;

    let shared_cat_id = "shared_game_cat_102";
    let mut shared_game_acc1 = create_mock_entitlement(acc1, 102, true);
    shared_game_acc1.catalog_item_id = shared_cat_id.to_string();

    let acc1_items = vec![
        create_mock_entitlement(acc1, 101, true),
        shared_game_acc1,
    ];
    mock.set_entitlements(acc1, acc1_items).await;

    // Account 2
    let acc2 = "acc_user_2";
    let code2 = "56785678567856785678567856785678";
    mock.register_auth_code(code2, acc2, "User Two").await;

    let mut shared_game_acc2 = create_mock_entitlement(acc2, 102, true);
    shared_game_acc2.catalog_item_id = shared_cat_id.to_string();

    let acc2_items = vec![
        shared_game_acc2,
        create_mock_entitlement(acc2, 103, true),
    ];
    mock.set_entitlements(acc2, acc2_items).await;

    store.login_with_code(code1).await.unwrap();
    store.login_with_code(code2).await.unwrap();

    let tmp = tempdir().unwrap();
    store.sync_library(acc1, Some(tmp.path())).await.unwrap();
    store.sync_library(acc2, Some(tmp.path())).await.unwrap();

    let all_games = db.get_games().await.unwrap();
    assert_eq!(all_games.len(), 3, "Total distinct games must be 3");

    let links = db.get_game_accounts().await.unwrap();
    // 2 links for acc1, 2 links for acc2 = 4 total links
    assert_eq!(links.len(), 4, "Total game-account associations must be 4");

    let acc1_games: Vec<_> = links
        .iter()
        .filter(|l| l.account_id == acc1)
        .map(|l| &l.game_id)
        .collect();
    assert_eq!(acc1_games.len(), 2);
    assert!(acc1_games.contains(&&format!("epic_cat_item_{}_101", acc1)));
    assert!(acc1_games.contains(&&format!("epic_{}", shared_cat_id)));

    let acc2_games: Vec<_> = links
        .iter()
        .filter(|l| l.account_id == acc2)
        .map(|l| &l.game_id)
        .collect();
    assert_eq!(acc2_games.len(), 2);
    assert!(acc2_games.contains(&&format!("epic_{}", shared_cat_id)));
    assert!(acc2_games.contains(&&format!("epic_cat_item_{}_103", acc2)));
}
