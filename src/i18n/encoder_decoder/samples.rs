//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::i18n::encoder_decoder::JwtPart;
use crate::i18n::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, term, with};

use super::Text;

samples! {
    plain FormatLabel;
    plain EncodeButton;
    plain DecodeButton;
    plain DecodeJwtButton;
    plain InputLabel;
    plain OutputLabel;
    term JwtHeaderLabel;
    term JwtPayloadLabel;
    plain JwtSignatureLabel;
    plain EncoderInputPlaceholder;
    plain EncoderOutputPlaceholder;
    plain FormatBase64;
    plain FormatBase64UrlSafe;
    plain FormatUrl;
    term FormatHex;
    plain FormatJwt;
    plain JwtEncodeUnsupported;
    with InvalidHexOddLength(NUMBER) [NUMBER_TEXT];
    with InvalidHexDigit { digit: 'Z', position: NUMBER } ["Z", NUMBER_TEXT];
    with InvalidBase64(DETAIL.into()) [DETAIL];
    with InvalidPercentAt(NUMBER) [NUMBER_TEXT];
    with InvalidPercentEncoding(DETAIL.into()) [DETAIL];
    with NotUtf8(DETAIL.into()) [DETAIL];
    plain JwtEmpty;
    with JwtPartCount(NUMBER) [NUMBER_TEXT];
    with JwtPartNotBase64 { part: JwtPart::Header, detail: DETAIL.into() } [DETAIL];
    with JwtPartNotJson { part: JwtPart::Payload, detail: DETAIL.into() } [DETAIL];
    with JwtPartNotRenderable { part: JwtPart::Header, detail: DETAIL.into() } [DETAIL];
}
