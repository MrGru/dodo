//! The Vietnamese column of the Cleaner.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::UnsupportedPlatform => {
                "Cleaner hiện chỉ có trên macOS. Hỗ trợ Windows và Linux sẽ được bổ sung ở các phiên bản sau.".into()
            }
        Text::Scan => "Quét".into(),
        Text::CancelScan => "Huỷ".into(),
        Text::NoResultsYet => {
                "Chưa có kết quả quét cho danh mục này.".into()
            }
        Text::StatusScanning => "Đang quét".into(),
        Text::StatusCancelling => "Đang huỷ".into(),
        Text::StatusPartial => "Hoàn tất một phần".into(),
        Text::StatusCompleted => "Hoàn tất".into(),
        Text::StatusCleaning => "Đang dọn dẹp".into(),
        Text::StatusFailed => "Thất bại".into(),
        Text::SectionCleanup => "Dọn dẹp".into(),
        Text::SectionApplications => "Ứng dụng".into(),
        Text::SectionAdvanced => "Nâng cao".into(),
        Text::CategorySystemJunk => "Rác hệ thống".into(),
        Text::CategoryUserCache => "Bộ đệm người dùng".into(),
        Text::CategoryMailFiles => "Tệp Mail".into(),
        Text::CategoryTrashBins => "Thùng rác".into(),
        Text::CategoryLargeOldFiles => {
                "Tệp lớn & cũ".into()
            }
        Text::CategoryInstalledApps => {
                "Ứng dụng đã cài".into()
            }
        Text::CategoryOrphanedFiles => "Tệp mồ côi".into(),
        Text::CategoryAiApps => "Ứng dụng AI".into(),
        Text::CategoryXcodeJunk => "Rác Xcode".into(),
        Text::CategoryHomebrewCache => {
                "Bộ đệm Homebrew".into()
            }
        Text::CategoryNodeToolingCache => {
                "Bộ đệm công cụ Node".into()
            }
        Text::CategoryDockerCache => "Bộ đệm Docker".into(),
        Text::CategoryUniversalBinaries => {
                "Universal Binary".into()
            }
        Text::CategoryLanguageFiles => {
                "Tệp ngôn ngữ".into()
            }
        Text::Warnings => "Cảnh báo".into(),
        Text::Path => "Đường dẫn".into(),
        Text::Explanation => "Giải thích".into(),
        Text::CopyPath => "Sao chép đường dẫn".into(),
        Text::RevealInFinder => {
                "Hiện trong Finder".into()
            }
        Text::RevealInExplorer => {
                "Hiện trong Explorer".into()
            }
        Text::RevealInFileManager => {
                "Hiện trong trình quản lý tệp".into()
            }
        Text::MoreActions => "Thêm hành động".into(),
        Text::ColumnName => "Tên".into(),
        Text::ColumnRisk => "Rủi ro".into(),
        Text::ColumnSize => "Kích thước".into(),
        Text::ColumnActions => "Hành động".into(),
        Text::RiskSafe => "An toàn".into(),
        Text::RiskReview => "Cần xem lại".into(),
        Text::RiskUserData => "Dữ liệu người dùng".into(),
        Text::RiskAppChange => "Thay đổi ứng dụng".into(),
        Text::RiskProtected => "Được bảo vệ".into(),
        Text::SelectItem => "Chọn".into(),
        Text::DeselectItem => "Bỏ chọn".into(),
        Text::SelectSafeItems => {
                "Chọn mục an toàn".into()
            }
        Text::CleanSelected => {
                "Dọn mục đã chọn".into()
            }
        Text::CleanupReport => {
                "Báo cáo dọn dẹp".into()
            }
        Text::CleanupConfirmTitle => {
                "Chuyển các mục đã chọn vào Thùng rác?".into()
            }
        Text::CleanupConfirmMessage { count, size } => format!("{count} mục sẽ được chuyển vào Thùng rác của macOS. Dung lượng ước tính: {size}.").into(),
        Text::CleanupSuccessCount(count) => {
                format!("Đã chuyển vào Thùng rác: {count}").into()
            }
        Text::CleanupFailureCount(count) => {
                format!("Thất bại: {count}").into()
            }
        Text::PermissionTitle => {
                "Toàn quyền truy cập ổ đĩa".into()
            }
        Text::PermissionExplanation => {
                "Một số danh mục Cleaner cần Toàn quyền truy cập ổ đĩa để kiểm tra an toàn dữ liệu macOS được bảo vệ.".into()
            }
        Text::PermissionOpenSettings => {
                "Mở cài đặt".into()
            }
        Text::PartialPermissionDenied => {
                "Một số vị trí đã bị bỏ qua vì không đủ quyền.".into()
            }
        Text::PartialRootUnavailable => {
                "Một số thư mục gốc cấu hình sẵn không có trên máy này.".into()
            }
        Text::PartialCancelled => {
                "Lượt quét đã bị huỷ trước khi mọi thư mục gốc hoàn tất.".into()
            }
        Text::PartialUnsupported => {
                "Danh mục này sẽ được bổ sung ở giai đoạn Cleaner sau.".into()
            }
        Text::BeginUninstallReview => {
                "Bắt đầu xem xét gỡ cài đặt".into()
            }
        Text::UninstallReviewTitle { name } => {
                format!("Gỡ cài đặt {name}?").into()
            }
        Text::UninstallLoading => {
                "Đang phân tích các tệp liên quan…".into()
            }
        Text::UninstallRefusedProtected => {
                "Không thể gỡ cài đặt ứng dụng hệ thống.".into()
            }
        Text::UninstallRefusedNotApplication => {
                "Không thể xem xét gỡ cài đặt cho mục này.".into()
            }
        Text::UninstallRelatedFilesHeader => {
                "Tệp liên quan".into()
            }
        Text::UninstallNoRelatedFiles => {
                "Không tìm thấy tệp liên quan nào.".into()
            }
        Text::UninstallDestinationNote => {
                "Ứng dụng và các tệp đã chọn sẽ được chuyển vào Thùng rác của macOS. Bạn có thể khôi phục từ Thùng rác cho đến khi nó được dọn sạch."
                    .into()
            }
        Text::UninstallScanOnlyBadge => {
                "Chỉ quét (vị trí hệ thống)".into()
            }
        Text::UninstallMoveToTrash => {
                "Chuyển vào Thùng rác".into()
            }
        Text::UninstallClose => "Đóng".into(),
        Text::UninstallApplication => "Gỡ cài đặt".into(),
        Text::ConfidenceConfirmed => "Chắc chắn".into(),
        Text::ConfidenceHigh => "Cao".into(),
        Text::ConfidenceMedium => "Trung bình".into(),
        Text::ConfidenceLow => "Thấp".into(),
        Text::ConfidenceSharedOrUnsafe => {
                "Chia sẻ hoặc không an toàn".into()
            }
        Text::KeepItem => "Giữ lại".into(),
        Text::IgnoreStoreError(detail) => format!(
                "Không đọc hoặc ghi được cleaner-ignored-items.json: {detail}"
            )
            .into(),
        Text::IgnoreStoreMissingVersion => {
                "cleaner-ignored-items.json không có trường version nên không phải do dodo ghi. \
                 dodo giữ nguyên tệp và không mục nào được đánh dấu giữ lại."
                    .into()
            }
        Text::IgnoreStoreUnsupportedVersion { found, understood } => format!(
                "cleaner-ignored-items.json là phiên bản {found}; bản dodo này hiểu phiên bản \
                 {understood}. dodo giữ nguyên tệp và không mục nào được đánh dấu giữ lại."
            )
            .into(),
        Text::DockerCleanupConfirmTitle => {
                "Xoá các đối tượng Docker đã chọn?".into()
            }
        Text::DockerCleanupConfirmMessage { count, size } => format!(
                "{count} đối tượng Docker sẽ bị xoá qua Docker CLI. Việc này không dùng Thùng \
                 rác và không thể hoàn tác qua dodo. Dung lượng ước tính: {size}."
            )
            .into(),
        Text::ScanDescription => {
                "Quét mục này để tìm các tệp có thể xoá an toàn.".into()
            }
        Text::EntriesScannedCount(count) => {
                format!("Đã quét {count} mục").into()
            }
        Text::BytesDiscovered(size) => {
                format!("Đã tìm thấy {size}").into()
            }
        Text::ReclaimableAmount(size) => {
                format!("Có thể giải phóng {size}").into()
            }
        Text::ItemsFound(count) => format!("{count} mục").into(),
        Text::SafeItemsCount(count) => {
                format!("{count} an toàn").into()
            }
        Text::WarningCount(count) => {
                format!("{count} cảnh báo").into()
            }
        Text::SelectedSummary { count, size } => {
                format!("Đã chọn {count} · {size}").into()
            }
        Text::CleanCount { count, size } => {
                format!("Xoá {count} mục · {size}").into()
            }
        Text::ScanWarningsSummary(count) => {
                format!("{count} vị trí không thể quét").into()
            }
        Text::ScanWarningsShowDetails => {
                "Xem chi tiết".into()
            }
        Text::ScanWarningsHideDetails => "Ẩn chi tiết".into(),
        Text::Rescan => "Quét lại".into(),
        Text::SelectAll => "Chọn tất cả".into(),
        Text::DeselectAll => "Bỏ chọn tất cả".into(),
        Text::PermissionNotNow => {
                "Không phải lúc này".into()
            }
        Text::StatusCompletedWithWarnings => {
                "Hoàn tất có cảnh báo".into()
            }
        Text::StatusCancelled => "Đã hủy".into(),
        Text::EmptyTrash => "Dọn sạch Thùng rác".into(),
        Text::EmptyTrashConfirmTitle => {
                "Dọn sạch Thùng rác?".into()
            }
        Text::EmptyTrashConfirmMessage { count, size } => format!("{count} mục sẽ bị xóa vĩnh viễn. Dung lượng ước tính: {size}.").into(),
        Text::OpenInstalledAppsSettings => {
                "Mở Ứng dụng đã cài đặt của Windows".into()
            }
    }
}
