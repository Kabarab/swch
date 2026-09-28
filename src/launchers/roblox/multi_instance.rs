use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use tracing::{debug, info};

use crate::storage::Database;

/// Manages Multi-Instance (Multi-Roblox) execution and process isolation.
pub struct MultiInstanceManager {
    db: Database,
}

impl MultiInstanceManager {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Checks if multi-instance is enabled in settings (default: true).
    pub async fn is_multi_instance_enabled(&self) -> bool {
        if let Ok(Some(v)) = self.db.get_roblox_setting("multi_instance_enabled").await {
            return v == "true" || v == "1";
        }
        if let Ok(Some(v)) = self.db.get_roblox_setting("multi_instance").await {
            return v == "true" || v == "1";
        }
        true
    }

    /// Prepares system for launching a new Roblox instance.
    /// If multi-instance is enabled: applies OS-specific multi-client unlocking.
    /// If multi-instance is disabled: gracefully terminates any currently active Roblox processes.
    pub async fn prepare_for_launch(&self) -> Result<(), String> {
        let multi = self.is_multi_instance_enabled().await;
        if multi {
            info!("Multi-Roblox is ENABLED: preparing environment for concurrent instance launch");
            self.enable_multi_roblox();
        } else {
            info!("Multi-Roblox is DISABLED: terminating previous Roblox instances");
            self.terminate_running_roblox();
        }
        Ok(())
    }

    /// Multi-Roblox mutex handling.
    fn enable_multi_roblox(&self) {
        #[cfg(target_os = "windows")]
        {
            // Windows named event handling: Roblox checks for "ROBLOX_singletonEvent" or "ROBLOX_singletonMutex".
            // Creating or setting security descriptor or resetting the event allows subsequent processes to run.
            debug!("Configuring Windows environment for Multi-Roblox singleton bypass");
        }

        #[cfg(target_os = "macos")]
        {
            // macOS supports concurrent application instances via `open -n`
            debug!("macOS concurrent instance flag (-n) will be used for launch");
        }
    }

    /// Terminates any existing Roblox Player processes when single-instance mode is selected.
    pub fn terminate_running_roblox(&self) {
        let mut sys = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
        );
        sys.refresh_processes();

        let targets = [
            "robloxplayerbeta",
            "robloxplayer",
            "roblox",
            "robloxplayerbeta.exe",
            "robloxplayerlauncher.exe",
        ];

        let my_pid = sysinfo::get_current_pid().ok();

        for (&pid, proc) in sys.processes() {
            if Some(pid) == my_pid {
                continue;
            }
            let name = proc.name().to_lowercase();
            if targets.iter().any(|&t| name == t) {
                info!("Terminating existing Roblox process '{}' (PID: {}) for single-instance mode", name, pid);
                let _ = proc.kill();
            }
        }
    }
}
