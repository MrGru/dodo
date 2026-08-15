//! The English column of the Encoder / Decoder.

use std::borrow::Cow;

use super::{JwtPart, Text};

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::FormatLabel => "Format:".into(),
        Text::EncodeButton => "Encode".into(),
        Text::DecodeButton => "Decode".into(),
        Text::DecodeJwtButton => "Decode JWT".into(),
        Text::InputLabel => "Input".into(),
        Text::OutputLabel => "Output".into(),
        Text::JwtHeaderLabel => "Header".into(),
        Text::JwtPayloadLabel => "Payload".into(),
        Text::JwtSignatureLabel => "Signature (not verified)".into(),
        Text::EncoderInputPlaceholder => {
                "Paste the text or token to convert here.".into()
            }
        Text::EncoderOutputPlaceholder => "Result appears here.".into(),
        Text::FormatBase64 => "Base64 (standard)".into(),
        Text::FormatBase64UrlSafe => "Base64 (URL-safe)".into(),
        Text::FormatUrl => "URL percent-encoding".into(),
        Text::FormatHex => "Hex".into(),
        Text::FormatJwt => "JWT (decode only)".into(),
        Text::JwtEncodeUnsupported => {
                "JWT is decode-only: no signing key is available.".into()
            }
        Text::InvalidHexOddLength(count) => {
                format!("Invalid hex: expected an even number of digits, got {count}.").into()
            }
        Text::InvalidHexDigit { digit, position } => {
                format!("Invalid hex: '{digit}' at position {position} is not a hex digit.").into()
            }
        Text::InvalidBase64(detail) => {
                format!("Invalid base64: {detail}").into()
            }
        Text::InvalidPercentAt(position) => format!(
                "Invalid percent-encoding: '%' at position {position} is not followed by two hex digits."
            )
            .into(),
        Text::InvalidPercentEncoding(detail) => {
                format!("Invalid percent-encoding: {detail}").into()
            }
        Text::NotUtf8(detail) => {
                format!("Decoded bytes are not valid UTF-8 text: {detail}").into()
            }
        Text::JwtEmpty => "Invalid JWT: the input is empty.".into(),
        Text::JwtPartCount(count) => {
                format!("Invalid JWT: expected 3 dot-separated parts, got {count}.").into()
            }
        Text::JwtPartNotBase64 { part, detail } => {
                let part = jwt_part(part);
                format!("Invalid JWT: the {part} is not valid base64url ({detail}).").into()
            }
        Text::JwtPartNotJson { part, detail } => {
                let part = jwt_part(part);
                format!("Invalid JWT: the {part} is not valid JSON ({detail}).").into()
            }
        Text::JwtPartNotRenderable { part, detail } => {
                let part = jwt_part(part);
                format!("Invalid JWT: could not render the {part} ({detail}).").into()
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
