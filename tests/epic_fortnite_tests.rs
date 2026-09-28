use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

use swch::launchers::epic::{
    keyring::{CredentialStore, MemoryCredentialStore},
    models::{
        resolve_cosmetic_metadata, DeviceAuth, EpicAccountDetails, FortniteAccountSummary,
        FortniteCosmeticType,
    },
    EpicAuthStore, EpicClient,
};
use swch::server::{create_app_with_state, AppState};
use swch::storage::{Account, Database};

#[test]
fn test_resolve_cosmetic_metadata_known() {
    let (name, cosmetic_type, rarity, is_exclusive) =
        resolve_cosmetic_metadata("AthenaCharacter:cid_028_athena_commando_f");
    assert_eq!(name, "Renegade Raider");
    assert_eq!(cosmetic_type, FortniteCosmeticType::Outfit);
    assert_eq!(rarity, "rare");
    assert!(is_exclusive);

    let (name, cosmetic_type, rarity, is_exclusive) =
        resolve_cosmetic_metadata("AthenaPickaxe:pickaxe_id_015_raider");
    assert_eq!(name, "Raider's Revenge");
    assert_eq!(cosmetic_type, FortniteCosmeticType::Pickaxe);
    assert_eq!(rarity, "epic");
    assert!(is_exclusive);

    let (name, cosmetic_type, rarity, is_exclusive) =
        resolve_cosmetic_metadata("AthenaDance:eid_floss");
    assert_eq!(name, "Floss");
    assert_eq!(cosmetic_type, FortniteCosmeticType::Emote);
    assert_eq!(rarity, "rare");
    assert!(is_exclusive);

    let (name, cosmetic_type, rarity, is_exclusive) =
        resolve_cosmetic_metadata("AthenaGlider:glider_id_001");
    assert_eq!(name, "Mako");
    assert_eq!(cosmetic_type, FortniteCosmeticType::Glider);
    assert_eq!(rarity, "uncommon");
    assert!(is_exclusive);

    let (name, cosmetic_type, rarity, is_exclusive) =
        resolve_cosmetic_metadata("AthenaCharacter:cid_113_athena_commando_m_founder");
    assert_eq!(name, "Warpaint");
    assert_eq!(cosmetic_type, FortniteCosmeticType::Outfit);
    assert_eq!(rarity, "legendary");
    assert!(is_exclusive);
}

#[test]
fn test_resolve_cosmetic_metadata_dynamic_heuristic() {
    let (name, cosmetic_type, rarity, is_exclusive) =
        resolve_cosmetic_metadata("AthenaCharacter:cid_515_athena_commando_m_barbecue");
    assert_eq!(name, "Barbecue");
    assert_eq!(cosmetic_type, FortniteCosmeticType::Outfit);
    assert_eq!(rarity, "epic");
    assert!(!is_exclusive);

    let (name, cosmetic_type, rarity, is_exclusive) =
        resolve_cosmetic_metadata("AthenaItemWrap:wrap_042_camo");
    assert_eq!(name, "Camo");
    assert_eq!(cosmetic_type, FortniteCosmeticType::Wrap);
    assert_eq!(rarity, "uncommon");
    assert!(!is_exclusive);
}

#[test]
fn test_deserialize_epic_account_details_map_and_array() {
    // 1. Array format
    let json_array = r#"{
        "id": "acc_test_1",
        "displayName": "PlayerOne",
        "email": "player@example.com",
        "emailVerified": true,
        "tfaEnabled": true,
        "country": "RU",
        "canUpdateDisplayName": false,
        "externalAuths": [
            {
                "accountId": "psn_123",
                "type": "psn",
                "externalDisplayName": "PsnGamer",
                "dateAdded": "2022-01-01T00:00:00Z"
            }
        ]
    }"#;

    let details: EpicAccountDetails = serde_json::from_str(json_array).unwrap();
    assert_eq!(details.id, "acc_test_1");
    assert_eq!(details.display_name.as_deref(), Some("PlayerOne"));
    assert!(details.email_verified);
    assert!(details.tfa_enabled);
    assert_eq!(details.country.as_deref(), Some("RU"));
    assert_eq!(details.external_auths.len(), 1);
    assert_eq!(details.external_auths[0].auth_type, "psn");
    assert_eq!(details.external_auths[0].external_display_name.as_deref(), Some("PsnGamer"));

    // 2. Map format
    let json_map = r#"{
        "id": "acc_test_2",
        "displayName": "PlayerTwo",
        "email": "two@example.com",
        "emailVerified": false,
        "tfaEnabled": false,
        "country": "US",
        "externalAuths": {
            "xbl": {
                "accountId": "xbl_456",
                "externalDisplayName": "XboxChief"
            },
            "nintendo": {
                "externalDisplayName": "SwitchPro"
            }
        }
    }"#;

    let details_map: EpicAccountDetails = serde_json::from_str(json_map).unwrap();
    assert_eq!(details_map.id, "acc_test_2");
    assert_eq!(details_map.external_auths.len(), 2);
    let xbl = details_map.external_auths.iter().find(|a| a.auth_type == "xbl").unwrap();
    assert_eq!(xbl.external_display_name.as_deref(), Some("XboxChief"));
}

