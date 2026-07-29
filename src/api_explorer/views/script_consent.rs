//! The approval prompt for a script that arrived by import.
//!
//! `decision-imported-script-consent` requires that the prompt **shows the
//! script**, not a yes/no about an invisible thing: the user approves what they
//! can read. So the body is a read-only code editor holding the exact text that
//! will run, and the two answers are stated as what they do — run it, or send
//! the request without it.
//!
//! Dismissing the dialog (Escape, the close button, a click on the backdrop)
//! does **neither**: the send simply does not start. "I did not mean to press
//! that" is the one answer a modal must always have, and silently choosing one
//! of the two on the user's behalf would be the wrong kind of helpful.
//!
//! It is a `window.open_dialog` for the reasons `environments_editor` and
//! `docker::views::detail` document at length, and it follows both of the
//! consequences those record: the body is an **entity**, and its width is
//! **stated** rather than `w_full`.

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement as _,
    Pixels, Render, Styled as _, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme as _, StyledExt as _, WindowExt as _, h_flex, v_flex};

use crate::api_explorer::models::script_consent::ConsentKey;
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::i18n::{Str, t};

/// The card's preferred width, and the height of the script view inside it.
/// Both shrink to fit a small window before the dialog is built — `Dialog`
/// computes its `left` from the width it is given, so an over-wide card is
/// pushed off-centre rather than clipped.
const PANEL_W: Pixels = px(560.);
const SCRIPT_H: Pixels = px(220.);
const PANEL_MARGIN: Pixels = px(24.);
/// `Dialog`'s own left and right padding (`Edges::all(16)`).
const DIALOG_PADDING_X: Pixels = px(32.);

/// Opens the prompt for one request's pre-request script.
///
/// Everything the body needs is passed in as plain data. It must not read the
/// page entity while being constructed: this is reached from a click listener,
/// so the page is leased for the whole call and a `page.read(cx)` here would
/// panic at runtime with no compile error to warn about it.
pub fn open(
    page: Entity<ApiExplorer>,
    tab: Entity<RequestTabState>,
    request_name: String,
    script: String,
    key: ConsentKey,
    window: &mut Window,
    cx: &mut App,
) {
    let view =
        cx.new(|cx| ScriptConsentDialog::new(page, tab, request_name, script, key, window, cx));

    window.open_dialog(cx, move |dialog, window, cx| {
        let view = view.clone();
        let width = card_width(window);
        dialog
            .w(width)
            .title(t(Str::ScriptConsentTitle, cx))
            // `content`, not `child`: a plain child is wrapped in an
            // `overflow_y_scrollbar` box that content-sizes everything inside.
            .content(move |content, _, _| {
                content.child(div().w(width - DIALOG_PADDING_X).child(view.clone()))
            })
    });
}

fn card_width(window: &Window) -> Pixels {
    PANEL_W.min(window.viewport_size().width - PANEL_MARGIN * 2.)
}

struct ScriptConsentDialog {
    page: Entity<ApiExplorer>,
    tab: Entity<RequestTabState>,
    request_name: String,
    key: ConsentKey,
    /// The script, in a read-only code editor: the same widget the Scripts tab
    /// edits it in, so the text the user approves looks exactly like the text
    /// they would have read there.
    script: Entity<InputState>,
    focus_handle: FocusHandle,
}

impl ScriptConsentDialog {
    fn new(
        page: Entity<ApiExplorer>,
        tab: Entity<RequestTabState>,
        request_name: String,
        script: String,
        key: ConsentKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("text")
                .multi_line(true)
                .line_number(true)
                .soft_wrap(true)
                .default_value(script)
        });

        Self {
            page,
            tab,
            request_name,
            key,
            script: editor,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Focusable for ScriptConsentDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ScriptConsentDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let script_h = SCRIPT_H.min(window.viewport_size().height / 3.);

        let approve = (self.page.clone(), self.tab.clone(), self.key.clone());
        let decline = (self.page.clone(), self.tab.clone());

        v_flex()
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::ScriptConsentExplain, cx)),
            )
            .child(
                div()
                    .text_xs()
                    .font_bold()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::ScriptConsentRequest(self.request_name.clone()), cx)),
            )
            .child(
                div()
                    .h(script_h)
                    .w_full()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        Input::new(&self.script)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .size_full(),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("script-consent-skip")
                            .label(t(Str::ScriptConsentSkip, cx))
                            .on_click(cx.listener(move |_, _, window, cx| {
                                let (page, tab) = decline.clone();
                                window.close_dialog(cx);
                                page.update(cx, |page, cx| {
                                    page.send_without_script(&tab, window, cx);
                                });
                            })),
                    )
                    .child(
                        Button::new("script-consent-run")
                            .primary()
                            .label(t(Str::ScriptConsentRun, cx))
                            .on_click(cx.listener(move |_, _, window, cx| {
                                let (page, tab, key) = approve.clone();
                                window.close_dialog(cx);
                                page.update(cx, |page, cx| {
                                    page.approve_script_and_send(&tab, key, window, cx);
                                });
                            })),
                    ),
            )
    }
}
