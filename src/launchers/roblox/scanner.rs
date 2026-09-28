use std::path::Path;
use tracing::{info, warn};
use walkdir::WalkDir;

use crate::storage::{Database, RobloxAccountInfo};
use super::client::RobloxClient;

/// Cross-platform scanner that discovers Roblox session cookies automatically
/// from local client installations and installed browsers.
pub struct RobloxScanner {
    client: RobloxClient,
    db: Database,
}

impl RobloxScanner {
    pub fn new(db: Database) -> Self {
        Self {
            client: RobloxClient::new(),
            db,
        }
    }

    /// Primary entry point: scans local system, extracts potential .ROBLOSECURITY cookies,
    /// validates them against Roblox API, and saves newly discovered accounts into SQLite.
    pub async fn scan_and_import(&self) -> Vec<RobloxAccountInfo> {
        info!("Starting automatic Roblox account scan across client and browsers...");
        let mut raw_candidates = Vec::new();

        // 1. Environment variable overrides (great for automation/headless)
        if let Ok(env_cookies) = std::env::var("SWCH_ROBLOX_COOKIES").or_else(|_| std::env::var("SWCH_ROBLOX_COOKIE")) {
            for c in env_cookies.split(';') {
                let trimmed = c.trim();
                if !trimmed.is_empty() {
                    raw_candidates.push(trimmed.to_string());
                }
            }
        }

        // 2. Local Roblox Client cookies
        raw_candidates.extend(self.scan_client_files());

        // 3. Browser cookies (Firefox, Chrome, Edge, Brave, Opera)
        raw_candidates.extend(self.scan_browser_cookies());

        // Deduplicate raw cookie strings
        raw_candidates.sort();
        raw_candidates.dedup();

        info!("Discovered {} candidate Roblox cookie(s). Validating with Roblox API...", raw_candidates.len());
        let mut imported_accounts = Vec::new();

        for cookie in raw_candidates {
            if let Ok(user) = self.client.get_authenticated_user(&cookie).await {
                let user_id = user.id;
                let user_id_str = user_id.to_string();
                let account_id = format!("roblox_{}", user_id_str);

                info!("Valid Roblox session found: @{} ({}) [ID: {}]", user.name, user.display_name, user_id_str);

                // Fetch details
                let robux = self.client.get_user_currency(user_id, &cookie).await.unwrap_or(0);
                let is_premium = self.client.get_user_premium(user_id, &cookie).await.unwrap_or(false);
                let (headshot, avatar) = self.client.get_avatar_thumbnails(user_id).await;
                let (_, rap) = self.client.get_collectibles(user_id, &cookie).await;

                // Sync favorites
                let favorites = self.client.get_favorite_places(user_id, &cookie).await;
                for fav in favorites {
                    let place_id_str = fav.id.to_string();
                    let icon = self.client.get_place_icon(fav.id).await;
                    let place = crate::storage::RobloxPlace {
                        place_id: place_id_str,
                        name: fav.name,
                        description: fav.description,
                        icon_url: icon,
                        creator_name: None,
                        is_manual: false,
                        updated_at: chrono::Utc::now().to_rfc3339(),
                    };
                    let _ = self.db.save_roblox_place(&place).await;
                }

                let existing_acc = self.db.get_roblox_account(&account_id).await.unwrap_or(None);
                let is_active = existing_acc.as_ref().map(|a| a.is_active).unwrap_or(false);

                let info = RobloxAccountInfo {
                    account_id: account_id.clone(),
                    username: user.name,
                    display_name: user.display_name,
                    user_id: user_id_str,
                    cookie: cookie.clone(),
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

                if let Err(e) = self.db.save_roblox_account(&info).await {
                    warn!("Failed to save discovered Roblox account {}: {}", account_id, e);
                } else {
                    imported_accounts.push(info);
                }
            }
        }

        // If no active account is chosen yet and we have imported accounts, set first one active
        let all_current = self.db.get_roblox_accounts().await.unwrap_or_default();
        if !all_current.is_empty() && !all_current.iter().any(|a| a.is_active) {
            let first_id = &all_current[0].account_id;
            let _ = self.db.set_active_roblox_account(first_id).await;
        }

        info!("Roblox account scan completed. Total active/saved accounts: {}", all_current.len());
        imported_accounts
    }

    /// Scans official Roblox client storage locations for cookies.
    fn scan_client_files(&self) -> Vec<String> {
        let mut cookies = Vec::new();

        #[cfg(target_os = "windows")]
        {
            if let Some(local_app) = dirs::data_local_dir() {
                let dat_path = local_app.join("Roblox\\LocalStorage\\RobloxCookies.dat");
                if dat_path.exists() {
                    if let Ok(bytes) = std::fs::read(&dat_path) {
                        cookies.extend(Self::extract_warning_cookies_from_bytes(&bytes));
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Some(home) = dirs::home_dir() {
                let paths = [
                    home.join("Library/Application Support/Roblox/LocalStorage"),
                    home.join("Library/HTTPStorages/com.roblox.RobloxPlayer"),
                    home.join("Library/Cookies/com.roblox.RobloxPlayer.binarycookies"),
                ];
                for p in &paths {
                    if p.exists() {
                        if let Ok(bytes) = std::fs::read(p) {
                            cookies.extend(Self::extract_warning_cookies_from_bytes(&bytes));
                        }
                    }
                }
            }
        }

        cookies
    }

    /// Scans supported web browsers for Roblox session cookies.
    fn scan_browser_cookies(&self) -> Vec<String> {
        let mut cookies = Vec::new();
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return cookies,
        };

        // 1. Firefox profiles (unencrypted SQLite database)
        let firefox_dirs = if cfg!(target_os = "macos") {
            vec![home.join("Library/Application Support/Firefox/Profiles")]
        } else if cfg!(target_os = "windows") {
            if let Some(appdata) = dirs::config_dir() {
                vec![appdata.join("Mozilla\\Firefox\\Profiles")]
            } else {
                vec![]
            }
        } else {
            vec![home.join(".mozilla/firefox")]
        };

        for fdir in firefox_dirs {
            if fdir.exists() {
                for entry in WalkDir::new(fdir).max_depth(3).into_iter().flatten() {
                    if entry.file_name() == "cookies.sqlite" {
                        cookies.extend(Self::read_firefox_cookies(entry.path()));
                    }
                }
            }
        }

        // 2. Chromium-based browsers (Chrome, Edge, Brave, Opera)
        let mut chromium_dirs = Vec::new();

        #[cfg(target_os = "macos")]
        {
            let app_sup = home.join("Library/Application Support");
            chromium_dirs.push(app_sup.join("Google/Chrome"));
            chromium_dirs.push(app_sup.join("Microsoft Edge"));
            chromium_dirs.push(app_sup.join("BraveSoftware/Brave-Browser"));
            chromium_dirs.push(app_sup.join("com.operasoftware.Opera"));
            chromium_dirs.push(app_sup.join("Yandex/YandexBrowser"));
        }

        #[cfg(target_os = "windows")]
        {
            if let Some(local) = dirs::data_local_dir() {
                chromium_dirs.push(local.join("Google\\Chrome\\User Data"));
                chromium_dirs.push(local.join("Microsoft\\Edge\\User Data"));
                chromium_dirs.push(local.join("BraveSoftware\\Brave-Browser\\User Data"));
            }
            if let Some(roaming) = dirs::config_dir() {
                chromium_dirs.push(roaming.join("Opera Software\\Opera Stable"));
            }
        }

        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            let config = home.join(".config");
            chromium_dirs.push(config.join("google-chrome"));
            chromium_dirs.push(config.join("microsoft-edge"));
            chromium_dirs.push(config.join("BraveSoftware/Brave-Browser"));
            chromium_dirs.push(config.join("opera"));
        }

        for cdir in chromium_dirs {
            if cdir.exists() {
                for entry in WalkDir::new(cdir).max_depth(4).into_iter().flatten() {
                    let fname = entry.file_name().to_string_lossy();
                    if fname == "Cookies" || fname == "Cookies-journal" {
                        cookies.extend(Self::scan_file_for_roblox_tokens(entry.path()));
                    }
                }
            }
        }

        cookies
    }

    /// Reads Firefox `cookies.sqlite` file directly using rusqlite.
    fn read_firefox_cookies(path: &Path) -> Vec<String> {
        let mut results = Vec::new();
        // Open read-only
        if let Ok(conn) = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        ) {
            let query = "SELECT value FROM moz_cookies WHERE host LIKE '%roblox.com' AND name = '.ROBLOSECURITY'";
            if let Ok(mut stmt) = conn.prepare(query) {
                if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
                    for r in rows.flatten() {
                        let trimmed = r.trim().to_string();
                        if !trimmed.is_empty() {
                            results.push(trimmed);
                        }
                    }
                }
            }
        }
        results
    }

