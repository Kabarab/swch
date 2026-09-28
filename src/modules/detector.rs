use std::path::PathBuf;
use tracing::debug;

use super::types::ModuleId;

/// Cross-platform detector that inspects whether third-party launchers
/// are installed on the local system.
#[derive(Debug, Clone, Default)]
pub struct LauncherDetector {
    mock_launchers: Option<Vec<String>>,
}

impl LauncherDetector {
    pub fn new() -> Self {
        Self {
            mock_launchers: None,
        }
    }

    pub fn with_mock_launchers(launchers: Vec<String>) -> Self {
        Self {
            mock_launchers: Some(launchers),
        }
    }

    pub fn set_mock_launchers(&mut self, launchers: Vec<String>) {
        self.mock_launchers = Some(launchers);
    }

    /// Checks whether the game launcher associated with `id` is installed.
    pub fn is_installed(&self, id: ModuleId) -> bool {
        self.detect_path(id).is_some()
    }

    /// Resolves the file system path of the launcher if found on the machine.
    pub fn detect_path(&self, id: ModuleId) -> Option<PathBuf> {
        // Direct instance-level mock override (ideal for concurrency-safe testing)
        if let Some(ref mocks) = self.mock_launchers {
            if mocks.iter().any(|m| m.eq_ignore_ascii_case(id.as_str())) {
                return Some(PathBuf::from(format!("/mock/launcher/{}", id.as_str())));
            } else {
                return None;
            }
        }

        // Global override for test runners and forced environments
        if std::env::var("SWCH_FORCE_ALL_MODULES").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
            || std::env::var("SWCH_ENABLE_ALL_MODULES").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
        {
            return Some(PathBuf::from(format!("/mock/launcher/{}", id.as_str())));
        }

        // Mock launcher list (comma-separated, e.g. "steam,epic")
        if let Ok(mock_list) = std::env::var("SWCH_MOCK_LAUNCHERS") {
            let names: Vec<String> = mock_list
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            if names.contains(&id.as_str().to_string()) {
                return Some(PathBuf::from(format!("/mock/launcher/{}", id.as_str())));
            } else {
                return None;
            }
        }

        match id {
            ModuleId::Steam => self.detect_steam(),
            ModuleId::Epic => self.detect_epic(),
            ModuleId::Riot => self.detect_riot(),
            ModuleId::Roblox => self.detect_roblox(),
            ModuleId::Minecraft => self.detect_minecraft(),
        }
    }

    /// Detects Steam client installation.
    pub fn detect_steam(&self) -> Option<PathBuf> {
        // 1. Explicit env override
        if let Ok(p) = std::env::var("SWCH_STEAM_DIR") {
            let path = PathBuf::from(p);
            if path.exists() {
                debug!("Steam detected via SWCH_STEAM_DIR: {}", path.display());
                return Some(path);
            }
        }

        // 2. macOS standard locations
        #[cfg(target_os = "macos")]
        {
            if let Some(home) = dirs::home_dir() {
                let app_support = home.join("Library/Application Support/Steam");
                if app_support.exists() {
                    return Some(app_support);
                }
                let home_app = home.join("Applications/Steam.app");
                if home_app.exists() {
                    return Some(home_app);
                }
            }
            let sys_app = PathBuf::from("/Applications/Steam.app");
            if sys_app.exists() {
                return Some(sys_app);
            }
        }

        // 3. Windows standard locations
        #[cfg(target_os = "windows")]
        {
            // Registry check via command or standard Program Files
            let prog_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
            let path_x86 = PathBuf::from(prog_x86).join("Steam");
            if path_x86.exists() {
                return Some(path_x86);
            }

            let prog = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
            let path = PathBuf::from(prog).join("Steam");
            if path.exists() {
                return Some(path);
            }
        }

        // 4. Linux standard locations
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            if let Some(home) = dirs::home_dir() {
                let p1 = home.join(".steam/steam");
                if p1.exists() {
                    return Some(p1);
                }
                let p2 = home.join(".local/share/Steam");
                if p2.exists() {
                    return Some(p2);
                }
            }
            let sys_bin = PathBuf::from("/usr/bin/steam");
            if sys_bin.exists() {
                return Some(sys_bin);
            }
        }

