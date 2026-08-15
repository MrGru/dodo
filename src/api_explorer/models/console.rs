//! The Console's entries and the bounded buffer that holds them.
//!
//! # Why the buffer survives a send
//!
//! Clearing on every send would destroy the one question a console answers
//! best: *what changed between the two attempts?* So a send does not clear —
//! it pushes a **separator** naming the run, and the previous run stays above
//! it. Clearing is an explicit button.
//!
//! # Why it is bounded, and why it says so
//!
//! A script can log in a loop. [`ConsoleLog`] keeps at most [`MAX_ENTRIES`]
//! entries **or** [`MAX_BYTES`] of text, whichever binds first, dropping oldest
//! first — and it counts what it dropped so the view can say so. Silently
//! discarding output is how a console starts lying.
//!
//! # Two kinds of line, one buffer
//!
//! A script's `console.log` is verbatim text: there is no translation of what a
//! script printed. dodo's own lines — the run separator, the note a skipped or
//! failed script leaves — are a [`Str`], so a line already on screen
//! re-translates when the language changes, the rule the error banners follow.
//! Both live in one [`ConsoleEntry`] rather than two buffers, because they have
//! to interleave in the order they happened.
//!
//! Nothing here touches GPUI, so all of it is unit testable.

use std::collections::VecDeque;

use crate::i18n::{Str, api_scripts};

/// Entries kept per tab.
pub const MAX_ENTRIES: usize = 500;
/// Bytes of message text kept per tab.
pub const MAX_BYTES: usize = 256 * 1024;

/// How loud one entry is. Ordered so a filter can be "this level and above".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConsoleLevel {
    Debug,
    Log,
    Warn,
    Error,
}

impl ConsoleLevel {
    pub const ALL: [ConsoleLevel; 4] = [
        ConsoleLevel::Debug,
        ConsoleLevel::Log,
        ConsoleLevel::Warn,
        ConsoleLevel::Error,
    ];

    pub fn label(self) -> Str {
        match self {
            ConsoleLevel::Debug => api_scripts::Text::ConsoleLevelDebug.into(),
            ConsoleLevel::Log => api_scripts::Text::ConsoleLevelLog.into(),
            ConsoleLevel::Warn => api_scripts::Text::ConsoleLevelWarn.into(),
            ConsoleLevel::Error => api_scripts::Text::ConsoleLevelError.into(),
        }
    }
}

/// Who produced an entry.
///
/// Keeping dodo's own voice apart from the script's is what lets the view say
/// "your script printed this" rather than blurring the two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleSource {
    Script,
    Runtime,
}

/// One line in the Console.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleEntry {
    pub level: ConsoleLevel,
    pub source: ConsoleSource,
    /// Script output, verbatim. Empty when [`localized`] carries the text.
    ///
    /// [`localized`]: ConsoleEntry::localized
    pub message: String,
    /// dodo's own words, held unrendered so they re-translate live.
    pub localized: Option<Str>,
    /// A run separator: drawn as a rule with its text as the caption, and never
    /// hidden by a level filter — hiding separators would merge two runs into
    /// one wall of text.
    pub separator: bool,
}

impl ConsoleEntry {
    /// A line a script printed.
    pub fn script(level: ConsoleLevel, message: impl Into<String>) -> Self {
        Self {
            level,
            source: ConsoleSource::Script,
            message: message.into(),
            localized: None,
            separator: false,
        }
    }

    /// A line dodo itself contributes.
    pub fn runtime(level: ConsoleLevel, text: Str) -> Self {
        Self {
            level,
            source: ConsoleSource::Runtime,
            message: String::new(),
            localized: Some(text),
            separator: false,
        }
    }

    /// The rule that separates one send's output from the last.
    pub fn separator(text: Str) -> Self {
        Self {
            separator: true,
            ..Self::runtime(ConsoleLevel::Log, text)
        }
    }

    /// Only verbatim script output counts against the byte cap: dodo's own
    /// lines are few, bounded by the entry cap, and are not what a runaway
    /// `console.log` loop produces.
    fn weight(&self) -> usize {
        self.message.len()
    }
}

