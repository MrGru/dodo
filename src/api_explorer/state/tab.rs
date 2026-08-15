//! One open request tab: its request, its response, and its in-flight task.
//!
//! Each tab is its own entity, which is what makes the tabs genuinely
//! independent — sending in one leaves the others' editors, responses and
//! scroll positions untouched.

use std::sync::Arc;
use std::time::Duration;

use gpui::{Context, Entity, EventEmitter, Task, Window};
use gpui_component::highlighter::{Diagnostic, DiagnosticSeverity};
use gpui_component::input::{InputState, Position};

use crate::api_explorer::models::exchange::{BodyKind, Exchange};
use crate::api_explorer::models::script::{ScriptSyntaxError, VariableWrite};
use crate::api_explorer::models::script_format;
use crate::api_explorer::models::test_result::TestReport;
use crate::api_explorer::models::variables::VariableSet;
use crate::api_explorer::services::Transport;
use crate::api_explorer::services::http::body;
use crate::api_explorer::services::script::ScriptEngine;
use crate::api_explorer::services::send::{ScriptJob, SendJob, send};
use crate::api_explorer::state::history::HistoryRecord;
use crate::api_explorer::state::request::{RequestState, ScriptSlot};
use crate::api_explorer::state::response::{Outcome, ResponseState, window_lines};
use crate::i18n::{Str, api_explorer, t};

pub struct RequestTabState {
    pub request: RequestState,
    pub response: ResponseState,
    /// The in-flight request, if any.
    ///
    /// Held so that dropping the tab cancels the request, and so that pressing
    /// Send twice replaces the first task rather than racing it: assigning a
    /// new `Task` drops the old one, which cancels it.
    send_task: Option<Task<()>>,
    /// The pending syntax check for each script editor, indexed by
    /// [`ScriptSlot::index`].
    ///
    /// The debounce *is* this field: a keystroke assigns a new task, which drops
    /// the one still sleeping. Nothing else is needed, and it cancels with the
    /// tab for free.
    script_checks: [Option<Task<()>>; 2],
}

/// How long a script editor must be quiet before it is parsed.
///
/// Long enough that ordinary typing never triggers a check, short enough that
/// pausing to think shows the answer. The check itself runs off the UI thread,
/// so this is about noise rather than about cost.
const CHECK_DEBOUNCE: Duration = Duration::from_millis(350);

/// A finished request is emitted so the page can record it in history. The page
/// subscribes to every tab; the tab is the one place that knows a request
/// completed, which is the seam phase 1 described.
impl EventEmitter<HistoryRecord> for RequestTabState {}

/// Variables a pre-request script wrote, on their way to the page.
///
/// A second event rather than a field on [`HistoryRecord`], because variables
/// are **cross-tab** state: the tab is not allowed to write them, so it says
/// what happened and `ApiExplorer::watch_tab` — which owns the environments and
/// the store — applies and persists them. That reuses the one ownership seam
/// that already exists rather than adding a rule.
pub struct ScriptWrites(pub Vec<VariableWrite>);

impl EventEmitter<ScriptWrites> for RequestTabState {}

