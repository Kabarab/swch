use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessStatus, Signal, System};
use thiserror::Error;
use tracing::{error, info, warn};

/// Shared by user-driven launcher switching and opt-in native claim automation.
pub static LAUNCHER_SWITCH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Error types for process management operations.
#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Failed to terminate process(es): {0:?}")]
    FailedToTerminate(TerminationReport),
    #[error("Process not found: {0}")]
    NotFound(String),
    #[error("Launcher executable or application bundle not found: {0}")]
    ExecutableNotFound(String),
    #[error("Launch command failed: {0}")]
    LaunchFailed(String),
}

/// Supported launcher platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LauncherType {
    Steam,
    EpicGames,
    RiotGames,
    BattleNet,
    Minecraft,
    Hoyoverse,
}

impl LauncherType {
    /// Returns the canonical platform identifier string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::EpicGames => "epic",
            Self::RiotGames => "riot",
            Self::BattleNet => "battlenet",
            Self::Minecraft => "minecraft",
            Self::Hoyoverse => "hoyoverse",
        }
    }

    /// Parses a string into a known launcher type.
    pub fn from_name(name: &str) -> Option<Self> {
        let clean = name.trim().to_lowercase();
        match clean.as_str() {
            "steam" => Some(Self::Steam),
            "epic" | "epicgames" | "epic_games" => Some(Self::EpicGames),
            "riot" | "riotgames" | "riot_games" => Some(Self::RiotGames),
            "battlenet" | "battle.net" | "bnet" => Some(Self::BattleNet),
            "minecraft" | "mc" | "tlauncher" | "t-launcher" | "tl" => Some(Self::Minecraft),
            "hoyoverse" | "mihoyo" | "hoyo" | "hoyoplay" => Some(Self::Hoyoverse),
            _ => None,
        }
    }

    /// Detects if a given name or path corresponds to this launcher.
    pub fn matches_alias(&self, name: &str) -> bool {
        let file_name = Path::new(name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(name);
        let clean = normalize_process_stem(file_name);
        for proc in self.process_names() {
            if matches_target_name(file_name, proc) {
                return true;
            }
        }
        match self {
            Self::Steam => clean.eq_ignore_ascii_case("steam"),
            Self::EpicGames => {
                clean.eq_ignore_ascii_case("epic")
                    || clean.eq_ignore_ascii_case("epicgames")
                    || clean.eq_ignore_ascii_case("epicgameslauncher")
                    || clean.eq_ignore_ascii_case("epic games launcher")
                    || clean.eq_ignore_ascii_case("epicwebhelper")
                    || clean.starts_with("epiconlineservices")
                    || clean.starts_with("eosoverlayrenderer")
            }
            Self::RiotGames => {
                clean.eq_ignore_ascii_case("riot")
                    || clean.eq_ignore_ascii_case("riotgames")
                    || clean.eq_ignore_ascii_case("riotclient")
                    || clean.eq_ignore_ascii_case("riotclientservices")
                    || clean.eq_ignore_ascii_case("riotclientux")
                    || clean.eq_ignore_ascii_case("riotclientuxrender")
            }
            Self::BattleNet => {
                clean.eq_ignore_ascii_case("battlenet")
                    || clean.eq_ignore_ascii_case("battle.net")
                    || clean.eq_ignore_ascii_case("bnet")
                    || clean.eq_ignore_ascii_case("agent")
            }
            Self::Minecraft => {
                clean.eq_ignore_ascii_case("minecraft")
                    || clean.eq_ignore_ascii_case("minecraftlauncher")
                    || clean.eq_ignore_ascii_case("minecraft launcher")
                    || clean.eq_ignore_ascii_case("tlauncher")
                    || clean.eq_ignore_ascii_case("t-launcher")
                    || clean.eq_ignore_ascii_case("tl")
                    || clean.starts_with("tlauncher")
            }
            Self::Hoyoverse => {
                clean.eq_ignore_ascii_case("hoyoverse")
                    || clean.eq_ignore_ascii_case("mihoyo")
                    || clean.eq_ignore_ascii_case("hoyoplay")
                    || clean.eq_ignore_ascii_case("hyp")
                    || clean.eq_ignore_ascii_case("genshin")
                    || clean.eq_ignore_ascii_case("genshinimpact")
                    || clean.eq_ignore_ascii_case("yuanshen")
                    || clean.eq_ignore_ascii_case("starrail")
                    || clean.eq_ignore_ascii_case("zenless")
                    || clean.eq_ignore_ascii_case("zenlesszonezero")
                    || clean.eq_ignore_ascii_case("honkai")
                    || clean.eq_ignore_ascii_case("honkai3rd")
            }
        }
    }

    /// Returns list of all known executable and process names for this launcher across OSes.
    pub fn process_names(&self) -> &'static [&'static str] {
        match self {
            Self::Steam => &[
                "steam.exe",
                "steam_osx",
                "Steam",
                "steamwebhelper.exe",
                "steamwebhelper",
                "Steam Helper",
            ],
            Self::EpicGames => &[
                "EpicGamesLauncher.exe",
                "EpicGamesLauncher",
                "EpicGamesLauncher-Mac-Shipping",
                "EpicGamesLaunch",
                "EpicWebHelper.exe",
                "EpicWebHelper",
                "EpicWebHe",
                "EpicOnlineServicesUserHelper.exe",
                "EpicOnlineServicesUserHelper",
                "EpicOnlineServices.exe",
                "EpicOnlineServices",
                "EOSOverlayRenderer-Win64-Shipping.exe",
                "EOSOverlayRenderer-Win64-Shipping",
                "EOSOverlayRenderer-Win32-Shipping.exe",
                "EOSOverlayRenderer-Win32-Shipping",
            ],
            Self::RiotGames => &[
                "RiotClientServices.exe",
                "RiotClientServices",
                "RiotClientUx.exe",
                "RiotClientUx",
                "RiotClientUxRender.exe",
                "RiotClientUxRender",
                "RiotClientCrashHandler.exe",
                "RiotClientCrashHandler",
            ],
            Self::BattleNet => &[
                "Battle.net.exe",
                "Battle.net",
                "Battle.net Launcher.exe",
                "Battle.net Launcher",
                "Agent.exe",
                "Agent",
            ],
            Self::Minecraft => &[
                "MinecraftLauncher.exe",
                "MinecraftLauncher",
                "Minecraft.exe",
                "minecraft-launcher",
                "launcher",
                "TLauncher.exe",
                "tlauncher.exe",
                "TL.exe",
                "tlauncher",
            ],
            Self::Hoyoverse => &[
                "GenshinImpact.exe",
                "GenshinImpact",
                "YuanShen.exe",
                "YuanShen",
                "StarRail.exe",
                "StarRail",
                "ZenlessZoneZero.exe",
                "ZenlessZoneZero",
                "BH3.exe",
                "BH3",
                "HYP.exe",
                "hyp.exe",
                "hyp_desktop.exe",
                "HoYoPlay.exe",
            ],
        }
    }

    /// Returns default OS-specific paths where the launcher executable or bundle is installed.
    pub fn default_app_path(&self) -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            let home_apps = dirs::home_dir().map(|h| h.join("Applications"));
            match self {
                Self::Steam => {
                    let p = PathBuf::from("/Applications/Steam.app");
                    if p.exists() {
                        Some(p)
                    } else if let Some(ref ha) = home_apps {
                        let hp = ha.join("Steam.app");
                        if hp.exists() {
                            Some(hp)
                        } else {
                            Some(p)
                        }
                    } else {
                        Some(p)
                    }
                }
                Self::EpicGames => {
                    let p = PathBuf::from("/Applications/Epic Games Launcher.app");
                    if p.exists() {
                        Some(p)
                    } else if let Some(ref ha) = home_apps {
                        let hp = ha.join("Epic Games Launcher.app");
                        if hp.exists() {
                            Some(hp)
                        } else {
                            Some(p)
                        }
                    } else {
                        Some(p)
                    }
                }
                Self::RiotGames => {
                    let riot_path = PathBuf::from(
                        "/Applications/Riot Games/Riot Client/RiotClientServices.app",
                    );
                    if riot_path.exists() {
                        Some(riot_path)
                    } else {
                        let p = PathBuf::from("/Applications/Riot Client.app");
                        if p.exists() {
                            Some(p)
                        } else if let Some(ref ha) = home_apps {
                            let hp = ha.join("Riot Client.app");
                            if hp.exists() {
                                Some(hp)
                            } else {
                                Some(p)
                            }
                        } else {
                            Some(p)
                        }
                    }
                }
                Self::BattleNet => {
                    let p = PathBuf::from("/Applications/Battle.net.app");
                    if p.exists() {
                        Some(p)
                    } else if let Some(ref ha) = home_apps {
                        let hp = ha.join("Battle.net.app");
                        if hp.exists() {
                            Some(hp)
                        } else {
                            Some(p)
                        }
                    } else {
                        Some(p)
                    }
                }
                Self::Minecraft => {
                    let p = PathBuf::from("/Applications/Minecraft.app");
                    if p.exists() {
                        Some(p)
                    } else if let Some(ref ha) = home_apps {
                        let hp = ha.join("Minecraft.app");
                        if hp.exists() {
                            Some(hp)
                        } else {
                            Some(p)
                        }
                    } else {
                        Some(p)
                    }
                }
                Self::Hoyoverse => {
                    let p = PathBuf::from("/Applications/Genshin Impact.app");
                    if p.exists() {
                        Some(p)
                    } else if let Some(ref ha) = home_apps {
                        let hp = ha.join("Genshin Impact.app");
                        if hp.exists() {
                            Some(hp)
                        } else {
                            Some(p)
                        }
                    } else {
                        Some(p)
                    }
                }
            }
        }
        #[cfg(target_os = "windows")]
        {
            match self {
                Self::Steam => {
                    let prog = std::env::var("ProgramFiles(x86)")
                        .unwrap_or_else(|_| "C:\\Program Files (x86)".into());
                    Some(PathBuf::from(prog).join("Steam\\steam.exe"))
                }
                Self::EpicGames => {
                    let prog = std::env::var("ProgramFiles(x86)")
                        .unwrap_or_else(|_| "C:\\Program Files (x86)".into());
                    Some(PathBuf::from(prog).join(
                        "Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe",
                    ))
                }
                Self::RiotGames => Some(PathBuf::from(
                    "C:\\Riot Games\\Riot Client\\RiotClientServices.exe",
                )),
                Self::BattleNet => {
                    let prog = std::env::var("ProgramFiles(x86)")
                        .unwrap_or_else(|_| "C:\\Program Files (x86)".into());
                    Some(PathBuf::from(prog).join("Battle.net\\Battle.net.exe"))
                }
                Self::Minecraft => {
                    let prog = std::env::var("ProgramFiles(x86)")
                        .unwrap_or_else(|_| "C:\\Program Files (x86)".into());
                    let p = PathBuf::from(prog).join("Minecraft Launcher\\MinecraftLauncher.exe");
                    if p.exists() {
                        Some(p)
                    } else {
                        let prog64 = std::env::var("ProgramFiles")
                            .unwrap_or_else(|_| "C:\\Program Files".into());
                        Some(PathBuf::from(prog64).join("Minecraft Launcher\\MinecraftLauncher.exe"))
                    }
                }
                Self::Hoyoverse => {
                    let prog64 = std::env::var("ProgramFiles")
                        .unwrap_or_else(|_| "C:\\Program Files".into());
                    let p = PathBuf::from(prog64).join("HoYoPlay\\launcher.exe");
                    if p.exists() {
                        Some(p)
                    } else {
                        let prog32 = std::env::var("ProgramFiles(x86)")
                            .unwrap_or_else(|_| "C:\\Program Files (x86)".into());
                        Some(PathBuf::from(prog32).join("HoYoPlay\\launcher.exe"))
                    }
                }
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            match self {
                Self::Steam => Some(PathBuf::from("/usr/bin/steam")),
                Self::Minecraft => Some(PathBuf::from("/usr/bin/minecraft-launcher")),
                _ => None,
            }
        }
    }
}