/// One tab's console output.
///
/// Per-tab rather than page-wide, following the tab model `state::tab`
/// documents: a global console would make "which request logged this?"
/// guesswork. The cost — you cannot compare two tabs side by side — is real,
/// and the run separator buys part of it back within a tab.
#[derive(Debug, Default)]
pub struct ConsoleLog {
    entries: VecDeque<ConsoleEntry>,
    bytes: usize,
    /// Entries the caps have dropped over this tab's life. Shown, not hidden.
    dropped: usize,
    /// How many sends this tab has logged, so each separator can name its run.
    runs: usize,
    /// Errors pushed since the tab was last looked at, for the tab badge.
    unread_errors: usize,
}

impl ConsoleLog {
    pub fn push(&mut self, entry: ConsoleEntry) {
        if entry.level == ConsoleLevel::Error && !entry.separator {
            self.unread_errors += 1;
        }
        self.bytes += entry.weight();
        self.entries.push_back(entry);
        self.trim();
    }

    pub fn extend(&mut self, entries: impl IntoIterator<Item = ConsoleEntry>) {
        for entry in entries {
            self.push(entry);
        }
    }

    /// Opens a run: bumps the counter and pushes the separator naming it.
    pub fn begin_run(&mut self, summary: String) {
        self.runs += 1;
        let run = self.runs;
        self.push(ConsoleEntry::separator(
            api_scripts::Text::ConsoleRunSeparator { run, summary }.into(),
        ));
    }

    /// Drops oldest-first until both caps hold. One entry always survives, so a
    /// single enormous message is kept rather than leaving the console blank
    /// with nothing to explain it.
    fn trim(&mut self) {
        while self.entries.len() > MAX_ENTRIES || (self.bytes > MAX_BYTES && self.entries.len() > 1)
        {
            let Some(dropped) = self.entries.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(dropped.weight());
            self.dropped += 1;
        }
    }

    /// Empties the buffer. The run counter keeps going: "Run 4" after a clear
    /// is truthful, and restarting at 1 would imply the earlier sends did not
    /// happen.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.dropped = 0;
        self.unread_errors = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn unread_errors(&self) -> usize {
        self.unread_errors
    }

    /// Called when the Console tab is shown, clearing its badge.
    pub fn mark_read(&mut self) {
        self.unread_errors = 0;
    }

    /// The entries a level filter leaves showing. Separators always survive.
    pub fn visible(&self, minimum: ConsoleLevel) -> impl Iterator<Item = &ConsoleEntry> {
        self.entries
            .iter()
            .filter(move |entry| entry.separator || entry.level >= minimum)
    }

