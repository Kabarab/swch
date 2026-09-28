use std::collections::HashMap;
use swch::launchers::epic::{EpicClient, models::DeviceAuth};
use swch::storage::Database;

#[tokio::test]
#[ignore]
async fn test_inspect_real_epic_catalog() {
    let db = Database::open_default().unwrap();
    let cred_json = db.get_epic_credential("48c258c6b57649578e5f97de32390b40").await.unwrap().unwrap();
    let auth: DeviceAuth = serde_json::from_str(&cred_json).unwrap();
    let client = EpicClient::new();
    let token_resp = client.login_with_refresh_token(auth.refresh_token.as_ref().unwrap()).await.unwrap();
    println!("Token received! Fetching entitlements...");
    let entitlements = client.fetch_all_entitlements(&token_resp.access_token, &auth.account_id).await.unwrap();
    println!("Total entitlements: {}", entitlements.len());

    // Group by namespace
    let mut by_ns: HashMap<String, Vec<String>> = HashMap::new();
    for ent in &entitlements {
        if ent.is_executable() {
            by_ns.entry(ent.namespace.clone()).or_default().push(ent.catalog_item_id.clone());
        }
    }

    for (ns, ids) in &by_ns {
        println!("\n--- Namespace: '{}' (count: {}) ---", ns, ids.len());
        let chunk: Vec<&str> = ids.iter().take(5).map(|s| s.as_str()).collect();
        match client.fetch_catalog_items(&token_resp.access_token, ns, &chunk).await {
            Ok(catalog_map) => {
                println!("Fetched {} catalog items:", catalog_map.len());
                for (id, item) in catalog_map {
                    println!("  ID: {} | Title: '{}' | TallArt: {:?} | Namespace: {}",
                        id,
                        item.title,
                        item.tall_box_art(),
                        item.namespace
                    );
                }
            }
            Err(e) => {
                println!("Error fetching catalog for ns '{}': {}", ns, e);
            }
        }
    }
}