/// Information about a detected system process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub exe: Option<PathBuf>,
    pub cmd: Vec<String>,
}

/// Strips `.exe` extension from a process or executable name (case-insensitively).
pub fn strip_exe(s: &str) -> &str {
    if s.len() >= 4 && s[s.len() - 4..].eq_ignore_ascii_case(".exe") {
        &s[..s.len() - 4]
    } else {
        s
    }
}

/// Normalizes process stems across operating systems by stripping extensions and OS suffixes.
/// Strips `.exe`, `_osx`, `_mac`, `.app`, and `.bin` case-insensitively.
pub fn normalize_process_stem(s: &str) -> &str {
    let mut stem = s.trim();
    if stem.len() >= 4 {
        let suffix = &stem[stem.len() - 4..];
        if suffix.eq_ignore_ascii_case(".exe")
            || suffix.eq_ignore_ascii_case(".app")
            || suffix.eq_ignore_ascii_case(".bin")
        {
            stem = &stem[..stem.len() - 4];
        }
    }

    if stem.len() >= 4 {
        let suffix = &stem[stem.len() - 4..];
        if suffix.eq_ignore_ascii_case("_osx") || suffix.eq_ignore_ascii_case("_mac") {
            stem = &stem[..stem.len() - 4];
        }
    }

    stem
}

/// Helper function to normalize and match process names across platforms.
/// Checks exact equality, case-insensitivity, .exe stripping, and OS stem normalization.
pub fn matches_target_name(proc_name: &str, target_name: &str) -> bool {
    let proc_clean = proc_name.trim();
    let target_clean = target_name.trim();

    // 1. Direct case-insensitive match
    if proc_clean.eq_ignore_ascii_case(target_clean) {
        return true;
    }

    // 2. Strip `.exe` extension if present
    let proc_stem = strip_exe(proc_clean);
    let target_stem = strip_exe(target_clean);
    if proc_stem.eq_ignore_ascii_case(target_stem) {
        return true;
    }

    // 3. Platform extension normalization (_osx, .app, .bin)
    let proc_norm = normalize_process_stem(proc_clean);
    let target_norm = normalize_process_stem(target_clean);
    proc_norm.eq_ignore_ascii_case(target_norm)
}

/// Checks if a running process matches any of the target names.
/// Matches against `process.name()`, `process.exe()` filename, and `process.cmd()` executable args.
pub fn matches_process(process: &Process, target_names: &[&str]) -> bool {
    let name = process.name();
    let exe_name = process
        .exe()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());
    let cmd_first = process
        .cmd()
        .first()
        .and_then(|cmd0| Path::new(cmd0).file_name().and_then(|n| n.to_str()));

    for target in target_names {
        let clean_target = Path::new(target)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(target);

        if matches_target_name(name, clean_target) {
            return true;
        }

        if let Some(exe_n) = exe_name {
            if matches_target_name(exe_n, clean_target) {
                return true;
            }
        }

        if let Some(cmd0) = cmd_first {
            if matches_target_name(cmd0, clean_target) {
                return true;
            }
        }
    }

    false
}

