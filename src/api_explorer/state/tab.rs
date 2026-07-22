//! One open request tab: its request, its response, and its in-flight task.
//!
//! Each tab is its own entity, which is what makes the tabs genuinely
//! independent — sending in one leaves the others' editors, responses and
//! scroll positions untouched.

use std::sync::Arc;

use gpui::{Context, Task, Window};

use crate::api_explorer::models::exchange::Exchange;
use crate::api_explorer::services::http::prepare;
use crate::api_explorer::services::{Transport, TransportError};
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

    fn fail(&mut self, error: TransportError, cx: &mut Context<Self>) {
        self.response.outcome = Outcome::Failed(error.message());
        self.send_task = None;
        cx.notify();
    }

    fn receive(&mut self, exchange: Exchange, window: &mut Window, cx: &mut Context<Self>) {
        self.response.reset_window();
        self.response.outcome = Outcome::Received(exchange);
        self.send_task = None;
        self.refresh_body(window, cx);
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

        let kind = exchange.kind;
        let text = match self.response.body_view {
            crate::api_explorer::state::response::BodyView::Pretty => {
                crate::api_explorer::services::http::body::prettify(&exchange.body, kind)
            }
            crate::api_explorer::state::response::BodyView::Raw => exchange.body.clone(),
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
