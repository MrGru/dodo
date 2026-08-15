//! The Vietnamese column of the Encoder / Decoder.

use std::borrow::Cow;

use super::{JwtPart, Text};

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::FormatLabel => "Định dạng:".into(),
        Text::EncodeButton => "Mã hoá".into(),
        Text::DecodeButton => "Giải mã".into(),
        Text::DecodeJwtButton => "Giải mã JWT".into(),
        Text::InputLabel => "Đầu vào".into(),
        Text::OutputLabel => "Đầu ra".into(),
        Text::JwtHeaderLabel => "Header".into(),
        Text::JwtPayloadLabel => "Payload".into(),
        Text::JwtSignatureLabel => "Chữ ký (chưa xác thực)".into(),
        Text::EncoderInputPlaceholder => {
                "Dán văn bản hoặc token cần chuyển đổi vào đây.".into()
            }
        Text::EncoderOutputPlaceholder => {
                "Kết quả hiển thị ở đây.".into()
            }
        Text::FormatBase64 => "Base64 (chuẩn)".into(),
        Text::FormatBase64UrlSafe => "Base64 (an toàn cho URL)".into(),
        Text::FormatUrl => "Mã hoá phần trăm URL".into(),
        Text::FormatHex => "Hex".into(),
        Text::FormatJwt => "JWT (chỉ giải mã)".into(),
        Text::JwtEncodeUnsupported => {
                "JWT chỉ hỗ trợ giải mã: không có khoá ký.".into()
            }
        Text::InvalidHexOddLength(count) => {
                format!("Hex không hợp lệ: cần số ký tự chẵn, nhận được {count}.").into()
            }
        Text::InvalidHexDigit { digit, position } => {
                format!("Hex không hợp lệ: '{digit}' ở vị trí {position} không phải ký tự hex.")
                    .into()
            }
        Text::InvalidBase64(detail) => {
                format!("Base64 không hợp lệ: {detail}").into()
            }
        Text::InvalidPercentAt(position) => format!(
                "Mã hoá phần trăm không hợp lệ: '%' ở vị trí {position} không được theo sau bởi hai ký tự hex."
            )
            .into(),
        Text::InvalidPercentEncoding(detail) => {
                format!("Mã hoá phần trăm không hợp lệ: {detail}").into()
            }
        Text::NotUtf8(detail) => {
                format!("Dữ liệu giải mã không phải văn bản UTF-8 hợp lệ: {detail}").into()
            }
        Text::JwtEmpty => {
                "JWT không hợp lệ: chưa có dữ liệu đầu vào.".into()
            }
        Text::JwtPartCount(count) => {
                format!("JWT không hợp lệ: cần 3 phần ngăn cách bởi dấu chấm, nhận được {count}.")
                    .into()
            }
        Text::JwtPartNotBase64 { part, detail } => {
                let part = jwt_part(part);
                format!("JWT không hợp lệ: phần {part} không phải base64url hợp lệ ({detail}).")
                    .into()
            }
        Text::JwtPartNotJson { part, detail } => {
                let part = jwt_part(part);
                format!("JWT không hợp lệ: phần {part} không phải JSON hợp lệ ({detail}).").into()
            }
        Text::JwtPartNotRenderable { part, detail } => {
                let part = jwt_part(part);
                format!("JWT không hợp lệ: không thể hiển thị phần {part} ({detail}).").into()
            }
    }
}

/// How this language names a JWT's two decodable parts.
pub(crate) fn jwt_part(part: JwtPart) -> &'static str {
    match part {
        JwtPart::Header => "header",
        JwtPart::Payload => "payload",
    }
}