        None
    }

    /// Detects Epic Games Launcher installation.
    pub fn detect_epic(&self) -> Option<PathBuf> {
        // 1. Explicit env override
        if let Ok(p) = std::env::var("SWCH_EPIC_DIR") {
            let path = PathBuf::from(p);
            if path.exists() {
                debug!("Epic detected via SWCH_EPIC_DIR: {}", path.display());
                return Some(path);
            }
        }

        // 2. macOS standard locations
        #[cfg(target_os = "macos")]
        {
            let sys_app = PathBuf::from("/Applications/Epic Games Launcher.app");
            if sys_app.exists() {
                return Some(sys_app);
            }

            if let Some(home) = dirs::home_dir() {
                let home_app = home.join("Applications/Epic Games Launcher.app");
                if home_app.exists() {
                    return Some(home_app);
                }
                let app_support = home.join("Library/Application Support/Epic/EpicGamesLauncher");
                if app_support.exists() {
                    return Some(app_support);
                }
            }
        }

        // 3. Windows standard locations
        #[cfg(target_os = "windows")]
        {
            let prog_data = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
            let epic_data = PathBuf::from(prog_data).join("Epic\\EpicGamesLauncher");
            if epic_data.exists() {
                return Some(epic_data);
            }

            let prog_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
            let epic_bin = PathBuf::from(prog_x86).join("Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe");
            if epic_bin.exists() {
                return Some(epic_bin);
            }
        }

        // 4. Linux Heroic / Lutris runner fallback
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            if let Some(home) = dirs::home_dir() {
                let heroic = home.join(".config/heroic");
                if heroic.exists() {
                    return Some(heroic);
                }
            }
        }

        None
    }

    /// Detects Riot Games / Riot Client installation.
    pub fn detect_riot(&self) -> Option<PathBuf> {
        // 1. Explicit env override
        if let Ok(p) = std::env::var("SWCH_RIOT_DIR") {
            let path = PathBuf::from(p);
            if path.exists() {
                debug!("Riot detected via SWCH_RIOT_DIR: {}", path.display());
                return Some(path);
            }
        }

        // 2. macOS standard locations
        #[cfg(target_os = "macos")]
        {
            let sys_riot = PathBuf::from("/Applications/Riot Games");
            if sys_riot.exists() {
                return Some(sys_riot);
            }

            let client_app = PathBuf::from("/Applications/Riot Games/Riot Client/RiotClientServices.app");
            if client_app.exists() {
                return Some(client_app);
            }

            let standalone = PathBuf::from("/Applications/Riot Client.app");
            if standalone.exists() {
                return Some(standalone);
            }

            let lol = PathBuf::from("/Applications/League of Legends.app");
            if lol.exists() {
                return Some(lol);
            }

            if let Some(home) = dirs::home_dir() {
                let home_riot = home.join("Applications/Riot Client.app");
                if home_riot.exists() {
                    return Some(home_riot);
                }
            }
        }

        // 3. Windows standard locations
        #[cfg(target_os = "windows")]
        {
            let riot_client = PathBuf::from("C:\\Riot Games\\Riot Client\\RiotClientServices.exe");
            if riot_client.exists() {
                return Some(riot_client);
            }

            let riot_dir = PathBuf::from("C:\\Riot Games");
            if riot_dir.exists() {
                return Some(riot_dir);
            }

            let prog_data = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
            let riot_data = PathBuf::from(prog_data).join("Riot Games");
            if riot_data.exists() {
                return Some(riot_data);
            }
        }

        None
    }

    /// Detects Roblox Player installation.
    pub fn detect_roblox(&self) -> Option<PathBuf> {
        // 1. Explicit env override
        if let Ok(p) = std::env::var("SWCH_ROBLOX_DIR").or_else(|_| std::env::var("SWCH_ROBLOX_CLIENT")) {
            let path = PathBuf::from(p);
            if path.exists() {
                debug!("Roblox detected via env override: {}", path.display());
                return Some(path);
            }
        }

        // 2. macOS standard locations
        #[cfg(target_os = "macos")]
        {
            let sys_app = PathBuf::from("/Applications/Roblox.app");
            if sys_app.exists() {
                return Some(sys_app);
            }

            if let Some(home) = dirs::home_dir() {
                let user_app = home.join("Applications/Roblox.app");
                if user_app.exists() {
                    return Some(user_app);
                }
                let app_support = home.join("Library/Application Support/Roblox");
                if app_support.exists() {
                    return Some(app_support);
                }
            }
        }

        // 3. Windows standard locations
        #[cfg(target_os = "windows")]
        {
            if let Some(local_app) = dirs::data_local_dir() {
                let rbx_dir = local_app.join("Roblox");
                let versions_dir = rbx_dir.join("Versions");
                if versions_dir.exists() {
                    if let Ok(entries) = std::fs::read_dir(&versions_dir) {
                        for entry in entries.flatten() {
                            let p = entry.path().join("RobloxPlayerBeta.exe");
                            if p.exists() {
                                return Some(p);
                            }
                        }
                    }
                }
                let launcher_exe = rbx_dir.join("RobloxPlayerLauncher.exe");
                if launcher_exe.exists() {
                    return Some(launcher_exe);
                }
                if rbx_dir.exists() {
                    return Some(rbx_dir);
                }
            }

            let prog_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
            let rbx_prog = PathBuf::from(prog_x86).join("Roblox");
            if rbx_prog.exists() {
                return Some(rbx_prog);
            }
        }

        // 4. Linux standard locations (e.g. Sober flatpak, Grapejuice, Vinegar)
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            if let Some(home) = dirs::home_dir() {
                let sober = home.join(".var/app/org.vinegarhq.Sober");
                if sober.exists() {
                    return Some(sober);
                }
                let grapejuice = home.join(".local/share/grapejuice");
                if grapejuice.exists() {
                    return Some(grapejuice);
                }
            }
        }

        None
    }

    /// Detects official Minecraft launcher installation.
    pub fn detect_minecraft(&self) -> Option<PathBuf> {
        // 1. Explicit env override
        if let Ok(p) = std::env::var("SWCH_MINECRAFT_LAUNCHER")
            .or_else(|_| std::env::var("SWCH_MINECRAFT_APP"))
            .or_else(|_| std::env::var("SWCH_MINECRAFT_DIR"))
        {
            let path = PathBuf::from(p);
            if path.exists() {
                debug!("Minecraft detected via env override: {}", path.display());
                return Some(path);
            }
        }

        // 2. macOS standard locations
        #[cfg(target_os = "macos")]
        {
            let sys_app = PathBuf::from("/Applications/Minecraft.app");
            if sys_app.exists() {
                return Some(sys_app);
            }

            if let Some(home) = dirs::home_dir() {
                let user_app = home.join("Applications/Minecraft.app");
                if user_app.exists() {
                    return Some(user_app);
                }
                let data_dir = home.join("Library/Application Support/minecraft");
                if data_dir.exists() {
                    return Some(data_dir);
                }
            }
        }

        // 3. Windows standard locations
        #[cfg(target_os = "windows")]
        {
            let prog_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
            let mc_x86 = PathBuf::from(prog_x86).join("Minecraft Launcher\\MinecraftLauncher.exe");
            if mc_x86.exists() {
                return Some(mc_x86);
            }

            let prog = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
            let mc_prog = PathBuf::from(prog).join("Minecraft Launcher\\MinecraftLauncher.exe");
            if mc_prog.exists() {
                return Some(mc_prog);
            }

            if let Some(local_app) = dirs::data_local_dir() {
                let mc_local = local_app.join("Programs\\Launcher\\MinecraftLauncher.exe");
                if mc_local.exists() {
                    return Some(mc_local);
                }
                let ms_store = local_app.join("Packages\\Microsoft.4297127D64C6C_8wekyb3d8bbwe\\LocalCache\\Roaming\\.minecraft");
                if ms_store.exists() {
                    return Some(ms_store);
                }
            }

            if let Some(config_dir) = dirs::config_dir() {
                let dot_mc = config_dir.join(".minecraft");
                if dot_mc.exists() {
                    return Some(dot_mc);
                }
            }
        }

        // 4. Linux standard locations
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            let sys_bin = PathBuf::from("/usr/bin/minecraft-launcher");
            if sys_bin.exists() {
                return Some(sys_bin);
            }
            let opt_bin = PathBuf::from("/opt/minecraft-launcher/minecraft-launcher");
            if opt_bin.exists() {
                return Some(opt_bin);
            }
            if let Some(home) = dirs::home_dir() {
                let local_bin = home.join(".local/share/minecraft-launcher/minecraft-launcher");
                if local_bin.exists() {
                    return Some(local_bin);
                }
                let dot_mc = home.join(".minecraft");
                if dot_mc.exists() {
                    return Some(dot_mc);
                }
            }
        }

        None
    }

    /// Resolves the .minecraft data directory.
    pub fn detect_minecraft_data_dir(&self) -> Option<PathBuf> {
        if let Ok(p) = std::env::var("SWCH_MINECRAFT_DIR").or_else(|_| std::env::var("MINECRAFT_DIR")) {
            let path = PathBuf::from(p);
            if path.exists() {
                return Some(path);
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Some(home) = dirs::home_dir() {
                let path = home.join("Library/Application Support/minecraft");
                if path.exists() {
                    return Some(path);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            if let Some(local_app) = dirs::data_local_dir() {
                let ms_store = local_app.join("Packages\\Microsoft.4297127D64C6C_8wekyb3d8bbwe\\LocalCache\\Roaming\\.minecraft");
                if ms_store.exists() {
                    return Some(ms_store);
                }
            }
            if let Some(config_dir) = dirs::config_dir() {
                let path = config_dir.join(".minecraft");
                if path.exists() {
                    return Some(path);
                }
            }
        }

        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            if let Some(home) = dirs::home_dir() {
                let path = home.join(".minecraft");
                if path.exists() {
                    return Some(path);
                }
            }
        }

        None
    }
}

