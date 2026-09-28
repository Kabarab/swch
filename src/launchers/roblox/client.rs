use reqwest::header::COOKIE;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RobloxUser {
    pub id: i64,
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyResponse {
    pub robux: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailData {
    #[serde(rename = "targetId")]
    pub target_id: i64,
    pub state: String,
    #[serde(rename = "imageUrl")]
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailResponse {
    pub data: Vec<ThumbnailData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectibleItem {
    #[serde(rename = "userAssetId")]
    pub user_asset_id: Option<i64>,
    #[serde(rename = "assetId")]
    pub asset_id: i64,
    pub name: String,
    #[serde(rename = "recentAveragePrice")]
    pub recent_average_price: Option<i64>,
    #[serde(rename = "originalPrice")]
    pub original_price: Option<i64>,
    #[serde(rename = "assetStock")]
    pub asset_stock: Option<i64>,
    pub serial_number: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiblesResponse {
    pub data: Vec<CollectibleItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WearingResponse {
    #[serde(rename = "assetIds")]
    pub asset_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoriteGameItem {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "rootPlace")]
    pub root_place: Option<RootPlace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootPlace {
    pub id: i64,
    #[serde(rename = "type")]
    pub place_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoriteGamesResponse {
    pub data: Vec<FavoriteGameItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceDetail {
    #[serde(rename = "placeId")]
    pub place_id: i64,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "builder")]
    pub builder: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RobloxClient {
    http: reqwest::Client,
    csrf_token: Arc<RwLock<Option<String>>>,
}

impl Default for RobloxClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RobloxClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(12))
            .build()
            .unwrap_or_default();
        Self {
            http,
            csrf_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Formats cookie into header standard. Strips warnings or whitespace if needed.
    pub fn format_cookie(cookie: &str) -> String {
        let trimmed = cookie.trim();
        if trimmed.starts_with(".ROBLOSECURITY=") {
            trimmed.to_string()
        } else {
            format!(".ROBLOSECURITY={}", trimmed)
        }
    }

    /// Validates and extracts current user information from a .ROBLOSECURITY cookie.
    pub async fn get_authenticated_user(&self, cookie: &str) -> Result<RobloxUser, String> {
        let cookie_header = Self::format_cookie(cookie);
        let resp = self
            .http
            .get("https://users.roblox.com/v1/users/authenticated")
            .header(COOKIE, cookie_header)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Roblox auth failed with status {}", resp.status()));
        }

        let user: RobloxUser = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse user JSON: {}", e))?;

        Ok(user)
    }

    /// Retrieves Robux balance for the given user.
    pub async fn get_user_currency(&self, user_id: i64, cookie: &str) -> Result<i64, String> {
        let cookie_header = Self::format_cookie(cookie);
        let url = format!("https://economy.roblox.com/v1/users/{}/currency", user_id);
        let resp = self
            .http
            .get(&url)
            .header(COOKIE, cookie_header)
            .send()
            .await
            .map_err(|e| format!("Currency network error: {}", e))?;

        if !resp.status().is_success() {
            return Ok(0);
        }

        let curr: CurrencyResponse = resp.json().await.unwrap_or(CurrencyResponse { robux: 0 });
        Ok(curr.robux)
    }

    /// Checks if user has an active Roblox Premium membership.
    pub async fn get_user_premium(&self, user_id: i64, cookie: &str) -> Result<bool, String> {
        let cookie_header = Self::format_cookie(cookie);
        let url = format!(
            "https://premiumfeatures.roblox.com/v1/users/{}/validate-membership",
            user_id
        );
        let resp = self
            .http
            .get(&url)
            .header(COOKIE, cookie_header)
            .send()
            .await
            .map_err(|e| format!("Premium network error: {}", e))?;

        if resp.status().is_success() {
            let is_prem: bool = resp.json().await.unwrap_or(false);
            Ok(is_prem)
        } else {
            Ok(false)
        }
    }

    /// Fetches headshot avatar URL and full-body avatar URL.
    pub async fn get_avatar_thumbnails(&self, user_id: i64) -> (Option<String>, Option<String>) {
        let headshot_url = format!(
            "https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds={}&size=150x150&format=Png&isCircular=false",
            user_id
        );
        let full_url = format!(
            "https://thumbnails.roblox.com/v1/users/avatar?userIds={}&size=420x420&format=Png&isCircular=false",
            user_id
        );

        let headshot = match self.http.get(&headshot_url).send().await {
            Ok(res) if res.status().is_success() => {
                if let Ok(data) = res.json::<ThumbnailResponse>().await {
                    data.data.into_iter().next().and_then(|t| t.image_url)
                } else {
                    None
                }
            }
            _ => None,
        };

        let full = match self.http.get(&full_url).send().await {
            Ok(res) if res.status().is_success() => {
                if let Ok(data) = res.json::<ThumbnailResponse>().await {
                    data.data.into_iter().next().and_then(|t| t.image_url)
                } else {
                    None
                }
            }
            _ => None,
        };

        (headshot, full)
    }

    /// Retrieves favorite games/places of the user.
    pub async fn get_favorite_places(&self, user_id: i64, cookie: &str) -> Vec<FavoriteGameItem> {
        let cookie_header = Self::format_cookie(cookie);
        let url = format!(
            "https://games.roblox.com/v2/users/{}/favorite/games?limit=50",
            user_id
        );
        let resp = self
            .http
            .get(&url)
            .header(COOKIE, cookie_header)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                r.json::<FavoriteGamesResponse>().await.map(|d| d.data).unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// Fetches place metadata (title, description, builder) by placeId.
    pub async fn get_place_details(&self, place_id: i64) -> Result<PlaceDetail, String> {
        let url = format!(
            "https://games.roblox.com/v1/games/multiget-place-details?placeIds={}",
            place_id
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Place detail error: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Place details request returned status {}", resp.status()));
        }

        let items: Vec<PlaceDetail> = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse place details: {}", e))?;

        items
            .into_iter()
            .next()
            .ok_or_else(|| format!("Place ID {} not found", place_id))
    }

    /// Fetches place icon image URL by placeId.
    pub async fn get_place_icon(&self, place_id: i64) -> Option<String> {
        let url = format!(
            "https://thumbnails.roblox.com/v1/places/gameicons?placeIds={}&size=150x150&format=Png",
            place_id
        );
        match self.http.get(&url).send().await {
            Ok(r) if r.status().is_success() => {
                if let Ok(data) = r.json::<ThumbnailResponse>().await {
                    data.data.into_iter().next().and_then(|t| t.image_url)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Retrieves list of asset IDs currently worn by the user.
    pub async fn get_currently_wearing(&self, user_id: i64) -> Vec<i64> {
        let url = format!(
            "https://avatar.roblox.com/v1/users/{}/currently-wearing",
            user_id
        );
        match self.http.get(&url).send().await {
            Ok(r) if r.status().is_success() => {
                r.json::<WearingResponse>().await.map(|d| d.asset_ids).unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// Retrieves collectibles (limiteds) owned by user and calculates RAP.
    pub async fn get_collectibles(&self, user_id: i64, cookie: &str) -> (Vec<CollectibleItem>, i64) {
        let cookie_header = Self::format_cookie(cookie);
        let url = format!(
            "https://inventory.roblox.com/v1/users/{}/assets/collectibles?limit=100",
            user_id
        );
        let resp = self
            .http
            .get(&url)
            .header(COOKIE, cookie_header)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let items = r
                    .json::<CollectiblesResponse>()
                    .await
                    .map(|d| d.data)
                    .unwrap_or_default();
                let total_rap: i64 = items
                    .iter()
                    .map(|i| i.recent_average_price.unwrap_or(0))
                    .sum();
                (items, total_rap)
            }
            _ => (Vec::new(), 0),
        }
    }

    /// Generates a single-use authentication ticket used to launch the Roblox Player
    /// seamlessly as this authenticated user.
    pub async fn generate_auth_ticket(&self, cookie: &str) -> Result<String, String> {
        let cookie_header = Self::format_cookie(cookie);
        let url = "https://auth.roblox.com/v1/authentication-ticket";

        // Try with cached CSRF token first
        let current_csrf = self.csrf_token.read().await.clone();
        let mut req = self
            .http
            .post(url)
            .header(COOKIE, &cookie_header)
            .header("Referer", "https://www.roblox.com/")
            .header("Content-Type", "application/json");

        if let Some(ref csrf) = current_csrf {
            req = req.header("x-csrf-token", csrf);
        }

        let resp = req.send().await.map_err(|e| format!("Ticket request error: {}", e))?;

        if resp.status().as_u16() == 403 {
            // CSRF challenge received
            if let Some(new_csrf) = resp.headers().get("x-csrf-token").and_then(|v| v.to_str().ok()) {
                let token_str = new_csrf.to_string();
                {
                    let mut lock = self.csrf_token.write().await;
                    *lock = Some(token_str.clone());
                }

                // Retry with new CSRF token
                let retry_resp = self
                    .http
                    .post(url)
                    .header(COOKIE, &cookie_header)
                    .header("Referer", "https://www.roblox.com/")
                    .header("x-csrf-token", &token_str)
                    .header("Content-Type", "application/json")
                    .send()
                    .await
                    .map_err(|e| format!("Ticket retry error: {}", e))?;

                if retry_resp.status().is_success() {
                    if let Some(ticket) = retry_resp.headers().get("rbx-authentication-ticket").and_then(|v| v.to_str().ok()) {
                        return Ok(ticket.to_string());
                    }
                    if let Ok(val) = retry_resp.json::<serde_json::Value>().await {
                        if let Some(t) = val.get("ticket").and_then(|v| v.as_str()) {
                            return Ok(t.to_string());
                        }
                    }
                    return Err("Auth ticket missing in successful response".into());
                } else {
                    let status = retry_resp.status();
                    return Err(format!("Failed to obtain auth ticket after CSRF refresh (status {})", status));
                }
            }
        }

        if resp.status().is_success() {
            if let Some(ticket) = resp.headers().get("rbx-authentication-ticket").and_then(|v| v.to_str().ok()) {
                return Ok(ticket.to_string());
            }
            if let Ok(val) = resp.json::<serde_json::Value>().await {
                if let Some(t) = val.get("ticket").and_then(|v| v.as_str()) {
                    return Ok(t.to_string());
                }
            }
            return Err("Auth ticket missing in successful response".into());
        }

        let status = resp.status();
        Err(format!("Roblox returned status {} when generating authentication ticket", status))
    }
}
