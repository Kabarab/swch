use std::process::Command;
use tracing::info;

use crate::storage::RobloxAccountInfo;
use super::client::RobloxClient;

/// Builds launch protocol URIs and executes the Roblox Player client.
pub struct RobloxLauncher {
    client: RobloxClient,
}

impl Default for RobloxLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl RobloxLauncher {
    pub fn new() -> Self {
        Self {
            client: RobloxClient::new(),
        }
    }

    /// Launches the Roblox Desktop Client application logged in as the specified account.
    pub async fn launch_app(
        &self,
        account: &RobloxAccountInfo,
        multi_instance: bool,
    ) -> Result<String, String> {
        let ticket = self.client.generate_auth_ticket(&account.cookie).await?;
        let now = chrono::Utc::now().timestamp_millis();
        let uri = format!(
            "roblox-player:1+launchmode:app+gameinfo:{}+launchtime:{}",
            ticket, now
        );

        info!("Launching Roblox client for user @{}...", account.username);
        Self::execute_uri(&uri, multi_instance)?;

        Ok(format!(
            "Roblox успешно запущен для аккаунта @{} ({})",
            account.username, account.display_name
        ))
    }

    /// Launches a specific Roblox Place / Game directly under the specified account.
    pub async fn launch_place(
        &self,
        account: &RobloxAccountInfo,
        place_id: &str,
        multi_instance: bool,
    ) -> Result<String, String> {
        let ticket = self.client.generate_auth_ticket(&account.cookie).await?;
        let now = chrono::Utc::now().timestamp_millis();
        let place_url_encoded = format!(
            "https%3A%2F%2Fassetgame.roblox.com%2Fgame%2FPlaceLauncher.ashx%3Frequest%3DRequestGame%26placeId%3D{}",
            place_id
        );
        let uri = format!(
            "roblox-player:1+launchmode:play+gameinfo:{}+launchtime:{}+placelauncherurl:{}",
            ticket, now, place_url_encoded
        );

        info!("Launching Roblox Place {} for user @{}...", place_id, account.username);
        Self::execute_uri(&uri, multi_instance)?;

        Ok(format!(
            "Плейс {} запущен для аккаунта @{}",
            place_id, account.username
        ))
    }

    /// Dispatches the URI protocol to the operating system.
    #[allow(unused_variables)]
    fn execute_uri(uri: &str, multi_instance: bool) -> Result<(), String> {
        if std::env::var("SWCH_DRY_RUN").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false) {
            info!("Dry run: skipping process launch for URI: {}", uri);
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            let mut cmd = Command::new("open");
            if multi_instance {
                cmd.arg("-n");
            }
            cmd.arg(uri);
            cmd.spawn().map_err(|e| format!("Failed to spawn open command: {}", e))?;
            return Ok(());
        }

        #[cfg(target_os = "windows")]
        {
            Command::new("cmd")
                .args(["/C", "start", "", uri])
                .spawn()
                .map_err(|e| format!("Failed to launch via cmd start: {}", e))?;
            return Ok(());
        }

        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            open::that(uri).map_err(|e| format!("Failed to open URI: {}", e))?;
            Ok(())
        }
    }
}
