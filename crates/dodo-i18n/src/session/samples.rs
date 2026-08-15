//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{DETAIL, Sample, plain, with};

use super::Text;

samples! {
    with StoreError(DETAIL.into()) [DETAIL];
    plain StoreMissingVersion;
    with StoreUnsupportedVersion { found: 9, understood: 1 } ["9", "1"];
    plain FeatureLastVisibleTool;
}
