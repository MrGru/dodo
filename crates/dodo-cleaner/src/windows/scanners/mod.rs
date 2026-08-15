pub(crate) mod installed_apps;
#[cfg(target_os = "windows")]
mod large_old_files;
#[cfg(target_os = "windows")]
mod system_junk;
#[cfg(target_os = "windows")]
mod trash_bins;
#[cfg(target_os = "windows")]
mod user_cache;

#[cfg(target_os = "windows")]
use std::sync::Arc;

#[cfg(target_os = "windows")]
use crate::ai_apps::AiAppsScanner;
#[cfg(target_os = "windows")]
use crate::core::scanner::CleanerScanner;
#[cfg(target_os = "windows")]
use crate::docker_cache::DockerCacheScanner;
#[cfg(target_os = "windows")]
use crate::node_tooling_cache::NodeToolingCacheScanner;

#[cfg(target_os = "windows")]
pub use installed_apps::InstalledAppsScanner;
#[cfg(target_os = "windows")]
pub use large_old_files::LargeOldFilesScanner;
#[cfg(target_os = "windows")]
pub use system_junk::SystemJunkScanner;
#[cfg(target_os = "windows")]
pub use trash_bins::TrashBinsScanner;
#[cfg(target_os = "windows")]
pub use user_cache::UserCacheScanner;

#[cfg(target_os = "windows")]
pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    vec![
        Arc::new(SystemJunkScanner::new()),
        Arc::new(UserCacheScanner::new()),
        Arc::new(LargeOldFilesScanner::new()),
        Arc::new(TrashBinsScanner::new()),
        Arc::new(InstalledAppsScanner::new()),
        Arc::new(NodeToolingCacheScanner::new()),
        Arc::new(AiAppsScanner::new(
            crate::paths::HostOs::Windows,
            crate::windows::platform::ai_app_activity,
        )),
        Arc::new(DockerCacheScanner::new()),
    ]
}
