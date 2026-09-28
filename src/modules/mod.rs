pub mod detector;
pub mod downloader;
pub mod manager;
pub mod types;

pub use detector::LauncherDetector;
pub use downloader::{ModuleDownloader, ModuleManifest};
pub use manager::{ModuleManager, ScanReport};
pub use types::{ModuleError, ModuleId, ModuleInfo, ModuleStatus};
