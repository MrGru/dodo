//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::i18n::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, with};

use super::Text;

samples! {
    plain JsonPlaceholder;
    plain IndentLabel;
    with IndentSpaces(NUMBER) [NUMBER_TEXT];
    with InvalidJson { line: NUMBER, column: 77, detail: DETAIL.into() } [NUMBER_TEXT, "77", DETAIL];
}