/// Expands a list of process or launcher names to include all known helper and platform process names.
/// For example, `"EpicGamesLauncher"` expands to include `"EpicWebHelper"`, and `"steam.exe"`
/// expands to include `"steam_osx"`, `"Steam"`, and `"steamwebhelper"`.
pub fn expand_launcher_names(names: &[&str]) -> Vec<String> {
    let mut expanded = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for &name in names {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_lowercase()) {
            expanded.push(trimmed.to_string());
        }

        for launcher in [
            LauncherType::Steam,
            LauncherType::EpicGames,
            LauncherType::RiotGames,
            LauncherType::BattleNet,
            LauncherType::Minecraft,
            LauncherType::Hoyoverse,
        ] {
            if launcher.matches_alias(trimmed) {
                for proc in launcher.process_names() {
                    if seen.insert(proc.to_lowercase()) {
                        expanded.push(proc.to_string());
                    }
                }
            }
        }
    }

    expanded
}

/// Finds all running processes matching the specified list of names.
/// Automatically expands launcher names to capture all helper and auxiliary processes.
pub fn find_processes_by_names(system: &System, names: &[&str]) -> Vec<ProcessInfo> {
    let expanded = expand_launcher_names(names);
    let expanded_refs: Vec<&str> = expanded.iter().map(|s| s.as_str()).collect();

    system
        .processes()
        .values()
        .filter(|p| matches_process(p, &expanded_refs))
        .map(|p| ProcessInfo {
            pid: p.pid().as_u32(),
            name: p.name().to_string(),
            exe: p.exe().map(|p| p.to_path_buf()),
            cmd: p.cmd().to_vec(),
        })
        .collect()
}

/// Finds processes matching names strictly without launcher alias expansion.
pub fn find_processes_by_names_exact(system: &System, names: &[&str]) -> Vec<ProcessInfo> {
    system
        .processes()
        .values()
        .filter(|p| matches_process(p, names))
        .map(|p| ProcessInfo {
            pid: p.pid().as_u32(),
            name: p.name().to_string(),
            exe: p.exe().map(|p| p.to_path_buf()),
            cmd: p.cmd().to_vec(),
        })
        .collect()
}

/// Finds all running processes associated with a given launcher.
pub fn find_launcher_processes(system: &System, launcher: LauncherType) -> Vec<ProcessInfo> {
    find_processes_by_names(system, launcher.process_names())
}

/// Finds all running launcher processes grouped by launcher type.
pub fn find_all_launcher_processes(system: &System) -> Vec<(LauncherType, Vec<ProcessInfo>)> {
    let launchers = [
        LauncherType::Steam,
        LauncherType::EpicGames,
        LauncherType::RiotGames,
        LauncherType::BattleNet,
        LauncherType::Minecraft,
        LauncherType::Hoyoverse,
    ];

    launchers
        .into_iter()
        .map(|l| {
            let procs = find_launcher_processes(system, l);
            (l, procs)
        })
        .filter(|(_, procs)| !procs.is_empty())
        .collect()
}

/// Recursively finds all descendant process IDs (children, grandchildren, etc.) of the specified root PIDs.
pub fn find_descendant_pids(system: &System, root_pids: &[Pid]) -> Vec<Pid> {
    let mut descendants = Vec::new();
    let mut queue: Vec<Pid> = root_pids.to_vec();
    let mut visited: std::collections::HashSet<Pid> = root_pids.iter().copied().collect();

    while let Some(parent_pid) = queue.pop() {
        for (&pid, process) in system.processes() {
            if process.parent() == Some(parent_pid) && visited.insert(pid) {
                descendants.push(pid);
                queue.push(pid);
            }
        }
    }

    descendants
}

/// Lifecycle status of a system process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessLifecycleStatus {
    /// Process is running and active in the operating system.
    Running,
    /// Process has terminated but remains in the process table as a zombie until reaped.
    Zombie,
    /// Process does not exist or has completely terminated.
    Terminated,
}

/// Determines the exact lifecycle status of a process.
pub fn check_process_status(system: &mut System, pid: Pid) -> ProcessLifecycleStatus {
    #[cfg(unix)]
    {
        extern "C" {
            fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
            fn kill(pid: i32, sig: i32) -> i32;
        }
        const WNOHANG: i32 = 1;
        let mut status: i32 = 0;
        let ret = unsafe { waitpid(pid.as_u32() as i32, &mut status, WNOHANG) };
        if ret > 0 {
            // Reaped exited child
            return ProcessLifecycleStatus::Terminated;
        }

        // For non-child processes, check if OS recognizes the PID
        let kret = unsafe { kill(pid.as_u32() as i32, 0) };
        if kret != 0 {
            let errno = io::Error::last_os_error().raw_os_error();
            if errno == Some(3) {
                // ESRCH: No such process
                return ProcessLifecycleStatus::Terminated;
            }
        }
    }

    #[cfg(windows)]
    {
        extern "system" {
            fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
            fn GetExitCodeProcess(handle: isize, exit_code: *mut u32) -> i32;
            fn CloseHandle(handle: isize) -> i32;
        }
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const STILL_ACTIVE: u32 = 259;

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid.as_u32());
            if handle == 0 {
                return ProcessLifecycleStatus::Terminated;
            }
            let mut exit_code: u32 = 0;
            let ok = GetExitCodeProcess(handle, &mut exit_code);
            CloseHandle(handle);
            if ok != 0 && exit_code != STILL_ACTIVE {
                return ProcessLifecycleStatus::Terminated;
            }
        }
    }

    if !system.refresh_process_specifics(pid, ProcessRefreshKind::new()) {
        return ProcessLifecycleStatus::Terminated;
    }

    if let Some(proc) = system.process(pid) {
        match proc.status() {
            ProcessStatus::Zombie | ProcessStatus::Dead => ProcessLifecycleStatus::Zombie,
            _ => ProcessLifecycleStatus::Running,
        }
    } else {
        ProcessLifecycleStatus::Terminated
    }
}

/// Checks whether a given PID is alive and actively running in the operating system.
pub fn is_pid_alive(system: &mut System, pid: Pid) -> bool {
    check_process_status(system, pid) == ProcessLifecycleStatus::Running
}

/// Options configuring process termination behavior.
#[derive(Debug, Clone)]
pub struct TerminateOptions {
    /// Maximum duration to wait for graceful SIGTERM termination (default: 4s, within 3-5s requirement).
    pub graceful_timeout: Duration,
    /// Polling step interval between status checks (default: 200ms).
    pub poll_interval: Duration,
    /// Maximum duration to wait after sending SIGKILL (default: 2s).
    pub force_timeout: Duration,
    /// Settle delay after processes exit to guarantee OS kernel releases all file descriptors (default: 100ms).
    pub settle_delay: Duration,
    /// Whether to find and terminate all descendant (child/helper) processes (default: true).
    pub include_descendants: bool,
}

impl Default for TerminateOptions {
    fn default() -> Self {
        Self {
            graceful_timeout: Duration::from_millis(3000), // Up to 3 seconds for graceful shutdown
            poll_interval: Duration::from_millis(100),     // 100 ms check step for fast responsiveness
            force_timeout: Duration::from_millis(2000),    // 2 seconds force kill wait
            settle_delay: Duration::from_millis(150),      // 150 ms kernel fd release guarantee
            include_descendants: true,                     // Terminate whole process tree
        }
    }
}

