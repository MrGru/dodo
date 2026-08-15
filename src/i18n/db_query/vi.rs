//! The Vietnamese column of the Database Explorer's query pane.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Query => "Truy vấn".into(),
        Text::Execute => "Chạy".into(),
        Text::Format => "Định dạng".into(),
        Text::Running => "Đang chạy…".into(),
        Text::NoStatement => "Không có gì để chạy.".into(),
        Text::Result => "Kết quả".into(),
        Text::NoResultYet => "Chưa có kết quả".into(),
        Text::NoResultYetHint => {
                "Chạy một câu lệnh để xem các dòng của nó ở đây.".into()
            }
        Text::NoRows => "Câu lệnh không trả về dòng nào.".into(),
        Text::FooterRows(count) => format!("{count} dòng").into(),
        Text::FooterRowsAffected(count) => {
                format!("{count} dòng bị ảnh hưởng").into()
            }
        Text::FooterElapsed(elapsed) => {
                format!("trong {elapsed}").into()
            }
        Text::FooterTruncated(shown) => {
                format!("chỉ hiện {shown} dòng đầu — câu lệnh trả về nhiều hơn").into()
            }
        Text::StatementLabel => "Câu lệnh".into(),
        Text::ColumnNull => "NULL".into(),
        Text::SelectConnection => "Chọn một kết nối".into(),
        Text::SelectConnectionHint => {
                "Chọn một kết nối ở bên trái để duyệt và chạy truy vấn.".into()
            }
        Text::QueryTabTitle(number) => {
                format!("Truy vấn {number}").into()
            }
        Text::NewQueryTab => "Truy vấn mới".into(),
        Text::CloseQueryTab => "Đóng truy vấn".into(),
        Text::CancelQuery => "Huỷ".into(),
        Text::CancelledTitle => "Đã huỷ".into(),
        Text::CancelledHint => {
                "Máy chủ xác nhận đã dừng, nên không còn gì đang chạy ở đó.".into()
            }
        Text::Explain => "Giải thích".into(),
        Text::CopyCell => "Sao chép ô".into(),
        Text::CopyRow => "Sao chép dòng".into(),
        Text::ExportCsv => "Xuất CSV".into(),
        Text::ExportJson => "Xuất JSON".into(),
        Text::History => "Lịch sử".into(),
        Text::HistorySearch => "Tìm trong lịch sử truy vấn…".into(),
        Text::HistoryEmpty => "Chưa có truy vấn nào được chạy.".into(),
        Text::HistoryNoMatches => "Không có truy vấn phù hợp.".into(),
        Text::EditCell => "Sửa ô".into(),
        Text::AddRow => "Thêm dòng".into(),
        Text::DeleteRow => "Xóa dòng".into(),
        Text::DuplicateRow => "Nhân đôi dòng".into(),
        Text::Commit => "Ghi thay đổi".into(),
        Text::Rollback => "Hoàn tác".into(),
        Text::EditSelectRow => "Trước tiên hãy chọn một dòng.".into(),
        Text::EditNoPending => "Không có thay đổi đang chờ.".into(),
        Text::PendingChanges(count) => {
                format!("{count} thay đổi dòng đang chờ").into()
            }
        Text::SetNull => "NULL".into(),
        Text::IdentityRequired(columns) => format!(
                "Nhập giá trị mới cho (các) cột định danh không tự sinh: {columns}."
            )
            .into(),
        Text::CommitTitle => "Xác nhận thay đổi cơ sở dữ liệu".into(),
        Text::CommitSummary(count) => format!(
                "Giao dịch này dự kiến tác động chính xác {count} dòng. Hãy xem từng câu lệnh trước khi ghi thay đổi."
            )
            .into(),
        Text::CommitExactStatements => "Các câu lệnh đã tạo".into(),
        Text::CommitParameters => "Tham số liên kết".into(),
        Text::CommitLostUpdateNotice => {
                "Phiên bản này không phát hiện thay đổi đồng thời; ghi thay đổi có thể ghi đè giá trị mới hơn từ máy khách khác.".into()
            }
        Text::CommitRunning => "Đang ghi thay đổi…".into(),
        Text::CommitStatementLabel(number) => {
                format!("Câu lệnh {number}").into()
            }
        Text::ExpectedOneRow => "Số dòng dự kiến tác động: 1".into(),
        Text::QueryStoreError(detail) => {
                format!("Không thể đọc hoặc ghi truy vấn đã lưu và lịch sử: {detail}").into()
            }
        Text::QueryStoreMissingVersion => {
                "Tệp truy vấn đã lưu không có phiên bản nên chưa được tải.".into()
            }
        Text::QueryStoreUnsupportedVersion { found, supported } => format!(
                "Tệp truy vấn đã lưu dùng phiên bản {found}; Dodo này chỉ hỗ trợ đến {supported}."
            )
            .into(),
        Text::SavedQueries => "Truy vấn đã lưu".into(),
        Text::SaveQuery => "Lưu truy vấn".into(),
        Text::SavedQuerySearch => "Tìm truy vấn đã lưu…".into(),
        Text::SavedQueryEmpty => "Chưa có truy vấn nào được lưu.".into(),
        Text::SavedQueryNoMatches => "Không có truy vấn đã lưu phù hợp.".into(),
        Text::SavedQueryCreateTitle => "Lưu truy vấn".into(),
        Text::SavedQueryEditTitle => "Sửa truy vấn đã lưu".into(),
        Text::SavedQueryName => "Tên".into(),
        Text::SavedQueryNamePlaceholder => "ví dụ: Đơn hàng gần đây".into(),
        Text::SavedQueryStatement => "Truy vấn".into(),
        Text::SavedQueryScope => "Kết nối".into(),
        Text::SavedQueryPlaintextNotice => {
                "Truy vấn được lưu dưới dạng văn bản thuần trên thiết bị này. Hãy xóa mật khẩu và bí mật khác trước khi lưu."
                    .into()
            }
        Text::SavedQueryNameRequired => "Hãy nhập tên cho truy vấn này.".into(),
        Text::SavedQueryStatementRequired => "Hãy nhập nội dung truy vấn để lưu.".into(),
        Text::SavedQueryEdit => "Sửa truy vấn đã lưu".into(),
        Text::SavedQueryDelete => "Xóa truy vấn đã lưu".into(),
        Text::HistoryClear => "Xóa lịch sử".into(),
        Text::HistorySucceeded => "Thành công".into(),
        Text::HistoryFailed => "Thất bại".into(),
        Text::HistoryJustNow => "Vừa xong".into(),
        Text::HistoryMinutesAgo(minutes) => format!("{minutes} phút trước").into(),
        Text::HistoryHoursAgo(hours) => format!("{hours} giờ trước").into(),
        Text::HistoryDaysAgo(days) => format!("{days} ngày trước").into(),
    }
}
