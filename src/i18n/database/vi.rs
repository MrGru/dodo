//! The Vietnamese column of the Database Explorer's connections.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Connections => "Các kết nối".into(),
        Text::NoConnections => "Chưa có kết nối nào".into(),
        Text::NoConnectionsHint => {
                "Thêm một kết nối để duyệt cơ sở dữ liệu và chạy truy vấn.".into()
            }
        Text::Connect => "Kết nối".into(),
        Text::Disconnect => "Ngắt kết nối".into(),
        Text::Reconnect => "Kết nối lại".into(),
        Text::EditConnection => "Sửa".into(),
        Text::DuplicateConnection => "Nhân bản".into(),
        Text::DeleteConnection => "Xoá".into(),
        Text::CopySuffix => "bản sao".into(),
        Text::StatusConnected => "Đã kết nối".into(),
        Text::StatusConnecting => "Đang kết nối…".into(),
        Text::StatusDisconnected => "Chưa kết nối".into(),
        Text::DeleteConnectionTitle => "Xoá kết nối?".into(),
        Text::DeleteConnectionMessage(name) => format!(
                "“{name}” sẽ bị xoá khỏi danh sách này. Bản thân cơ sở dữ liệu không bị đụng tới."
            )
            .into(),
        Text::TreeEmpty => "Không có gì".into(),
        Text::TreeNotConnected => "Chưa kết nối".into(),
        Text::RefreshTree => "Tải lại".into(),
        Text::QueryPlaceholder => {
                "Viết SQL ở đây rồi nhấn Chạy.".into()
            }
        Text::Unreachable(detail) => {
                format!("Không thể kết nối tới cơ sở dữ liệu: {detail}").into()
            }
        Text::ServerError(detail) => {
                format!("Máy chủ từ chối câu lệnh: {detail}").into()
            }
        Text::ServerErrorCoded { code, detail } => {
                format!("Máy chủ từ chối câu lệnh ({code}): {detail}").into()
            }
        Text::CancelFailed(detail) => format!(
                "Dodo không liên hệ được máy chủ để huỷ, nên câu lệnh có thể vẫn đang chạy: \
                 {detail}"
            )
            .into(),
        Text::ExportSucceeded { rows, path } => {
                format!("Đã xuất {rows} dòng vào {path}.").into()
            }
        Text::ExportCancelled => "Đã huỷ xuất dữ liệu.".into(),
        Text::ExportFailed(detail) => {
                format!("Không thể xuất kết quả: {detail}").into()
            }
        Text::CommandPlaceholder => {
                "Nhập một lệnh Redis trên mỗi dòng.".into()
            }
        Text::EditUnsupported => {
                "Kết quả này chỉ đọc: cơ sở dữ liệu này không hỗ trợ chỉnh sửa bảng an toàn.".into()
            }
        Text::EditNoColumns => {
                "Kết quả này chỉ đọc vì không có cột.".into()
            }
        Text::EditMissingOrigin(column) => format!(
                "Kết quả này chỉ đọc: cột {column} không đến từ một bảng cơ sở."
            )
            .into(),
        Text::EditMultipleTables => {
                "Kết quả này chỉ đọc vì kết hợp nhiều bảng.".into()
            }
        Text::EditDuplicateColumn(column) => format!(
                "Kết quả này chỉ đọc vì cột cơ sở {column} xuất hiện nhiều lần."
            )
            .into(),
        Text::EditNoUniqueIdentity(table) => format!(
                "Kết quả này chỉ đọc: {table} không có khóa chính hoặc chỉ mục duy nhất không NULL."
            )
            .into(),
        Text::EditMissingIdentityColumns { table, columns } => format!(
                "Kết quả này chỉ đọc: (các) cột định danh {columns} của {table} không có trong kết quả."
            )
            .into(),
        Text::EditMetadataFailed(detail) => {
                format!("Kết quả này chỉ đọc vì không thể tải siêu dữ liệu định danh: {detail}").into()
            }
        Text::EditIdentityColumn => {
                "Không thể sửa trực tiếp cột định danh.".into()
            }
        Text::EditIdentityUnavailable => {
                "Không thể thay đổi dòng này vì giá trị định danh đầy đủ không có sẵn.".into()
            }
        Text::EditUnsupportedCell => {
                "Không thể chỉnh sửa ô này một cách an toàn trong kết quả này.".into()
            }
        Text::EditCellTitle(column) => {
                format!("Sửa {column}").into()
            }
        Text::AddRowTitle => "Thêm dòng".into(),
        Text::DuplicateRowTitle => "Nhân đôi dòng".into(),
        Text::CommitSucceeded(count) => {
                format!("Đã ghi {count} thay đổi dòng.").into()
            }
        Text::CommitAffectedMismatch { statement, actual } => format!(
                "Câu lệnh {statement} khớp {actual} dòng thay vì chính xác 1. Toàn bộ giao dịch đã được hoàn tác."
            )
            .into(),
        Text::CommitFailed { statement, detail } => format!(
                "Câu lệnh {statement} thất bại: {detail}. Toàn bộ giao dịch đã được hoàn tác."
            )
            .into(),
        Text::CommitTransactionFailed(detail) => {
                format!("Không thể hoàn tất giao dịch: {detail}").into()
            }
        Text::CommitBuildFailed => {
                "Không thể tạo các thay đổi đang chờ một cách an toàn.".into()
            }
        Text::ResolvePending => {
                "Trước tiên hãy Ghi thay đổi hoặc Hoàn tác các thay đổi đang chờ.".into()
            }
        Text::EditDuplicateRows => {
                "Kết quả này chỉ đọc vì nhiều dòng đang hiển thị có cùng một định danh duy nhất.".into()
            }
        Text::SavedQueryDeleteTitle => "Xóa truy vấn đã lưu?".into(),
        Text::SavedQueryDeleteMessage(name) => {
                format!("Xóa “{name}”? Không thể hoàn tác thao tác này.").into()
            }
        Text::SavedQueryScopeMismatch(name) => format!(
                "Chỉ mở dưới dạng văn bản vì kết nối đã lưu “{name}” không còn hoặc hiện trỏ đến nơi khác. Hãy chọn đúng kết nối trước khi chạy."
            )
            .into(),
        Text::HistoryClearTitle => "Xóa lịch sử truy vấn?".into(),
        Text::HistoryClearMessage => {
                "Xóa toàn bộ lịch sử truy vấn đã lưu? Các truy vấn đã lưu sẽ không bị ảnh hưởng.".into()
            }
        Text::CatalogSearchConnectionUnavailable(name) => format!(
                "Không thể mở kết quả danh mục vì kết nối “{name}” không còn được kết nối hoặc hiện trỏ đến nơi khác."
            )
            .into(),
        Text::QuickNavOpenedConnection(name) => {
                format!("Đã mở kết nối đã lưu \"{name}\".").into()
            }
        Text::QuickNavKeptStoredPassword(name) => format!(
                "Đã mở kết nối đã lưu \"{name}\". dodo giữ mật khẩu đã lưu; mật khẩu vừa dán không \
                 được dùng."
            )
            .into(),
        Text::QuickNavCreatedConnection(name) => {
                format!("Đã tạo kết nối \"{name}\" từ URI vừa dán.").into()
            }
        Text::QuickNavConnectionsLoading => {
                "Các kết nối đã lưu vẫn đang được tải nên chưa tạo gì cả. Hãy dán lại URI sau giây \
                 lát."
                    .into()
            }
    }
}
