//! The Vietnamese column of the API Explorer's scripting.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::ScriptsSandboxNotice => {
            "Cả hai kịch bản chạy trong hộp cát không có tệp, không có mạng và không có \
                 mô-đun. pm.sendRequest, require và setTimeout không khả dụng."
                .into()
        }
        Text::PreRequestScriptLabel => "Kịch bản trước yêu cầu".into(),
        Text::PostResponseScriptLabel => "Kịch bản sau phản hồi".into(),
        Text::Copy => "Sao chép".into(),
        Text::BodyTruncated => "Nội dung quá lớn nên đã bị cắt bớt.".into(),
        Text::InsertTemplate => "Chèn mẫu".into(),
        Text::TemplateSetHeader => "Đặt một header".into(),
        Text::TemplateSetBearerToken => "Đặt bearer token".into(),
        Text::TemplateSetTimestamp => "Đặt biến thời gian".into(),
        Text::TemplateAssertStatus => "Kiểm tra trạng thái là 200".into(),
        Text::TemplateLogResponse => "Ghi nhật ký nội dung phản hồi".into(),
        Text::TemplateExtractField => "Trích một trường JSON".into(),
        Text::Threw(detail) => format!("Kịch bản lỗi: {detail}").into(),
        Text::Deadline(seconds) => {
            format!("Kịch bản không kết thúc trong {seconds} giây và đã bị dừng.").into()
        }
        Text::OutOfMemory => "Kịch bản yêu cầu nhiều bộ nhớ hơn mức cho phép mỗi lần chạy.".into(),
        Text::Unsupported(name) => {
            format!("dodo không hỗ trợ {name}, nên kịch bản này không chạy được.").into()
        }
        Text::NoEngine => {
            "Bản dựng này không có bộ chạy kịch bản, nên không có gì được chạy.".into()
        }
        Text::SkippedByPolicy => {
            "Kịch bản đang tắt trong Cài đặt, nên kịch bản này không chạy.".into()
        }
        Text::SkippedByConsent => {
            "Kịch bản nhập vào này chưa được duyệt, nên nó không chạy.".into()
        }
        Text::ConsoleLevelDebug => "Gỡ lỗi".into(),
        Text::ConsoleLevelLog => "Nhật ký".into(),
        Text::ConsoleLevelWarn => "Cảnh báo".into(),
        Text::ConsoleLevelError => "Lỗi".into(),
        Text::ConsoleRunSeparator { run, summary } => format!("Lần chạy {run} · {summary}").into(),
        Text::ConsoleEmpty => "Chưa có gì được ghi".into(),
        Text::ConsoleEmptyHint => {
            "console.log từ kịch bản hiện ở đây và được giữ qua các lần gửi.".into()
        }
        Text::ConsoleClear => "Xoá".into(),
        Text::ConsoleDropped(count) => format!("Đã bỏ {count} dòng cũ").into(),
        Text::RunScriptsNever => "Không bao giờ".into(),
        Text::RunScriptsAskImported => "Hỏi khi nhập vào".into(),
        Text::RunScriptsAlways => "Luôn luôn".into(),
        Text::ConsentTitle => "Chạy kịch bản nhập vào này?".into(),
        Text::ConsentExplain => {
            "Kịch bản này đến từ bộ sưu tập nhập vào và chưa từng chạy. Hãy đọc trước khi \
                 duyệt: nó có thể thay đổi yêu cầu này và ghi vào biến của bạn."
                .into()
        }
        Text::ConsentRequest(name) => format!("Yêu cầu: {name}").into(),
        Text::ConsentRun => "Chạy kịch bản".into(),
        Text::ConsentSkip => "Gửi mà không chạy".into(),
        Text::ConsentStoreError(detail) => {
            format!("Không đọc hoặc ghi được danh sách kịch bản đã duyệt: {detail}").into()
        }
        Text::ConsentStoreMissingVersion => {
            "Tệp kịch bản đã duyệt không có phiên bản lược đồ, nên không được đọc.".into()
        }
        Text::ConsentStoreUnsupportedVersion { found, supported } => format!(
            "Tệp kịch bản đã duyệt này dùng lược đồ {found}; bản dodo này chỉ đọc \
                 {supported}. Mọi kịch bản nhập vào sẽ hỏi lại."
        )
        .into(),
        Text::ConsentExplainChanged => {
            "Kịch bản nhập vào này đã thay đổi kể từ khi bạn duyệt, nên lần duyệt trước \
                 không còn hiệu lực. Hãy đọc lại trước khi duyệt: nó có thể thay đổi yêu cầu \
                 này và ghi vào biến của bạn."
                .into()
        }
        Text::SyntaxErrorAt { line, detail } => format!("Dòng {line}: {detail}").into(),
        Text::TestsNone => "Yêu cầu này chưa có kiểm thử".into(),
        Text::TestsNoneHint => {
            "Kịch bản sau phản hồi có thể kiểm tra kết quả trả về bằng pm.test.".into()
        }
        Text::TestsAddOne => "Thêm kiểm thử".into(),
        Text::TestsScriptDefinedNone => "Kịch bản đã chạy và không định nghĩa kiểm thử nào".into(),
        Text::TestsScriptDefinedNoneHint => "Những gì nó in ra nằm trong Console.".into(),
        Text::TestsNotRun => "Yêu cầu này có kịch bản kiểm thử, nhưng nó đã không chạy".into(),
        Text::TestsPassedCount(count) => format!("{count} đạt").into(),
        Text::TestsFailedCount(count) => format!("{count} không đạt").into(),
        Text::TestsErroredCount(count) => format!("{count} lỗi kịch bản").into(),
        Text::TestsDropped(count) => format!("Đã bỏ thêm {count} kết quả").into(),
    }
}