fn build_mock_epic_backend_router() -> Router {
    Router::new()
        // Account public endpoint
        .route(
            "/account/api/public/account/epic_test_user",
            get(|| async {
                Json(serde_json::json!({
                    "id": "epic_test_user",
                    "displayName": "FortniteChamp",
                    "email": "champ@fn.com",
                    "emailVerified": true,
                    "tfaEnabled": true,
                    "country": "RU",
                    "canUpdateDisplayName": true,
                    "externalAuths": {
                        "psn": {
                            "accountId": "psn_999",
                            "externalDisplayName": "ChampPSN"
                        },
                        "steam": {
                            "accountId": "76561198000000000",
                            "externalDisplayName": "ChampSteam"
                        }
                    }
                }))
            }),
        )
        // Token exchange
        .route(
            "/account/api/oauth/token",
            post(|| async {
                Json(serde_json::json!({
                    "access_token": "mock_valid_bearer_token",
                    "token_type": "bearer",
                    "expires_in": 3600,
                    "account_id": "epic_test_user",
                    "client_id": "mock_client",
                    "displayName": "FortniteChamp"
                }))
            }),
        )
        // Fortnite QueryProfile for common_core (V-Bucks & STW)
        .route(
            "/fortnite/api/game/v2/profile/epic_test_user/client/QueryProfile",
            post(|axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                let profile_id = params.get("profileId").map(|s| s.as_str()).unwrap_or("");
                if profile_id == "common_core" {
                    Json(serde_json::json!({
                        "profileRevision": 100,
                        "profileId": "common_core",
                        "profileChanges": [
                            {
                                "changeType": "fullProfileUpdate",
                                "profile": {
                                    "items": {
                                        "mtx_purchased": {
                                            "templateId": "Currency:MtxPurchased",
                                            "quantity": 1200
                                        },
                                        "mtx_earned": {
                                            "templateId": "Currency:MtxEarned",
                                            "quantity": 350
                                        },
                                        "stw_token": {
                                            "templateId": "Token:campaignaccess",
                                            "quantity": 1
                                        },
                                        "stw_founder": {
                                            "templateId": "Token:founderspack_standard",
                                            "quantity": 1
                                        }
                                    }
                                }
                            }
                        ]
                    }))
                } else if profile_id == "athena" {
                    Json(serde_json::json!({
                        "profileRevision": 200,
                        "profileId": "athena",
                        "profileChanges": [
                            {
                                "changeType": "fullProfileUpdate",
                                "profile": {
                                    "items": {
                                        "skin_1": {
                                            "templateId": "AthenaCharacter:cid_028_athena_commando_f",
                                            "attributes": {
                                                "favorite": true,
                                                "creation_time": "2018-01-01T12:00:00Z",
                                                "variants": [
                                                    {
                                                        "channel": "Material",
                                                        "active": "Mat1",
                                                        "owned": ["Mat1", "Mat2", "Mat3"]
                                                    }
                                                ]
                                            }
                                        },
                                        "skin_2": {
                                            "templateId": "AthenaCharacter:cid_035_athena_commando_m_medieval",
                                            "attributes": {
                                                "favorite": false
                                            }
                                        },
                                        "pick_1": {
                                            "templateId": "AthenaPickaxe:pickaxe_id_015_raider",
                                            "attributes": {
                                                "favorite": true
                                            }
                                        },
                                        "glider_1": {
                                            "templateId": "AthenaGlider:glider_id_001",
                                            "attributes": {
                                                "favorite": false
                                            }
                                        },
                                        "dance_1": {
                                            "templateId": "AthenaDance:eid_floss",
                                            "attributes": {
                                                "favorite": false
                                            }
                                        }
                                    },
                                    "stats": {
                                        "attributes": {
                                            "level": 85,
                                            "accountLevel": 155,
                                            "bookLevel": 100,
                                            "bookPurchased": true,
                                            "lifetimeWins": 73,
                                            "lastMatchEndDatetime": "2024-01-01T12:00:00Z",
                                            "pastSeasons": [
                                                {
                                                    "seasonNumber": 2,
                                                    "seasonLevel": 70,
                                                    "bookLevel": 70,
                                                    "bookPurchased": true,
                                                    "numWins": 25
                                                }
                                            ]
                                        }
                                    }
                                }
                            }
                        ]
                    }))
                } else {
                    Json(serde_json::json!({
                        "profileRevision": 0,
                        "profileId": profile_id,
                        "profileChanges": []
                    }))
                }
            }),
        )
}

