//! Direct Game Launch Engine for Epic Games (Heroic/Legendary Architecture).
//!
//! Provides CLI argument construction, sensitive credential redaction,
//! working directory resolution, and cross-platform game process execution.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::launchers::epic::models::EpicError;
use crate::parsers::EpicManifestItem;

/// Errors arising during game launch preparation or execution.
#[derive(Error, Debug)]
pub enum LaunchError {
    #[error("Executable file not found at path: {0}")]
    ExecutableNotFound(PathBuf),

    #[error("Game is marked as incomplete installation")]
    IncompleteInstall,

    #[error("Game manifest specifies an invalid or empty install location")]
    InvalidInstallLocation,

    #[error("Cannot directly launch DLC/Add-on '{app_name}'; launch main game '{main_game}' instead")]
    CannotLaunchDlc {
        app_name: String,
        main_game: String,
    },

    #[error("Missing active Epic account for authenticated game launch")]
    MissingAccount,

    #[error("Failed to generate exchange token: {0}")]
    ExchangeTokenError(String),

    #[error("Process launch execution failed: {0}")]
    ProcessFailed(#[from] std::io::Error),

    #[error("Epic store error: {0}")]
    Store(String),
}

impl From<LaunchError> for EpicError {
    fn from(err: LaunchError) -> Self {
        match err {
            LaunchError::ExecutableNotFound(p) => {
                EpicError::Io(format!("Executable file not found: {}", p.display()))
            }
            LaunchError::IncompleteInstall => {
                EpicError::Database("Game is marked as incomplete installation".to_string())
            }
            LaunchError::InvalidInstallLocation => {
                EpicError::Database("Game manifest specifies an invalid or empty install location".to_string())
            }
            LaunchError::CannotLaunchDlc { app_name, main_game } => {
                EpicError::Database(format!(
                    "Cannot directly launch DLC '{}'; launch main game '{}' instead",
                    app_name, main_game
                ))
            }
            LaunchError::MissingAccount => EpicError::NoActiveAccount,
            LaunchError::ExchangeTokenError(s) => {
                EpicError::Auth(format!("Failed to generate exchange token: {}", s))
            }
            LaunchError::ProcessFailed(e) => EpicError::Io(e.to_string()),
            LaunchError::Store(s) => EpicError::Database(s),
        }
    }
}

impl From<EpicError> for LaunchError {
    fn from(err: EpicError) -> Self {
        match err {
            EpicError::NoActiveAccount => LaunchError::MissingAccount,
            EpicError::Auth(msg) => LaunchError::ExchangeTokenError(msg),
            other => LaunchError::Store(other.to_string()),
        }
    }
}

/// Prepared launch command containing both raw arguments (for OS process spawning)
/// and sanitized display arguments (for logging, tracing, and UI presentation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchCommand {
    /// Absolute or relative path to game executable or bundle
    pub executable_path: PathBuf,
    /// Exact arguments passed to the binary (includes active exchange code)
    pub args: Vec<String>,
    /// Redacted arguments safe for logging and display (-AUTH_PASSWORD=********)
    pub display_args: Vec<String>,
    /// Working directory for game process execution
    pub working_dir: PathBuf,
    /// Environment variables populated for game process
    pub env: HashMap<String, String>,
}

impl LaunchCommand {
    /// Formats the command line with masked tokens for logging and inspection.
    pub fn display_command_line(&self) -> String {
        format!("\"{}\" {}", self.executable_path.display(), self.display_args.join(" "))
    }

    /// Returns a redacted command-line string suitable for log messages.
    pub fn redacted_command_line(&self) -> String {
        self.display_command_line()
    }

    /// Helper returning display wrapper that masks secrets.
    pub fn redacted_args(&self) -> RedactedLaunchArgs<'_> {
        RedactedLaunchArgs(&self.args)
    }

    /// Converts this LaunchCommand into a standard `std::process::Command`.
    pub fn to_std_command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.executable_path);
        cmd.current_dir(&self.working_dir);
        cmd.args(&self.args);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd
    }

    /// Executes the game command directly, returning the spawned child process.
    pub fn spawn(&self) -> Result<std::process::Child, LaunchError> {
        self.to_std_command().spawn().map_err(LaunchError::ProcessFailed)
    }
}