impl RequestTabState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            request: RequestState::new(window, cx),
            response: ResponseState::new(window, cx),
            send_task: None,
            script_checks: [None, None],
        }
    }

    /// Re-parses one script editor after a pause, and underlines where it does
    /// not parse.
    ///
    /// **The engine that will run the script is the one that answers**, so the
    /// editor and the Console can never disagree about whether something
    /// parses; see [`ScriptEngine::check`]. The parse runs on the background
    /// executor for the same reason everything else does — a pathological source
    /// gets the engine's full 2 s budget, and a frame must not wait for it.
    pub fn check_script(
        &mut self,
        slot: ScriptSlot,
        engine: Arc<dyn ScriptEngine>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.request.script_editor(slot).clone();
        self.script_checks[slot.index()] = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(CHECK_DEBOUNCE).await;

            let source = editor.read_with(cx, |state, _| state.value().to_string());
            let found = cx
                .background_executor()
                .spawn(async move { engine.check(&source) })
                .await;

            let _ = this.update_in(cx, |this, window, cx| {
                this.apply_script_check(slot, found, window, cx);
            });
        }));
    }

    /// Puts one check's answer on screen: the underline in the editor, the
    /// message beside it.
    fn apply_script_check(
        &mut self,
        slot: ScriptSlot,
        found: Option<ScriptSyntaxError>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.request.script_editor(slot).clone();
        let message = found.as_ref().map(|error| {
            t(
                api_explorer::Text::ScriptSyntaxError(error.detail.clone()),
                cx,
            )
        });

        editor.update(cx, |state, cx| {
            let text = state.text().clone();
            let Some(diagnostics) = state.diagnostics_mut() else {
                return;
            };
            match (&found, message) {
                (Some(error), Some(message)) => {
                    // `reset(&rope)` rather than `clear()`: the diagnostics are
                    // anchored into the text, which has just changed.
                    diagnostics.reset(&text);
                    let line = error.line as u32;
                    let column = error.column as u32;
                    diagnostics.push(
                        Diagnostic::new(
                            Position::new(line, column)..Position::new(line, column + 1),
                            message,
                        )
                        .with_severity(DiagnosticSeverity::Error),
                    );
                }
                _ => diagnostics.clear(),
            }
            cx.notify();
        });

        self.request.set_script_error(slot, found);
        cx.notify();
    }

    /// Sends the request this tab currently describes.
    ///
    /// Reading the editors out into a [`RequestDraft`] is the only part that
    /// happens here: it needs the entities, which live on the UI thread.
    /// **The script, substitution, validation and assembly all go to the
    /// background executor with the request itself.** Building the body may
    /// read a file the Body tab points at, and a script may loop for its whole
    /// 2 s budget; neither may stall a frame. The cost is that a mistyped URL
    /// is reported one task hop later than the keystroke — invisible, and the
    /// right trade.
    ///
    /// `variables` is the page's [`VariableSet`] and `script` the decision the
    /// page's consent ledger already made, both read on the UI thread at the
    /// moment Send is pressed and moved into the task: a request in flight
    /// resolves against the environment that was active when it started,
    /// whatever the user switches to while it runs.
    ///
    /// [`RequestDraft`]: crate::api_explorer::models::request::RequestDraft
    pub fn send(
        &mut self,
        transport: Arc<dyn Transport>,
        engine: Arc<dyn ScriptEngine>,
        variables: VariableSet,
        scripts: (ScriptJob, ScriptJob),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = self.request.draft(cx);
        let summary = self.request.snapshot(cx).summary();
        let (pre, post) = scripts;

        self.response.console.begin_run(summary);
        self.response.outcome = Outcome::InFlight;
        // The previous send's results describe the previous send. Leaving them
        // beside a new response would be the kind of stale green a test pane
        // must never show.
        self.response.tests = TestReport::default();
        cx.notify();

        let job = SendJob {
            draft,
            variables,
            pre,
            post,
        };

        self.send_task = Some(cx.spawn_in(window, async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { send(job, engine.as_ref(), transport.as_ref()) })
                .await;

            // The window or the tab can be gone by the time this lands; both
            // are ordinary shutdown paths, not errors.
            let _ = this.update_in(cx, |this, window, cx| {
                this.response.console.extend(outcome.logs);
                this.response.tests = outcome.tests;
                if !outcome.writes.is_empty() {
                    cx.emit(ScriptWrites(outcome.writes));
                }
                match outcome.result {
                    Ok(exchange) => this.receive(exchange, window, cx),
                    Err(error) => this.fail(error, cx),
                }
            });
        }));
    }

    /// Cancels an in-flight request, if there is one. Used when the tab is
    /// closed.
    pub fn cancel(&mut self) {
        self.send_task = None;
    }

    /// Pretty-prints the request body in place, for the body types that have a
    /// pretty form.
    ///
    /// Deliberately an explicit action rather than something sending does: a
    /// server that cares about byte-for-byte payloads must receive what is on
    /// screen. Reformatting is `replace_all` rather than `set_value` so it can
    /// be undone, and a document that does not parse is left exactly as typed —
    /// the same rule the response viewer's Pretty toggle follows.
    pub fn format_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.request.body_type.is_formattable() {
            return;
        }

        let editor = self.request.body_editor.clone();
        let current = editor.read(cx).value().to_string();
        let formatted = body::prettify(&current, BodyKind::Json);
        if formatted == current {
            return;
        }

        editor.update(cx, |state, cx| {
            state.replace_all(formatted, window, cx);
        });
        self.request.dirty = true;
        cx.notify();
    }

    /// Re-indents one of the two script editors in place.
    ///
    /// The same bargain [`format_body`](Self::format_body) makes: an explicit
    /// action, `replace_all` so it can be undone, and nothing at all when the
    /// text is already in shape — so pressing Format twice does not stack two
    /// undo steps. What "format" means here is deliberately narrow; see
    /// [`script_format`](crate::api_explorer::models::script_format).
    pub fn format_script(
        &mut self,
        editor: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = editor.read(cx).value().to_string();
        let formatted = script_format::format(&current);
        if formatted == current {
            return;
        }

        editor.update(cx, |state, cx| {
            state.replace_all(formatted, window, cx);
        });
        self.request.dirty = true;
        cx.notify();
    }

    /// No response arrived. `error` is already the message: a transport failure
    /// and a failed pre-request script both end here, and the banner should not
    /// have to know which.
    fn fail(&mut self, error: Str, cx: &mut Context<Self>) {
        self.response.outcome = Outcome::Failed(error);
        self.send_task = None;
        // A failed request is still history: no status, no timing. Any tests a
        // pre-request script managed to define still travel with it.
        let snapshot = self.request.snapshot(cx);
        let tests = self.response.tests.summary();
        cx.emit(HistoryRecord {
            snapshot,
            status: None,
            elapsed: None,
            tests: (!tests.is_empty()).then_some(tests),
        });
        cx.notify();
    }

    fn receive(&mut self, exchange: Exchange, window: &mut Window, cx: &mut Context<Self>) {
        // Read the metadata history needs before the exchange is moved into the
        // outcome.
        let status = exchange.status;
        let elapsed = exchange.elapsed;

        self.response.reset_window();
        self.response.reset_json_tree();
        self.response.outcome = Outcome::Received(exchange);
        self.send_task = None;
        self.refresh_body(window, cx);

        let snapshot = self.request.snapshot(cx);
        // A summary, not the results: history is capped by count, so 200
        // unbounded result lists would be an unbounded footprint on a
        // session-scoped convenience, and a history row has space for one badge.
        let tests = self.response.tests.summary();
        cx.emit(HistoryRecord {
            snapshot,
            status: Some(status),
            elapsed: Some(elapsed),
            tests: (!tests.is_empty()).then_some(tests),
        });
        cx.notify();
    }

    /// Pushes the current body — pretty or raw, windowed to the visible line
    /// count — into the editor, and points the highlighter at the right
    /// grammar.
    ///
    /// Called on arrival, on a Pretty/Raw switch and on "load more", so those
    /// three paths cannot drift apart.
    pub fn refresh_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(exchange) = self.response.exchange() else {
            return;
        };

        use crate::api_explorer::state::response::BodyView;

        let kind = exchange.kind;
        let text = match self.response.body_view {
            BodyView::Pretty => body::prettify(&exchange.body, kind),
            BodyView::Raw => exchange.body.clone(),
            BodyView::Preview => body::preview(&exchange.body, kind),
            // Tree mode renders its own element from the parsed tree; the shared
            // editor is not shown, so there is nothing to refresh here.
            BodyView::Tree => return,
        };

        let (windowed, total) = window_lines(&text, self.response.visible_lines);
        self.response.total_lines = total;

        let body = self.response.body.clone();
        body.update(cx, |state, cx| {
            state.set_highlighter(kind.language(), cx);
            state.set_value(windowed, window, cx);
        });
    }
}