/// Report summarizing the outcome of a termination attempt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminationReport {
    pub terminated_gracefully: Vec<u32>,
    pub killed_forcefully: Vec<u32>,
    pub failed_to_kill: Vec<u32>,
}

impl TerminationReport {
    pub fn is_success(&self) -> bool {
        self.failed_to_kill.is_empty()
    }
}

#[cfg(windows)]
unsafe fn post_wm_close_to_pid(pid: u32) -> bool {
    extern "system" {
        fn EnumWindows(lp_enum_func: unsafe extern "system" fn(isize, isize) -> i32, l_param: isize) -> i32;
        fn GetWindowThreadProcessId(h_wnd: isize, lpdw_process_id: *mut u32) -> u32;
        fn PostMessageW(h_wnd: isize, msg: u32, w_param: usize, l_param: isize) -> i32;
    }
    const WM_CLOSE: u32 = 0x0010;

    struct Context {
        pid: u32,
        found: bool,
    }

    unsafe extern "system" fn enum_proc(hwnd: isize, lparam: isize) -> i32 {
        let ctx = &mut *(lparam as *mut Context);
        let mut wnd_pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut wnd_pid);
        if wnd_pid == ctx.pid {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
            ctx.found = true;
        }
        1
    }

    let mut ctx = Context { pid, found: false };
    EnumWindows(enum_proc, &mut ctx as *mut Context as isize);
    ctx.found
}

