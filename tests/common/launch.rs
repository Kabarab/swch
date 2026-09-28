#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use thiserror::Error;

use super::models::EpicManifestItem;

#[derive(Error, Debug)]
pub enum LaunchError {
    #[error("Executable file not found at path: {0}")]
    ExecutableNotFound(PathBuf),
    #[error("Game is marked as incomplete installation")]
    IncompleteInstall,
    #[error("Missing active Epic account for authenticated game launch")]
    MissingAccount,
    #[error("Failed to generate exchange token: {0}")]
    ExchangeTokenError(String),
    #[error("Process launch execution failed: {0}")]
    ProcessFailed(#[from] std::io::Error),
}

/// Prepared launch command containing both raw arguments and masked display arguments
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub executable_path: PathBuf,
    pub args: Vec<String>,
    pub display_args: Vec<String>,
    pub working_dir: PathBuf,
    pub env: HashMap<String, String>,
}

/// Helper to format launch arguments with sensitive tokens masked
pub struct RedactedLaunchArgs<'a>(pub &'a [String]);

impl<'a> fmt::Display for RedactedLaunchArgs<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let masked = mask_launch_arguments(self.0);
        write!(f, "{}", masked.join(" "))
    }
}

/// Masks sensitive credentials in CLI arguments
pub fn mask_launch_arguments(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|arg| {
            if arg.starts_with("-AUTH_PASSWORD=") {
                "-AUTH_PASSWORD=********".to_string()
            } else if arg.starts_with("-epicovt=") {
                "-epicovt=[REDACTED]".to_string()
            } else {
                arg.clone()
            }
        })
        .collect()
}

/// Constructs standard Unreal Engine / Epic Launcher CLI launch parameters
pub fn build_launch_parameters(
    exchange_code: &str,
    app_name: &str,
    account_id: &str,
    display_name: &str,
    namespace: &str,
    locale: Option<&str>,
    custom_args: Option<&[String]>,
) -> Vec<String> {
    let mut args = Vec::new();

    // Standard Epic / Unreal authentication parameters
    args.push("-AUTH_LOGIN=unused".to_string());
    args.push(format!("-AUTH_PASSWORD={}", exchange_code));
    args.push("-AUTH_TYPE=exchangecode".to_string());
    args.push(format!("-epicapp={}", app_name));
    args.push("-epicenv=Prod".to_string());
    args.push("-EpicPortal".to_string());

    // Extended session context
    args.push(format!("-epicusername={}", display_name));
    args.push(format!("-epicuserid={}", account_id));
    args.push(format!("-epiclocale={}", locale.unwrap_or("en")));
    args.push(format!("-epicsandboxid={}", namespace));

    // Append any extra user-defined arguments
    if let Some(extras) = custom_args {
        for extra in extras {
            args.push(extra.clone());
        }
    }

    args
}

/// Resolves launch command for a manifest item
pub fn prepare_launch_command(
    manifest: &EpicManifestItem,
    exchange_code: &str,
    account_id: &str,
    display_name: &str,
    locale: Option<&str>,
    custom_args: Option<&[String]>,
) -> Result<LaunchCommand, LaunchError> {
    if manifest.is_incomplete_install.unwrap_or(false) {
        return Err(LaunchError::IncompleteInstall);
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

    Ok(LaunchCommand {
        executable_path,
        args,
        display_args,
        working_dir,
        env,
    })
}
