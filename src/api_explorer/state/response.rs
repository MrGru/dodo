//! The response half of one open tab: what came back, and how it is shown.

use gpui::{AppContext as _, Context, Entity, Window};
use gpui_component::input::InputState;

use crate::api_explorer::models::exchange::Exchange;
use crate::i18n::Str;

/// How many lines of a body are put into the editor at once.
///
/// The editor itself virtualizes rows, but the highlighter and the rope do not
/// come free, and a 200 000-line minified bundle should not cost a visible
/// pause. What is being withheld is stated in the footer, never hidden.
pub const LINE_WINDOW: usize = 500;

/// Which response tab is showing.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ResponseTab {
    #[default]
    Body,
    Headers,
    Cookies,
    Tests,
    Console,
}

impl ResponseTab {
    pub const ALL: [ResponseTab; 5] = [
        ResponseTab::Body,
        ResponseTab::Headers,
        ResponseTab::Cookies,
        ResponseTab::Tests,
        ResponseTab::Console,
    ];

    pub fn label(self) -> Str {
        match self {
            ResponseTab::Body => Str::ResponseTabBody,
            ResponseTab::Headers => Str::ResponseTabHeaders,
            ResponseTab::Cookies => Str::ResponseTabCookies,
            ResponseTab::Tests => Str::ResponseTabTests,
            ResponseTab::Console => Str::ResponseTabConsole,
        }
    }

    pub fn is_implemented(self) -> bool {
        matches!(self, ResponseTab::Body | ResponseTab::Headers)
    }
}

/// Whether the body is shown as sent or pretty-printed.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum BodyView {
    #[default]
    Pretty,
    Raw,
}

/// Where a request tab is in the request lifecycle.
#[derive(Default)]
pub enum Outcome {
    /// Nothing sent yet in this tab.
    #[default]
    Idle,
    InFlight,
    /// A response arrived. An HTTP error status lands here, not in `Failed`.
    Received(Exchange),
    /// No response arrived. Held as a [`Str`] rather than rendered text so the
    /// banner re-translates when the language changes while it is on screen.
    Failed(Str),
}

/// The response half of one open tab.
pub struct ResponseState {
    pub outcome: Outcome,
    pub active_tab: ResponseTab,
    pub body_view: BodyView,
    /// How many lines of the body are currently in the editor.
    pub visible_lines: usize,
    /// Lines the current body has in total, cached so the footer does not
    /// recount a large string every frame.
    pub total_lines: usize,
    /// The editor the body is rendered in. Reused across responses so the
    /// widget, its scroll position and its highlighter are not rebuilt each
    /// time.
    pub body: Entity<InputState>,
    pub collapsed: bool,
}

impl ResponseState {
    pub fn new(window: &mut Window, cx: &mut Context<super::tab::RequestTabState>) -> Self {
        Self {
            outcome: Outcome::default(),
            active_tab: ResponseTab::default(),
            body_view: BodyView::default(),
            visible_lines: LINE_WINDOW,
            total_lines: 0,
            body: cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor("text")
                    .multi_line(true)
                    .line_number(true)
            }),
            collapsed: false,
        }
    }

    pub fn is_in_flight(&self) -> bool {
        matches!(self.outcome, Outcome::InFlight)
    }

    /// The exchange to render, if the last send produced one.
    pub fn exchange(&self) -> Option<&Exchange> {
        match &self.outcome {
            Outcome::Received(exchange) => Some(exchange),
            _ => None,
        }
    }

    /// How many response headers arrived — the count badge on the Headers tab.
    pub fn header_count(&self) -> usize {
        self.exchange().map_or(0, |exchange| exchange.headers.len())
    }

    /// Whether any of the body is still being withheld by the line window.
    pub fn has_more_lines(&self) -> bool {
        self.total_lines > self.visible_lines
    }

    /// Extends the line window by one more screenful's worth.
    pub fn show_more_lines(&mut self) {
        self.visible_lines = self.visible_lines.saturating_add(LINE_WINDOW);
    }

    /// Resets the window for a newly arrived body.
    pub fn reset_window(&mut self) {
        self.visible_lines = LINE_WINDOW;
    }
}

/// The first `limit` lines of `body`, and how many lines it has in total.
///
/// Counting and slicing in one pass so that a large body is walked once.
pub fn window_lines(body: &str, limit: usize) -> (String, usize) {
    let total = body.lines().count();
    if total <= limit {
        return (body.to_string(), total);
    }

    let mut out = String::new();
    for (index, line) in body.lines().take(limit).enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    (out, total)
}

#[cfg(test)]
mod tests {
    use super::window_lines;

    #[test]
    fn a_short_body_is_returned_whole() {
        let (text, total) = window_lines("a\nb\nc", 10);
        assert_eq!(text, "a\nb\nc");
        assert_eq!(total, 3);
    }

    #[test]
    fn a_long_body_is_cut_to_the_window_and_still_counted() {
        let body = (0..100)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let (text, total) = window_lines(&body, 3);
        assert_eq!(text, "0\n1\n2");
        assert_eq!(total, 100);
    }

    #[test]
    fn an_empty_body_has_no_lines() {
        let (text, total) = window_lines("", 10);
        assert_eq!(text, "");
        assert_eq!(total, 0);
    }

    #[test]
    fn a_body_exactly_at_the_limit_is_not_truncated() {
        let (text, total) = window_lines("a\nb", 2);
        assert_eq!(text, "a\nb");
        assert_eq!(total, 2);
    }
}