/// Formatter wrapper that guarantees all sensitive arguments are masked when formatted.
pub struct RedactedLaunchArgs<'a>(pub &'a [String]);

impl<'a> fmt::Display for RedactedLaunchArgs<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let masked = mask_launch_arguments(self.0);
        write!(f, "{}", masked.join(" "))
    }
}

impl<'a> fmt::Debug for RedactedLaunchArgs<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Masks sensitive credentials and tokens in CLI argument slices.
///
/// Rules:
/// - Any argument starting with `-AUTH_PASSWORD=` (case-insensitive) is replaced with `-AUTH_PASSWORD=********`.
/// - Any argument starting with `-epicovt=` (case-insensitive) is replaced with `-epicovt=[REDACTED]`.
/// - All other arguments are preserved verbatim without modification.
pub fn mask_launch_arguments(args: &[String]) -> Vec<String> {
    args.iter().map(|arg| mask_arg(arg)).collect()
}

/// Redacts a single argument string if it contains sensitive tokens.
pub fn mask_arg(arg: &str) -> String {
    if arg.to_ascii_uppercase().starts_with("-AUTH_PASSWORD=") {
        "-AUTH_PASSWORD=********".to_string()
    } else if arg.to_ascii_lowercase().starts_with("-epicovt=") {
        "-epicovt=[REDACTED]".to_string()
    } else {
        arg.to_string()
    }
}

/// Constructs standard Unreal Engine / Epic Launcher CLI launch parameters.
///
/// Argument ordering:
/// 1. Mandatory Epic/Unreal auth flags (`-AUTH_LOGIN=unused`, `-AUTH_PASSWORD=...`, `-AUTH_TYPE=exchangecode`,
///    `-epicapp=...`, `-epicenv=Prod`, `-EpicPortal`)
/// 2. Session context flags (`-epicusername=...`, `-epicuserid=...`, `-epiclocale=...`, `-epicsandboxid=...`)
/// 3. User-defined custom parameters appended at the end.
pub fn build_launch_parameters(
    exchange_code: &str,
    app_name: &str,
    account_id: &str,
    display_name: &str,
    namespace: &str,
    locale: Option<&str>,
    custom_args: Option<&[String]>,
) -> Vec<String> {
    let mut args = Vec::with_capacity(10 + custom_args.map_or(0, |c| c.len()));

    // 1. Mandatory Epic / Unreal authentication parameters
    args.push("-AUTH_LOGIN=unused".to_string());
    args.push(format!("-AUTH_PASSWORD={}", exchange_code));
    args.push("-AUTH_TYPE=exchangecode".to_string());
    args.push(format!("-epicapp={}", app_name));
    args.push("-epicenv=Prod".to_string());
    args.push("-EpicPortal".to_string());

    // 2. Extended session context
    args.push(format!("-epicusername={}", display_name));
    args.push(format!("-epicuserid={}", account_id));

    let effective_locale = locale
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("en");
    args.push(format!("-epiclocale={}", effective_locale));

    args.push(format!("-epicsandboxid={}", namespace));

    // 3. User-defined custom arguments
    if let Some(extras) = custom_args {
        for extra in extras {
            args.push(extra.clone());
        }
    }

    args
}

