mod large_old_files;
mod system_junk;
mod trash_bins;
mod user_cache;

use std::sync::Arc;

use crate::cleaner::ai_apps::AiAppsScanner;
use crate::cleaner::core::scanner::CleanerScanner;
use crate::cleaner::docker_cache::DockerCacheScanner;
use crate::cleaner::node_tooling_cache::NodeToolingCacheScanner;

pub use large_old_files::LargeOldFilesScanner;
pub use system_junk::SystemJunkScanner;
pub use trash_bins::TrashBinsScanner;
pub use user_cache::UserCacheScanner;

pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    vec![
        Arc::new(SystemJunkScanner::new()),
        Arc::new(UserCacheScanner::new()),
        Arc::new(LargeOldFilesScanner::new()),
        Arc::new(TrashBinsScanner::new()),
        Arc::new(NodeToolingCacheScanner::new()),
        Arc::new(AiAppsScanner::new(
            crate::paths::HostOs::Unix,
            crate::cleaner::linux::platform::ai_app_activity,
        )),
        Arc::new(DockerCacheScanner::new()),
    ]
}
