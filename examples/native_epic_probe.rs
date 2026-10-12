//! Read-only native identity/game guard probe. No screenshots, input, auth changes, or orders.
use serde_json::json;
use swch::{launchers::epic::{EpicAuthStore,EpicSessionManager},storage::Database};
#[tokio::main]
async fn main()->Result<(),Box<dyn std::error::Error>> {
    let db=Database::open_default()?;
    let Some(account)=db.get_active_epic_account().await? else {println!("{}",json!({"native_account_bound":false,"reason":"no_active_account"}));return Ok(());};
    let epic=EpicAuthStore::default_store(db);
    let games=epic.native_claim_game_paths().await?;
    match EpicSessionManager::new().native_claim_binding(&account.id,&games) {
        Ok(binding)=>println!("{}",json!({"native_account_bound":binding.is_some(),"known_game_executables":games.len()})),
        Err(swch::launchers::epic::EpicError::Auth(reason))=>println!("{}",json!({"native_account_bound":false,"reason":reason})),
        Err(_)=>println!("{}",json!({"native_account_bound":false,"reason":"native_guard_requires_attention"})),
    }
    Ok(())
}
