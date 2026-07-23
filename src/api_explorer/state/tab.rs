//! One open request tab: its request, its response, and its in-flight task.
//!
//! Each tab is its own entity, which is what makes the tabs genuinely
//! independent — sending in one leaves the others' editors, responses and
//! scroll positions untouched.

use std::sync::Arc;

use gpui::{Context, EventEmitter, Task, Window};

use crate::api_explorer::models::exchange::{BodyKind, Exchange};
use crate::api_explorer::services::http::{body, prepare};
use crate::api_explorer::services::{Transport, TransportError};
use crate::api_explorer::state::history::HistoryRecord;
use crate::api_explorer::state::request::RequestState;
use crate::api_explorer::state::response::{Outcome, ResponseState, window_lines};

pub struct RequestTabState {
    pub request: RequestState,
    pub response: ResponseState,
    /// The in-flight request, if any.
    ///
    /// Held so that dropping the tab cancels the request, and so that pressing
    /// Send twice replaces the first task rather than racing it: assigning a
    /// new `Task` drops the old one, which cancels it.
    send_task: Option<Task<()>>,
}

/// A finished request is emitted so the page can record it in history. The page
/// subscribes to every tab; the tab is the one place that knows a request
/// completed, which is the seam phase 1 described.
impl EventEmitter<HistoryRecord> for RequestTabState {}

impl RequestTabState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            request: RequestState::new(window, cx),
            response: ResponseState::new(window, cx),
            send_task: None,
        }
    }

    /// Sends the request this tab currently describes.
    ///
    /// Validation happens here, on the UI thread, because it is cheap and a
    /// mistyped URL should be reported instantly rather than after a task
    /// hop. The request itself never runs here: [`Transport::execute`] is
    /// blocking and goes to the background executor.
    pub fn send(
        &mut self,
        transport: Arc<dyn Transport>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = self.request.draft(cx);
        let prepared = match prepare::prepare(&draft) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };

        self.response.outcome = Outcome::InFlight;
        cx.notify();

        self.send_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { transport.execute(prepared) })
                .await;

            // The window or the tab can be gone by the time this lands; both
            // are ordinary shutdown paths, not errors.
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(exchange) => this.receive(exchange, window, cx),
                Err(error) => this.fail(error, cx),
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

    fn fail(&mut self, error: TransportError, cx: &mut Context<Self>) {
        self.response.outcome = Outcome::Failed(error.message());
        self.send_task = None;
        // A failed request is still history: no status, no timing.
        let snapshot = self.request.snapshot(cx);
        cx.emit(HistoryRecord {
            snapshot,
            status: None,
            elapsed: None,
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
        cx.emit(HistoryRecord {
            snapshot,
            status: Some(status),
            elapsed: Some(elapsed),
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

        use crate::api_explorer::services::http::body;
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
