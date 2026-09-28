use serde::{Deserialize, Serialize};

/// Represents a game in the library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub launcher: String,
    pub app_id: String,
    pub name: String,
    pub install_path: Option<String>,
    pub executable_path: Option<String>,
    pub size_bytes: i64,
    pub is_installed: bool,
    pub banner_url: Option<String>,
    pub launch_uri: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Represents a launcher user account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub launcher: String,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub is_active: bool,
    pub last_used: Option<String>,
}

/// Represents the association between a game and an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameAccount {
    pub game_id: String,
    pub account_id: String,
    pub launcher: String,
}

/// Record of a free game claim or redemption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimHistory {
    pub id: Option<i64>,
    pub offer_id: Option<String>,
    pub launcher: String,
    pub game_title: String,
    pub account_id: Option<String>,
    pub claimed_at: String,
}

/// Represents an aggregated Epic Games library entry enriched with ownership and installation metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpicLibraryGame {
    pub id: String,
    pub app_name: String,
    pub catalog_item_id: String,
    pub namespace: String,
    pub title: String,
    pub install_path: Option<String>,
    pub executable_path: Option<String>,
    pub is_installed: bool,
    pub size_bytes: i64,
    pub banner_url: Option<String>,
    pub launch_uri: Option<String>,
    pub account_id: String,
}

/// Information about a ban in a specific game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GameBanInfo {
    pub app_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ban_type: Option<String>,
}

/// Ban evidence obtained from Steam, kept separately from user-entered labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VerifiedBanStatus {
    #[serde(default)] pub vac_bans_count: u32,
    #[serde(default)] pub game_bans_count: u32,
    #[serde(default)] pub community_banned: bool,
    #[serde(default)] pub trade_banned: bool,
    #[serde(default)] pub market_banned: bool,
    #[serde(default)] pub is_limited: bool,
    #[serde(default)] pub game_bans: Vec<GameBanInfo>,
}

impl VerifiedBanStatus {
    pub fn absorb(&mut self, observed: &AccountRestrictions) {
        if observed.vac_banned { self.vac_bans_count = self.vac_bans_count.max(observed.vac_bans_count.max(1)); }
        self.game_bans_count = self.game_bans_count.max(observed.game_bans_count);
        self.community_banned |= observed.community_banned;
        self.trade_banned |= observed.trade_banned || observed.economy_ban != "none";
        self.market_banned |= observed.market_banned;
        self.is_limited |= observed.is_limited;
        for ban in &observed.game_bans {
            if !self.game_bans.iter().any(|known| known.app_id == ban.app_id) {
                self.game_bans.push(ban.clone());
            }
        }
        self.game_bans_count = self.game_bans_count.max(self.game_bans.len() as u32);
    }

    pub fn apply_to(&self, restrictions: &mut AccountRestrictions) {
        if self.vac_bans_count > 0 {
            restrictions.vac_banned = true;
            restrictions.vac_bans_count = restrictions.vac_bans_count.max(self.vac_bans_count);
        }
        restrictions.game_bans_count = restrictions.game_bans_count.max(self.game_bans_count);
        restrictions.community_banned |= self.community_banned;
        restrictions.trade_banned |= self.trade_banned;
        restrictions.market_banned |= self.market_banned;
        restrictions.is_limited |= self.is_limited;
        if self.trade_banned && restrictions.economy_ban == "none" { restrictions.economy_ban = "banned".into(); }
        for ban in &self.game_bans {
            restrictions.game_bans.retain(|entry| entry.app_id != ban.app_id);
            restrictions.game_bans.push(ban.clone());
        }
        restrictions.game_bans_count = restrictions.game_bans_count.max(restrictions.game_bans.len() as u32);
        restrictions.verified = self.clone();
    }
}

fn default_updated_at() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn default_launcher() -> String {
    "steam".to_string()
}

/// Restrictions and ban status associated with an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRestrictions {
    pub account_id: String,
    #[serde(default = "default_launcher")]
    pub launcher: String,
    #[serde(default)]
    pub community_banned: bool,
    #[serde(default)]
    pub trade_banned: bool,
    #[serde(default)]
    pub market_banned: bool,
    #[serde(default)]
    pub vac_banned: bool,
    #[serde(default)]
    pub vac_bans_count: u32,
    #[serde(default)]
    pub game_bans_count: u32,
    #[serde(default = "default_economy_ban")]
    pub economy_ban: String,
    #[serde(default)]
    pub is_limited: bool,
    #[serde(default)]
    pub game_bans: Vec<GameBanInfo>,
    #[serde(default, skip_deserializing)]
    pub verified: VerifiedBanStatus,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default = "default_updated_at")]
    pub updated_at: String,
}

fn default_economy_ban() -> String {
    "none".to_string()
}

impl Default for AccountRestrictions {
    fn default() -> Self {
        Self {
            account_id: String::new(),
            launcher: "steam".to_string(),
            community_banned: false,
            trade_banned: false,
            market_banned: false,
            vac_banned: false,
            vac_bans_count: 0,
            game_bans_count: 0,
            economy_ban: "none".to_string(),
            is_limited: false,
            game_bans: Vec::new(),
            verified: VerifiedBanStatus::default(),
            details: None,
            currency: None,
            country: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Detailed account metadata and cached credentials for Roblox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RobloxAccountInfo {
    pub account_id: String,
    pub username: String,
    pub display_name: String,
    pub user_id: String,
    pub cookie: String,
    pub is_active: bool,
    pub is_premium: bool,
    pub robux_balance: i64,
    pub avatar_url: Option<String>,
    pub headshot_url: Option<String>,
    pub rap_value: i64,
    pub is_banned: bool,
    pub last_validated: i64,
    pub updated_at: String,
}

/// Represents a Roblox Place (experience) in the library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RobloxPlace {
    pub place_id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub creator_name: Option<String>,
    pub is_manual: bool,
    pub updated_at: String,
}

/// Account representation in Minecraft official launcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftAccount {
    pub account_id: String,
    pub username: String,
    pub account_type: String, // "msa", "mojang", "offline"
    pub uuid: String,
    pub avatar_url: Option<String>,
    pub skin_url: Option<String>,
    pub is_active: bool,
    pub access_token: Option<String>,
    pub raw_json: Option<String>,
    pub last_used: Option<String>,
    pub updated_at: String,
}

/// Installation profile from launcher_profiles.json (Fabric, Forge, Vanilla, Snapshots, etc.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftProfile {
    pub id: String,
    pub name: String,
    pub profile_type: String,
    pub version_id: String,
    pub icon: Option<String>,
    pub game_dir: Option<String>,
    pub java_dir: Option<String>,
    pub java_args: Option<String>,
    pub last_used: Option<String>,
    pub last_version_id: Option<String>,
    pub created: Option<String>,
}

/// Singleplayer world saved in .minecraft/saves/
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftWorld {
    pub folder_name: String,
    pub level_name: String,
    pub last_played: i64,
    pub game_mode: String,
    pub hardcore: bool,
    pub version_name: Option<String>,
    pub icon_base64: Option<String>,
    pub size_bytes: i64,
}

/// Minecraft multiplayer server from servers.dat or custom added
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftServer {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub icon: Option<String>,
    pub is_favorite: bool,
    pub is_imported: bool,
    pub last_played: Option<String>,
    pub updated_at: String,
}

