use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::ProgressSink;
use crate::cleaner::core::report::CategoryScanResult;
use crate::cleaner::core::scan_context::ScanContext;

pub trait CleanerScanner: Send + Sync {
    /// Which category this scanner answers for. The orchestrator runs every
    /// scanner in turn and reads the category back off the *result*, so no
    /// shipping code calls this yet; it is what a per-category scan, or
    /// naming the category in a failure, would select on. One unit test does
    /// read it — `category`'s
    /// `a_hidden_category_is_never_one_this_build_scans` cross-checks this
    /// build's registry against what the window lists — but a `cfg(test)`-only
    /// use does not satisfy the lint for the ordinary build, so the allow
    /// stays until a real caller lands.
    #[allow(dead_code)]
    fn category(&self) -> CleanerCategory;
    /// What the scanner needs granted before it can see anything. Pending with
    /// `core::permissions`: nothing checks Full Disk Access in round 1.
    #[allow(dead_code)]
    fn required_permissions(&self) -> &[MacPermission];
    fn scan(
        &self,
        context: &ScanContext,
        progress: &dyn ProgressSink,
        cancellation: &CancellationToken,
    ) -> Result<CategoryScanResult, ScanError>;
}