    /// Scans arbitrary binary files or SQLite storage for the unmistakable
    /// Roblox `.ROBLOSECURITY` signature: `_|WARNING:-DO-NOT-SHARE-THIS...`
    fn scan_file_for_roblox_tokens(path: &Path) -> Vec<String> {
        if let Ok(bytes) = std::fs::read(path) {
            Self::extract_warning_cookies_from_bytes(&bytes)
        } else {
            Vec::new()
        }
    }

    /// Helper that extracts all `_|WARNING:-DO-NOT-SHARE-THIS...` occurrences from byte slices.
    pub fn extract_warning_cookies_from_bytes(bytes: &[u8]) -> Vec<String> {
        let mut found = Vec::new();
        let pattern = b"_|WARNING:-DO-NOT-SHARE-THIS";
        let mut i = 0;

        while i + pattern.len() <= bytes.len() {
            if &bytes[i..i + pattern.len()] == pattern {
                // Find end of token: whitespace, quote, null byte, semicolon or control char
                let mut end = i + pattern.len();
                while end < bytes.len() {
                    let b = bytes[end];
                    if b == 0 || b == b'"' || b == b'\'' || b == b';' || b == b'\r' || b == b'\n' || b < 32 || b > 126 {
                        break;
                    }
                    end += 1;
                }
                if let Ok(cookie_str) = std::str::from_utf8(&bytes[i..end]) {
                    // Valid Roblox cookie is long (>200 chars)
                    if cookie_str.len() > 150 {
                        found.push(cookie_str.to_string());
                    }
                }
                i = end;
            } else {
                i += 1;
            }
        }

        found
    }
}
