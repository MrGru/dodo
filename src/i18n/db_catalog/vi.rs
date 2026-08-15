//! The Vietnamese column of the Database Explorer's catalog.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::StatusError => "Lỗi".into(),
        Text::GroupTables => "Bảng".into(),
        Text::GroupViews => "Khung nhìn".into(),
        Text::GroupColumns => "Cột".into(),
        Text::GroupIndexes => "Chỉ mục".into(),
        Text::GroupConstraints => "Ràng buộc".into(),
        Text::TreeLoading => "Đang tải…".into(),
        Text::FooterCapped(count) => {
                format!("{count} giá trị lớn đã được rút gọn").into()
            }
        Text::CancelledMessage => {
                "Máy chủ đã dừng câu lệnh vì bạn huỷ nó.".into()
            }
        Text::DetailData => "Dữ liệu".into(),
        Text::DetailDdl => "DDL".into(),
        Text::DetailFieldNullable => "Cho phép NULL".into(),
        Text::DetailFieldNotNull => "Không NULL".into(),
        Text::DetailFieldDefault => "Mặc định".into(),
        Text::DetailFieldUnique => "Duy nhất".into(),
        Text::DetailFieldPrimary => "Chính".into(),
        Text::DetailFieldDefinition => "Định nghĩa".into(),
        Text::DetailClose => "Đóng chi tiết đối tượng".into(),
        Text::DetailUnavailable => {
                "Chi tiết này không có sẵn cho đối tượng này.".into()
            }
        Text::DetailNoRows => {
                "Đối tượng này không có dòng nào.".into()
            }
        Text::DetailNoMetadata => {
                "Không có siêu dữ liệu nào được báo cáo.".into()
            }
        Text::DetailPrevious => "Trước".into(),
        Text::DetailNext => "Tiếp".into(),
        Text::DetailPage(page) => format!("Trang {page}").into(),
        Text::DetailRowsRange { first, last } => {
                format!("Dòng {first}–{last}").into()
            }
        Text::DetailDdlReconstructed => {
                "Được dựng lại từ siêu dữ liệu danh mục PostgreSQL; có thể thiếu phân vùng, kế \
                 thừa, thiết lập lưu trữ, chú thích và quyền sở hữu."
                    .into()
            }
        Text::DetailConstraintsPartial => {
                "SQLite không cung cấp ràng buộc CHECK dưới dạng dòng danh mục. Xem DDL đã lưu \
                 để biết định nghĩa đầy đủ."
                    .into()
            }
        Text::DetailCopyDdl => "Sao chép DDL".into(),
        Text::DetailMetadataTruncated(count) => {
                format!("Đang hiện {count} dòng siêu dữ liệu đầu tiên.").into()
            }
        Text::GroupMore => "Thêm…".into(),
        Text::CatalogSearch => "Tìm trong danh mục".into(),
        Text::CatalogSearchPlaceholder => "Tìm đối tượng danh mục…".into(),
        Text::CatalogSearchLoading => "Đang tải danh mục đã kết nối…".into(),
        Text::CatalogSearchEmpty => "Không tìm thấy đối tượng danh mục nào.".into(),
        Text::CatalogSearchNoMatches => "Không có đối tượng danh mục phù hợp.".into(),
        Text::CatalogSearchConnectedOnly => {
                "Tìm kiếm bao gồm các cơ sở dữ liệu đã kết nối và tạo một bộ nhớ đệm danh mục trong bộ nhớ có giới hạn."
                    .into()
            }
        Text::CatalogSearchTruncated(count) => {
                format!("Tìm kiếm dừng ở giới hạn danh mục sau khi lập chỉ mục {count} đối tượng.").into()
            }
        Text::CatalogSearchPartial(count) => {
                format!("Không thể tìm trong {count} nhánh danh mục.").into()
            }
        Text::CatalogKindDatabase => "Cơ sở dữ liệu".into(),
        Text::CatalogKindSchema => "Lược đồ".into(),
        Text::CatalogKindTable => "Bảng".into(),
        Text::CatalogKindView => "Khung nhìn".into(),
        Text::CatalogKindColumn => "Cột".into(),
        Text::CatalogKindIndex => "Chỉ mục".into(),
        Text::CatalogKindConstraint => "Ràng buộc".into(),
        Text::CatalogKindNamespace => "Không gian tên".into(),
        Text::CatalogKindKey => "Khóa".into(),
        Text::CatalogKindObject => "Đối tượng".into(),
    }
}
