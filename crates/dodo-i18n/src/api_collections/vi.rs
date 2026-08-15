//! The Vietnamese column of the API Explorer's collections.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Collections => "Bộ sưu tập".into(),
        Text::NoCollections => "Chưa có bộ sưu tập nào".into(),
        Text::NoCollectionsHint => "Các yêu cầu đã lưu sẽ được nhóm ở đây.".into(),
        Text::ImportCollection => "Nhập bộ sưu tập".into(),
        Text::NewCollection => "Bộ sưu tập mới".into(),
        Text::NewFolder => "Thư mục mới".into(),
        Text::Rename => "Đổi tên".into(),
        Text::Duplicate => "Nhân đôi".into(),
        Text::Open => "Mở".into(),
        Text::MoreActions => "Thao tác".into(),
        Text::CollectionStoreError(detail) => format!("Không lưu được bộ sưu tập: {detail}").into(),
        Text::CollectionImportError(detail) => format!("Không nhập được tệp đó: {detail}").into(),
        Text::History => "Lịch sử".into(),
        Text::NoHistory => "Chưa có yêu cầu nào".into(),
        Text::NoHistoryHint => "Các yêu cầu bạn gửi sẽ hiện ở đây, mới nhất trước.".into(),
        Text::HistoryReopen => "Mở lại trong thẻ mới".into(),
        Text::HistoryResend => "Gửi lại".into(),
        Text::HistoryClearAll => "Xoá tất cả".into(),
        Text::HistoryJustNow => "vừa xong".into(),
        Text::HistoryMinutesAgo(minutes) => format!("{minutes} phút trước").into(),
        Text::HistoryHoursAgo(hours) => format!("{hours} giờ trước").into(),
        Text::HistoryDaysAgo(days) => format!("{days} ngày trước").into(),
    }
}
