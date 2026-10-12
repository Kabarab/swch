//! Developer diagnostic: prints only connection status, never credentials.
use std::sync::Arc;
use swch::{
    auto_claim::{native_steam, providers::Stores},
    launchers::epic::EpicAuthStore,
    storage::Database,
};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::open_default()?;
    use swch::auto_claim::providers::Provider;
    let stores = Stores::new(Arc::new(EpicAuthStore::default_store(db.clone())));
    let accounts = stores
        .accounts(db.get_accounts().await?)
        .await?
        .into_iter()
        .filter(|a| a.launcher == "steam")
        .collect::<Vec<_>>();
    let requested = std::env::args().nth(1);
    if requested.is_none() {
        println!(
            "{}",
            serde_json::json!({"accounts":accounts.len(),"local_logins":accounts.iter().filter(|a|native_steam::available(a)).count()})
        );
        return Ok(());
    }
    for account in accounts.iter().filter(|a| {
        matches!(requested.as_deref(), Some("all" | "cached"))
            || requested.as_deref() == Some(&a.id)
    }) {
        let result = if requested.as_deref() == Some("cached") {
            match swch::auto_claim::steam_auth::SteamSession::load(&account.id) {
                Ok(session) => stores.verify_steam_session(&session).await,
                Err(e) => Err(e),
            }
        } else {
            stores.automatic_steam_connect(account).await
        };
        println!(
            "{}",
            serde_json::json!({"account_id":account.id,"connected":result.is_ok(),"error":result.err()})
        );
    }
    Ok(())
}