/// Resolves and prepares a complete `LaunchCommand` for a locally installed manifest item.
///
/// Steps:
/// 1. Verifies that the installation is complete (`bIsIncompleteInstall` is false/absent).
/// 2. Checks DLC constraints (cannot directly launch DLC).
/// 3. Resolves executable path using manifest logic (including macOS Mach-O bundle resolution).
/// 4. Determines the process working directory (parent directory of binary if file exists, else install directory).
/// 5. Builds raw launch parameters and masked display arguments.
/// 6. Injects standard environment variables (`EPIC_APP_NAME`, `EPIC_NAMESPACE`, `EPIC_ACCOUNT_ID`, `EPIC_DISPLAY_NAME`).
pub fn prepare_launch_command(
    manifest: &EpicManifestItem,
    exchange_code: &str,
    account_id: &str,
    display_name: &str,
    locale: Option<&str>,
    custom_args: Option<&[String]>,
) -> Result<LaunchCommand, LaunchError> {
    if manifest.install_location.trim().is_empty() {
        return Err(LaunchError::InvalidInstallLocation);
    }

    if manifest.is_incomplete_install.unwrap_or(false) {
        return Err(LaunchError::IncompleteInstall);
    }

    if manifest.is_dlc() {
        return Err(LaunchError::CannotLaunchDlc {
            app_name: manifest.app_name.clone(),
            main_game: manifest.main_game_app_name.clone().unwrap_or_default(),
        });
    }

    let executable_path = manifest.resolve_executable_path();
    let working_dir = if executable_path.is_file() {
        executable_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(&manifest.install_location))
    } else {
        PathBuf::from(&manifest.install_location)
    };

    let args = build_launch_parameters(
        exchange_code,
        &manifest.app_name,
        account_id,
        display_name,
        &manifest.catalog_namespace,
        locale,
        custom_args,
    );

    let display_args = mask_launch_arguments(&args);

    let mut env = HashMap::new();
    env.insert("EPIC_APP_NAME".to_string(), manifest.app_name.clone());
    env.insert(
        "EPIC_NAMESPACE".to_string(),
        manifest.catalog_namespace.clone(),
    );
    env.insert("EPIC_ACCOUNT_ID".to_string(), account_id.to_string());
    env.insert("EPIC_DISPLAY_NAME".to_string(), display_name.to_string());

    Ok(LaunchCommand {
        executable_path,
        args,
        display_args,
        working_dir,
        env,
    })
}

/// Standard I/O redirection mode for spawned game processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StdioMode {
    Null,
    #[default]
    Inherit,
    Piped,
}

impl StdioMode {
    pub fn to_stdio(self) -> std::process::Stdio {
        match self {
            StdioMode::Null => std::process::Stdio::null(),
            StdioMode::Inherit => std::process::Stdio::inherit(),
            StdioMode::Piped => std::process::Stdio::piped(),
        }
    }
}

/// Options controlling game process execution and simulation.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// When true, validates execution without spawning an OS process (ideal for CI/tests)
    pub dry_run: bool,
    /// Standard input handling
    pub stdin: StdioMode,
    /// Standard output handling
    pub stdout: StdioMode,
    /// Standard error handling
    pub stderr: StdioMode,
    /// Additional custom environment variables to merge
    pub extra_env: HashMap<String, String>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            stdin: StdioMode::Null,
            stdout: StdioMode::Inherit,
            stderr: StdioMode::Inherit,
            extra_env: HashMap::new(),
        }
    }
}

impl LaunchOptions {
    pub fn simulated() -> Self {
        Self {
            dry_run: true,
            ..Default::default()
        }
    }
}

/// Represents a spawned game process or a dry-run simulation.
pub struct LaunchedProcess {
    pid: u32,
    child: Option<std::process::Child>,
    dry_run: bool,
    display_command: String,
}

impl fmt::Debug for LaunchedProcess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaunchedProcess")
            .field("pid", &self.pid)
            .field("dry_run", &self.dry_run)
            .field("display_command", &self.display_command)
            .finish()
    }
}

impl LaunchedProcess {
    pub fn new_live(child: std::process::Child, display_command: String) -> Self {
        let pid = child.id();
        Self {
            pid,
            child: Some(child),
            dry_run: false,
            display_command,
        }
    }

    pub fn new_simulated(pid: u32, display_command: String) -> Self {
        Self {
            pid,
            child: None,
            dry_run: true,
            display_command,
        }
    }

    /// Process ID (OS PID for live processes, synthetic PID for dry-run simulation)
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Returns true if this process was launched in simulation/dry-run mode
    pub fn is_simulated(&self) -> bool {
        self.dry_run
    }

    /// Returns the sanitized command line used to launch the process
    pub fn display_command(&self) -> &str {
        &self.display_command
    }

    /// Extracts the inner `std::process::Child` if running live
    pub fn take_child(&mut self) -> Option<std::process::Child> {
        self.child.take()
    }

