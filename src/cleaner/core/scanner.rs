use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::ProgressSink;
use crate::cleaner::core::report::CategoryScanResult;
use crate::cleaner::core::scan_context::ScanContext;

pub trait CleanerScanner: Send + Sync {
    /// Which category this scanner answers for. Round 1's orchestrator runs
    /// every scanner in turn and reads the category back off the result, so
    /// nothing calls this yet; it is what a per-category scan, or naming the
    /// category in a failure, would select on. The allow comes off then.
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
