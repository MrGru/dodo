mod installed_apps;
mod large_old_files;
pub(crate) mod mail_files;
mod system_junk;
mod trash_bins;
mod user_cache;

use std::sync::Arc;

use crate::cleaner::core::scanner::CleanerScanner;

pub use installed_apps::InstalledAppsScanner;
pub use large_old_files::LargeOldFilesScanner;
pub use mail_files::MailFilesScanner;
pub use system_junk::SystemJunkScanner;
pub use trash_bins::TrashBinsScanner;
pub use user_cache::UserCacheScanner;

pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    vec![
        Arc::new(SystemJunkScanner::new()),
        Arc::new(UserCacheScanner::new()),
        Arc::new(MailFilesScanner::new()),
        Arc::new(LargeOldFilesScanner::new()),
        Arc::new(TrashBinsScanner::new()),
        Arc::new(InstalledAppsScanner::new()),
    ]
}