#[tokio::test]
async fn test_epic_client_fetch_fortnite_summary() {
    let mock_router = build_mock_epic_backend_router();
    let client = EpicClient::from_router(mock_router);

    let summary = client
        .fetch_fortnite_summary("mock_token", "epic_test_user")
        .await
        .expect("Failed to fetch Fortnite summary");

    assert!(summary.has_fortnite);
    assert_eq!(summary.account_id, "epic_test_user");
    assert_eq!(summary.display_name, "FortniteChamp");
    assert!(summary.email_verified);
    assert!(summary.tfa_enabled);
    assert_eq!(summary.country.as_deref(), Some("RU"));

    // Check V-Bucks
    assert_eq!(summary.total_vbucks, 1550);
    assert_eq!(summary.vbucks_purchased, 1200);
    assert_eq!(summary.vbucks_earned, 350);

    // Check Save The World
    assert!(summary.has_save_the_world);
    assert!(summary.is_stw_founder);

    // Check stats
    assert_eq!(summary.season_level, 85);
    assert_eq!(summary.account_level, 155);
    assert_eq!(summary.battle_pass_level, 100);
    assert!(summary.battle_pass_purchased);
    assert_eq!(summary.lifetime_wins, 73);
    assert_eq!(summary.past_seasons.len(), 1);
    assert_eq!(summary.last_match_end_datetime.as_deref(), Some("2024-01-01T12:00:00Z"));
    assert!(summary.days_inactive.is_some());

    // Check cosmetics
    assert_eq!(summary.outfits_count, 2);
    assert_eq!(summary.pickaxes_count, 1);
    assert_eq!(summary.gliders_count, 1);
    assert_eq!(summary.emotes_count, 1);
    assert_eq!(summary.total_cosmetics, 5);
    assert_eq!(summary.exclusive_count, 5);

    // Both items are favorited and exclusive; Raider's Revenge is Epic (rank 7) so it appears before Renegade Raider (Rare, rank 6)
    assert_eq!(summary.cosmetics[0].name, "Raider's Revenge");
    assert!(summary.cosmetics[0].is_favorite);
    assert!(summary.cosmetics[0].is_exclusive);
    assert_eq!(summary.cosmetics[1].name, "Renegade Raider");
    assert!(summary.cosmetics[1].is_favorite);
    assert!(summary.cosmetics[1].is_exclusive);
    assert_eq!(summary.cosmetics[1].variants_count, 3);
    assert_eq!(summary.cosmetics[1].variant_styles, vec!["Mat1".to_string(), "Mat2".to_string(), "Mat3".to_string()]);
}

