use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::info;

use crate::storage::models::{
    Account, AccountRestrictions, ClaimHistory, EpicLibraryGame, Game, GameAccount, GameBanInfo,
    MinecraftAccount, MinecraftProfile, MinecraftServer, MinecraftWorld,
    RobloxAccountInfo, RobloxPlace, VerifiedBanStatus,
};

#[derive(Error, Debug)]
pub enum DbError {
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("Configuration directory not found")]
    ConfigDirNotFound,
}

fn read_verified_bans(conn: &Connection, account_id: &str) -> Result<VerifiedBanStatus, DbError> {
    let json: Option<String> = conn.query_row(
        "SELECT evidence_json FROM verified_account_bans WHERE account_id = ?1",
        [account_id], |row| row.get(0),
    ).optional()?;
    Ok(json.and_then(|value| serde_json::from_str(&value).ok()).unwrap_or_default())
}

/// Thread-safe database handle wrapping a SQLite connection.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    db_path: Option<PathBuf>,
}

impl Database {
    /// Normalized profile data only: credentials never enter this table.
    pub async fn riot_profiles(&self) -> Result<Vec<crate::launchers::riot::profile::Profile>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT payload FROM riot_profiles ORDER BY account_id, app_id")?;
        let rows = stmt.query_map([], |r| r.get::<_,String>(0))?;
        let mut profiles=Vec::new();
        for row in rows { profiles.push(serde_json::from_str(&row?).map_err(|_|DbError::Io("Invalid Riot snapshot".into()))?); }
        Ok(profiles)
    }
    pub async fn save_riot_profile(&self, mut profile: crate::launchers::riot::profile::Profile) -> Result<(), DbError> {
        use rusqlite::OptionalExtension;
        let conn = self.conn.lock().await;
        let old: Option<String> = conn.query_row("SELECT payload FROM riot_profiles WHERE account_id=?1 AND app_id=?2", params![profile.account_id,profile.app_id], |r| r.get(0)).optional()?;
        if let Some(old)=old.and_then(|s|serde_json::from_str(&s).ok()) { profile.retain_failed_sections(old); }
        let payload=serde_json::to_string(&profile).map_err(|_|DbError::Io("Invalid Riot snapshot".into()))?;
        conn.execute("INSERT INTO riot_profiles VALUES (?1,?2,?3) ON CONFLICT(account_id,app_id) DO UPDATE SET payload=excluded.payload",params![profile.account_id,profile.app_id,payload])?;
        Ok(())
    }

    /// Only normalized Steam data is cached; credentials never enter SQLite.
    pub async fn steam_cache_get(&self, key: &str) -> Result<Option<(String, i64)>, DbError> {
        use rusqlite::OptionalExtension;
        let conn = self.conn.lock().await;
        Ok(conn.query_row("SELECT payload, updated_at FROM steam_cache WHERE cache_key = ?1", [key], |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
    }
    pub async fn steam_cache_prefix(&self, prefix: &str) -> Result<Vec<(String, String, i64)>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT cache_key, payload, updated_at FROM steam_cache WHERE substr(cache_key, 1, length(?1)) = ?1")?;
        let rows = stmt.query_map([prefix], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
    pub async fn steam_cache_put(&self, key: &str, payload: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute("INSERT INTO steam_cache VALUES (?1, ?2, ?3) ON CONFLICT(cache_key) DO UPDATE SET payload=excluded.payload, updated_at=excluded.updated_at", params![key, payload, chrono::Utc::now().timestamp()])?;
        Ok(())
    }

    /// Resolves the default database path according to OS standards using `dirs::config_dir()`.
    /// E.g. macOS: ~/Library/Application Support/swch/swch_library.db
    /// Windows: %APPDATA%/swch/swch_library.db
    /// Linux: ~/.config/swch/swch_library.db
    pub fn default_path() -> Result<PathBuf, DbError> {
        if let Ok(env_path) = std::env::var("SWCH_DB_PATH") {
            if !env_path.trim().is_empty() {
                return Ok(PathBuf::from(env_path));
            }
        }

        let base_dir = dirs::config_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".swch")))
            .ok_or(DbError::ConfigDirNotFound)?;

        let swch_dir = if base_dir.ends_with(".swch") {
            base_dir
        } else {
            base_dir.join("swch")
        };

        if !swch_dir.exists() {
            std::fs::create_dir_all(&swch_dir).map_err(|e| {
                DbError::Io(format!(
                    "Failed to create directory {}: {}",
                    swch_dir.display(),
                    e
                ))
            })?;
        }

        Ok(swch_dir.join("swch_library.db"))
    }

    /// Opens or creates the SQLite database at the default OS location.
    pub fn open_default() -> Result<Self, DbError> {
        let path = Self::default_path()?;
        Self::open(path)
    }

    /// Opens or creates the SQLite database at a specific path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, DbError> {
        let path_buf = path.as_ref().to_path_buf();
        if let Some(parent) = path_buf.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    DbError::Io(format!(
                        "Failed to create directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        let conn = Connection::open(&path_buf)?;
        info!("Opened SQLite database at: {}", path_buf.display());
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: Some(path_buf),
        })
    }

    /// Opens an in-memory database (useful for unit and integration testing).
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        info!("Opened in-memory SQLite database");
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: None,
        })
    }

    /// Returns the path to the database file if not in-memory.
    pub fn path(&self) -> Option<&PathBuf> {
        self.db_path.as_ref()
    }

    /// Executes database migrations, creating necessary tables and indexes.
    pub async fn run_migrations(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().await;

        // Configure connection pragmas for reliability and performance
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            "#,
        )?;

        // Run migrations
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS riot_profiles (account_id TEXT NOT NULL, app_id TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(account_id, app_id));
            CREATE TABLE IF NOT EXISTS steam_cache (cache_key TEXT PRIMARY KEY, payload TEXT NOT NULL, updated_at INTEGER NOT NULL);

            -- Games table
            CREATE TABLE IF NOT EXISTS games (
                id TEXT PRIMARY KEY,
                launcher TEXT NOT NULL,
                app_id TEXT NOT NULL,
                name TEXT NOT NULL,
                install_path TEXT,
                executable_path TEXT,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                is_installed INTEGER NOT NULL DEFAULT 0,
                banner_url TEXT,
                launch_uri TEXT,
                namespace TEXT
            );

            -- Accounts table
            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                launcher TEXT NOT NULL,
                username TEXT NOT NULL,
                display_name TEXT,
                email TEXT,
                is_active INTEGER NOT NULL DEFAULT 0,
                last_used TEXT
            );

            -- Game to Account association table
            CREATE TABLE IF NOT EXISTS game_accounts (
                game_id TEXT NOT NULL,
                account_id TEXT NOT NULL,
                launcher TEXT NOT NULL,
                created_at TEXT,
                PRIMARY KEY (game_id, account_id),
                FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
                FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            -- Free games / offers claim history table
            CREATE TABLE IF NOT EXISTS claim_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                offer_id TEXT,
                launcher TEXT NOT NULL,
                game_title TEXT NOT NULL,
                account_id TEXT,
                claimed_at TEXT NOT NULL
            );

            -- Epic credentials store (durable fallback alongside native OS Keyring)
            CREATE TABLE IF NOT EXISTS epic_credentials (
                account_id TEXT PRIMARY KEY,
                device_auth_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            -- Account restrictions and ban tracking
            CREATE TABLE IF NOT EXISTS account_restrictions (
                account_id TEXT PRIMARY KEY,
                launcher TEXT NOT NULL,
                community_banned INTEGER NOT NULL DEFAULT 0,
                trade_banned INTEGER NOT NULL DEFAULT 0,
                market_banned INTEGER NOT NULL DEFAULT 0,
                vac_banned INTEGER NOT NULL DEFAULT 0,
                vac_bans_count INTEGER NOT NULL DEFAULT 0,
                game_bans_count INTEGER NOT NULL DEFAULT 0,
                economy_ban TEXT NOT NULL DEFAULT 'none',
                is_limited INTEGER NOT NULL DEFAULT 0,
                game_bans_json TEXT NOT NULL DEFAULT '[]',
                details TEXT,
                currency TEXT,
                country TEXT,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS verified_account_bans (
                account_id TEXT PRIMARY KEY,
                evidence_json TEXT NOT NULL
            );

            -- Roblox tables
            CREATE TABLE IF NOT EXISTS roblox_credentials (
                account_id TEXT PRIMARY KEY,
                username TEXT NOT NULL,
                display_name TEXT NOT NULL,
                cookie TEXT NOT NULL,
                user_id TEXT NOT NULL,
                is_premium INTEGER NOT NULL DEFAULT 0,
                robux_balance INTEGER NOT NULL DEFAULT 0,
                avatar_url TEXT,
                headshot_url TEXT,
                rap_value INTEGER NOT NULL DEFAULT 0,
                is_banned INTEGER NOT NULL DEFAULT 0,
                last_validated INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS roblox_cache (
                cache_key TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS roblox_places (
                place_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                icon_url TEXT,
                creator_name TEXT,
                is_manual INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS roblox_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            INSERT OR IGNORE INTO roblox_settings (key, value) VALUES ('multi_instance_enabled', 'true');

            -- Minecraft tables
            CREATE TABLE IF NOT EXISTS minecraft_accounts (
                account_id TEXT PRIMARY KEY,
                username TEXT NOT NULL,
                account_type TEXT NOT NULL,
                uuid TEXT NOT NULL,
                avatar_url TEXT,
                skin_url TEXT,
                is_active INTEGER NOT NULL DEFAULT 0,
                access_token TEXT,
                raw_json TEXT,
                last_used TEXT,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS minecraft_profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                profile_type TEXT NOT NULL,
                version_id TEXT NOT NULL,
                icon TEXT,
                game_dir TEXT,
                java_dir TEXT,
                java_args TEXT,
                last_used TEXT,
                last_version_id TEXT,
                created TEXT
            );

            CREATE TABLE IF NOT EXISTS minecraft_worlds (
                folder_name TEXT PRIMARY KEY,
                level_name TEXT NOT NULL,
                last_played INTEGER NOT NULL,
                game_mode TEXT NOT NULL,
                hardcore INTEGER NOT NULL DEFAULT 0,
                version_name TEXT,
                icon_base64 TEXT,
                size_bytes INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS minecraft_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                ip TEXT NOT NULL,
                icon TEXT,
                is_favorite INTEGER NOT NULL DEFAULT 0,
                is_imported INTEGER NOT NULL DEFAULT 0,
                last_played TEXT,
                updated_at TEXT NOT NULL
            );

            -- Query optimization indexes
            CREATE INDEX IF NOT EXISTS idx_games_launcher ON games(launcher);
            CREATE INDEX IF NOT EXISTS idx_games_installed ON games(is_installed);
            CREATE INDEX IF NOT EXISTS idx_accounts_launcher ON accounts(launcher);
            CREATE INDEX IF NOT EXISTS idx_claim_history_account ON claim_history(account_id);
            CREATE INDEX IF NOT EXISTS idx_claim_history_launcher ON claim_history(launcher);
            CREATE INDEX IF NOT EXISTS idx_account_restrictions_launcher ON account_restrictions(launcher);
            CREATE INDEX IF NOT EXISTS idx_roblox_places_manual ON roblox_places(is_manual);
            CREATE INDEX IF NOT EXISTS idx_minecraft_accounts_active ON minecraft_accounts(is_active);
            CREATE INDEX IF NOT EXISTS idx_minecraft_servers_favorite ON minecraft_servers(is_favorite);
            "#,
        )?;

        // Ensure currency and country columns exist in account_restrictions
        let _ = conn.execute("ALTER TABLE account_restrictions ADD COLUMN currency TEXT;", []);
        let _ = conn.execute("ALTER TABLE account_restrictions ADD COLUMN country TEXT;", []);

        // Idempotent migration: ensure 'email' column exists in 'accounts' table for existing databases
        let has_email: bool = {
            let mut stmt = conn.prepare("PRAGMA table_info(accounts)")?;
            let mut rows = stmt.query([])?;
            let mut found = false;
            while let Some(row) = rows.next()? {
                let col_name: String = row.get(1)?;
                if col_name == "email" {
                    found = true;
                    break;
                }
            }
            found
        };

        if !has_email {
            conn.execute("ALTER TABLE accounts ADD COLUMN email TEXT;", [])?;
            info!("Applied migration: Added 'email' column to 'accounts' table");
        }

        // Idempotent migration: ensure 'namespace' column exists in 'games' table
        let has_namespace: bool = {
            let mut stmt = conn.prepare("PRAGMA table_info(games)")?;
            let mut rows = stmt.query([])?;
            let mut found = false;
            while let Some(row) = rows.next()? {
                let col_name: String = row.get(1)?;
                if col_name == "namespace" {
                    found = true;
                    break;
                }
            }
            found
        };

        if !has_namespace {
            conn.execute("ALTER TABLE games ADD COLUMN namespace TEXT;", [])?;
            info!("Applied migration: Added 'namespace' column to 'games' table");
        }

        // Idempotent migration: ensure 'created_at' column exists in 'game_accounts' table
        let has_created_at: bool = {
            let mut stmt = conn.prepare("PRAGMA table_info(game_accounts)")?;
            let mut rows = stmt.query([])?;
            let mut found = false;
            while let Some(row) = rows.next()? {
                let col_name: String = row.get(1)?;
                if col_name == "created_at" {
                    found = true;
                    break;
                }
            }
            found
        };

        if !has_created_at {
            conn.execute("ALTER TABLE game_accounts ADD COLUMN created_at TEXT;", [])?;
            info!("Applied migration: Added 'created_at' column to 'game_accounts' table");
        }

        // Ensure high-performance composite and lookup indexes
        conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_games_app_id ON games(app_id);
            CREATE INDEX IF NOT EXISTS idx_game_accounts_account_id ON game_accounts(account_id);
            "#,
        )?;

        // Older saved Steam Support records predate evidence tracking. Their
        // explicit source label is sufficient to preserve them on upgrade.
        let legacy: Vec<(String, String)> = {
            let mut stmt = conn.prepare("SELECT account_id, game_bans_json FROM account_restrictions WHERE launcher = 'steam'")?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        for (account_id, json) in legacy {
            let confirmed: Vec<GameBanInfo> = serde_json::from_str::<Vec<GameBanInfo>>(&json)
                .unwrap_or_default().into_iter().filter(|ban| ban.ban_type.as_deref()
                    .is_some_and(|kind| kind.contains("Steam Support") || kind.contains("Steam API") || kind.contains("Блокировка Steam API")))
                .collect();
            if !confirmed.is_empty() {
                let evidence = VerifiedBanStatus { game_bans_count: confirmed.len() as u32,
                    game_bans: confirmed, ..Default::default() };
                conn.execute("INSERT OR IGNORE INTO verified_account_bans (account_id, evidence_json) VALUES (?1, ?2)",
                    params![account_id, serde_json::to_string(&evidence).unwrap_or_default()])?;
            }
        }
        info!("SQLite schema migrations applied successfully");
        Ok(())
    }

    /// Quick ping check to verify database connectivity.
    pub async fn ping(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.query_row("SELECT 1", [], |_| Ok(()))?;
        Ok(())
    }

    /// Fetches all games from the database.
    pub async fn get_games(&self) -> Result<Vec<Game>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, launcher, app_id, name, install_path, executable_path,
                   size_bytes, is_installed, banner_url, launch_uri, namespace
            FROM games
            ORDER BY name ASC
            "#,
        )?;

        let game_iter = stmt.query_map([], |row| {
            let is_installed_int: i64 = row.get(7)?;
            Ok(Game {
                id: row.get(0)?,
                launcher: row.get(1)?,
                app_id: row.get(2)?,
                name: row.get(3)?,
                install_path: row.get(4)?,
                executable_path: row.get(5)?,
                size_bytes: row.get(6)?,
                is_installed: is_installed_int != 0,
                banner_url: row.get(8)?,
                launch_uri: row.get(9)?,
                namespace: row.get(10)?,
            })
        })?;

        let mut games = Vec::new();
        for game in game_iter {
            games.push(game?);
        }
        Ok(games)
    }

    /// Inserts or updates a game in the database.
    pub async fn upsert_game(&self, game: &Game) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO games (id, launcher, app_id, name, install_path, executable_path, size_bytes, is_installed, banner_url, launch_uri, namespace)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(id) DO UPDATE SET
                launcher = excluded.launcher,
                app_id = excluded.app_id,
                name = excluded.name,
                install_path = excluded.install_path,
                executable_path = excluded.executable_path,
                size_bytes = excluded.size_bytes,
                is_installed = excluded.is_installed,
                banner_url = excluded.banner_url,
                launch_uri = excluded.launch_uri,
                namespace = excluded.namespace
            "#,
            params![
                game.id,
                game.launcher,
                game.app_id,
                game.name,
                game.install_path,
                game.executable_path,
                game.size_bytes,
                if game.is_installed { 1 } else { 0 },
                game.banner_url,
                game.launch_uri,
                game.namespace
            ],
        )?;
        Ok(())
    }

    /// Fetches all accounts from the database.
    pub async fn get_accounts(&self) -> Result<Vec<Account>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, launcher, username, display_name, email, is_active, last_used
            FROM accounts
            ORDER BY launcher ASC, username ASC
            "#,
        )?;

        let account_iter = stmt.query_map([], |row| {
            let is_active_int: i64 = row.get(5)?;
            Ok(Account {
                id: row.get(0)?,
                launcher: row.get(1)?,
                username: row.get(2)?,
                display_name: row.get(3)?,
                email: row.get(4)?,
                is_active: is_active_int != 0,
                last_used: row.get(6)?,
            })
        })?;

        let mut accounts = Vec::new();
        for account in account_iter {
            accounts.push(account?);
        }
        Ok(accounts)
    }

    /// Inserts or updates an account in the database.
    pub async fn upsert_account(&self, account: &Account) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO accounts (id, launcher, username, display_name, email, is_active, last_used)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                launcher = excluded.launcher,
                username = excluded.username,
                display_name = excluded.display_name,
                email = excluded.email,
                is_active = excluded.is_active,
                last_used = excluded.last_used
            "#,
            params![
                account.id,
                account.launcher,
                account.username,
                account.display_name,
                account.email,
                if account.is_active { 1 } else { 0 },
                account.last_used
            ],
        )?;
        Ok(())
    }

    /// Inserts or updates an Epic account in the database.
    pub async fn upsert_epic_account(&self, account: &Account) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO accounts (id, launcher, username, display_name, email, is_active, last_used)
            VALUES (?1, 'epic', ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                launcher = 'epic',
                username = excluded.username,
                display_name = excluded.display_name,
                email = excluded.email,
                is_active = CASE WHEN accounts.is_active = 1 THEN 1 ELSE excluded.is_active END,
                last_used = excluded.last_used
            "#,
            params![
                account.id,
                account.username,
                account.display_name,
                account.email,
                if account.is_active { 1 } else { 0 },
                account.last_used
            ],
        )?;
        Ok(())
    }

    /// Saves an Epic Games DeviceAuth JSON blob to SQLite as durable fallback.
    pub async fn save_epic_credential(&self, account_id: &str, device_auth_json: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO epic_credentials (account_id, device_auth_json, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(account_id) DO UPDATE SET
                device_auth_json = excluded.device_auth_json,
                updated_at = excluded.updated_at
            "#,
            params![account_id, device_auth_json, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Retrieves the Epic Games DeviceAuth JSON blob from SQLite if available.
    pub async fn get_epic_credential(&self, account_id: &str) -> Result<Option<String>, DbError> {
        use rusqlite::OptionalExtension;
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT device_auth_json FROM epic_credentials WHERE account_id = ?1")?;
        let json: Option<String> = stmt.query_row(params![account_id], |r| r.get(0)).optional()?;
        Ok(json)
    }

    /// Removes an Epic Games DeviceAuth credential blob from SQLite.
    pub async fn delete_epic_credential(&self, account_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM epic_credentials WHERE account_id = ?1", params![account_id])?;
        Ok(())
    }

    /// Fetches all Epic accounts from the database ordered by active status first, then last used.
    pub async fn get_epic_accounts(&self) -> Result<Vec<Account>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, launcher, username, display_name, email, is_active, last_used
            FROM accounts
            WHERE launcher = 'epic'
            ORDER BY is_active DESC, last_used DESC, username ASC
            "#,
        )?;

        let account_iter = stmt.query_map([], |row| {
            let is_active_int: i64 = row.get(5)?;
            Ok(Account {
                id: row.get(0)?,
                launcher: row.get(1)?,
                username: row.get(2)?,
                display_name: row.get(3)?,
                email: row.get(4)?,
                is_active: is_active_int != 0,
                last_used: row.get(6)?,
            })
        })?;

        let mut accounts = Vec::new();
        for account in account_iter {
            accounts.push(account?);
        }
        Ok(accounts)
    }

    /// Saves or updates a Roblox account and its credentials/metadata.
    pub async fn save_roblox_account(&self, info: &RobloxAccountInfo) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        // 1. Upsert into core accounts table
        conn.execute(
            r#"
            INSERT INTO accounts (id, launcher, username, display_name, email, is_active, last_used)
            VALUES (?1, 'roblox', ?2, ?3, NULL, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                username = excluded.username,
                display_name = excluded.display_name,
                is_active = excluded.is_active,
                last_used = excluded.last_used
            "#,
            params![
                info.account_id,
                info.username,
                info.display_name,
                if info.is_active { 1 } else { 0 },
                info.updated_at
            ],
        )?;

        // 2. Upsert into roblox_credentials table
        conn.execute(
            r#"
            INSERT INTO roblox_credentials (
                account_id, username, display_name, cookie, user_id,
                is_premium, robux_balance, avatar_url, headshot_url, rap_value,
                is_banned, last_validated, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(account_id) DO UPDATE SET
                username = excluded.username,
                display_name = excluded.display_name,
                cookie = excluded.cookie,
                user_id = excluded.user_id,
                is_premium = excluded.is_premium,
                robux_balance = excluded.robux_balance,
                avatar_url = excluded.avatar_url,
                headshot_url = excluded.headshot_url,
                rap_value = excluded.rap_value,
                is_banned = excluded.is_banned,
                last_validated = excluded.last_validated,
                updated_at = excluded.updated_at
            "#,
            params![
                info.account_id,
                info.username,
                info.display_name,
                info.cookie,
                info.user_id,
                if info.is_premium { 1 } else { 0 },
                info.robux_balance,
                info.avatar_url,
                info.headshot_url,
                info.rap_value,
                if info.is_banned { 1 } else { 0 },
                info.last_validated,
                info.updated_at
            ],
        )?;

        Ok(())
    }

    /// Retrieves all saved Roblox accounts enriched with active state and metadata.
    pub async fn get_roblox_accounts(&self) -> Result<Vec<RobloxAccountInfo>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT c.account_id, c.username, c.display_name, c.user_id, c.cookie,
                   COALESCE(a.is_active, 0) as is_active,
                   c.is_premium, c.robux_balance, c.avatar_url, c.headshot_url,
                   c.rap_value, c.is_banned, c.last_validated, c.updated_at
            FROM roblox_credentials c
            LEFT JOIN accounts a ON a.id = c.account_id
            ORDER BY is_active DESC, c.updated_at DESC
            "#,
        )?;

        let iter = stmt.query_map([], |r| {
            let is_act: i64 = r.get(5)?;
            let is_prem: i64 = r.get(6)?;
            let is_ban: i64 = r.get(11)?;
            Ok(RobloxAccountInfo {
                account_id: r.get(0)?,
                username: r.get(1)?,
                display_name: r.get(2)?,
                user_id: r.get(3)?,
                cookie: r.get(4)?,
                is_active: is_act != 0,
                is_premium: is_prem != 0,
                robux_balance: r.get(7)?,
                avatar_url: r.get(8)?,
                headshot_url: r.get(9)?,
                rap_value: r.get(10)?,
                is_banned: is_ban != 0,
                last_validated: r.get(12)?,
                updated_at: r.get(13)?,
            })
        })?;

        let mut res = Vec::new();
        for item in iter {
            res.push(item?);
        }
        Ok(res)
    }

    /// Retrieves a single Roblox account by account_id, user_id, or username.
    pub async fn get_roblox_account(&self, target: &str) -> Result<Option<RobloxAccountInfo>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT c.account_id, c.username, c.display_name, c.user_id, c.cookie,
                   COALESCE(a.is_active, 0) as is_active,
                   c.is_premium, c.robux_balance, c.avatar_url, c.headshot_url,
                   c.rap_value, c.is_banned, c.last_validated, c.updated_at
            FROM roblox_credentials c
            LEFT JOIN accounts a ON a.id = c.account_id
            WHERE c.account_id = ?1 OR c.user_id = ?1 OR LOWER(c.username) = LOWER(?1)
            LIMIT 1
            "#,
        )?;

        let opt = stmt.query_row(params![target], |r| {
            let is_act: i64 = r.get(5)?;
            let is_prem: i64 = r.get(6)?;
            let is_ban: i64 = r.get(11)?;
            Ok(RobloxAccountInfo {
                account_id: r.get(0)?,
                username: r.get(1)?,
                display_name: r.get(2)?,
                user_id: r.get(3)?,
                cookie: r.get(4)?,
                is_active: is_act != 0,
                is_premium: is_prem != 0,
                robux_balance: r.get(7)?,
                avatar_url: r.get(8)?,
                headshot_url: r.get(9)?,
                rap_value: r.get(10)?,
                is_banned: is_ban != 0,
                last_validated: r.get(12)?,
                updated_at: r.get(13)?,
            })
        }).optional()?;

        Ok(opt)
    }

    /// Sets the active Roblox account.
    pub async fn set_active_roblox_account(&self, account_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute("UPDATE accounts SET is_active = 0 WHERE launcher = 'roblox'", [])?;
        conn.execute("UPDATE accounts SET is_active = 1, last_used = ?2 WHERE id = ?1", params![account_id, chrono::Utc::now().to_rfc3339()])?;
        Ok(())
    }

    /// Deletes a Roblox account.
    pub async fn delete_roblox_account(&self, account_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM roblox_credentials WHERE account_id = ?1", params![account_id])?;
        conn.execute("DELETE FROM accounts WHERE id = ?1 AND launcher = 'roblox'", params![account_id])?;
        Ok(())
    }

    /// Gets a cached entry from roblox_cache.
    pub async fn roblox_cache_get(&self, key: &str) -> Result<Option<(String, i64)>, DbError> {
        let conn = self.conn.lock().await;
        Ok(conn.query_row(
            "SELECT payload, updated_at FROM roblox_cache WHERE cache_key = ?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?)
    }

    /// Puts a cached entry into roblox_cache.
    pub async fn roblox_cache_put(&self, key: &str, payload: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO roblox_cache VALUES (?1, ?2, ?3)
            ON CONFLICT(cache_key) DO UPDATE SET
                payload = excluded.payload,
                updated_at = excluded.updated_at
            "#,
            params![key, payload, chrono::Utc::now().timestamp()],
        )?;
        Ok(())
    }

    /// Saves or updates a Roblox place.
    pub async fn save_roblox_place(&self, place: &RobloxPlace) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO roblox_places (place_id, name, description, icon_url, creator_name, is_manual, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(place_id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                icon_url = excluded.icon_url,
                creator_name = excluded.creator_name,
                is_manual = CASE WHEN excluded.is_manual = 1 THEN 1 ELSE roblox_places.is_manual END,
                updated_at = excluded.updated_at
            "#,
            params![
                place.place_id,
                place.name,
                place.description,
                place.icon_url,
                place.creator_name,
                if place.is_manual { 1 } else { 0 },
                place.updated_at
            ],
        )?;
        Ok(())
    }

    /// Retrieves all saved Roblox places.
    pub async fn get_roblox_places(&self) -> Result<Vec<RobloxPlace>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT place_id, name, description, icon_url, creator_name, is_manual, updated_at
            FROM roblox_places
            ORDER BY is_manual DESC, updated_at DESC, name ASC
            "#,
        )?;

        let iter = stmt.query_map([], |r| {
            let is_man: i64 = r.get(5)?;
            Ok(RobloxPlace {
                place_id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                icon_url: r.get(3)?,
                creator_name: r.get(4)?,
                is_manual: is_man != 0,
                updated_at: r.get(6)?,
            })
        })?;

        let mut res = Vec::new();
        for item in iter {
            res.push(item?);
        }
        Ok(res)
    }

    /// Deletes a Roblox place.
    pub async fn delete_roblox_place(&self, place_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM roblox_places WHERE place_id = ?1", params![place_id])?;
        Ok(())
    }

    /// Reads a Roblox setting value.
    pub async fn get_roblox_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().await;
        Ok(conn.query_row("SELECT value FROM roblox_settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    /// Writes a Roblox setting value.
    pub async fn set_roblox_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO roblox_settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ==========================================
    // MINECRAFT DATABASE METHODS
    // ==========================================

    /// Saves or updates a Minecraft account and synchronizes with base accounts table.
    pub async fn save_minecraft_account(&self, acc: &MinecraftAccount) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        // 1. Sync to base accounts table (launcher = 'minecraft')
        conn.execute(
            r#"
            INSERT INTO accounts (id, launcher, username, display_name, email, is_active, last_used)
            VALUES (?1, 'minecraft', ?2, ?3, NULL, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                launcher = 'minecraft',
                username = excluded.username,
                display_name = excluded.display_name,
                is_active = excluded.is_active,
                last_used = excluded.last_used
            "#,
            params![
                acc.account_id,
                acc.username,
                acc.username,
                if acc.is_active { 1 } else { 0 },
                acc.last_used,
            ],
        )?;

        // 2. Upsert into minecraft_accounts table
        conn.execute(
            r#"
            INSERT INTO minecraft_accounts (
                account_id, username, account_type, uuid, avatar_url, skin_url,
                is_active, access_token, raw_json, last_used, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(account_id) DO UPDATE SET
                username = excluded.username,
                account_type = excluded.account_type,
                uuid = excluded.uuid,
                avatar_url = excluded.avatar_url,
                skin_url = excluded.skin_url,
                is_active = excluded.is_active,
                access_token = excluded.access_token,
                raw_json = excluded.raw_json,
                last_used = excluded.last_used,
                updated_at = excluded.updated_at
            "#,
            params![
                acc.account_id,
                acc.username,
                acc.account_type,
                acc.uuid,
                acc.avatar_url,
                acc.skin_url,
                if acc.is_active { 1 } else { 0 },
                acc.access_token,
                acc.raw_json,
                acc.last_used,
                acc.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Retrieves all saved Minecraft accounts ordered by active status then username.
    pub async fn get_minecraft_accounts(&self) -> Result<Vec<MinecraftAccount>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT account_id, username, account_type, uuid, avatar_url, skin_url,
                   is_active, access_token, raw_json, last_used, updated_at
            FROM minecraft_accounts
            ORDER BY is_active DESC, username ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            let is_active_int: i64 = row.get(6)?;
            Ok(MinecraftAccount {
                account_id: row.get(0)?,
                username: row.get(1)?,
                account_type: row.get(2)?,
                uuid: row.get(3)?,
                avatar_url: row.get(4)?,
                skin_url: row.get(5)?,
                is_active: is_active_int != 0,
                access_token: row.get(7)?,
                raw_json: row.get(8)?,
                last_used: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Retrieves a single Minecraft account by account_id, username, or player uuid.
    pub async fn get_minecraft_account(&self, target: &str) -> Result<Option<MinecraftAccount>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT account_id, username, account_type, uuid, avatar_url, skin_url,
                   is_active, access_token, raw_json, last_used, updated_at
            FROM minecraft_accounts
            WHERE account_id = ?1 OR username = ?1 OR uuid = ?1
            LIMIT 1
            "#,
        )?;
        let mut rows = stmt.query(params![target])?;
        if let Some(row) = rows.next()? {
            let is_active_int: i64 = row.get(6)?;
            Ok(Some(MinecraftAccount {
                account_id: row.get(0)?,
                username: row.get(1)?,
                account_type: row.get(2)?,
                uuid: row.get(3)?,
                avatar_url: row.get(4)?,
                skin_url: row.get(5)?,
                is_active: is_active_int != 0,
                access_token: row.get(7)?,
                raw_json: row.get(8)?,
                last_used: row.get(9)?,
                updated_at: row.get(10)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Sets the active Minecraft account and synchronizes with base accounts.
    pub async fn set_active_minecraft_account(&self, account_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute("UPDATE minecraft_accounts SET is_active = 0", [])?;
        conn.execute("UPDATE minecraft_accounts SET is_active = 1 WHERE account_id = ?1", params![account_id])?;
        conn.execute("UPDATE accounts SET is_active = 0 WHERE launcher = 'minecraft'", [])?;
        conn.execute("UPDATE accounts SET is_active = 1 WHERE id = ?1 AND launcher = 'minecraft'", params![account_id])?;
        Ok(())
    }

    /// Deletes a Minecraft account from both minecraft_accounts and accounts table.
    pub async fn delete_minecraft_account(&self, account_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM minecraft_accounts WHERE account_id = ?1", params![account_id])?;
        conn.execute("DELETE FROM accounts WHERE id = ?1 AND launcher = 'minecraft'", params![account_id])?;
        Ok(())
    }

    /// Saves or updates a list of Minecraft launcher profiles.
    pub async fn save_minecraft_profiles(&self, profiles: &[MinecraftProfile]) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        for p in profiles {
            conn.execute(
                r#"
                INSERT INTO minecraft_profiles (
                    id, name, profile_type, version_id, icon, game_dir,
                    java_dir, java_args, last_used, last_version_id, created
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    profile_type = excluded.profile_type,
                    version_id = excluded.version_id,
                    icon = excluded.icon,
                    game_dir = excluded.game_dir,
                    java_dir = excluded.java_dir,
                    java_args = excluded.java_args,
                    last_used = excluded.last_used,
                    last_version_id = excluded.last_version_id,
                    created = excluded.created
                "#,
                params![
                    p.id, p.name, p.profile_type, p.version_id, p.icon,
                    p.game_dir, p.java_dir, p.java_args, p.last_used,
                    p.last_version_id, p.created
                ],
            )?;
        }
        Ok(())
    }

    /// Retrieves all saved Minecraft installation profiles.
    pub async fn get_minecraft_profiles(&self) -> Result<Vec<MinecraftProfile>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, profile_type, version_id, icon, game_dir,
                   java_dir, java_args, last_used, last_version_id, created
            FROM minecraft_profiles
            ORDER BY last_used DESC, name ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MinecraftProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                profile_type: row.get(2)?,
                version_id: row.get(3)?,
                icon: row.get(4)?,
                game_dir: row.get(5)?,
                java_dir: row.get(6)?,
                java_args: row.get(7)?,
                last_used: row.get(8)?,
                last_version_id: row.get(9)?,
                created: row.get(10)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Saves or updates a list of Minecraft singleplayer save worlds.
    pub async fn save_minecraft_worlds(&self, worlds: &[MinecraftWorld]) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        for w in worlds {
            conn.execute(
                r#"
                INSERT INTO minecraft_worlds (
                    folder_name, level_name, last_played, game_mode, hardcore,
                    version_name, icon_base64, size_bytes
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(folder_name) DO UPDATE SET
                    level_name = excluded.level_name,
                    last_played = excluded.last_played,
                    game_mode = excluded.game_mode,
                    hardcore = excluded.hardcore,
                    version_name = excluded.version_name,
                    icon_base64 = excluded.icon_base64,
                    size_bytes = excluded.size_bytes
                "#,
                params![
                    w.folder_name, w.level_name, w.last_played, w.game_mode,
                    if w.hardcore { 1 } else { 0 }, w.version_name, w.icon_base64, w.size_bytes
                ],
            )?;
        }
        Ok(())
    }

    /// Retrieves all saved Minecraft worlds ordered by last played timestamp.
    pub async fn get_minecraft_worlds(&self) -> Result<Vec<MinecraftWorld>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT folder_name, level_name, last_played, game_mode, hardcore,
                   version_name, icon_base64, size_bytes
            FROM minecraft_worlds
            ORDER BY last_played DESC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            let hardcore_int: i64 = row.get(4)?;
            Ok(MinecraftWorld {
                folder_name: row.get(0)?,
                level_name: row.get(1)?,
                last_played: row.get(2)?,
                game_mode: row.get(3)?,
                hardcore: hardcore_int != 0,
                version_name: row.get(5)?,
                icon_base64: row.get(6)?,
                size_bytes: row.get(7)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Saves or updates a Minecraft multiplayer server and synchronizes with games library.
    pub async fn save_minecraft_server(&self, server: &MinecraftServer) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO minecraft_servers (
                id, name, ip, icon, is_favorite, is_imported, last_played, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                ip = excluded.ip,
                icon = excluded.icon,
                is_favorite = excluded.is_favorite,
                is_imported = excluded.is_imported,
                last_played = excluded.last_played,
                updated_at = excluded.updated_at
            "#,
            params![
                server.id,
                server.name,
                server.ip,
                server.icon,
                if server.is_favorite { 1 } else { 0 },
                if server.is_imported { 1 } else { 0 },
                server.last_played,
                server.updated_at,
            ],
        )?;

        // Also sync to games catalog as an individual server card
        let game_id = format!("minecraft_server_{}", server.id);
        conn.execute(
            r#"
            INSERT INTO games (
                id, launcher, app_id, name, install_path, executable_path,
                size_bytes, is_installed, banner_url, launch_uri, namespace
            ) VALUES (?1, 'minecraft', ?2, ?3, NULL, NULL, 0, 1, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                launcher = 'minecraft',
                app_id = excluded.app_id,
                name = excluded.name,
                banner_url = excluded.banner_url,
                launch_uri = excluded.launch_uri,
                namespace = excluded.namespace
            "#,
            params![
                game_id,
                server.id,
                server.name,
                server.icon,
                format!("minecraft://connect/{}", server.ip),
                server.ip,
            ],
        )?;
        Ok(())
    }

    /// Retrieves all saved Minecraft servers ordered by favorite status then name.
    pub async fn get_minecraft_servers(&self) -> Result<Vec<MinecraftServer>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, ip, icon, is_favorite, is_imported, last_played, updated_at
            FROM minecraft_servers
            ORDER BY is_favorite DESC, name ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            let fav: i64 = row.get(4)?;
            let imp: i64 = row.get(5)?;
            Ok(MinecraftServer {
                id: row.get(0)?,
                name: row.get(1)?,
                ip: row.get(2)?,
                icon: row.get(3)?,
                is_favorite: fav != 0,
                is_imported: imp != 0,
                last_played: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Deletes a Minecraft server from both minecraft_servers and games table.
    pub async fn delete_minecraft_server(&self, server_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM minecraft_servers WHERE id = ?1", params![server_id])?;
        let game_id = format!("minecraft_server_{}", server_id);
        conn.execute("DELETE FROM games WHERE id = ?1 OR id = ?2", params![server_id, game_id])?;
        Ok(())
    }

    /// Fetches the currently active Epic account, if any.
    pub async fn get_active_epic_account(&self) -> Result<Option<Account>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, launcher, username, display_name, email, is_active, last_used
            FROM accounts
            WHERE launcher = 'epic' AND is_active = 1
            LIMIT 1
            "#,
        )?;

        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let is_active_int: i64 = row.get(5)?;
            Ok(Some(Account {
                id: row.get(0)?,
                launcher: row.get(1)?,
                username: row.get(2)?,
                display_name: row.get(3)?,
                email: row.get(4)?,
                is_active: is_active_int != 0,
                last_used: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Sets the specified Epic account as active and all other Epic accounts as inactive.
    /// Executes in a single statement for atomic zero-latency switching (< 2ms).
    pub async fn set_active_epic_account(&self, account_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().await;

        let exists_val: i64 = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE (id = ?1 OR username = ?1) AND launcher = 'epic')",
            params![account_id],
            |row| row.get(0),
        )?;
        let exists = exists_val != 0;

        if !exists {
            return Err(DbError::Io(format!("Epic account '{}' not found in database", account_id)));
        }

        conn.execute(
            r#"
            UPDATE accounts
            SET is_active = CASE WHEN (id = ?1 OR username = ?1) THEN 1 ELSE 0 END,
                last_used = CASE WHEN (id = ?1 OR username = ?1) THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE last_used END
            WHERE launcher = 'epic'
            "#,
            params![account_id],
        )?;
        Ok(())
    }

    /// Deletes an account from the database by ID (or username) and launcher.
    /// Foreign key cascades remove associated entries in game_accounts.
    /// Returns true if a record was matched and deleted.
    pub async fn delete_account(&self, account_id: &str, launcher: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().await;
        let rows = conn.execute(
            "DELETE FROM accounts WHERE (id = ?1 OR username = ?1) AND launcher = ?2",
            params![account_id, launcher],
        )?;
        Ok(rows > 0)
    }

    /// Deletes an Epic account from the database by ID or username.
    pub async fn delete_epic_account(&self, account_id: &str) -> Result<bool, DbError> {
        self.delete_account(account_id, "epic").await
    }

    /// Fetches restrictions for a specific account.
    pub async fn get_account_restrictions(&self, account_id: &str) -> Result<Option<AccountRestrictions>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT account_id, launcher, community_banned, trade_banned, market_banned,
                   vac_banned, vac_bans_count, game_bans_count, economy_ban, is_limited,
                   game_bans_json, details, currency, country, updated_at
            FROM account_restrictions
            WHERE account_id = ?1
            "#,
        )?;

        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            let game_bans_json: String = row.get(10)?;
            let game_bans: Vec<GameBanInfo> = serde_json::from_str(&game_bans_json).unwrap_or_default();
            let mut restrictions = AccountRestrictions {
                account_id: row.get(0)?,
                launcher: row.get(1)?,
                community_banned: row.get::<_, i64>(2)? != 0,
                trade_banned: row.get::<_, i64>(3)? != 0,
                market_banned: row.get::<_, i64>(4)? != 0,
                vac_banned: row.get::<_, i64>(5)? != 0,
                vac_bans_count: row.get::<_, i64>(6)? as u32,
                game_bans_count: row.get::<_, i64>(7)? as u32,
                economy_ban: row.get(8)?,
                is_limited: row.get::<_, i64>(9)? != 0,
                game_bans,
                verified: VerifiedBanStatus::default(),
                details: row.get(11)?,
                currency: row.get(12)?,
                country: row.get(13)?,
                updated_at: row.get(14)?,
            };
            restrictions.verified = read_verified_bans(&conn, account_id)?;
            restrictions.verified.clone().apply_to(&mut restrictions);
            Ok(Some(restrictions))
        } else {
            Ok(None)
        }
    }

    /// Fetches all account restrictions.
    pub async fn get_all_account_restrictions(&self) -> Result<Vec<AccountRestrictions>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT account_id, launcher, community_banned, trade_banned, market_banned,
                   vac_banned, vac_bans_count, game_bans_count, economy_ban, is_limited,
                   game_bans_json, details, currency, country, updated_at
            FROM account_restrictions
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            let game_bans_json: String = row.get(10)?;
            let game_bans: Vec<GameBanInfo> = serde_json::from_str(&game_bans_json).unwrap_or_default();
            Ok(AccountRestrictions {
                account_id: row.get(0)?,
                launcher: row.get(1)?,
                community_banned: row.get::<_, i64>(2)? != 0,
                trade_banned: row.get::<_, i64>(3)? != 0,
                market_banned: row.get::<_, i64>(4)? != 0,
                vac_banned: row.get::<_, i64>(5)? != 0,
                vac_bans_count: row.get::<_, i64>(6)? as u32,
                game_bans_count: row.get::<_, i64>(7)? as u32,
                economy_ban: row.get(8)?,
                is_limited: row.get::<_, i64>(9)? != 0,
                game_bans,
                verified: VerifiedBanStatus::default(),
                details: row.get(11)?,
                currency: row.get(12)?,
                country: row.get(13)?,
                updated_at: row.get(14)?,
            })
        })?;

        let mut list = Vec::new();
        for item in rows {
            list.push(item?);
        }
        for item in &mut list {
            item.verified = read_verified_bans(&conn, &item.account_id)?;
            item.verified.clone().apply_to(item);
        }
        Ok(list)
    }

    pub async fn get_verified_account_bans(&self, account_id: &str) -> Result<VerifiedBanStatus, DbError> {
        let conn = self.conn.lock().await;
        read_verified_bans(&conn, account_id)
    }

    pub async fn upsert_verified_account_bans(&self, account_id: &str, verified: &VerifiedBanStatus) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        let json = serde_json::to_string(verified).unwrap_or_default();
        conn.execute("INSERT INTO verified_account_bans (account_id, evidence_json) VALUES (?1, ?2) ON CONFLICT(account_id) DO UPDATE SET evidence_json = excluded.evidence_json", params![account_id, json])?;
        Ok(())
    }

    /// Upserts account restrictions.
    pub async fn upsert_account_restrictions(&self, r: &AccountRestrictions) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        let mut r = r.clone();
        read_verified_bans(&conn, &r.account_id)?.apply_to(&mut r);
        let game_bans_json = serde_json::to_string(&r.game_bans).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            r#"
            INSERT INTO account_restrictions (
                account_id, launcher, community_banned, trade_banned, market_banned,
                vac_banned, vac_bans_count, game_bans_count, economy_ban, is_limited,
                game_bans_json, details, currency, country, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(account_id) DO UPDATE SET
                launcher = excluded.launcher,
                community_banned = excluded.community_banned,
                trade_banned = excluded.trade_banned,
                market_banned = excluded.market_banned,
                vac_banned = excluded.vac_banned,
                vac_bans_count = excluded.vac_bans_count,
                game_bans_count = excluded.game_bans_count,
                economy_ban = excluded.economy_ban,
                is_limited = excluded.is_limited,
                game_bans_json = excluded.game_bans_json,
                details = excluded.details,
                currency = excluded.currency,
                country = excluded.country,
                updated_at = excluded.updated_at
            "#,
            params![
                r.account_id,
                r.launcher,
                r.community_banned as i64,
                r.trade_banned as i64,
                r.market_banned as i64,
                r.vac_banned as i64,
                r.vac_bans_count as i64,
                r.game_bans_count as i64,
                r.economy_ban,
                r.is_limited as i64,
                game_bans_json,
                r.details,
                r.currency,
                r.country,
                r.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Deletes account restrictions by account ID.
    pub async fn delete_account_restrictions(&self, account_id: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM verified_account_bans WHERE account_id = ?1", [account_id])?;
        let rows = conn.execute(
            "DELETE FROM account_restrictions WHERE account_id = ?1",
            params![account_id],
        )?;
        Ok(rows > 0)
    }

    /// Associates an account with a game.
    pub async fn link_game_account(&self, link: &GameAccount) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO game_accounts (game_id, account_id, launcher)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(game_id, account_id) DO UPDATE SET
                launcher = excluded.launcher
            "#,
            params![link.game_id, link.account_id, link.launcher],
        )?;
        Ok(())
    }

    /// Fetches all game-account links.
    pub async fn get_game_accounts(&self) -> Result<Vec<GameAccount>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT game_id, account_id, launcher FROM game_accounts")?;
        let link_iter = stmt.query_map([], |row| {
            Ok(GameAccount {
                game_id: row.get(0)?,
                account_id: row.get(1)?,
                launcher: row.get(2)?,
            })
        })?;

        let mut links = Vec::new();
        for link in link_iter {
            links.push(link?);
        }
        Ok(links)
    }

    /// Records a claimed game offer in claim_history.
    pub async fn record_claim(&self, claim: &ClaimHistory) -> Result<i64, DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO claim_history (offer_id, launcher, game_title, account_id, claimed_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                claim.offer_id,
                claim.launcher,
                claim.game_title,
                claim.account_id,
                claim.claimed_at
            ],
        )?;
        let id = conn.last_insert_rowid();
        Ok(id)
    }

    /// Fetches the claim history.
    pub async fn get_claim_history(&self) -> Result<Vec<ClaimHistory>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, offer_id, launcher, game_title, account_id, claimed_at
            FROM claim_history
            ORDER BY claimed_at DESC
            "#,
        )?;

        let claim_iter = stmt.query_map([], |row| {
            Ok(ClaimHistory {
                id: Some(row.get(0)?),
                offer_id: row.get(1)?,
                launcher: row.get(2)?,
                game_title: row.get(3)?,
                account_id: row.get(4)?,
                claimed_at: row.get(5)?,
            })
        })?;

        let mut claims = Vec::new();
        for claim in claim_iter {
            claims.push(claim?);
        }
        Ok(claims)
    }

    /// Inserts or updates an Epic game in the database.
    pub async fn upsert_epic_game(&self, game: &Game) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO games (id, launcher, app_id, name, install_path, executable_path, size_bytes, is_installed, banner_url, launch_uri)
            VALUES (?1, 'epic', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                launcher = 'epic',
                app_id = excluded.app_id,
                name = excluded.name,
                install_path = excluded.install_path,
                executable_path = excluded.executable_path,
                size_bytes = excluded.size_bytes,
                is_installed = excluded.is_installed,
                banner_url = excluded.banner_url,
                launch_uri = excluded.launch_uri
            "#,
            params![
                game.id,
                game.app_id,
                game.name,
                game.install_path,
                game.executable_path,
                game.size_bytes,
                if game.is_installed { 1 } else { 0 },
                game.banner_url,
                game.launch_uri,
            ],
        )?;
        Ok(())
    }

    /// Associates an account with a game using individual IDs.
    pub async fn link_game_account_simple(&self, game_id: &str, account_id: &str) -> Result<(), DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO game_accounts (game_id, account_id, launcher, created_at)
            VALUES (?1, ?2, 'epic', ?3)
            ON CONFLICT(game_id, account_id) DO UPDATE SET
                launcher = 'epic',
                created_at = COALESCE(game_accounts.created_at, excluded.created_at)
            "#,
            params![game_id, account_id, now],
        )?;
        Ok(())
    }

    /// Fetches the aggregated Epic library, optionally filtered by a specific account ID.
    /// If account_id is None, defaults to games owned by the currently active Epic account.
    pub async fn get_epic_library(&self, account_id: Option<&str>) -> Result<Vec<EpicLibraryGame>, DbError> {
        let conn = self.conn.lock().await;

        let (sql, filter_id) = match account_id {
            Some(id) if !id.trim().is_empty() => {
                let query_sql = r#"
                    SELECT g.id, g.app_id, COALESCE(g.namespace, ''), g.name,
                           g.install_path, g.executable_path, g.size_bytes,
                           g.is_installed, g.banner_url, g.launch_uri, ga.account_id
                    FROM games g
                    INNER JOIN game_accounts ga ON g.id = ga.game_id
                    WHERE g.launcher = 'epic' AND (ga.account_id = ?1 OR ga.account_id = ('acc_epic_' || ?1) OR ('acc_epic_' || ga.account_id) = ?1)
                    ORDER BY g.name ASC
                "#;
                (query_sql, Some(id.to_string()))
            }
            _ => {
                let query_sql = r#"
                    SELECT g.id, g.app_id, COALESCE(g.namespace, ''), g.name,
                           g.install_path, g.executable_path, g.size_bytes,
                           g.is_installed, g.banner_url, g.launch_uri, ga.account_id
                    FROM games g
                    INNER JOIN game_accounts ga ON g.id = ga.game_id
                    LEFT JOIN accounts a ON (a.id = ga.account_id OR ('acc_epic_' || a.id) = ga.account_id OR a.id = ('acc_epic_' || ga.account_id))
                    WHERE g.launcher = 'epic' AND (a.is_active = 1 OR NOT EXISTS (SELECT 1 FROM accounts WHERE launcher = 'epic' AND is_active = 1))
                    ORDER BY g.name ASC
                "#;
                (query_sql, None)
            }
        };

        let mut stmt = conn.prepare(sql)?;
        let row_mapper = |row: &rusqlite::Row| {
            let is_installed_int: i64 = row.get(7)?;
            let app_id: String = row.get(1)?;
            let id: String = row.get(0)?;
            let catalog_item_id = if app_id.len() == 32 && app_id.chars().all(|c| c.is_ascii_hexdigit()) {
                app_id.clone()
            } else {
                id.strip_prefix("epic_").unwrap_or(&id).to_string()
            };

            Ok(EpicLibraryGame {
                id,
                app_name: app_id,
                catalog_item_id,
                namespace: row.get(2)?,
                title: row.get(3)?,
                install_path: row.get(4)?,
                executable_path: row.get(5)?,
                size_bytes: row.get(6)?,
                is_installed: is_installed_int != 0,
                banner_url: row.get(8)?,
                launch_uri: row.get(9)?,
                account_id: row.get(10)?,
            })
        };

        let games_iter = if let Some(ref id) = filter_id {
            stmt.query_map(params![id], row_mapper)?
        } else {
            stmt.query_map([], row_mapper)?
        };

        let mut games = Vec::new();
        for game in games_iter {
            games.push(game?);
        }
        Ok(games)
    }

    /// Atomically executes batch upsert of games and game_accounts links in a single SQLite WAL transaction.
    pub async fn sync_epic_library_batch(
        &self,
        games: &[Game],
        links: &[GameAccount],
    ) -> Result<(), DbError> {
        let mut conn = self.conn.lock().await;
        let tx = conn.transaction()?;

        // 1. Clear removed ownership links for the affected accounts (preserve existing to keep created_at)
        let mut affected_accounts = std::collections::HashSet::new();
        for link in links {
            affected_accounts.insert(link.account_id.clone());
        }
        for acc_id in &affected_accounts {
            let current_game_ids: Vec<&str> = links
                .iter()
                .filter(|l| &l.account_id == acc_id)
                .map(|l| l.game_id.as_str())
                .collect();
            if current_game_ids.is_empty() {
                tx.execute(
                    "DELETE FROM game_accounts WHERE (account_id = ?1 OR account_id = ('acc_epic_' || ?1) OR ('acc_epic_' || account_id) = ?1) AND launcher = 'epic'",
                    params![acc_id],
                )?;
            } else {
                let placeholders = current_game_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let query = format!(
                    "DELETE FROM game_accounts WHERE (account_id = ?1 OR account_id = ('acc_epic_' || ?1) OR ('acc_epic_' || account_id) = ?1) AND launcher = 'epic' AND game_id NOT IN ({})",
                    placeholders
                );
                let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::new();
                params_vec.push(acc_id);
                for gid in &current_game_ids {
                    params_vec.push(gid);
                }
                tx.execute(&query, rusqlite::params_from_iter(params_vec))?;
            }
        }

        // 2. Remove orphan epic games that no longer have any active account links
        tx.execute(
            "DELETE FROM games WHERE launcher = 'epic' AND id NOT IN (SELECT game_id FROM game_accounts WHERE launcher = 'epic')",
            [],
        )?;

        // 3. Upsert newly resolved games
        {
            let mut upsert_stmt = tx.prepare_cached(
                r#"
                INSERT INTO games (id, launcher, app_id, name, install_path, executable_path, size_bytes, is_installed, banner_url, launch_uri, namespace)
                VALUES (?1, 'epic', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(id) DO UPDATE SET
                    launcher = 'epic',
                    app_id = excluded.app_id,
                    name = excluded.name,
                    install_path = excluded.install_path,
                    executable_path = excluded.executable_path,
                    size_bytes = excluded.size_bytes,
                    is_installed = excluded.is_installed,
                    banner_url = excluded.banner_url,
                    launch_uri = excluded.launch_uri,
                    namespace = excluded.namespace
                "#,
            )?;

            for game in games {
                upsert_stmt.execute(params![
                    game.id,
                    game.app_id,
                    game.name,
                    game.install_path,
                    game.executable_path,
                    game.size_bytes,
                    if game.is_installed { 1 } else { 0 },
                    game.banner_url,
                    game.launch_uri,
                    game.namespace,
                ])?;
            }
        }

        // 4. Insert game_accounts links
        {
            let mut link_stmt = tx.prepare_cached(
                r#"
                INSERT INTO game_accounts (game_id, account_id, launcher, created_at)
                VALUES (?1, ?2, 'epic', ?3)
                ON CONFLICT(game_id, account_id) DO UPDATE SET
                    launcher = 'epic',
                    created_at = COALESCE(game_accounts.created_at, excluded.created_at)
                "#,
            )?;

            let now = chrono::Utc::now().to_rfc3339();
            for link in links {
                link_stmt.execute(params![
                    link.game_id,
                    link.account_id,
                    now,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_db_in_memory_migrations() {
        let db = Database::open_in_memory().expect("Failed to open in-memory DB");
        db.run_migrations().await.expect("Failed to run migrations");
        db.ping().await.expect("DB ping failed");

        // Verify tables exist
        let conn = db.conn.lock().await;
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert!(tables.contains(&"games".to_string()));
        assert!(tables.contains(&"accounts".to_string()));
        assert!(tables.contains(&"game_accounts".to_string()));
        assert!(tables.contains(&"claim_history".to_string()));
    }

    #[tokio::test]
    async fn test_game_crud() {
        let db = Database::open_in_memory().expect("Failed to open in-memory DB");
        db.run_migrations().await.expect("Failed to run migrations");

        let game = Game {
            id: "steam_570".to_string(),
            launcher: "steam".to_string(),
            app_id: "570".to_string(),
            name: "Dota 2".to_string(),
            install_path: Some("/Games/Steam/steamapps/common/dota 2 beta".to_string()),
            executable_path: Some(
                "/Games/Steam/steamapps/common/dota 2 beta/game/bin/osx/dota2".to_string(),
            ),
            size_bytes: 45_000_000_000,
            is_installed: true,
            banner_url: Some(
                "https://cdn.akamai.steamstatic.com/steam/apps/570/header.jpg".to_string(),
            ),
            launch_uri: Some("steam://run/570".to_string()),
            namespace: None,
        };

        db.upsert_game(&game).await.expect("Failed to insert game");

        let games = db.get_games().await.expect("Failed to fetch games");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0], game);

        // Update game to not installed
        let mut updated_game = game.clone();
        updated_game.is_installed = false;
        db.upsert_game(&updated_game)
            .await
            .expect("Failed to update game");

        let games_updated = db.get_games().await.expect("Failed to fetch updated games");
        assert_eq!(games_updated.len(), 1);
        assert!(!games_updated[0].is_installed);
    }

    #[tokio::test]
    async fn test_account_and_relations() {
        let db = Database::open_in_memory().expect("Failed to open in-memory DB");
        db.run_migrations().await.expect("Failed to run migrations");

        let account = Account {
            id: "acc_steam_1".to_string(),
            launcher: "steam".to_string(),
            username: "jedi_knight".to_string(),
            display_name: Some("Luke Skywalker".to_string()),
            email: None,
            is_active: true,
            last_used: Some("2026-09-13T10:00:00Z".to_string()),
        };

        db.upsert_account(&account)
            .await
            .expect("Failed to insert account");

        let accounts = db.get_accounts().await.expect("Failed to fetch accounts");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0], account);

        // Game relation
        let game = Game {
            id: "steam_10".to_string(),
            launcher: "steam".to_string(),
            app_id: "10".to_string(),
            name: "Counter-Strike".to_string(),
            install_path: None,
            executable_path: None,
            size_bytes: 500_000_000,
            is_installed: false,
            banner_url: None,
            launch_uri: None,
            namespace: None,
        };
        db.upsert_game(&game).await.expect("Failed to insert game");

        let link = GameAccount {
            game_id: "steam_10".to_string(),
            account_id: "acc_steam_1".to_string(),
            launcher: "steam".to_string(),
        };
        db.link_game_account(&link)
            .await
            .expect("Failed to link game account");

        let links = db
            .get_game_accounts()
            .await
            .expect("Failed to fetch game accounts");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], link);
    }

    #[tokio::test]
    async fn test_claim_history() {
        let db = Database::open_in_memory().expect("Failed to open in-memory DB");
        db.run_migrations().await.expect("Failed to run migrations");

        let claim = ClaimHistory {
            id: None,
            offer_id: Some("epic_weekly_123".to_string()),
            launcher: "epic".to_string(),
            game_title: "Death Stranding".to_string(),
            account_id: Some("acc_epic_main".to_string()),
            claimed_at: "2026-09-13T11:00:00Z".to_string(),
        };

        let claim_id = db
            .record_claim(&claim)
            .await
            .expect("Failed to record claim");
        assert!(claim_id > 0);

        let history = db
            .get_claim_history()
            .await
            .expect("Failed to get claim history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].game_title, "Death Stranding");
        assert_eq!(history[0].id, Some(claim_id));
    }

    #[test]
    fn test_default_path_resolution() {
        let path_res = Database::default_path();
        assert!(path_res.is_ok());
        let path = path_res.unwrap();
        assert!(path.to_string_lossy().contains("swch"));
        assert!(path.ends_with("swch_library.db"));
    }

    #[tokio::test]
    async fn test_email_column_migration() {
        // Create in-memory connection with old schema lacking 'email' column
        let db = Database::open_in_memory().expect("Failed to open DB");
        {
            let conn = db.conn.lock().await;
            conn.execute_batch(
                r#"
                CREATE TABLE accounts (
                    id TEXT PRIMARY KEY,
                    launcher TEXT NOT NULL,
                    username TEXT NOT NULL,
                    display_name TEXT,
                    is_active INTEGER NOT NULL DEFAULT 0,
                    last_used TEXT
                );
                INSERT INTO accounts (id, launcher, username, display_name, is_active, last_used)
                VALUES ('acc_legacy', 'epic', 'legacy_user', 'Legacy User', 1, '2026-09-01T00:00:00Z');
                "#,
            )
            .expect("Failed to create legacy table");
        }

        // Run migrations which should detect missing 'email' and add it via ALTER TABLE
        db.run_migrations().await.expect("Migration failed");

        // Verify existing account survived with email = None
        let accounts = db.get_accounts().await.expect("Failed to get accounts");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "acc_legacy");
        assert_eq!(accounts[0].email, None);

        // Verify we can insert an account with email
        let new_acc = Account {
            id: "acc_new".to_string(),
            launcher: "epic".to_string(),
            username: "new_user".to_string(),
            display_name: Some("New User".to_string()),
            email: Some("new_user@epicgames.com".to_string()),
            is_active: false,
            last_used: None,
        };
        db.upsert_account(&new_acc).await.expect("Failed to insert");
        let accounts = db.get_accounts().await.expect("Failed to get accounts");
        assert_eq!(accounts.len(), 2);
        let found = accounts.into_iter().find(|a| a.id == "acc_new").unwrap();
        assert_eq!(found.email.as_deref(), Some("new_user@epicgames.com"));
    }

    #[tokio::test]
    async fn test_epic_account_crud_and_switching() {
        let db = Database::open_in_memory().expect("Failed to open DB");
        db.run_migrations().await.expect("Failed to run migrations");

        assert!(db.get_active_epic_account().await.unwrap().is_none());
        assert!(db.get_epic_accounts().await.unwrap().is_empty());

        let acc1 = Account {
            id: "epic_1".to_string(),
            launcher: "epic".to_string(),
            username: "acc1_user".to_string(),
            display_name: Some("Account One".to_string()),
            email: Some("one@epic.com".to_string()),
            is_active: true,
            last_used: Some("2026-09-13T10:00:00Z".to_string()),
        };
        let acc2 = Account {
            id: "epic_2".to_string(),
            launcher: "epic".to_string(),
            username: "acc2_user".to_string(),
            display_name: Some("Account Two".to_string()),
            email: Some("two@epic.com".to_string()),
            is_active: false,
            last_used: Some("2026-09-12T10:00:00Z".to_string()),
        };

        db.upsert_epic_account(&acc1).await.unwrap();
        db.upsert_epic_account(&acc2).await.unwrap();

        let epic_accs = db.get_epic_accounts().await.unwrap();
        assert_eq!(epic_accs.len(), 2);
        // Active account should be ordered first
        assert_eq!(epic_accs[0].id, "epic_1");

        let active = db.get_active_epic_account().await.unwrap();
        assert!(active.is_some());
        assert_eq!(active.unwrap().id, "epic_1");

        // Switch active to epic_2
        db.set_active_epic_account("epic_2").await.unwrap();

        let active_switched = db.get_active_epic_account().await.unwrap().unwrap();
        assert_eq!(active_switched.id, "epic_2");
        assert!(active_switched.is_active);

        // Verify acc1 is now inactive
        let epic_accs_after = db.get_epic_accounts().await.unwrap();
        let acc1_after = epic_accs_after.into_iter().find(|a| a.id == "epic_1").unwrap();
        assert!(!acc1_after.is_active);

        // Switching to non-existent account returns error
        assert!(db.set_active_epic_account("non_existent").await.is_err());
    }

    #[tokio::test]
    async fn test_account_restrictions() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().await.unwrap();

        let acc_id = "76561198809112858";
        assert!(db.get_account_restrictions(acc_id).await.unwrap().is_none());

        let restrictions = AccountRestrictions {
            account_id: acc_id.to_string(),
            launcher: "steam".to_string(),
            community_banned: false,
            trade_banned: true,
            market_banned: true,
            vac_banned: true,
            vac_bans_count: 1,
            game_bans_count: 0,
            economy_ban: "banned".to_string(),
            is_limited: true,
            game_bans: vec![
                GameBanInfo {
                    app_id: "730".to_string(),
                    name: "Counter-Strike 2".to_string(),
                    ban_type: Some("VAC".to_string()),
                },
            ],
            verified: VerifiedBanStatus::default(),
            details: Some("Market and Trade restricted".to_string()),
            currency: Some("BRL".to_string()),
            country: Some("BR".to_string()),
            updated_at: "2026-09-17T10:00:00Z".to_string(),
        };

        db.upsert_account_restrictions(&restrictions).await.unwrap();

        let fetched = db.get_account_restrictions(acc_id).await.unwrap().expect("should find restrictions");
        assert_eq!(fetched.account_id, acc_id);
        assert!(fetched.trade_banned);
        assert!(fetched.market_banned);
        assert!(fetched.vac_banned);
        assert_eq!(fetched.currency.as_deref(), Some("BRL"));
        assert_eq!(fetched.country.as_deref(), Some("BR"));
        assert_eq!(fetched.game_bans.len(), 1);
        assert_eq!(fetched.game_bans[0].app_id, "730");
        assert_eq!(fetched.game_bans[0].name, "Counter-Strike 2");

        let all = db.get_all_account_restrictions().await.unwrap();
        assert_eq!(all.len(), 1);

        // Update restrictions
        let mut updated = fetched.clone();
        updated.trade_banned = false;
        updated.game_bans.push(GameBanInfo {
            app_id: "252490".to_string(),
            name: "Rust".to_string(),
            ban_type: Some("Game Ban".to_string()),
        });
        db.upsert_account_restrictions(&updated).await.unwrap();

        let fetched_updated = db.get_account_restrictions(acc_id).await.unwrap().unwrap();
        assert!(!fetched_updated.trade_banned);
        assert_eq!(fetched_updated.game_bans.len(), 2);

        // Delete restrictions
        assert!(db.delete_account_restrictions(acc_id).await.unwrap());
        assert!(db.get_account_restrictions(acc_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn confirmed_bans_survive_user_edits_but_manual_bans_can_be_removed() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().await.unwrap();
        let id = "76561197960265729";
        let confirmed = GameBanInfo { app_id: "730".into(), name: "Counter-Strike 2".into(),
            ban_type: Some("Steam Support".into()) };
        let manual = GameBanInfo { app_id: "570".into(), name: "Dota 2".into(),
            ban_type: Some("Game Ban".into()) };
        let evidence = VerifiedBanStatus { vac_bans_count: 1, game_bans_count: 1,
            game_bans: vec![confirmed.clone()], ..Default::default() };
        db.upsert_verified_account_bans(id, &evidence).await.unwrap();
        let initial = AccountRestrictions { account_id: id.into(), vac_banned: true,
            vac_bans_count: 1, game_bans_count: 2,
            game_bans: vec![confirmed.clone(), manual], market_banned: true, ..Default::default() };
        db.upsert_account_restrictions(&initial).await.unwrap();
        let attempted_clear = AccountRestrictions { account_id: id.into(),
            game_bans: vec![GameBanInfo { name: "Altered".into(), ..confirmed.clone() }],
            ..Default::default() };
        db.upsert_account_restrictions(&attempted_clear).await.unwrap();
        let saved = db.get_account_restrictions(id).await.unwrap().unwrap();
        assert!(saved.vac_banned);
        assert_eq!(saved.vac_bans_count, 1);
        assert_eq!(saved.game_bans_count, 1);
        assert_eq!(saved.game_bans, vec![confirmed]);
        assert!(!saved.market_banned);
        assert_eq!(saved.verified, evidence);
    }
}
