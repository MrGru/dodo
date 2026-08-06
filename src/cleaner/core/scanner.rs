use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::ProgressSink;
use crate::cleaner::core::report::CategoryScanResult;
use crate::cleaner::core::scan_context::ScanContext;

pub trait CleanerScanner: Send + Sync {
    fn category(&self) -> CleanerCategory;
    fn required_permissions(&self) -> &[MacPermission];
    fn scan(
        &self,
        context: &ScanContext,
        progress: &dyn ProgressSink,
        cancellation: &CancellationToken,
    ) -> Result<CategoryScanResult, ScanError>;
}
