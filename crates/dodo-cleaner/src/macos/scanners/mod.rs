pub(crate) mod homebrew_cache;
mod installed_apps;
mod language_files;
mod large_old_files;
pub(crate) mod mail_files;
mod orphaned_files;
mod system_junk;
mod trash_bins;
mod universal_binaries;
mod user_cache;
pub(crate) mod xcode_junk;

use std::sync::Arc;

use crate::ai_apps::AiAppsScanner;
use crate::core::scanner::CleanerScanner;
use crate::docker_cache::DockerCacheScanner;
use crate::node_tooling_cache::NodeToolingCacheScanner;
pub use homebrew_cache::HomebrewCacheScanner;
pub use installed_apps::InstalledAppsScanner;
pub use language_files::LanguageFilesScanner;
pub use large_old_files::LargeOldFilesScanner;
pub use mail_files::MailFilesScanner;
pub use orphaned_files::OrphanedFilesScanner;
pub use system_junk::SystemJunkScanner;
pub use trash_bins::TrashBinsScanner;
pub use universal_binaries::UniversalBinariesScanner;
pub use user_cache::UserCacheScanner;
pub use xcode_junk::XcodeJunkScanner;

pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    vec![
        Arc::new(SystemJunkScanner::new()),
        Arc::new(UserCacheScanner::new()),
        Arc::new(MailFilesScanner::new()),
        Arc::new(LargeOldFilesScanner::new()),
        Arc::new(TrashBinsScanner::new()),
        Arc::new(InstalledAppsScanner::new()),
        Arc::new(OrphanedFilesScanner::new()),
        Arc::new(XcodeJunkScanner::new()),
        Arc::new(HomebrewCacheScanner::new()),
        Arc::new(NodeToolingCacheScanner::new()),
        Arc::new(AiAppsScanner::new(
            crate::paths::HostOs::MacOs,
            crate::macos::platform::ai_app_activity,
        )),
        Arc::new(DockerCacheScanner::new()),
        Arc::new(UniversalBinariesScanner::new()),
        Arc::new(LanguageFilesScanner::new()),
    ]
}
