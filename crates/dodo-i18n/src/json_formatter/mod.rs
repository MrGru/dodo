//! The JSON formatter tool.
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
    // JSON formatter.
    JsonPlaceholder,
    IndentLabel,
    /// "{count} spaces" — the indent-width dropdown options.
    IndentSpaces(usize),
    /// serde_json's message is third-party English and is kept verbatim.
    InvalidJson {
        line: usize,
        column: usize,
        detail: String,
    },
}
