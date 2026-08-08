//! Which engine, as opposed to which setting.
//!
//! # This is not a third language type
//!
//! dodo already has two, and the rule that they never merge is enforced by
//! tests — see [`tray::InputLanguage`](crate::tray::input_language::InputLanguage)
//! for the full statement. [`LanguageId`] is not a third: it names an **engine
//! that exists in this build**, and there is deliberately no `English` variant,
//! because typing English needs no engine at all.
//!
//! The relationship is one-way and lives outside this module, in the round that
//! wires the tray up: `tray::InputLanguage` → `Option<LanguageId>`, where
//! `English` maps to `None` (no engine; keys pass straight through) and
//! `Japanese` will map to `None` until a Japanese engine exists. Nothing here
//! may import `tray`, and [`purity_lint`](crate::purity_lint)
//! fails the build if anything tries — which is what keeps that mapping at the
//! boundary instead of leaking a UI concept into the state machine.
//!
//! It is **never persisted**. `session.json` stores `tray.input_language`; this
//! is derived from that at runtime and has no stable code of its own to store.

/// A language engine this build carries.
///
/// One variant today. Korean, Japanese and Chinese join it when their engines
/// land, not before — an id for an engine that does not exist would let a host
/// select nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LanguageId {
    Vietnamese,
}

impl LanguageId {
    /// Every engine in this build.
    pub const ALL: [LanguageId; 1] = [LanguageId::Vietnamese];

    /// A short identifier for logs and tests.
    ///
    /// Matching `tray::InputLanguage::code`'s spelling for Vietnamese is a
    /// coincidence of both being the ISO 639-1 code, not a shared vocabulary;
    /// the two tables are unrelated and must stay that way.
    pub fn code(self) -> &'static str {
        match self {
            LanguageId::Vietnamese => "vi",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LanguageId;

    #[test]
    fn every_engine_has_a_distinct_code() {
        let mut codes: Vec<_> = LanguageId::ALL.iter().map(|id| id.code()).collect();
        codes.sort_unstable();
        let count = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), count, "two engines share a code");
    }

    /// The list names engines, not settings. `English` is absent because typing
    /// English needs no engine; a variant for it would be a second spelling of
    /// `tray::InputLanguage`, which is the merge this project has already
    /// refused twice.
    #[test]
    fn there_is_no_engine_for_english() {
        assert_eq!(LanguageId::ALL, [LanguageId::Vietnamese]);
        assert!(!LanguageId::ALL.iter().any(|id| id.code() == "en"));
    }
}
