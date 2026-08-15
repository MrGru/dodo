//! Quick navigation: the pasted-content routes and their editable patterns.
//!
//! `en` and `vi` each render every variant below; the compiler names any
//! string a language has not been given.

pub(crate) mod en;
pub(crate) mod vi;

#[cfg(test)]
pub(crate) mod samples;

/// The strings this area owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    CurlPattern,
    DatabasePattern,
    JwtPattern,
    JsonPattern,
    Base64Pattern,
    PatternInvalid(String),
    PatternTooLong { length: usize, limit: usize },
    StoreError(String),
    StoreMissingVersion,
    StoreUnsupportedVersion { found: u64, understood: u32 },
}
