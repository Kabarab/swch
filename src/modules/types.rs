use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Unique identifier for supported game launcher modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleId {
    Steam,
    Epic,
    Riot,
    Roblox,
    Minecraft,
    Ubisoft,
    Gog,
    Hoyoverse,
}

impl ModuleId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Epic => "epic",
            Self::Riot => "riot",
            Self::Roblox => "roblox",
            Self::Minecraft => "minecraft",
            Self::Ubisoft => "ubisoft",
            Self::Gog => "gog",
            Self::Hoyoverse => "hoyoverse",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Epic => "Epic Games",
            Self::Riot => "Riot Games",
            Self::Roblox => "Roblox",
            Self::Minecraft => "Minecraft",
            Self::Ubisoft => "Ubisoft Connect",
            Self::Gog => "GOG GALAXY",
            Self::Hoyoverse => "HoYoverse",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Steam => "Интеграция с библиотекой, инвентарем и аккаунтами Steam",
            Self::Epic => "Интеграция с играми, авторизацией и аккаунтами Epic Games Store",
            Self::Riot => "Интеграция с профилями Riot Games (League of Legends, VALORANT)",
            Self::Roblox => "Интеграция с клиентом, мульти-аккаунтами и плейсами Roblox",
            Self::Minecraft => "Интеграция с официальным лаунчером Minecraft, профилями, мирами и серверами",
            Self::Ubisoft => "Установленные игры и переключение сохранённых сессий Ubisoft Connect",
            Self::Gog => "Онлайн-библиотека, аккаунты GOG и запуск установленных игр через GALAXY",
            Self::Hoyoverse => "Быстрая смена аккаунтов и библиотека игр miHoYo / HoYoverse (Genshin Impact, Honkai: Star Rail, ZZZ, Honkai Impact 3rd)",
        }
    }

    pub fn all() -> &'static [ModuleId] {
        &[Self::Steam, Self::Epic, Self::Riot, Self::Roblox, Self::Minecraft, Self::Ubisoft, Self::Gog, Self::Hoyoverse]
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ModuleId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "steam" => Ok(Self::Steam),
            "epic" | "epicgames" | "epic_games" => Ok(Self::Epic),
            "riot" | "riotgames" | "riot_games" => Ok(Self::Riot),
            "roblox" | "rbx" => Ok(Self::Roblox),
            "minecraft" | "mc" => Ok(Self::Minecraft),
            "ubisoft" | "ubi" | "uplay" | "ubisoft_connect" => Ok(Self::Ubisoft),
            "gog" | "galaxy" | "gog_galaxy" => Ok(Self::Gog),
            "hoyoverse" | "mihoyo" | "hoyo" | "hoyoplay" => Ok(Self::Hoyoverse),
            _ => Err(format!("Unknown module id: {}", s)),
        }
    }
}

/// Lifecycle status of a launcher module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "detail", rename_all = "snake_case")]
pub enum ModuleStatus {
    /// External launcher is not detected on the computer; module is not installed (0 MB RAM).
    NotDetected,
    /// External launcher is detected on PC, but module is not yet installed or downloaded.
    DetectedNotInstalled,
    /// Module package or binary is currently being downloaded/prepared.
    Installing { progress_pct: u8, message: String },
    /// Module is fully installed and active in the launcher.
    Active,
    /// Module was manually disabled by the user.
    Disabled,
    /// Error encountered during initialization or download.
    Error { message: String },
}

impl ModuleStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    pub fn as_status_str(&self) -> &'static str {
        match self {
            Self::NotDetected => "not_detected",
            Self::DetectedNotInstalled => "detected_not_installed",
            Self::Installing { .. } => "installing",
            Self::Active => "active",
            Self::Disabled => "disabled",
            Self::Error { .. } => "error",
        }
    }
}

/// Information packet describing a module to the frontend and API clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub launcher_detected: bool,
    pub launcher_path: Option<String>,
    pub module_installed: bool,
    pub active: bool,
    pub status: String,
    pub status_detail: Option<String>,
    pub ram_bytes: u64,
    pub games_count: usize,
    pub accounts_count: usize,
    pub module_dir: Option<String>,
    pub version: String,
}

/// Errors occurring in the modular system.
#[derive(Error, Debug)]
pub enum ModuleError {
    #[error("Module not found: {0}")]
    NotFound(String),

    #[error("Launcher '{0}' is not installed on this system")]
    LauncherNotInstalled(String),

    #[error("Module download failed: {0}")]
    DownloadFailed(String),

    #[error("I/O error in module storage: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error in module: {0}")]
    Database(String),

    #[error("Module operation failed: {0}")]
    OperationFailed(String),
}