    /// What Copy puts on the clipboard: exactly what is on screen, filter and
    /// all, matching the Body tab's Copy. `render` is the view's own
    /// entry-to-text, so a translated line is copied as the user reads it.
    pub fn copy_text(
        &self,
        minimum: ConsoleLevel,
        render: impl Fn(&ConsoleEntry) -> String,
    ) -> String {
        self.visible(minimum)
            .map(render)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::{ConsoleEntry, ConsoleLevel, ConsoleLog, ConsoleSource, MAX_BYTES, MAX_ENTRIES};
    use crate::i18n::{Language, Str, api_scripts};

    fn log_line(n: usize) -> ConsoleEntry {
        ConsoleEntry::script(ConsoleLevel::Log, format!("line {n}"))
    }

    /// Everything currently in the buffer, filter off.
    fn all(log: &ConsoleLog) -> Vec<&ConsoleEntry> {
        log.visible(ConsoleLevel::Debug).collect()
    }

    /// What the view does, reduced to what a test needs.
    fn render(entry: &ConsoleEntry) -> String {
        match &entry.localized {
            Some(str) => str.clone().text(Language::English).into_owned(),
            None => entry.message.clone(),
        }
    }

    #[test]
    fn the_entry_cap_drops_oldest_first_and_counts_what_it_dropped() {
        let mut log = ConsoleLog::default();
        for n in 0..MAX_ENTRIES + 10 {
            log.push(log_line(n));
        }
        assert_eq!(all(&log).len(), MAX_ENTRIES);
        assert_eq!(log.dropped(), 10);
        assert_eq!(
            all(&log).first().map(|entry| entry.message.as_str()),
            Some("line 10")
        );
    }

    #[test]
    fn the_byte_cap_binds_before_the_entry_cap_when_lines_are_large() {
        let mut log = ConsoleLog::default();
        let big = "x".repeat(64 * 1024);
        for _ in 0..8 {
            log.push(ConsoleEntry::script(ConsoleLevel::Log, big.clone()));
        }
        assert!(
            all(&log).len() < 8,
            "the byte cap did not bind: {} entries",
            all(&log).len()
        );
        assert!(log.dropped() > 0);
    }

    #[test]
    fn one_enormous_line_is_kept_rather_than_leaving_the_console_blank() {
        let mut log = ConsoleLog::default();
        log.push(ConsoleEntry::script(
            ConsoleLevel::Log,
            "y".repeat(MAX_BYTES * 2),
        ));
        assert_eq!(all(&log).len(), 1);
    }

    #[test]
    fn a_separator_survives_every_level_filter() {
        let mut log = ConsoleLog::default();
        log.begin_run("GET /things".into());
        log.push(log_line(1));

        let visible: Vec<_> = log.visible(ConsoleLevel::Error).collect();
        assert_eq!(visible.len(), 1);
        assert!(visible[0].separator);
    }

    #[test]
    fn each_send_gets_its_own_run_number() {
        let mut log = ConsoleLog::default();
        log.begin_run("GET /a".into());
        log.begin_run("GET /b".into());

        let numbers: Vec<usize> = all(&log)
            .into_iter()
            .filter_map(|entry| match &entry.localized {
                Some(Str::ApiScripts(api_scripts::Text::ConsoleRunSeparator { run, .. })) => {
                    Some(*run)
                }
                _ => None,
            })
            .collect();
        assert_eq!(numbers, vec![1, 2]);
    }

    #[test]
    fn the_filter_keeps_the_level_it_names_and_everything_louder() {
        let mut log = ConsoleLog::default();
        for level in ConsoleLevel::ALL {
            log.push(ConsoleEntry::script(level, "message"));
        }
        assert_eq!(log.visible(ConsoleLevel::Debug).count(), 4);
        assert_eq!(log.visible(ConsoleLevel::Warn).count(), 2);
        assert_eq!(log.visible(ConsoleLevel::Error).count(), 1);
    }

    #[test]
    fn only_unread_errors_count_towards_the_badge() {
        let mut log = ConsoleLog::default();
        log.begin_run("GET /x".into());
        log.push(log_line(1));
        assert_eq!(log.unread_errors(), 0);

        log.push(ConsoleEntry::script(ConsoleLevel::Error, "boom"));
        assert_eq!(log.unread_errors(), 1);

        log.mark_read();
        assert_eq!(log.unread_errors(), 0);
        assert_eq!(all(&log).len(), 3, "marking read must not drop anything");
    }

    #[test]
    fn copying_respects_the_filter_and_renders_dodos_own_lines() {
        let mut log = ConsoleLog::default();
        log.push(ConsoleEntry::script(ConsoleLevel::Debug, "quiet"));
        log.push(ConsoleEntry::runtime(
            ConsoleLevel::Error,
            api_scripts::Text::OutOfMemory.into(),
        ));

        let loud = log.copy_text(ConsoleLevel::Error, render);
        assert_eq!(
            loud,
            Str::from(api_scripts::Text::OutOfMemory)
                .text(Language::English)
                .into_owned()
        );
        assert!(
            log.copy_text(ConsoleLevel::Debug, render)
                .starts_with("quiet")
        );
    }

    #[test]
    fn clearing_resets_the_dropped_count_but_not_the_run_number() {
        let mut log = ConsoleLog::default();
        log.begin_run("GET /a".into());
        for n in 0..MAX_ENTRIES + 5 {
            log.push(log_line(n));
        }
        log.clear();
        assert!(log.is_empty());
        assert_eq!(log.dropped(), 0);

        log.begin_run("GET /b".into());
        let run = all(&log)
            .into_iter()
            .find_map(|entry| match &entry.localized {
                Some(Str::ApiScripts(api_scripts::Text::ConsoleRunSeparator { run, .. })) => {
                    Some(*run)
                }
                _ => None,
            });
        assert_eq!(
            run,
            Some(2),
            "clearing must not pretend the first send did not happen"
        );
    }

    #[test]
    fn a_runtime_entry_is_told_apart_from_a_script_one() {
        let entry = ConsoleEntry::runtime(
            ConsoleLevel::Warn,
            api_scripts::Text::SkippedByPolicy.into(),
        );
        assert_eq!(entry.source, ConsoleSource::Runtime);
        assert!(entry.message.is_empty());
    }
}