#[tokio::test]
async fn test_server_fortnite_endpoints() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    // Create an Epic account in DB
    db.upsert_epic_account(&Account {
        id: "epic_test_user".to_string(),
        launcher: "epic".to_string(),
        username: "FortniteChamp".to_string(),
        display_name: Some("FortniteChamp".to_string()),
        email: Some("champ@fn.com".to_string()),
        is_active: true,
        last_used: None,
    })
    .await
    .unwrap();

    let keyring = Arc::new(MemoryCredentialStore::new());
    let device_auth = DeviceAuth::new("epic_test_user", "dev_1", "sec_1");
    keyring.set_device_auth(&device_auth).unwrap();

    let mock_router = build_mock_epic_backend_router();
    let client = Arc::new(EpicClient::from_router(mock_router));

    let epic_store = Arc::new(EpicAuthStore::new(db.clone(), keyring, client));
    let state = AppState::new(db.clone(), epic_store);
    let app = create_app_with_state(state);

    // 1. GET /api/epic/account-details
    let req = Request::builder()
        .method("GET")
        .uri("/api/epic/account-details?account_id=epic_test_user")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let details: EpicAccountDetails = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(details.id, "epic_test_user");
    assert_eq!(details.display_name.as_deref(), Some("FortniteChamp"));
    assert!(details.email_verified);
    assert!(details.tfa_enabled);
    assert_eq!(details.country.as_deref(), Some("RU"));
    assert_eq!(details.external_auths.len(), 2);

    // 2. GET /api/epic/fortnite/summary
    let req = Request::builder()
        .method("GET")
        .uri("/api/epic/fortnite/summary?account_id=epic_test_user")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let summary: FortniteAccountSummary = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(summary.total_vbucks, 1550);
    assert_eq!(summary.outfits_count, 2);
    assert_eq!(summary.total_cosmetics, 5);

    // 3. GET /api/epic/fortnite/skins?type=outfit
    let req = Request::builder()
        .method("GET")
        .uri("/api/epic/fortnite/skins?account_id=epic_test_user&type=outfit")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let skins: Vec<Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(skins.len(), 2);
    assert!(skins.iter().all(|s| s["cosmetic_type"] == "outfit"));

    // 4. GET /api/epic/fortnite/skins?search=renegade
    let req = Request::builder()
        .method("GET")
        .uri("/api/epic/fortnite/skins?account_id=epic_test_user&search=renegade")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let skins: Vec<Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(skins.len(), 1);
    assert_eq!(skins[0]["name"], "Renegade Raider");

    // 5. POST /api/epic/fortnite/refresh
    let req = Request::builder()
        .method("POST")
        .uri("/api/epic/fortnite/refresh")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"account_id":"epic_test_user"}"#))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let ref_summary: FortniteAccountSummary = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(ref_summary.total_vbucks, 1550);

    // 6. GET /api/epic/fortnite/skins with non-matching search
    let req = Request::builder()
        .method("GET")
        .uri("/api/epic/fortnite/skins?account_id=epic_test_user&search=nonexistent_skin_xyz")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let skins: Vec<Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(skins.len(), 0);

    // 7. GET /api/epic/account-details for nonexistent account returns error
    let req = Request::builder()
        .method("GET")
        .uri("/api/epic/account-details?account_id=nonexistent_user")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_fortnite_account_without_game_profile() {
    let mock_router = Router::new()
        .route(
            "/account/api/public/account/empty_user",
            get(|| async {
                Json(serde_json::json!({
                    "id": "empty_user",
                    "displayName": "EmptyGamer",
                    "emailVerified": false,
                    "tfaEnabled": false
                }))
            }),
        )
        .route(
            "/fortnite/api/game/v2/profile/empty_user/client/QueryProfile",
            post(|| async {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "errorCode": "errors.com.epicgames.modules.profiles.profile_not_found",
                        "errorMessage": "Profile not found",
                        "numericErrorCode": 12801
                    })),
                )
            }),
        );

    let client = EpicClient::from_router(mock_router);
    let summary = client
        .fetch_fortnite_summary("mock_token", "empty_user")
        .await
        .expect("Summary should not fail even if Fortnite profile 404s");

    assert_eq!(summary.account_id, "empty_user");
    assert_eq!(summary.display_name, "EmptyGamer");
    assert!(!summary.has_fortnite);
    assert_eq!(summary.total_vbucks, 0);
    assert_eq!(summary.outfits_count, 0);
    assert_eq!(summary.total_cosmetics, 0);
    assert!(!summary.has_save_the_world);
    assert!(!summary.is_stw_founder);
}

