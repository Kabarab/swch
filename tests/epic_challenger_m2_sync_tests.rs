//! Empirical Adversarial Test Suite for Milestone 2 (teamwork_preview_challenger_m2_2)
//!
//! Focus areas:
//! 1. Library sync atomicity & concurrency (same account & multiple accounts).
//! 2. Account switching SLA (< 5ms) and library ownership isolation.
//! 3. Foreign key cascade integrity and game catalog preservation.
//! 4. Identification of schema/ID inconsistencies between `accounts` and `game_accounts`.

#[allow(unused_imports)]
mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::mock_server::MockEpicServer;
use common::models::Entitlement;
use swch::launchers::epic::{EpicAuthStore, EpicClient, MemoryCredentialStore};
use swch::storage::Database;

#[allow(dead_code)]
fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

async fn setup_challenger_env() -> (MockEpicServer, Database, EpicAuthStore) {
    let mock = MockEpicServer::start().await;
    let db = Database::open_in_memory().expect("Failed to create in-memory DB");
    db.run_migrations().await.expect("Migrations failed");

    let client = Arc::new(EpicClient::from_router(mock.router()));
    let keyring = Arc::new(MemoryCredentialStore::default());
    let store = EpicAuthStore::new(db.clone(), keyring, client);
    (mock, db, store)
}

fn create_sample_entitlements(account_id: &str, count: usize) -> Vec<Entitlement> {
    (0..count)
        .map(|i| {
            let cat_id = format!("cat_{:032x}", i + 1);
            Entitlement {
                id: format!("ent_{}_{}", account_id, i),
                entitlement_name: format!("Game Title {}", i),
                namespace: "sample_namespace".to_string(),
                catalog_item_id: cat_id,
                account_id: account_id.to_string(),
                identity_id: Some(account_id.to_string()),
                entitlement_type: "EXECUTABLE".to_string(),
                grant_date: Some("2026-01-01T00:00:00.000Z".to_string()),
                consumable: Some(false),
                status: "ACTIVE".to_string(),
                active: true,
                use_count: None,
                created: None,
                updated: None,
            }
        })
        .collect()
}

// ============================================================================
// 1. Empirical Bug Discovery: Foreign Key Failure with Real Hex Account IDs
// ============================================================================

#[tokio::test]
async fn test_challenge_foreign_key_and_account_id_integrity() {
    let (mock, db, store) = setup_challenger_env().await;

    // Real Epic Games OAuth accounts have 32-character hex account IDs (NO "acc_epic_" prefix)
    let real_epic_acc_id = "34a02cf8f4414e29b15921876da36f9a";
    let auth_code = "a0b1c2d3e4f5a0b1c2d3e4f5a0b1c2d3";
    mock.register_auth_code(auth_code, real_epic_acc_id, "HexPlayer").await;

    let entitlements = create_sample_entitlements(real_epic_acc_id, 3);
    mock.set_entitlements(real_epic_acc_id, entitlements).await;

    // 1. Login succeeds and inserts row into `accounts` table with id = "34a02cf8f4414e29b15921876da36f9a"
    let login_result = store.login_with_code(auth_code).await;
    assert!(login_result.is_ok(), "Login failed: {:?}", login_result.err());
    let logged_in = login_result.unwrap();
    assert_eq!(logged_in.account_id, real_epic_acc_id);

    // Verify account row in DB
    let accounts = db.get_epic_accounts().await.unwrap();
    let db_account = accounts.iter().find(|a| a.id == real_epic_acc_id);
    assert!(
        db_account.is_some(),
        "Account must exist in DB with primary key id = {}",
        real_epic_acc_id
    );

    // 2. Now attempt sync_library using the real production store
    // In src/launchers/epic/store.rs lines 432-436:
    // canonical_account_id = if account_id.starts_with("acc_epic_") { ... } else { format!("acc_epic_{}", account_id) }
    // This prefixes "acc_epic_" to the account_id when constructing GameAccount links!
    // But accounts table primary key is "34a02cf8f4414e29b15921876da36f9a" (WITHOUT "acc_epic_").
    // Therefore, FOREIGN KEY (account_id) REFERENCES accounts(id) in SQLite FAILS!
    let sync_res = store.sync_library(real_epic_acc_id, None).await;

    let summary = sync_res.unwrap();
    println!("Sync succeeded with summary: {:?}", summary);

    let db_accounts = db.get_epic_accounts().await.unwrap();
    println!("DB Accounts: {:?}", db_accounts);

    let links = db.get_game_accounts().await.unwrap();
    println!("DB Game Accounts Links: {:?}", links);

    let library = db.get_epic_library(Some(real_epic_acc_id)).await.unwrap();
    println!("Library games returned for {}: {:?}", real_epic_acc_id, library);
    assert_eq!(library.len(), 3, "Should return 3 games");

    // Also check get_epic_library with None (active account)
    let active_library = db.get_epic_library(None).await.unwrap();
    println!("Active library games returned: {:?}", active_library);
    assert_eq!(active_library.len(), 3, "Active library should return 3 games");
}

