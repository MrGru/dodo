//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, with};

use super::Text;

samples! {
    plain CurlPattern;
    plain DatabasePattern;
    plain JwtPattern;
    plain JsonPattern;
    plain Base64Pattern;
    with PatternInvalid(DETAIL.into()) [DETAIL];
    with PatternTooLong { length: NUMBER, limit: 512 } [NUMBER_TEXT, "512"];
    with StoreError(DETAIL.into()) [DETAIL];
    plain StoreMissingVersion;
    with StoreUnsupportedVersion { found: 9, understood: 1 } ["9", "1"];
}
