pub(crate) mod installed_apps;
#[cfg(target_os = "linux")]
mod large_old_files;
#[cfg(target_os = "linux")]
mod system_junk;
#[cfg(target_os = "linux")]
mod trash_bins;
#[cfg(target_os = "linux")]
mod user_cache;

#[cfg(target_os = "linux")]
use std::sync::Arc;

#[cfg(target_os = "linux")]
use crate::cleaner::ai_apps::AiAppsScanner;
#[cfg(target_os = "linux")]
use crate::cleaner::core::scanner::CleanerScanner;
#[cfg(target_os = "linux")]
use crate::cleaner::docker_cache::DockerCacheScanner;
#[cfg(target_os = "linux")]
use crate::cleaner::node_tooling_cache::NodeToolingCacheScanner;

#[cfg(target_os = "linux")]
pub use installed_apps::InstalledAppsScanner;
#[cfg(target_os = "linux")]
pub use large_old_files::LargeOldFilesScanner;
#[cfg(target_os = "linux")]
pub use system_junk::SystemJunkScanner;
#[cfg(target_os = "linux")]
pub use trash_bins::TrashBinsScanner;
#[cfg(target_os = "linux")]
pub use user_cache::UserCacheScanner;

#[cfg(target_os = "linux")]
pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    vec![
        Arc::new(SystemJunkScanner::new()),
        Arc::new(UserCacheScanner::new()),
        Arc::new(LargeOldFilesScanner::new()),
        Arc::new(TrashBinsScanner::new()),
        Arc::new(InstalledAppsScanner::new()),
        Arc::new(NodeToolingCacheScanner::new()),
        Arc::new(AiAppsScanner::new(
            crate::paths::HostOs::Unix,
            crate::cleaner::linux::platform::ai_app_activity,
        )),
        Arc::new(DockerCacheScanner::new()),
    ]
}