// ============================================================================
// 2. Foreign Key Cascade on Account Deletion (using prefixed IDs to test cascade)
// ============================================================================

#[tokio::test]
async fn test_challenge_foreign_key_cascade_preserves_catalog_and_cleans_links() {
    let (mock, db, store) = setup_challenger_env().await;

    // Accounts starting with "acc_epic_" allow canonical_account_id to match accounts.id
    let acc_a = "acc_epic_alpha_12345678901234567";
    let acc_b = "acc_epic_beta_123456789012345678";
    let code_a = "11111111111111111111111111111111";
    let code_b = "22222222222222222222222222222222";

    mock.register_auth_code(code_a, acc_a, "Alpha").await;
    mock.register_auth_code(code_b, acc_b, "Beta").await;

    // Both accounts own Game 1; Alpha also owns Game 2
    let ent_game1_a = Entitlement {
        id: "ent_game1_a".to_string(),
        entitlement_name: "Shared Game 1".to_string(),
        namespace: "shared_ns".to_string(),
        catalog_item_id: "catalog_item_shared_001".to_string(),
        account_id: acc_a.to_string(),
        identity_id: Some(acc_a.to_string()),
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: Some("2026-01-01T00:00:00.000Z".to_string()),
        consumable: Some(false),
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    let ent_game2_a = Entitlement {
        id: "ent_game2_a".to_string(),
        entitlement_name: "Alpha Exclusive Game 2".to_string(),
        namespace: "exclusive_ns".to_string(),
        catalog_item_id: "catalog_item_alpha_002".to_string(),
        account_id: acc_a.to_string(),
        identity_id: Some(acc_a.to_string()),
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: Some("2026-01-01T00:00:00.000Z".to_string()),
        consumable: Some(false),
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };
    let ent_game1_b = Entitlement {
        id: "ent_game1_b".to_string(),
        entitlement_name: "Shared Game 1".to_string(),
        namespace: "shared_ns".to_string(),
        catalog_item_id: "catalog_item_shared_001".to_string(),
        account_id: acc_b.to_string(),
        identity_id: Some(acc_b.to_string()),
        entitlement_type: "EXECUTABLE".to_string(),
        grant_date: Some("2026-01-01T00:00:00.000Z".to_string()),
        consumable: Some(false),
        status: "ACTIVE".to_string(),
        active: true,
        use_count: None,
        created: None,
        updated: None,
    };

    mock.set_entitlements(acc_a, vec![ent_game1_a, ent_game2_a]).await;
    mock.set_entitlements(acc_b, vec![ent_game1_b]).await;

    store.login_with_code(code_a).await.unwrap();
    store.login_with_code(code_b).await.unwrap();

    store.sync_library(acc_a, None).await.unwrap();
    store.sync_library(acc_b, None).await.unwrap();

    // Verify initial state
    let initial_games = db.get_games().await.unwrap();
    assert_eq!(initial_games.len(), 2, "There should be 2 unique catalog games");

    let initial_links = db.get_game_accounts().await.unwrap();
    assert_eq!(initial_links.len(), 3, "There should be 3 ownership links: A owns 2, B owns 1");

    // Remove account A
    store.remove_account(acc_a).await.unwrap();

    // Verify CASCADE behavior:
    // 1. Account A is removed from `accounts` table
    let accounts_after = db.get_epic_accounts().await.unwrap();
    assert_eq!(accounts_after.len(), 1);
    assert_eq!(accounts_after[0].id, acc_b);

    // 2. Account A's ownership links in `game_accounts` must be deleted by cascade
    let links_after = db.get_game_accounts().await.unwrap();
    let account_a_links: Vec<_> = links_after.iter().filter(|l| l.account_id == acc_a).collect();
    assert!(
        account_a_links.is_empty(),
        "Orphan links detected! Account A was deleted but links remain: {:?}",
        account_a_links
    );

    // 3. Account B's link to Shared Game 1 must remain intact
    let account_b_links: Vec<_> = links_after.iter().filter(|l| l.account_id == acc_b).collect();
    assert_eq!(account_b_links.len(), 1, "Account B must retain its ownership link");

    // 4. Catalog games in `games` table must NOT be deleted or corrupted!
    let games_after = db.get_games().await.unwrap();
    assert_eq!(
        games_after.len(),
        2,
        "Catalog games in `games` table must not be deleted when an account is removed"
    );

    // 5. Account B's library query still works and returns Game 1
    let b_lib = db.get_epic_library(Some(acc_b)).await.unwrap();
    assert_eq!(b_lib.len(), 1);
    assert_eq!(b_lib[0].catalog_item_id, "catalog_item_shared_001");
}

// ============================================================================
// 3. Concurrency Stress Test: Same Account Concurrent Syncs
// ============================================================================

#[tokio::test]
async fn test_challenge_concurrent_sync_same_account() {
    let (mock, db, store) = setup_challenger_env().await;

    let acc_id = "acc_epic_concurrent_same_123456";
    let code = "33333333333333333333333333333333";
    mock.register_auth_code(code, acc_id, "ConcurrentSame").await;

    let entitlements = create_sample_entitlements(acc_id, 20);
    mock.set_entitlements(acc_id, entitlements).await;

    store.login_with_code(code).await.unwrap();

    let store_arc = Arc::new(store);
    let mut tasks = Vec::new();

    // Spawn 10 concurrent sync tasks on the exact same account
    for task_idx in 0..10 {
        let store_clone = store_arc.clone();
        tasks.push(tokio::spawn(async move {
            let start = Instant::now();
            let res = store_clone.sync_library(acc_id, None).await;
            (task_idx, res, start.elapsed())
        }));
    }

    let mut successful_syncs = 0;
    for t in tasks {
        let (idx, res, dur) = t.await.unwrap();
        match res {
            Ok(summary) => {
                successful_syncs += 1;
                assert_eq!(summary.base_games_count, 20);
                println!("Task {} completed sync in {:?}", idx, dur);
            }
            Err(e) => {
                panic!("Concurrent sync task {} failed with error: {:?}", idx, e);
            }
        }
    }

    assert_eq!(successful_syncs, 10, "All 10 concurrent syncs should succeed");

    // Verify no duplicates in database
    let games = db.get_games().await.unwrap();
    assert_eq!(games.len(), 20, "Should have exactly 20 unique games in DB");

    let links = db.get_game_accounts().await.unwrap();
    assert_eq!(links.len(), 20, "Should have exactly 20 ownership links in DB (no duplicates)");
}

// ============================================================================
// 4. Concurrency & Isolation Stress Test: Multiple Accounts Concurrent Syncs
// ============================================================================

#[tokio::test]
async fn test_challenge_concurrent_sync_different_accounts_isolation() {
    let (mock, db, store) = setup_challenger_env().await;

    let num_accounts = 5;
    let mut account_ids = Vec::new();

    for i in 0..num_accounts {
        let acc_id = format!("acc_epic_multi_{:019x}", i + 1);
        let code = format!("{:032x}", i + 100);
        mock.register_auth_code(&code, &acc_id, &format!("User{}", i)).await;

        // Each account gets 10 games; some shared, some unique
        let mut ents = Vec::new();
        for g in 0..10 {
            // Games 0..4 are unique to each account; Games 5..9 are shared
            let cat_id = if g < 5 {
                format!("cat_unique_{}_{}", i, g)
            } else {
                format!("cat_shared_{}", g)
            };

            ents.push(Entitlement {
                id: format!("ent_{}_{}", i, g),
                entitlement_name: format!("Game {}", cat_id),
                namespace: "ns".to_string(),
                catalog_item_id: cat_id,
                account_id: acc_id.clone(),
                identity_id: Some(acc_id.clone()),
                entitlement_type: "EXECUTABLE".to_string(),
                grant_date: Some("2026-01-01T00:00:00.000Z".to_string()),
                consumable: Some(false),
                status: "ACTIVE".to_string(),
                active: true,
                use_count: None,
                created: None,
                updated: None,
            });
        }
        mock.set_entitlements(&acc_id, ents).await;
        store.login_with_code(&code).await.unwrap();
        account_ids.push(acc_id);
    }

    let store_arc = Arc::new(store);
    let mut tasks = Vec::new();

    // Concurrently sync all accounts
    for acc in &account_ids {
        let s = store_arc.clone();
        let target_acc = acc.clone();
        tasks.push(tokio::spawn(async move {
            let start = Instant::now();
            let res = s.sync_library(&target_acc, None).await;
            (target_acc, res, start.elapsed())
        }));
    }

    for t in tasks {
        let (acc, res, dur) = t.await.unwrap();
        match res {
            Ok(summary) => {
                assert_eq!(summary.base_games_count, 10);
                println!("Synced account {} in {:?}", acc, dur);
            }
            Err(e) => {
                panic!("Failed to sync account {}: {:?}", acc, e);
            }
        }
    }

    // Verify Isolation:
    // Total unique games = (5 accounts * 5 unique) + 5 shared = 30 unique games
    let games = db.get_games().await.unwrap();
    assert_eq!(games.len(), 30, "Expected 30 unique games in catalog");

    // Total links = 5 accounts * 10 games = 50 links
    let links = db.get_game_accounts().await.unwrap();
    assert_eq!(links.len(), 50, "Expected 50 ownership links");

    // Verify per-account isolation query
    for (i, acc) in account_ids.iter().enumerate() {
        let lib = db.get_epic_library(Some(acc)).await.unwrap();
        assert_eq!(lib.len(), 10, "Account {} should see exactly 10 games", acc);

        // Verify it contains its unique games and shared games, but NO OTHER account's unique games
        for game in &lib {
            if game.catalog_item_id.starts_with("cat_unique_") {
                let expected_prefix = format!("cat_unique_{}_", i);
                assert!(
                    game.catalog_item_id.starts_with(&expected_prefix),
                    "Data leak! Account {} sees game '{}' belonging to another account!",
                    acc,
                    game.catalog_item_id
                );
            }
        }
    }
}

// ============================================================================
// 5. Account Switch SLA Benchmarking & Active Library Synchronization
// ============================================================================

#[tokio::test]
async fn test_challenge_account_switch_sla_and_active_library_consistency() {
    let (mock, db, store) = setup_challenger_env().await;

    let acc_1 = "acc_epic_switch_sla_1_123456789";
    let acc_2 = "acc_epic_switch_sla_2_123456789";
    let code_1 = "44444444444444444444444444444444";
    let code_2 = "55555555555555555555555555555555";

    mock.register_auth_code(code_1, acc_1, "User1").await;
    mock.register_auth_code(code_2, acc_2, "User2").await;

    // Account 1 owns 5 games, Account 2 owns 8 games
    let ent_1 = create_sample_entitlements(acc_1, 5);
    let ent_2 = create_sample_entitlements(acc_2, 8);
    mock.set_entitlements(acc_1, ent_1).await;
    mock.set_entitlements(acc_2, ent_2).await;

    store.login_with_code(code_1).await.unwrap();
    store.login_with_code(code_2).await.unwrap();

    store.sync_library(acc_1, None).await.unwrap();
    store.sync_library(acc_2, None).await.unwrap();

    let iterations = 100;
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let target = if i % 2 == 0 { acc_1 } else { acc_2 };
        let expected_count = if i % 2 == 0 { 5 } else { 8 };

        let start = Instant::now();
        let switched = store.switch_active_account(target).await.unwrap();
        let elapsed = start.elapsed();
        latencies.push(elapsed);

        assert_eq!(switched.account_id, target);

        // Immediate query of active library (account_id = None)
        let active_lib = db.get_epic_library(None).await.unwrap();
        assert_eq!(
            active_lib.len(),
            expected_count,
            "Immediate library query after switch must reflect newly active account!"
        );
    }

    latencies.sort();
    let min = latencies[0];
    let max = latencies[latencies.len() - 1];
    let median = latencies[latencies.len() / 2];
    let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
    let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];
    let sum: Duration = latencies.iter().sum();
    let avg_ms = (sum.as_secs_f64() * 1000.0) / iterations as f64;

    println!(
        "Account Switch SLA Benchmark (100 switches):\n\
         - Min: {:?}\n\
         - Median: {:?}\n\
         - Average: {:.3} ms\n\
         - P95: {:?}\n\
         - P99: {:?}\n\
         - Max: {:?}",
        min, median, avg_ms, p95, p99, max
    );

    assert!(
        avg_ms < 2.0,
        "Average switch latency ({:.3} ms) must be well below 2ms",
        avg_ms
    );
    assert!(
        median < Duration::from_millis(2),
        "Median switch latency ({:?}) must be below 2ms",
        median
    );
    assert!(
        max < Duration::from_millis(10),
        "Max switch latency ({:?}) must strictly satisfy the < 10ms acceptance SLA",
        max
    );
}