    /// Non-blocking check for process exit
    pub fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>, std::io::Error> {
        if let Some(ref mut child) = self.child {
            child.try_wait()
        } else {
            Ok(None)
        }
    }

    /// Blocking wait for process termination
    pub fn wait(&mut self) -> Result<std::process::ExitStatus, std::io::Error> {
        if let Some(ref mut child) = self.child {
            child.wait()
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                Ok(std::process::ExitStatus::from_raw(0))
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::ExitStatusExt;
                Ok(std::process::ExitStatus::from_raw(0))
            }
        }
    }

    /// Terminates the process
    pub fn kill(&mut self) -> Result<(), std::io::Error> {
        if let Some(ref mut child) = self.child {
            child.kill()
        } else {
            Ok(())
        }
    }
}

/// Spawns the game process with default options.
/// Checks the `SWCH_EPIC_SIMULATE_LAUNCH` environment variable for automated test simulation.
pub fn execute_game(cmd: &LaunchCommand) -> Result<LaunchedProcess, LaunchError> {
    let mut options = LaunchOptions::default();
    if std::env::var("SWCH_EPIC_SIMULATE_LAUNCH").is_ok() {
        options.dry_run = true;
    }
    execute_game_with_options(cmd, &options)
}

/// Spawns the game process with explicit options (dry-run, stdio redirection, extra env).
pub fn execute_game_with_options(
    cmd: &LaunchCommand,
    options: &LaunchOptions,
) -> Result<LaunchedProcess, LaunchError> {
    // 1. Log launch details with masked arguments (Never log unmasked cmd.args!)
    let display_cmd_line = cmd.display_command_line();
    tracing::info!(
        executable = %cmd.executable_path.display(),
        working_dir = %cmd.working_dir.display(),
        args = %cmd.redacted_args(),
        "Spawning Epic game process"
    );

    // 2. Handle simulation / dry-run mode
    if options.dry_run {
        let simulated_pid = 42424;
        tracing::info!(
            pid = simulated_pid,
            command = %display_cmd_line,
            "Simulated launch completed successfully (dry-run)"
        );
        return Ok(LaunchedProcess::new_simulated(simulated_pid, display_cmd_line));
    }

    // 3. Cross-platform executable resolution & pre-flight verification
    let real_executable = resolve_platform_spawn_executable(&cmd.executable_path)?;

    // 4. Construct Command
    let mut command = std::process::Command::new(&real_executable);
    command.current_dir(&cmd.working_dir);
    command.args(&cmd.args);

    for (k, v) in &cmd.env {
        command.env(k, v);
    }
    for (k, v) in &options.extra_env {
        command.env(k, v);
    }

    command.stdin(options.stdin.to_stdio());
    command.stdout(options.stdout.to_stdio());
    command.stderr(options.stderr.to_stdio());

    // 5. Non-blocking spawn
    let child = command.spawn().map_err(|e| {
        tracing::error!(
            executable = %real_executable.display(),
            error = %e,
            "Failed to spawn game process"
        );
        LaunchError::ProcessFailed(e)
    })?;

    let pid = child.id();
    tracing::info!(
        pid = pid,
        executable = %real_executable.display(),
        "Game process successfully started"
    );

    Ok(LaunchedProcess::new_live(child, display_cmd_line))
}

/// Helper to resolve the binary to spawn across platforms.
/// On macOS, if the target is a `.app` bundle directory, checks `Contents/MacOS` for Mach-O binary.
fn resolve_platform_spawn_executable(path: &Path) -> Result<PathBuf, LaunchError> {
    if !path.exists() {
        return Err(LaunchError::ExecutableNotFound(path.to_path_buf()));
    }

    #[cfg(target_os = "macos")]
    {
        if path.is_dir() && path.extension().is_some_and(|ext| ext == "app") {
            let macos_dir = path.join("Contents/MacOS");
            if macos_dir.is_dir() {
                let plist = path.join("Contents/Info.plist");
                if let Ok(content) = std::fs::read_to_string(&plist) {
                    if let Some(exe_name) = extract_cf_bundle_exe(&content) {
                        let candidate = macos_dir.join(&exe_name);
                        if candidate.is_file() {
                            return Ok(candidate);
                        }
                    }
                }
                if let Some(stem) = path.file_stem() {
                    let candidate = macos_dir.join(stem);
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                }
            }
        }
    }

    Ok(path.to_path_buf())
}