#[tokio::test]
async fn test_stw_founder_vs_standard_campaign_access() {
    // Non-founder STW pack only has Token:campaignaccess
    let router_non_founder = Router::new()
        .route(
            "/account/api/public/account/starter_user",
            get(|| async {
                Json(serde_json::json!({
                    "id": "starter_user",
                    "displayName": "StarterPlayer"
                }))
            }),
        )
        .route(
            "/fortnite/api/game/v2/profile/starter_user/client/QueryProfile",
            post(|axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                let profile_id = params.get("profileId").map(|s| s.as_str()).unwrap_or("");
                if profile_id == "common_core" {
                    Json(serde_json::json!({
                        "profileRevision": 1,
                        "profileId": "common_core",
                        "profileChanges": [{
                            "changeType": "fullProfileUpdate",
                            "profile": {
                                "items": {
                                    "campaign": {
                                        "templateId": "Token:campaignaccess",
                                        "quantity": 1
                                    }
                                }
                            }
                        }]
                    }))
                } else {
                    Json(serde_json::json!({
                        "profileRevision": 1,
                        "profileId": profile_id,
                        "profileChanges": []
                    }))
                }
            }),
        );

    let client = EpicClient::from_router(router_non_founder);
    let summary = client.fetch_fortnite_summary("mock_token", "starter_user").await.unwrap();

    // Standard STW has save the world, but is NOT a founder
    assert!(summary.has_save_the_world);
    assert!(!summary.is_stw_founder);

    // Founder STW has Token:founderspack_standard or Token:founderspack_deluxe
    let router_founder = Router::new()
        .route(
            "/account/api/public/account/founder_user",
            get(|| async {
                Json(serde_json::json!({
                    "id": "founder_user",
                    "displayName": "FounderPlayer"
                }))
            }),
        )
        .route(
            "/fortnite/api/game/v2/profile/founder_user/client/QueryProfile",
            post(|axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                let profile_id = params.get("profileId").map(|s| s.as_str()).unwrap_or("");
                if profile_id == "common_core" {
                    Json(serde_json::json!({
                        "profileRevision": 1,
                        "profileId": "common_core",
                        "profileChanges": [{
                            "changeType": "fullProfileUpdate",
                            "profile": {
                                "items": {
                                    "founder_pack": {
                                        "templateId": "Token:founderspack_deluxe",
                                        "quantity": 1
                                    }
                                }
                            }
                        }]
                    }))
                } else {
                    Json(serde_json::json!({
                        "profileRevision": 1,
                        "profileId": profile_id,
                        "profileChanges": []
                    }))
                }
            }),
        );

    let client_founder = EpicClient::from_router(router_founder);
    let summary_founder = client_founder.fetch_fortnite_summary("mock_token", "founder_user").await.unwrap();

    assert!(summary_founder.has_save_the_world);
    assert!(summary_founder.is_stw_founder);
}

#[test]
fn test_resolve_cosmetic_metadata_case_insensitivity() {
    let (name, cosmetic_type, rarity, is_exclusive) =
        resolve_cosmetic_metadata("athenacharacter:cid_028_athena_commando_f");
    assert_eq!(name, "Renegade Raider");
    assert_eq!(cosmetic_type, FortniteCosmeticType::Outfit);
    assert_eq!(rarity, "rare");
    assert!(is_exclusive);

    let (name, cosmetic_type, rarity, is_exclusive) =
        resolve_cosmetic_metadata("ATHENAPICKAXE:pickaxe_id_015_raider");
    assert_eq!(name, "Raider's Revenge");
    assert_eq!(cosmetic_type, FortniteCosmeticType::Pickaxe);
    assert_eq!(rarity, "epic");
    assert!(is_exclusive);

    let (name, cosmetic_type, _rarity, is_exclusive) =
        resolve_cosmetic_metadata("AthenaGlider:glider_founder");
    assert_eq!(name, "Founder's Glider");
    assert_eq!(cosmetic_type, FortniteCosmeticType::Glider);
    assert!(is_exclusive);
}

#[tokio::test]
async fn test_auth_error_not_swallowed() {
    let mock_router = Router::new()
        .route(
            "/account/api/public/account/expired_user",
            get(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "errorCode": "errors.com.epicgames.common.authentication.token_verification_failed",
                        "errorMessage": "Token is expired or invalid",
                        "numericErrorCode": 1014
                    })),
                )
            }),
        );

    let client = EpicClient::from_router(mock_router);
    let err = client
        .fetch_fortnite_summary("expired_token", "expired_user")
        .await
        .expect_err("Expired or unauthorized token must fail");

    assert!(matches!(err, swch::launchers::epic::models::EpicError::Auth(_)));
}

