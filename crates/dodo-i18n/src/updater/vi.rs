//! The Vietnamese column of the in-app updater.

use std::borrow::Cow;

use crate::Language;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::SoftwareUpdate => "Cập nhật phần mềm".into(),
        Text::Checking => "Đang kiểm tra cập nhật…".into(),
        Text::UpToDate => "dodo đã là bản mới nhất.".into(),
        Text::CurrentVersion(version) => format!("Bạn đang dùng phiên bản {version}.").into(),
        Text::AvailableHeadline(version) => format!("Đã có phiên bản {version}.").into(),
        Text::Published(when) => format!("Phát hành {when}").into(),
        Text::DownloadSize(size) => format!("Dung lượng tải về {size}").into(),
        Text::ReleaseNotes => "Ghi chú phát hành".into(),
        Text::DownloadAction => "Tải về và cài đặt".into(),
        Text::DownloadProgress {
            done,
            total,
            percent,
        } => format!("Đang tải… {done} trên {total} ({percent}%)").into(),
        Text::Verifying => "Đang xác minh tệp tải về…".into(),
        Text::Installing => "Đang cài đặt…".into(),
        Text::InstalledHeadline(version) => format!("Đã cài đặt phiên bản {version}.").into(),
        Text::RestartNow => "Khởi động lại ngay".into(),
        Text::Later => "Để sau".into(),
        Text::SkipVersion => "Bỏ qua phiên bản này".into(),
        Text::Cancel => "Huỷ".into(),
        Text::Retry => "Thử lại".into(),
        Text::CheckAutomatically => "Tự động kiểm tra cập nhật".into(),
        Text::ManualInstall(path) => format!(
            "Bản cập nhật đã được tải về và xác minh, nhưng dodo không thể tự thay thế ở \
                 vị trí đang cài. Tệp nén nằm tại {path}."
        )
        .into(),
        Text::ManualNotABundle => {
            "dodo đang chạy dưới dạng tệp thực thi đơn lẻ, không phải từ gói ứng dụng.".into()
        }
        Text::ManualNotWritable => "Không thể ghi vào thư mục đang cài dodo.".into(),
        Text::ManualReadOnly => "dodo đang chạy từ một vị trí chỉ đọc.".into(),
        Text::FailedHeadline => "Không thể hoàn tất bản cập nhật.".into(),
        Text::ErrorNetwork(detail) => {
            format!("Không kết nối được máy chủ cập nhật: {detail}").into()
        }
        Text::ErrorManifestMalformed(detail) => {
            format!("Không đọc được tệp kê khai cập nhật: {detail}").into()
        }
        Text::ErrorManifestMissingVersion => {
            "Tệp kê khai cập nhật không ghi phiên bản, nên dodo không biết cách đọc nó.".into()
        }
        Text::ErrorManifestUnsupportedVersion { found, supported } => format!(
            "Tệp kê khai cập nhật ở phiên bản {found}; dodo này chỉ hiểu phiên bản \
                 {supported}. Hãy cập nhật dodo thủ công."
        )
        .into(),
        Text::ErrorManifestUnreadableVersion(text) => {
            format!("Tệp kê khai cập nhật ghi một phiên bản dodo không đọc được: {text}").into()
        }
        Text::ErrorManifestInvalidFile { platform, detail } => format!(
            "Mục {platform} trong tệp kê khai cập nhật không dùng được: {}",
            detail.text(Language::Vietnamese)
        )
        .into(),
        Text::ErrorManifestBadDigest(digest) => {
            format!("{digest} không phải là mã băm SHA-256").into()
        }
        Text::ErrorManifestZeroSize => "dung lượng tải về bằng không".into(),
        Text::ErrorManifestInsecureUrl(url) => {
            format!("địa chỉ tải về không dùng https: {url}").into()
        }
        Text::ErrorPlatformMissing(key) => {
            format!("Bản phát hành này không có tệp tải về cho {key}.").into()
        }
        Text::ErrorDownload(detail) => format!("Tải về thất bại: {detail}").into(),
        Text::ErrorChecksum { expected, actual } => format!(
            "Tệp tải về không khớp mã băm mà bản phát hành công bố — cần {expected}, nhận \
                 được {actual}. Tệp đã bị xoá và không có gì được cài đặt."
        )
        .into(),
        Text::ErrorSize { expected, actual } => format!(
            "Tệp tải về có {actual} byte; bản phát hành ghi {expected}. Tệp đã bị xoá và \
                 không có gì được cài đặt."
        )
        .into(),
        Text::ErrorInstall(detail) => format!("Không thể cài đặt bản cập nhật: {detail}").into(),
        Text::ErrorIo(detail) => format!("Không thể ghi tệp: {detail}").into(),
    }
}