#[cfg(target_os = "macos")]
fn extract_cf_bundle_exe(plist: &str) -> Option<String> {
    let key = "<key>CFBundleExecutable</key>";
    if let Some(pos) = plist.find(key) {
        let after = &plist[pos + key.len()..];
        if let Some(start) = after.find("<string>") {
            let after_str = &after[start + 8..];
            if let Some(end) = after_str.find("</string>") {
                let name = after_str[..end].trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

/// Result of preparing and/or launching an Epic game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchResult {
    /// Target application / catalog identifier (e.g. "Sugar", "Fortnite")
    pub app_name: String,

    /// Raw CLI arguments passed to the process (contains sensitive one-time exchange code)
    pub launch_arguments: Vec<String>,

    /// Sanitized CLI arguments safe for display and logging (-AUTH_PASSWORD=********)
    pub display_arguments: Vec<String>,

    /// Process ID of the spawned game child process (if executed), or None
    pub pid: Option<u32>,

    /// Absolute path to the resolved executable binary
    pub executable_path: Option<PathBuf>,

    /// Working directory for process execution
    pub working_dir: Option<PathBuf>,
}

/// Optional configuration parameters for game launching.
#[derive(Debug, Clone, Default)]
pub struct LaunchGameOptions {
    /// Specific Epic account ID. If None, uses the currently active Epic account.
    pub account_id: Option<String>,

    /// Target application or catalog item name (e.g. "Sugar", "Fortnite").
    pub app_name: String,

    /// Catalog namespace / sandbox ID (e.g. "fn", "0a2d5924765643a39e05465c04da7d50").
    pub namespace: Option<String>,

    /// Game locale (e.g. "en", "ru-RU"). Defaults to "en" if None.
    pub locale: Option<String>,

    /// Additional custom user CLI arguments appended to command line.
    pub additional_args: Option<Vec<String>>,

    /// When true, verifies and constructs launch command without spawning an OS process.
    pub dry_run: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_launch_parameters_mandatory_keys_order() {
        let args = build_launch_parameters(
            "ex_code_999",
            "Fortnite",
            "acc_test_1",
            "Gamer123",
            "fn",
            Some("en"),
            None,
        );

        // Verify exact mandatory keys
        assert_eq!(args[0], "-AUTH_LOGIN=unused");
        assert_eq!(args[1], "-AUTH_PASSWORD=ex_code_999");
        assert_eq!(args[2], "-AUTH_TYPE=exchangecode");
        assert_eq!(args[3], "-epicapp=Fortnite");
        assert_eq!(args[4], "-epicenv=Prod");
        assert_eq!(args[5], "-EpicPortal");

        // Verify context keys
        assert_eq!(args[6], "-epicusername=Gamer123");
        assert_eq!(args[7], "-epicuserid=acc_test_1");
        assert_eq!(args[8], "-epiclocale=en");
        assert_eq!(args[9], "-epicsandboxid=fn");
    }

    #[test]
    fn test_build_launch_parameters_custom_locale_and_empty_fallback() {
        let custom_loc = build_launch_parameters(
            "code", "App", "acc", "User", "ns", Some("ru-RU"), None,
        );
        assert!(custom_loc.contains(&"-epiclocale=ru-RU".to_string()));

        let empty_loc = build_launch_parameters(
            "code", "App", "acc", "User", "ns", Some("   "), None,
        );
        assert!(empty_loc.contains(&"-epiclocale=en".to_string()));

        let none_loc = build_launch_parameters(
            "code", "App", "acc", "User", "ns", None, None,
        );
        assert!(none_loc.contains(&"-epiclocale=en".to_string()));
    }

    #[test]
    fn test_build_launch_parameters_custom_args_appended() {
        let custom = vec!["-fullscreen".to_string(), "-novid".to_string(), "-fps=144".to_string()];
        let args = build_launch_parameters(
            "code", "App", "acc", "User", "ns", None, Some(&custom),
        );

        assert_eq!(args.len(), 13);
        assert_eq!(args[10], "-fullscreen");
        assert_eq!(args[11], "-novid");
        assert_eq!(args[12], "-fps=144");
    }

    #[test]
    fn test_mask_launch_arguments_password() {
        let args = vec![
            "-AUTH_LOGIN=unused".to_string(),
            "-AUTH_PASSWORD=super_confidential_token_xyz".to_string(),
            "-AUTH_TYPE=exchangecode".to_string(),
        ];
        let masked = mask_launch_arguments(&args);
        assert_eq!(masked[0], "-AUTH_LOGIN=unused");
        assert_eq!(masked[1], "-AUTH_PASSWORD=********");
        assert_eq!(masked[2], "-AUTH_TYPE=exchangecode");
    }

    #[test]
    fn test_mask_launch_arguments_empty_password() {
        let args = vec!["-AUTH_PASSWORD=".to_string()];
        let masked = mask_launch_arguments(&args);
        assert_eq!(masked[0], "-AUTH_PASSWORD=********");
    }

    #[test]
    fn test_mask_launch_arguments_multiple_secrets() {
        let args = vec![
            "-AUTH_PASSWORD=secret1".to_string(),
            "-epicovt=confidential_ovt".to_string(),
            "-epicapp=Game".to_string(),
        ];
        let masked = mask_launch_arguments(&args);
        assert_eq!(masked[0], "-AUTH_PASSWORD=********");
        assert_eq!(masked[1], "-epicovt=[REDACTED]");
        assert_eq!(masked[2], "-epicapp=Game");
    }

    #[test]
    fn test_mask_launch_arguments_case_insensitivity() {
        let args = vec![
            "-auth_password=case_variation".to_string(),
            "-EPICOVT=another_case".to_string(),
        ];
        let masked = mask_launch_arguments(&args);
        assert_eq!(masked[0], "-AUTH_PASSWORD=********");
        assert_eq!(masked[1], "-epicovt=[REDACTED]");
    }

    #[test]
    fn test_mask_launch_arguments_non_sensitive_untouched() {
        let args = vec![
            "-epicapp=Sugar".to_string(),
            "-epicenv=Prod".to_string(),
            "-epicusername=TestUser".to_string(),
        ];
        let masked = mask_launch_arguments(&args);
        assert_eq!(masked, args);
    }

    #[test]
    fn test_redacted_launch_args_display_and_debug() {
        let secret = "sensitive_exchange_code_443322";
        let args = vec![
            "-AUTH_LOGIN=unused".to_string(),
            format!("-AUTH_PASSWORD={}", secret),
            "-EpicPortal".to_string(),
        ];

        let display_out = format!("{}", RedactedLaunchArgs(&args));
        assert!(!display_out.contains(secret));
        assert!(display_out.contains("-AUTH_PASSWORD=********"));

        let debug_out = format!("{:?}", RedactedLaunchArgs(&args));
        assert!(!debug_out.contains(secret));
        assert!(debug_out.contains("-AUTH_PASSWORD=********"));
    }

    #[test]
    fn test_prepare_launch_command_incomplete_install() {
        let manifest = EpicManifestItem {
            format_version: Some(0),
            app_name: "IncompleteGame".to_string(),
            catalog_item_id: "cat_1".to_string(),
            catalog_namespace: "ns_1".to_string(),
            display_name: "Incomplete".to_string(),
            installation_guid: "guid".to_string(),
            install_location: "/games/incomplete".to_string(),
            install_size: 100,
            launch_executable: "game.exe".to_string(),
            launch_command: None,
            main_game_app_name: None,
            mandatory_app_folder_name: None,
            is_incomplete_install: Some(true),
            can_run_offline: Some(true),
            requires_auth: Some(true),
            app_version_string: None,
        };

        let res = prepare_launch_command(&manifest, "token", "acc_1", "User", None, None);
        assert!(matches!(res, Err(LaunchError::IncompleteInstall)));
    }

    #[test]
    fn test_prepare_launch_command_dlc_rejection() {
        let manifest = EpicManifestItem {
            format_version: Some(0),
            app_name: "DlcApp".to_string(),
            catalog_item_id: "cat_dlc".to_string(),
            catalog_namespace: "ns_1".to_string(),
            display_name: "Expansion Pack".to_string(),
            installation_guid: "guid_dlc".to_string(),
            install_location: "/games/base".to_string(),
            install_size: 50,
            launch_executable: "base.exe".to_string(),
            launch_command: None,
            main_game_app_name: Some("BaseGameApp".to_string()),
            mandatory_app_folder_name: None,
            is_incomplete_install: Some(false),
            can_run_offline: Some(true),
            requires_auth: Some(true),
            app_version_string: None,
        };

        let res = prepare_launch_command(&manifest, "token", "acc_1", "User", None, None);
        assert!(matches!(res, Err(LaunchError::CannotLaunchDlc { .. })));
    }

    #[test]
    fn test_execute_game_dry_run_simulation() {
        let cmd = LaunchCommand {
            executable_path: PathBuf::from("/nonexistent/game.exe"),
            args: vec!["-AUTH_LOGIN=unused".to_string()],
            display_args: vec!["-AUTH_LOGIN=unused".to_string()],
            working_dir: PathBuf::from("/nonexistent"),
            env: HashMap::new(),
        };

        let options = LaunchOptions::simulated();
        let proc = execute_game_with_options(&cmd, &options).expect("Dry-run should succeed");
        assert_eq!(proc.pid(), 42424);
        assert!(proc.is_simulated());
    }

    #[test]
    fn test_execute_game_nonexistent_executable_fails() {
        let cmd = LaunchCommand {
            executable_path: PathBuf::from("/definitely/does/not/exist/at/all_game.exe"),
            args: vec!["-AUTH_LOGIN=unused".to_string()],
            display_args: vec!["-AUTH_LOGIN=unused".to_string()],
            working_dir: PathBuf::from("/definitely"),
            env: HashMap::new(),
        };

        let options = LaunchOptions::default();
        let res = execute_game_with_options(&cmd, &options);
        assert!(matches!(res, Err(LaunchError::ExecutableNotFound(_))));
    }

    #[test]
    fn test_prepare_launch_command_empty_install_location_rejected() {
        let mut manifest = EpicManifestItem {
            format_version: Some(0),
            app_name: "TestApp".to_string(),
            catalog_item_id: "cat_1".to_string(),
            catalog_namespace: "ns_1".to_string(),
            display_name: "Test Game".to_string(),
            installation_guid: "guid_1".to_string(),
            install_location: "".to_string(),
            install_size: 100,
            launch_executable: "game.exe".to_string(),
            launch_command: None,
            main_game_app_name: None,
            mandatory_app_folder_name: None,
            is_incomplete_install: Some(false),
            can_run_offline: Some(true),
            requires_auth: Some(true),
            app_version_string: None,
        };

        let res = prepare_launch_command(&manifest, "token", "acc_1", "User", None, None);
        assert!(matches!(res, Err(LaunchError::InvalidInstallLocation)));

        manifest.install_location = "   \t \n  ".to_string();
        let res_whitespace = prepare_launch_command(&manifest, "token", "acc_1", "User", None, None);
        assert!(matches!(res_whitespace, Err(LaunchError::InvalidInstallLocation)));
    }

    #[test]
    fn test_launched_process_debug_implementation() {
        let proc = LaunchedProcess::new_simulated(12345, "game.exe -AUTH_PASSWORD=********".to_string());
        let debug_str = format!("{:?}", proc);
        assert!(debug_str.contains("LaunchedProcess"));
        assert!(debug_str.contains("12345"));
        assert!(debug_str.contains("dry_run: true"));
        assert!(debug_str.contains("display_command: \"game.exe -AUTH_PASSWORD=********\""));

        // Verify std Result unwrap_err / expect_err idioms work with LaunchedProcess: fmt::Debug
        let res: Result<LaunchedProcess, &'static str> = Err("test error");
        assert_eq!(res.unwrap_err(), "test error");
    }
}