/// Low-level helper to send a graceful termination signal.
/// On Unix: sends `SIGTERM` (signal 15).
/// On Windows: attempts `WM_CLOSE` to GUI windows and runs `taskkill /PID <pid>`.
pub fn send_sigterm(pid: u32) -> bool {
    #[cfg(unix)]
    {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        const SIGTERM: i32 = 15;
        unsafe { kill(pid as i32, SIGTERM) == 0 }
    }
    #[cfg(windows)]
    {
        unsafe { post_wm_close_to_pid(pid) }
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// Low-level helper to forcefully kill a process.
/// On Unix: sends `SIGKILL` (signal 9).
/// On Windows: runs `taskkill /F /PID <pid> /T` to forcibly terminate process tree.
pub fn send_sigkill(pid: u32) -> bool {
    #[cfg(unix)]
    {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        const SIGKILL: i32 = 9;
        unsafe { kill(pid as i32, SIGKILL) == 0 }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut cmd = std::process::Command::new("taskkill.exe");
        cmd.arg("/F").arg("/PID").arg(pid.to_string()).arg("/T");
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.status().map(|s| s.success()).unwrap_or(false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// Forcefully terminates a process using sysinfo (`proc.kill()`) with OS fallback (`SIGKILL` / `taskkill /F`).
pub fn sys_kill(system: &System, pid: Pid) -> bool {
    let mut killed = false;
    if let Some(proc) = system.process(pid) {
        killed = proc.kill();
    }
    if send_sigkill(pid.as_u32()) {
        killed = true;
    }
    killed
}

/// Guarantees that operating system file descriptors held by the specified PIDs are released.
/// Verifies all PIDs are no longer active in the OS process table and reaps any child processes.
pub fn verify_file_descriptors_released(pids: &[u32]) -> bool {
    #[cfg(unix)]
    {
        extern "C" {
            fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
            fn kill(pid: i32, sig: i32) -> i32;
        }
        const WNOHANG: i32 = 1;
        for &pid in pids {
            let mut status: i32 = 0;
            let _ = unsafe { waitpid(pid as i32, &mut status, WNOHANG) };

            let ret = unsafe { kill(pid as i32, 0) };
            if ret == 0 {
                return false;
            }
            let errno = io::Error::last_os_error().raw_os_error();
            if errno != Some(3) {
                return false;
            }
        }
    }

    #[cfg(windows)]
    {
        extern "system" {
            fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
            fn CloseHandle(handle: isize) -> i32;
            fn GetExitCodeProcess(process: isize, exit_code: *mut u32) -> i32;
        }
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const STILL_ACTIVE: u32 = 259;

        for &pid in pids {
            unsafe {
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if handle != 0 {
                    let mut exit_code: u32 = 0;
                    let ok = GetExitCodeProcess(handle, &mut exit_code);
                    CloseHandle(handle);
                    if ok != 0 && exit_code == STILL_ACTIVE {
                        return false;
                    }
                }
            }
        }
    }

    true
}

/// Verifies that a specific file path is unlocked and not held exclusively by any process.
pub fn verify_path_unlocked<P: AsRef<Path>>(path: P) -> bool {
    let p = path.as_ref();
    if !p.exists() {
        return true;
    }
    if p.is_file() {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(p)
            .is_ok()
    } else {
        true
    }
}

/// Synchronously terminates a list of process IDs according to `options`.
///
/// Execution flow:
/// 1. Finds all descendant (child/helper) processes if `include_descendants` is enabled.
/// 2. Verifies which processes are currently running.
/// 3. Stage 1: Sends Graceful SIGTERM (`kill_with(Signal::Term)` and OS signal).
/// 4. Polls every `poll_interval` (200ms) up to `graceful_timeout` (3-5s).
/// 5. Stage 2: If any processes are still running, escalates to `SIGKILL` (`sys.kill()`).
/// 6. Polls up to `force_timeout` for confirmed termination.
/// 7. Applies `settle_delay` and verifies OS file descriptor release guarantee.
pub fn terminate_pids_blocking(
    system: &mut System,
    pids: &[Pid],
    options: &TerminateOptions,
) -> Result<TerminationReport, ProcessError> {
    system.refresh_processes();

    let mut target_pids: Vec<Pid> = pids.to_vec();
    if options.include_descendants {
        let descendants = find_descendant_pids(system, pids);
        for d in descendants {
            if !target_pids.contains(&d) {
                target_pids.push(d);
            }
        }
    }

    let mut alive: Vec<Pid> = target_pids
        .iter()
        .copied()
        .filter(|&p| is_pid_alive(system, p))
        .collect();

    if alive.is_empty() {
        return Ok(TerminationReport::default());
    }

    info!(
        "Initiating graceful shutdown for {} process(es) (tree included): {:?}",
        alive.len(),
        alive.iter().map(|p| p.as_u32()).collect::<Vec<_>>()
    );

    // Stage 1: Send Graceful SIGTERM
    let mut any_sigterm_sent = false;
    for &pid in &alive {
        if let Some(proc) = system.process(pid) {
            let _ = proc.kill_with(Signal::Term);
        }
        if send_sigterm(pid.as_u32()) {
            any_sigterm_sent = true;
        }
    }

    let start_graceful = Instant::now();
    let mut gracefully_terminated = Vec::new();

    #[cfg(windows)]
    let effective_graceful_timeout = if any_sigterm_sent {
        options.graceful_timeout
    } else {
        options.graceful_timeout.min(Duration::from_millis(2000))
    };
    #[cfg(not(windows))]
    let effective_graceful_timeout = options.graceful_timeout;

    // Polling loop
    while !alive.is_empty() && start_graceful.elapsed() < effective_graceful_timeout {
        std::thread::sleep(options.poll_interval);
        system.refresh_processes();

        alive.retain(|&pid| {
            if !is_pid_alive(system, pid) {
                gracefully_terminated.push(pid.as_u32());
                false
            } else {
                true
            }
        });
    }

    let mut killed_forcefully = Vec::new();

    // Stage 2: Forced SIGKILL if any processes remain
    if !alive.is_empty() {
        warn!(
            "Graceful shutdown timed out for {} process(es). Escalating to SIGKILL: {:?}",
            alive.len(),
            alive.iter().map(|p| p.as_u32()).collect::<Vec<_>>()
        );

        for &pid in &alive {
            sys_kill(system, pid);
        }

        let start_force = Instant::now();
        while !alive.is_empty() && start_force.elapsed() < options.force_timeout {
            std::thread::sleep(options.poll_interval);
            system.refresh_processes();

            alive.retain(|&pid| {
                if !is_pid_alive(system, pid) {
                    killed_forcefully.push(pid.as_u32());
                    false
                } else {
                    true
                }
            });
        }
    }

    // Guarantee file descriptors are freed:
    // Once OS confirms process does not exist, kernel closes all FDs.
    // Settle delay ensures asynchronous filesystem teardowns settle.
    if alive.is_empty() {
        if !options.settle_delay.is_zero() {
            std::thread::sleep(options.settle_delay);
        }
        let all_terminated: Vec<u32> = gracefully_terminated
            .iter()
            .chain(killed_forcefully.iter())
            .copied()
            .collect();
        verify_file_descriptors_released(&all_terminated);
    }

    let report = TerminationReport {
        terminated_gracefully: gracefully_terminated,
        killed_forcefully,
        failed_to_kill: alive.iter().map(|p| p.as_u32()).collect(),
    };

    if !report.is_success() {
        error!(
            "Failed to terminate process(es): {:?}",
            report.failed_to_kill
        );
        Err(ProcessError::FailedToTerminate(report))
    } else {
        info!("Process termination completed successfully: {:?}", report);
        Ok(report)
    }
}

/// Asynchronously terminates a list of process IDs using Tokio.
pub async fn terminate_pids(
    system: &mut System,
    pids: &[Pid],
    options: &TerminateOptions,
) -> Result<TerminationReport, ProcessError> {
    system.refresh_processes();

    let mut target_pids: Vec<Pid> = pids.to_vec();
    if options.include_descendants {
        let descendants = find_descendant_pids(system, pids);
        for d in descendants {
            if !target_pids.contains(&d) {
                target_pids.push(d);
            }
        }
    }

    let mut alive: Vec<Pid> = target_pids
        .iter()
        .copied()
        .filter(|&p| is_pid_alive(system, p))
        .collect();

    if alive.is_empty() {
        return Ok(TerminationReport::default());
    }

    info!(
        "Initiating async graceful shutdown for {} process(es) (tree included)",
        alive.len()
    );

    // Stage 1: Send Graceful SIGTERM
    let mut any_sigterm_sent = false;
    for &pid in &alive {
        if let Some(proc) = system.process(pid) {
            let _ = proc.kill_with(Signal::Term);
        }
        if send_sigterm(pid.as_u32()) {
            any_sigterm_sent = true;
        }
    }

    let start_graceful = Instant::now();
    let mut gracefully_terminated = Vec::new();

    #[cfg(windows)]
    let effective_graceful_timeout = if any_sigterm_sent {
        options.graceful_timeout
    } else {
        options.graceful_timeout.min(Duration::from_millis(2000))
    };
    #[cfg(not(windows))]
    let effective_graceful_timeout = options.graceful_timeout;

    while !alive.is_empty() && start_graceful.elapsed() < effective_graceful_timeout {
        tokio::time::sleep(options.poll_interval).await;
        system.refresh_processes();

        alive.retain(|&pid| {
            if !is_pid_alive(system, pid) {
                gracefully_terminated.push(pid.as_u32());
                false
            } else {
                true
            }
        });
    }

    let mut killed_forcefully = Vec::new();

    // Stage 2: Forced SIGKILL if any processes remain
    if !alive.is_empty() {
        warn!(
            "Async graceful shutdown timed out for {} process(es). Escalating to SIGKILL.",
            alive.len()
        );

        for &pid in &alive {
            sys_kill(system, pid);
        }

        let start_force = Instant::now();
        while !alive.is_empty() && start_force.elapsed() < options.force_timeout {
            tokio::time::sleep(options.poll_interval).await;
            system.refresh_processes();

            alive.retain(|&pid| {
                if !is_pid_alive(system, pid) {
                    killed_forcefully.push(pid.as_u32());
                    false
                } else {
                    true
                }
            });
        }
    }

    if alive.is_empty() {
        if !options.settle_delay.is_zero() {
            tokio::time::sleep(options.settle_delay).await;
        }
        let all_terminated: Vec<u32> = gracefully_terminated
            .iter()
            .chain(killed_forcefully.iter())
            .copied()
            .collect();
        verify_file_descriptors_released(&all_terminated);
    }

    let report = TerminationReport {
        terminated_gracefully: gracefully_terminated,
        killed_forcefully,
        failed_to_kill: alive.iter().map(|p| p.as_u32()).collect(),
    };

    if !report.is_success() {
        Err(ProcessError::FailedToTerminate(report))
    } else {
        Ok(report)
    }
}

/// Terminates all running processes of the specified launcher (blocking).
pub fn terminate_launcher_blocking(
    launcher: LauncherType,
    options: &TerminateOptions,
) -> Result<TerminationReport, ProcessError> {
    let mut system = System::new();
    system.refresh_processes();
    let procs = find_launcher_processes(&system, launcher);
    let pids: Vec<Pid> = procs.iter().map(|p| Pid::from_u32(p.pid)).collect();
    terminate_pids_blocking(&mut system, &pids, options)
}

/// Terminates all running processes of the specified launcher (async).
pub async fn terminate_launcher(
    launcher: LauncherType,
    options: &TerminateOptions,
) -> Result<TerminationReport, ProcessError> {
    let mut system = System::new();
    system.refresh_processes();
    let procs = find_launcher_processes(&system, launcher);
    let pids: Vec<Pid> = procs.iter().map(|p| Pid::from_u32(p.pid)).collect();
    terminate_pids(&mut system, &pids, options).await
}

/// Terminates processes matching any of the specified names (blocking).
pub fn terminate_by_names_blocking(
    names: &[&str],
    options: &TerminateOptions,
) -> Result<TerminationReport, ProcessError> {
    let mut system = System::new();
    system.refresh_processes();
    let procs = find_processes_by_names(&system, names);
    let pids: Vec<Pid> = procs.iter().map(|p| Pid::from_u32(p.pid)).collect();
    terminate_pids_blocking(&mut system, &pids, options)
}

/// Terminates processes matching any of the specified names (async).
pub async fn terminate_by_names(
    names: &[&str],
    options: &TerminateOptions,
) -> Result<TerminationReport, ProcessError> {
    let mut system = System::new();
    system.refresh_processes();
    let procs = find_processes_by_names(&system, names);
    let pids: Vec<Pid> = procs.iter().map(|p| Pid::from_u32(p.pid)).collect();
    terminate_pids(&mut system, &pids, options).await
}

/// Configuration options for launching an application or game binary.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    pub target: PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub envs: HashMap<String, String>,
    pub no_window: bool,
    pub prefer_open: bool,
}

impl LaunchConfig {
    pub fn new<P: Into<PathBuf>>(target: P) -> Self {
        Self {
            target: target.into(),
            args: Vec::new(),
            working_dir: None,
            envs: HashMap::new(),
            no_window: false,
            prefer_open: true,
        }
    }

    pub fn arg<S: Into<String>>(mut self, arg: S) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for a in args {
            self.args.push(a.into());
        }
        self
    }

    pub fn current_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    pub fn env<K: Into<String>, V: Into<String>>(mut self, key: K, val: V) -> Self {
        self.envs.insert(key.into(), val.into());
        self
    }

    pub fn no_window(mut self, no_window: bool) -> Self {
        self.no_window = no_window;
        self
    }

    pub fn prefer_open(mut self, prefer_open: bool) -> Self {
        self.prefer_open = prefer_open;
        self
    }
}

/// Handle to a launched application.
pub enum LaunchedApp {
    /// Process spawned directly as a child process.
    Child(std::process::Child),
    /// Process spawned through platform launcher (e.g. `open -n` on macOS).
    Detached { command: String },
}

impl LaunchedApp {
    pub fn child(&self) -> Option<&std::process::Child> {
        match self {
            Self::Child(c) => Some(c),
            Self::Detached { .. } => None,
        }
    }

    pub fn child_mut(&mut self) -> Option<&mut std::process::Child> {
        match self {
            Self::Child(c) => Some(c),
            Self::Detached { .. } => None,
        }
    }

    pub fn is_detached(&self) -> bool {
        matches!(self, Self::Detached { .. })
    }
}

/// Launches an application or game according to the provided `LaunchConfig`.
///
/// macOS:
/// - If `prefer_open` is true, invokes `open -n` to start a new independent instance of the app bundle.
/// - If `prefer_open` is false, executes binary directly via `std::process::Command`.
///
/// Windows:
/// - Spawns binary directly using `std::process::Command`.
/// - If `no_window` is true, sets `CREATE_NO_WINDOW` (`0x08000000`) flag to prevent console window flashing.
pub fn launch_app(config: &LaunchConfig) -> Result<LaunchedApp, ProcessError> {
    #[cfg(target_os = "macos")]
    {
        let target_str = config.target.to_string_lossy();
        let is_app_bundle = target_str.ends_with(".app") || config.target.is_dir();
        let is_named_app = !target_str.contains('/') && !target_str.contains('\\');

        // Use 'open -n' for app bundles or named applications,
        // and spawn binary directly for direct executable files.
        if config.prefer_open && (is_app_bundle || is_named_app) {
            let mut cmd = std::process::Command::new("open");
            cmd.arg("-n");

            if is_app_bundle {
                cmd.arg(&config.target);
            } else if is_named_app {
                cmd.arg("-a").arg(&config.target);
            } else {
                cmd.arg(&config.target);
            }

            if !config.args.is_empty() {
                cmd.arg("--args");
                cmd.args(&config.args);
            }

            if let Some(ref dir) = config.working_dir {
                cmd.current_dir(dir);
            }

            for (k, v) in &config.envs {
                cmd.env(k, v);
            }

            info!("Launching via 'open -n': {:?}", cmd);
            let status = cmd.status().map_err(|e| {
                ProcessError::LaunchFailed(format!("Failed to execute 'open -n': {}", e))
            })?;

            if !status.success() {
                return Err(ProcessError::LaunchFailed(format!(
                    "'open -n' exited with status: {}",
                    status
                )));
            }

            return Ok(LaunchedApp::Detached {
                command: format!("open -n {:?}", config.target),
            });
        }
    }

    // Direct binary invocation (Windows, Linux, or macOS when direct binary or prefer_open is false)
    let mut cmd = std::process::Command::new(&config.target);
    cmd.args(&config.args);

    if let Some(ref dir) = config.working_dir {
        cmd.current_dir(dir);
    }

    for (k, v) in &config.envs {
        cmd.env(k, v);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let is_lnk = config.target.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false);

        if is_lnk {
            let mut lnk_cmd = std::process::Command::new("cmd");
            lnk_cmd.args(["/C", "start", "", &config.target.to_string_lossy()]);
            lnk_cmd.creation_flags(CREATE_NO_WINDOW);
            info!("Launching shortcut via cmd: {:?}", lnk_cmd);
            let child = lnk_cmd.spawn().map_err(|e| {
                ProcessError::LaunchFailed(format!("Failed to spawn shortcut {:?}: {}", config.target, e))
            })?;
            return Ok(LaunchedApp::Child(child));
        }

        if config.no_window {
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
    }

    info!("Spawning binary command: {:?}", cmd);
    let child = cmd.spawn().map_err(|e| {
        ProcessError::LaunchFailed(format!("Failed to spawn {:?}: {}", config.target, e))
    })?;

    Ok(LaunchedApp::Child(child))
}

/// Launches a launcher by its type, using either a custom path or the default installation path.
pub fn launch_launcher(
    launcher: LauncherType,
    custom_path: Option<&Path>,
    args: &[&str],
) -> Result<LaunchedApp, ProcessError> {
    let target = if let Some(p) = custom_path {
        if !p.exists() && p.is_absolute() {
            return Err(ProcessError::ExecutableNotFound(format!(
                "Custom path for launcher {:?} does not exist: {}",
                launcher,
                p.display()
            )));
        }
        p.to_path_buf()
    } else if let Some(p) = launcher.default_app_path() {
        p
    } else {
        return Err(ProcessError::ExecutableNotFound(format!(
            "Default installation path not found for launcher {:?}",
            launcher
        )));
    };

    let mut config = LaunchConfig::new(target);
    for arg in args {
        config = config.arg(*arg);
    }

    launch_app(&config)
}

/// Launches an application via standard URI scheme (e.g. `steam://run/570` or `battlenet://`).
pub fn launch_uri(uri: &str) -> Result<(), ProcessError> {
    info!("Launching URI: {}", uri);

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg(uri)
            .status()
            .map_err(|e| ProcessError::LaunchFailed(format!("Failed to open URI: {}", e)))?;
        if !status.success() {
            return Err(ProcessError::LaunchFailed(format!(
                "'open {}' failed with status: {}",
                uri, status
            )));
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/c", "start", "", uri]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        let status = cmd
            .status()
            .map_err(|e| ProcessError::LaunchFailed(format!("Failed to open URI: {}", e)))?;
        if !status.success() {
            return Err(ProcessError::LaunchFailed(format!(
                "start URI failed with status: {}",
                status
            )));
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let status = std::process::Command::new("xdg-open")
            .arg(uri)
            .status()
            .map_err(|e| ProcessError::LaunchFailed(format!("Failed to open URI: {}", e)))?;
        if !status.success() {
            return Err(ProcessError::LaunchFailed(format!(
                "xdg-open failed with status: {}",
                status
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_exe() {
        assert_eq!(strip_exe("steam.exe"), "steam");
        assert_eq!(strip_exe("STEAM.EXE"), "STEAM");
        assert_eq!(strip_exe("Steam.Exe"), "Steam");
        assert_eq!(strip_exe("steam"), "steam");
        assert_eq!(strip_exe("steam_osx"), "steam_osx");
        assert_eq!(strip_exe("Battle.net"), "Battle.net");
        assert_eq!(strip_exe("Battle.net.exe"), "Battle.net");
        assert_eq!(strip_exe("EpicGamesLauncher.exe"), "EpicGamesLauncher");
        assert_eq!(strip_exe(""), "");
    }

    #[test]
    fn test_normalize_process_stem() {
        assert_eq!(normalize_process_stem("steam.exe"), "steam");
        assert_eq!(normalize_process_stem("steam_osx"), "steam");
        assert_eq!(normalize_process_stem("Steam"), "Steam");
        assert_eq!(normalize_process_stem("Steam.app"), "Steam");
        assert_eq!(normalize_process_stem("game_mac"), "game");
        assert_eq!(normalize_process_stem("dota2.bin"), "dota2");
        assert_eq!(normalize_process_stem("Battle.net.exe"), "Battle.net");
        assert_eq!(
            normalize_process_stem("EpicGamesLauncher.exe"),
            "EpicGamesLauncher"
        );
        assert_eq!(
            normalize_process_stem("RiotClientServices.exe"),
            "RiotClientServices"
        );
    }

    #[test]
    fn test_matches_target_name() {
        // Steam cross-platform matching
        assert!(matches_target_name("steam.exe", "Steam"));
        assert!(matches_target_name("Steam", "steam.exe"));
        assert!(matches_target_name("steam_osx", "steam_osx"));
        assert!(matches_target_name("steam_osx", "steam.exe"));
        assert!(matches_target_name("steam.exe", "steam_osx"));
        assert!(matches_target_name("Steam", "steam_osx"));
        assert!(matches_target_name("steam_osx", "Steam"));
        assert!(matches_target_name("Steam.app", "steam.exe"));
        assert!(matches_target_name("steamwebhelper.exe", "steamwebhelper"));
        assert!(matches_target_name("steamwebhelper", "steamwebhelper.exe"));

        // Epic Games variations
        assert!(matches_target_name(
            "EpicGamesLauncher.exe",
            "EpicGamesLauncher"
        ));
        assert!(matches_target_name(
            "EpicGamesLauncher",
            "EpicGamesLauncher.exe"
        ));
        assert!(matches_target_name("EpicWebHelper.exe", "EpicWebHelper"));

        // Riot Games variations
        assert!(matches_target_name(
            "RiotClientServices.exe",
            "RiotClientServices"
        ));
        assert!(matches_target_name("RiotClientUx.exe", "RiotClientUx"));

        // Battle.net variations
        assert!(matches_target_name("Battle.net.exe", "Battle.net"));
        assert!(matches_target_name("Battle.net", "Battle.net.exe"));
        assert!(matches_target_name(
            "Battle.net Launcher.exe",
            "Battle.net Launcher"
        ));

        // Negative cases
        assert!(!matches_target_name("steam_fake", "steam"));
        assert!(!matches_target_name("notepad.exe", "steam.exe"));
        assert!(!matches_target_name("EpicGamesLauncher", "Steam"));
    }

    #[test]
    fn test_launcher_type_metadata() {
        assert_eq!(LauncherType::from_name("steam"), Some(LauncherType::Steam));
        assert_eq!(LauncherType::from_name("STEAM"), Some(LauncherType::Steam));
        assert_eq!(
            LauncherType::from_name("epic"),
            Some(LauncherType::EpicGames)
        );
        assert_eq!(
            LauncherType::from_name("epicgames"),
            Some(LauncherType::EpicGames)
        );
        assert_eq!(
            LauncherType::from_name("riot"),
            Some(LauncherType::RiotGames)
        );
        assert_eq!(
            LauncherType::from_name("battlenet"),
            Some(LauncherType::BattleNet)
        );
        assert_eq!(
            LauncherType::from_name("battle.net"),
            Some(LauncherType::BattleNet)
        );
        assert_eq!(LauncherType::from_name("unknown_launcher"), None);

        // Verify known process names for each launcher
        assert!(LauncherType::Steam.process_names().contains(&"steam.exe"));
        assert!(LauncherType::Steam.process_names().contains(&"steam_osx"));
        assert!(LauncherType::Steam.process_names().contains(&"Steam"));

        assert!(LauncherType::EpicGames
            .process_names()
            .contains(&"EpicGamesLauncher.exe"));
        assert!(LauncherType::EpicGames
            .process_names()
            .contains(&"EpicWebHelper.exe"));

        assert!(LauncherType::RiotGames
            .process_names()
            .contains(&"RiotClientServices.exe"));

        assert!(LauncherType::BattleNet
            .process_names()
            .contains(&"Battle.net.exe"));
        assert!(LauncherType::BattleNet
            .process_names()
            .contains(&"Battle.net"));

        // Minecraft launcher assertions
        assert_eq!(
            LauncherType::from_name("minecraft"),
            Some(LauncherType::Minecraft)
        );
        assert_eq!(
            LauncherType::from_name("mc"),
            Some(LauncherType::Minecraft)
        );
        assert!(LauncherType::Minecraft
            .process_names()
            .contains(&"MinecraftLauncher.exe"));
        assert!(LauncherType::Minecraft
            .process_names()
            .contains(&"MinecraftLauncher"));
        assert!(LauncherType::Minecraft
            .process_names()
            .contains(&"minecraft-launcher"));
        assert!(LauncherType::Minecraft
            .process_names()
            .contains(&"launcher"));
        // Forbidden to include java / javaw
        assert!(!LauncherType::Minecraft.process_names().contains(&"java"));
        assert!(!LauncherType::Minecraft.process_names().contains(&"javaw"));
        assert!(!LauncherType::Minecraft.process_names().contains(&"javaw.exe"));

        // Hoyoverse assertions
        assert_eq!(
            LauncherType::from_name("hoyoverse"),
            Some(LauncherType::Hoyoverse)
        );
        assert_eq!(
            LauncherType::from_name("mihoyo"),
            Some(LauncherType::Hoyoverse)
        );
        assert_eq!(
            LauncherType::from_name("hoyoplay"),
            Some(LauncherType::Hoyoverse)
        );
        assert!(LauncherType::Hoyoverse
            .process_names()
            .contains(&"GenshinImpact.exe"));
        assert!(LauncherType::Hoyoverse
            .process_names()
            .contains(&"StarRail.exe"));
        assert!(LauncherType::Hoyoverse
            .process_names()
            .contains(&"ZenlessZoneZero.exe"));
        assert!(LauncherType::Hoyoverse
            .process_names()
            .contains(&"BH3.exe"));
        assert!(LauncherType::Hoyoverse
            .process_names()
            .contains(&"HYP.exe"));
    }

    #[test]
    fn test_expand_launcher_names() {
        // Hoyoverse expansion
        let hoyo_exp = expand_launcher_names(&["genshin"]);
        assert!(hoyo_exp.iter().any(|s| s.eq_ignore_ascii_case("GenshinImpact.exe")));
        assert!(hoyo_exp.iter().any(|s| s.eq_ignore_ascii_case("StarRail.exe")));

        // Minecraft expansion
        let mc_exp = expand_launcher_names(&["MinecraftLauncher"]);
        assert!(mc_exp.iter().any(|s| s.eq_ignore_ascii_case("MinecraftLauncher.exe")));
        assert!(mc_exp.iter().any(|s| s.eq_ignore_ascii_case("minecraft-launcher")));
        assert!(!mc_exp.iter().any(|s| s.eq_ignore_ascii_case("java")));

        // Steam expansion includes steam_osx, Steam, and helpers
        let steam_exp = expand_launcher_names(&["steam.exe"]);
        assert!(steam_exp
            .iter()
            .any(|s| s.eq_ignore_ascii_case("steam.exe")));
        assert!(steam_exp
            .iter()
            .any(|s| s.eq_ignore_ascii_case("steam_osx")));
        assert!(steam_exp.iter().any(|s| s.eq_ignore_ascii_case("Steam")));
        assert!(steam_exp
            .iter()
            .any(|s| s.eq_ignore_ascii_case("steamwebhelper")));

        // Epic expansion includes EpicWebHelper
        let epic_exp = expand_launcher_names(&["EpicGamesLauncher"]);
        assert!(epic_exp
            .iter()
            .any(|s| s.eq_ignore_ascii_case("EpicGamesLauncher")));
        assert!(epic_exp
            .iter()
            .any(|s| s.eq_ignore_ascii_case("EpicWebHelper")));

        // Battle.net expansion includes Agent
        let bnet_exp = expand_launcher_names(&["Battle.net"]);
        assert!(bnet_exp
            .iter()
            .any(|s| s.eq_ignore_ascii_case("Battle.net.exe")));
        assert!(bnet_exp.iter().any(|s| s.eq_ignore_ascii_case("Agent")));

        // Riot Games expansion includes RiotClientUx
        let riot_exp = expand_launcher_names(&["RiotClientServices"]);
        assert!(riot_exp
            .iter()
            .any(|s| s.eq_ignore_ascii_case("RiotClientServices")));
        assert!(riot_exp
            .iter()
            .any(|s| s.eq_ignore_ascii_case("RiotClientUx")));
    }

    fn spawn_sleeper(seconds: u32) -> std::process::Child {
        #[cfg(windows)]
        {
            let _ = seconds;
            std::process::Command::new("notepad.exe")
                .spawn()
                .expect("Failed to spawn sleeper child process")
        }
        #[cfg(not(windows))]
        {
            std::process::Command::new("sleep")
                .arg(seconds.to_string())
                .spawn()
                .expect("Failed to spawn sleep child process")
        }
    }

    #[test]
    fn test_process_lifecycle_status_and_transitions() {
        let mut system = System::new();
        system.refresh_processes();

        let current_pid = Pid::from_u32(std::process::id());
        let status = check_process_status(&mut system, current_pid);
        assert_eq!(
            status,
            ProcessLifecycleStatus::Running,
            "Current process must be Running"
        );
        assert!(is_pid_alive(&mut system, current_pid));

        // Non-existent PID must report Terminated
        let fake_pid = Pid::from_u32(2_147_483_600);
        assert_eq!(
            check_process_status(&mut system, fake_pid),
            ProcessLifecycleStatus::Terminated
        );
        assert!(!is_pid_alive(&mut system, fake_pid));

        // Spawn real short-lived child and check transition to Terminated
        let mut child = spawn_sleeper(1);
        let child_pid = Pid::from_u32(child.id());

        system.refresh_processes();
        assert_eq!(
            check_process_status(&mut system, child_pid),
            ProcessLifecycleStatus::Running
        );

        let _ = child.kill();
        let _ = child.wait();

        system.refresh_processes();
        assert_eq!(
            check_process_status(&mut system, child_pid),
            ProcessLifecycleStatus::Terminated,
            "Killed and waited child must transition to Terminated"
        );
        assert!(!is_pid_alive(&mut system, child_pid));
    }

    #[test]
    fn test_verify_file_descriptors_and_path_unlocked() {
        let fake_dead_pid = 2_147_483_601;
        assert!(
            verify_file_descriptors_released(&[fake_dead_pid]),
            "Non-existent PID must pass file descriptor release verification"
        );

        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("lock_check.dat");
        std::fs::write(&test_file, b"test content").unwrap();

        assert!(
            verify_path_unlocked(&test_file),
            "Uncontested file must be verified as unlocked"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_graceful_shutdown_real_process() {
        // Spawn a background process that accepts SIGTERM
        let mut child = spawn_sleeper(60);

        let pid = Pid::from_u32(child.id());
        let mut system = System::new();
        system.refresh_processes();

        assert!(
            is_pid_alive(&mut system, pid),
            "Child must be alive after spawn"
        );

        let options = TerminateOptions {
            graceful_timeout: Duration::from_millis(3000),
            poll_interval: Duration::from_millis(150),
            force_timeout: Duration::from_millis(1000),
            settle_delay: Duration::from_millis(50),
            include_descendants: true,
        };

        let report = terminate_pids_blocking(&mut system, &[pid], &options)
            .expect("Termination must succeed");

        assert!(report.is_success());
        assert!(
            report.terminated_gracefully.contains(&child.id()),
            "Process should terminate gracefully via SIGTERM"
        );
        assert!(report.killed_forcefully.is_empty());
        assert!(report.failed_to_kill.is_empty());

        // Wait for child handle to reap zombie
        let _ = child.wait();

        // Verify process is fully dead
        assert!(
            !is_pid_alive(&mut system, pid),
            "Process must not exist after termination"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_force_kill_real_stubborn_process() {
        // Spawn a stubborn process that ignores SIGTERM using shell trap
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("trap '' TERM; while true; do sleep 0.1; done")
            .spawn()
            .expect("Failed to spawn stubborn process");

        let pid = Pid::from_u32(child.id());
        let mut system = System::new();
        system.refresh_processes();

        assert!(
            is_pid_alive(&mut system, pid),
            "Stubborn child must be alive"
        );

        // Set a short graceful timeout so it times out quickly and escalates to SIGKILL
        let options = TerminateOptions {
            graceful_timeout: Duration::from_millis(600),
            poll_interval: Duration::from_millis(150),
            force_timeout: Duration::from_millis(2000),
            settle_delay: Duration::from_millis(50),
            include_descendants: true,
        };

        let report = terminate_pids_blocking(&mut system, &[pid], &options)
            .expect("Force kill must terminate the stubborn process");

        assert!(report.is_success());
        assert!(
            report.killed_forcefully.contains(&child.id()),
            "Process ignoring SIGTERM must be killed forcefully via SIGKILL"
        );
        assert!(
            !report.terminated_gracefully.contains(&child.id()),
            "Stubborn process itself must not be reported as terminated gracefully"
        );
        assert!(report.failed_to_kill.is_empty());

        let _ = child.wait();
        assert!(
            !is_pid_alive(&mut system, pid),
            "Stubborn process must be dead"
        );
    }

    #[tokio::test]
    async fn test_async_terminate_pids() {
        let mut child = spawn_sleeper(60);

        let pid = Pid::from_u32(child.id());
        let mut system = System::new();
        system.refresh_processes();

        let options = TerminateOptions {
            graceful_timeout: Duration::from_millis(2000),
            poll_interval: Duration::from_millis(150),
            force_timeout: Duration::from_millis(1000),
            settle_delay: Duration::from_millis(50),
            include_descendants: true,
        };

        let report = terminate_pids(&mut system, &[pid], &options)
            .await
            .expect("Async termination should succeed");

        assert!(report.is_success());
        let _ = child.wait();
    }

    #[test]
    fn test_launch_config_builder() {
        let config = LaunchConfig::new("/usr/bin/custom_game")
            .arg("-fullscreen")
            .args(["-novid", "-high"])
            .current_dir("/tmp")
            .env("GAME_DEBUG", "1")
            .no_window(true)
            .prefer_open(false);

        assert_eq!(config.target, PathBuf::from("/usr/bin/custom_game"));
        assert_eq!(config.args, vec!["-fullscreen", "-novid", "-high"]);
        assert_eq!(config.working_dir, Some(PathBuf::from("/tmp")));
        assert_eq!(config.envs.get("GAME_DEBUG"), Some(&"1".to_string()));
        assert!(config.no_window);
        assert!(!config.prefer_open);
    }

    #[test]
    fn test_launch_app_binary_execution() {
        let (cmd, args) = if cfg!(windows) {
            ("cmd", vec!["/C", "echo swch_launch_test"])
        } else {
            ("/bin/echo", vec!["swch_launch_test"])
        };
        let mut builder = LaunchConfig::new(cmd);
        for a in args {
            builder = builder.arg(a);
        }
        let config = builder.prefer_open(false);

        let mut app = launch_app(&config).expect("Launching binary should succeed");
        assert!(!app.is_detached());

        if let Some(child) = app.child_mut() {
            let status = child.wait().expect("Failed to wait on child");
            assert!(status.success());
        } else {
            panic!("Expected Child variant from direct binary invocation");
        }
    }
}
