//! The Vietnamese column of the Docker tool.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Docker => "Docker".into(),
        Text::Containers => "Containers".into(),
        Text::Images => "Images".into(),
        Text::Volumes => "Volumes".into(),
        Text::Networks => "Networks".into(),
        Text::SearchPlaceholder => "Tìm container".into(),
        Text::Refresh => "Làm mới".into(),
        Text::Filter => "Bộ lọc".into(),
        Text::Create => "Tạo mới".into(),
        Text::ColumnName => "Tên".into(),
        Text::ColumnImage => "Image".into(),
        Text::ColumnStatus => "Trạng thái".into(),
        Text::ColumnCpu => "CPU %".into(),
        Text::ColumnPorts => "Cổng".into(),
        Text::ColumnLastStarted => "Khởi động lần cuối".into(),
        Text::ColumnActions => "Thao tác".into(),
        Text::StatusRunning => "Đang chạy".into(),
        Text::StatusExited => "Đã dừng".into(),
        Text::StatusCreated => "Đã tạo".into(),
        Text::StatusRestarting => "Đang khởi động lại".into(),
        Text::StatusPaused => "Tạm dừng".into(),
        Text::StatusDead => "Đã hỏng".into(),
        Text::StatusRemoving => "Đang xoá".into(),
        Text::StatusStopping => "Đang dừng".into(),
        Text::StatusUnknown => "Không rõ".into(),
        Text::Start => "Khởi động".into(),
        Text::Stop => "Dừng".into(),
        Text::Restart => "Khởi động lại".into(),
        Text::DeleteTitle => "Xoá container?".into(),
        Text::DeleteMessage(name) => {
                format!("Xoá vĩnh viễn \"{name}\"? Hành động này không thể hoàn tác.").into()
            }
        Text::Cancel => "Huỷ".into(),
        Text::NoContainers => "Không tìm thấy container nào.".into(),
        Text::NoContainersHint => {
                "Các container bạn tạo sẽ hiển thị ở đây.".into()
            }
        Text::Retry => "Thử lại".into(),
        Text::ConnectionError(detail) => {
                format!("Không kết nối được tới Docker engine: {detail}").into()
            }
        Text::OperationError(detail) => {
                format!("Không thể hoàn tất thao tác đó: {detail}").into()
            }
        Text::SelectAll => "Chọn tất cả".into(),
        Text::SelectRow => "Chọn container".into(),
        Text::RelNever => "Chưa bao giờ".into(),
        Text::RelJustNow => "vừa xong".into(),
        Text::RelSecondsAgo(n) => format!("{n} giây trước").into(),
        Text::RelMinutesAgo(n) => format!("{n} phút trước").into(),
        Text::RelHoursAgo(n) => format!("{n} giờ trước").into(),
        Text::RelDaysAgo(n) => format!("{n} ngày trước").into(),
        Text::RelWeeksAgo(n) => format!("{n} tuần trước").into(),
        Text::RelMonthsAgo(n) => format!("{n} tháng trước").into(),
        Text::RelYearsAgo(n) => format!("{n} năm trước").into(),
        Text::UnreachableTitle => {
                "Không kết nối được Docker engine".into()
            }
        Text::Ungrouped => "Chưa nhóm".into(),
        Text::GroupContainers(n) => {
                format!("{n} container").into()
            }
        Text::GroupRunning(n) => {
                format!("{n} đang chạy").into()
            }
        Text::FilterWithCount(n) => {
                format!("Bộ lọc ({n})").into()
            }
        Text::FilterTitle => "Bộ lọc".into(),
        Text::FilterProject => "Dự án Compose".into(),
        Text::FilterPublishedPorts => "Có cổng công bố".into(),
        Text::FilterFavorites => {
                "Yêu thích (sắp có)".into()
            }
        Text::FilterClear => "Xoá bộ lọc".into(),
        Text::BulkSelected(n) => format!("Đã chọn {n}").into(),
        Text::BulkStart => "Khởi động mục đã chọn".into(),
        Text::BulkStop => "Dừng mục đã chọn".into(),
        Text::BulkDelete => "Xoá mục đã chọn".into(),
        Text::BulkClear => "Bỏ chọn".into(),
        Text::BulkDeleteTitle => "Xoá các container?".into(),
        Text::BulkDeleteMessage(n) => {
                format!("Xoá vĩnh viễn {n} container? Hành động này không thể hoàn tác.").into()
            }
        Text::BulkFailures(n) => {
                format!("{n} container không thể cập nhật.").into()
            }
        Text::ColumnRepository => "Kho ảnh".into(),
        Text::ColumnTag => "Thẻ".into(),
        Text::ColumnImageId => "Mã ảnh".into(),
        Text::ColumnSize => "Kích thước".into(),
        Text::ColumnCreated => "Đã tạo".into(),
        Text::ColumnContainersUsing => "Container đang dùng".into(),
        Text::ColumnDriver => "Trình điều khiển".into(),
        Text::ColumnMountPoint => "Điểm gắn kết".into(),
        Text::ColumnScope => "Phạm vi".into(),
        Text::SearchImages => "Tìm ảnh".into(),
        Text::SearchVolumes => "Tìm volume".into(),
        Text::SearchNetworks => "Tìm mạng".into(),
        Text::NoImages => "Không có ảnh".into(),
        Text::NoImagesHint => {
                "Kéo về hoặc dựng một ảnh và nó sẽ xuất hiện ở đây.".into()
            }
        Text::NoVolumes => "Không có volume".into(),
        Text::NoVolumesHint => {
                "Tạo một volume và nó sẽ xuất hiện ở đây.".into()
            }
        Text::NoNetworks => "Không có mạng".into(),
        Text::NoNetworksHint => {
                "Tạo một mạng và nó sẽ xuất hiện ở đây.".into()
            }
        Text::NotAvailable => "N/A".into(),
        Text::None => "<none>".into(),
        Text::Inspect => "Xem chi tiết".into(),
        Text::NetworkPredefined => {
                "Không thể xoá mạng định sẵn".into()
            }
        Text::ViewLogs => "Xem nhật ký".into(),
        Text::OpenTerminal => "Mở terminal".into(),
        Text::ComingSoonLabel => "Sắp có".into(),
        Text::Details => "Chi tiết".into(),
        Text::RawJson => "JSON gốc".into(),
        Text::DetailErrorTitle => "Không tải được".into(),
        Text::NoLogs => "Không có nhật ký.".into(),
        Text::NoLogsHint => {
                "Container này chưa ghi gì ra stdout hoặc stderr.".into()
            }
        Text::LogsTail(n) => {
                format!("Đang hiển thị {n} dòng cuối").into()
            }
        Text::Yes => "Có".into(),
        Text::No => "Không".into(),
        Text::FieldId => "ID".into(),
        Text::FieldCommand => "Lệnh".into(),
        Text::FieldStarted => "Khởi động lúc".into(),
        Text::FieldExitCode => "Mã thoát".into(),
        Text::FieldRestartPolicy => {
                "Chính sách khởi động lại".into()
            }
        Text::FieldIpAddress => "Địa chỉ IP".into(),
        Text::FieldMounts => "Điểm gắn".into(),
        Text::FieldTags => "Thẻ".into(),
        Text::FieldDigest => "Digest".into(),
        Text::FieldArchitecture => "Kiến trúc".into(),
        Text::FieldOs => "Hệ điều hành".into(),
        Text::FieldLayers => "Lớp".into(),
        Text::FieldLabels => "Nhãn".into(),
        Text::FieldOptions => "Tuỳ chọn".into(),
        Text::FieldInternal => "Nội bộ".into(),
        Text::FieldAttachable => "Cho phép gắn".into(),
        Text::FieldSubnet => "Dải mạng".into(),
        Text::FieldGateway => "Gateway".into(),
        Text::Pull => "Tải về".into(),
        Text::Build => "Dựng image".into(),
        Text::Stats => "Thống kê".into(),
        Text::OpenDetails => "Mở chi tiết".into(),
        Text::Runtimes => "Runtimes".into(),
        Text::RuntimesDescription => {
                "Tự động phát hiện các runtime container trên máy này và điều khiển trực tiếp trong Dodo.".into()
            }
        Text::RuntimePodmanMachine => "Podman Machine".into(),
        Text::RuntimeKubernetes => "Kubernetes".into(),
        Text::RuntimeContainerd => "containerd".into(),
        Text::RuntimeStatusRunning => "Đang chạy".into(),
        Text::RuntimeStatusStopped => "Đã dừng".into(),
        Text::RuntimeStatusNotInstalled => "Chưa cài đặt".into(),
        Text::RuntimeStatusUnsupported => {
                "Không hỗ trợ trên nền tảng này".into()
            }
        Text::RuntimeStatusUnknown => "Không rõ".into(),
        Text::RuntimeManagedExternally => {
                "Được quản lý bởi nhà cung cấp cụm của bạn (Docker Desktop, minikube, kind, …), không phải từ đây.".into()
            }
        Text::RuntimeStarting => "Đang khởi động…".into(),
        Text::RuntimeStopping => "Đang dừng…".into(),
        Text::RuntimeBinaryNotFound => {
                "Không tìm thấy công cụ dòng lệnh cần thiết trên máy này.".into()
            }
        Text::RuntimeActionUnsupported => {
                "Thao tác này không khả dụng cho runtime này.".into()
            }
    }
}
