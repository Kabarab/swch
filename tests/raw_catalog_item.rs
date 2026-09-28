use swch::launchers::epic::{EpicClient, models::DeviceAuth};
use swch::storage::Database;

#[tokio::test]
#[ignore]
async fn test_print_raw_catalog_item() {
    let db = Database::open_default().unwrap();
    let cred_json = db.get_epic_credential("48c258c6b57649578e5f97de32390b40").await.unwrap().unwrap();
    let auth: DeviceAuth = serde_json::from_str(&cred_json).unwrap();
    let client = EpicClient::new();
    let token_resp = client.login_with_refresh_token(auth.refresh_token.as_ref().unwrap()).await.unwrap();

    let url = "https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace/aca2ee0619aa482791f0a71dbf3eeac0/bulk/items?id=0f14b1aadf8e4b09a1adb5a00f1055cb";
    let req = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {}", token_resp.access_token))
        .header("User-Agent", "UERefreshed/1.0");
    let resp = req.send().await.unwrap();
    let text = resp.text().await.unwrap();
    println!("RAW CATALOG JSON:\n{}", text);
}
