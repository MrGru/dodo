//! The Generate code dialog: one request, four languages, and a Copy action.
//!
//! # Why a dialog and not a sixth request tab
//!
//! The request tab strip is already five tabs wide and the request column is
//! routinely 520px; a sixth would have pushed the strip into horizontal scrolling
//! for something you open, read, copy and leave. That is a dialog's shape, so it
//! is a `window.open_dialog`, following the pattern
//! [`environments_editor`](super::environments_editor) and
//! [`script_consent`](super::script_consent) document at length — including both
//! consequences: the body is an **entity** (a dialog layer does not repaint on
//! the page's `cx.notify()`), and its width is **stated** rather than `w_full`.
//!
//! # It never touches the page
//!
//! Unlike the other two dialogs, this one holds no [`ApiExplorer`] handle at all.
//! Everything it needs — the request and the variables to resolve it against — is
//! plain data read by the caller and passed in, so the "may not read the page
//! entity while being constructed" trap cannot be sprung here: there is nothing
//! to read. It also means the snippet is a snapshot of the request as it was when
//! the button was pressed, which is what a user reading generated code expects.
//!
//! # What the notice says, and why it is never absent
//!
//! Generated code carries credentials. Which ones, exactly, is
//! [`services::codegen`]'s policy — a reference to a variable marked `secret` is
//! withheld as `{{name}}`, everything else is resolved — and the whole point of
//! stating it here is that a user about to press Copy can see what is in the
//! text. So one of three lines is **always** on screen:
//!
//! - secrets withheld → the warning colour, naming them, and saying plainly that
//!   everything else (including a token typed straight into the Auth tab) is in
//!   the code;
//! - secrets resolved → the danger colour, saying the code holds those values in
//!   plain text;
//! - no secret involved → the muted colour, still saying the code carries the
//!   request's real values.
//!
//! There is no state in which the dialog is silent about this, and no state in
//! which it implies a value is protected when it is in the copied text.
//!
//! [`services::codegen`]: crate::services::codegen

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable, IntoElement,
    ParentElement as _, Pixels, Render, Styled as _, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputState};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme as _, Icon, WindowExt as _, h_flex, v_flex};

use crate::app_icon::AppIcon;
use crate::i18n::{Str, api_explorer, api_scripts, t};
use crate::models::codegen::CodeTarget;
use crate::models::snapshot::RequestSnapshot;
use crate::models::variables::VariableSet;
use crate::services::codegen;

/// The card's preferred width, and the height of the code view inside it. Both
/// shrink to fit a small window *before* the dialog is built — `Dialog` computes
/// its `left` from the width it is given, so an over-wide card is pushed
/// off-centre rather than clipped.
const PANEL_W: Pixels = px(620.);
const CODE_H: Pixels = px(300.);
/// A floor under the code view, so a short window leaves a readable snippet
/// rather than a two-line slot.
const MIN_CODE_H: Pixels = px(120.);
const PANEL_MARGIN: Pixels = px(24.);
/// `Dialog`'s own left and right padding (`Edges::all(16)`).
const DIALOG_PADDING_X: Pixels = px(32.);

/// Opens the dialog for one request.
///
/// `snapshot` and `variables` are read by the caller — see this module's doc.
pub fn open(snapshot: RequestSnapshot, variables: VariableSet, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| GenerateCodeDialog::new(snapshot, variables, window, cx));

    window.open_dialog(cx, move |dialog, window, cx| {
        let view = view.clone();
        let width = card_width(window);
        dialog
            .w(width)
            .title(t(api_explorer::Text::GenerateCode, cx))
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

pub struct GenerateCodeDialog {
    snapshot: RequestSnapshot,
    variables: VariableSet,
    target: CodeTarget,
    /// Whether the secret values are resolved into the snippet. Starts off; see
    /// this module's doc.
    reveal_secrets: bool,
    /// The snippet, in the same code editor the Scripts tab uses — so a generated
    /// JavaScript body is highlighted and a long line soft-wraps rather than
    /// running off the card.
    editor: Entity<InputState>,
    /// The secret variables the snippet left as placeholders, as the last
    /// generation reported them.
    withheld: Vec<String>,
    /// Whether any secret variable is defined at all. Decides whether the toggle
    /// is worth showing — a request in an environment with no secrets should not
    /// carry a control that can do nothing.
    has_secrets: bool,
    /// The substitution failure shown in place of the snippet, if any.
    error: Option<Str>,
    focus_handle: FocusHandle,
}

impl GenerateCodeDialog {
    fn new(
        snapshot: RequestSnapshot,
        variables: VariableSet,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let has_secrets = !variables.secret_names().is_empty();
        let editor = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor(CodeTarget::default().editor_language())
                .multi_line(true)
                .line_number(true)
                .soft_wrap(true)
        });

        let mut dialog = Self {
            snapshot,
            variables,
            target: CodeTarget::default(),
            reveal_secrets: false,
            editor,
            withheld: Vec::new(),
            has_secrets,
            error: None,
            focus_handle: cx.focus_handle(),
        };
        dialog.regenerate(window, cx);
        dialog
    }

    /// Re-runs the generator and pushes the result into the editor.
    ///
    /// Called on construction and whenever the target or the reveal toggle
    /// changes — never per frame: generating walks the whole request and the
    /// editor re-parses what it is given.
    fn regenerate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.target.editor_language();
        self.editor
            .update(cx, |state, cx| state.set_highlighter(language, cx));

        match codegen::generate(
            self.target,
            &self.snapshot,
            &self.variables,
            self.reveal_secrets,
        ) {
            Ok(generated) => {
                self.withheld = generated.withheld;
                self.error = None;
                self.editor.update(cx, |state, cx| {
                    state.set_value(generated.code, window, cx);
                });
            }
            Err(error) => {
                self.withheld.clear();
                self.error = Some(error.message());
                // The stale snippet is cleared rather than left under an error
                // banner that contradicts it.
                self.editor
                    .update(cx, |state, cx| state.set_value("", window, cx));
            }
        }
        cx.notify();
    }

    fn select(&mut self, target: CodeTarget, window: &mut Window, cx: &mut Context<Self>) {
        if self.target == target {
            return;
        }
        self.target = target;
        self.regenerate(window, cx);
    }

    fn set_reveal_secrets(&mut self, reveal: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.reveal_secrets == reveal {
            return;
        }
        self.reveal_secrets = reveal;
        self.regenerate(window, cx);
    }

    /// Copies whatever is in the editor — not the last generated string, so an
    /// edit made in the dialog before pressing Copy is honoured.
    fn copy(&self, cx: &mut Context<Self>) {
        let code = self.editor.read(cx).value().to_string();
        if code.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(code));
    }
}

impl Focusable for GenerateCodeDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for GenerateCodeDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A third of the window, so a short one leaves room for the notice and
        // the buttons rather than pushing them out of the card.
        let code_h = CODE_H
            .min(window.viewport_size().height / 3.)
            .max(MIN_CODE_H);

        v_flex()
            .w_full()
            .gap_3()
            .child(self.target_bar(cx))
            .child(self.notice(cx))
            .when(self.has_secrets, |this| this.child(self.reveal_toggle(cx)))
            .children(self.error_banner(cx))
            .child(
                div()
                    .h(code_h)
                    .w_full()
                    .min_w_0()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        Input::new(&self.editor)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .size_full(),
                    ),
            )
            .child(
                h_flex().w_full().justify_end().child(
                    Button::new("generate-code-copy")
                        .primary()
                        .icon(AppIcon::Copy)
                        .label(t(api_scripts::Text::Copy, cx))
                        .on_click(cx.listener(|this, _, _, cx| this.copy(cx))),
                ),
            )
    }
}

impl GenerateCodeDialog {
    /// The four targets. `min_w_0` and `overflow_hidden` are what keep the strip
    /// inside a card narrowed to a 520px window instead of widening it — the same
    /// rule the request tab strip carries.
    fn target_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                TabBar::new("generate-code-targets")
                    .selected_index(self.target.index())
                    .children(CodeTarget::ALL.map(|target| Tab::new().label(t(target.label(), cx))))
                    .on_click(cx.listener(move |this, index: &usize, window, cx| {
                        if let Some(target) = CodeTarget::ALL.get(*index).copied() {
                            this.select(target, window, cx);
                        }
                    })),
            )
    }

    /// The one line that is never absent. See this module's doc for the three
    /// states and why each is worded the way it is.
    fn notice(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (message, colour) = if self.reveal_secrets && self.has_secrets {
            // Deliberately uncounted: how many secrets a request *uses* is not
            // what `withheld` holds once they are resolved, and a wrong number
            // in this particular sentence would be worse than no number.
            (
                api_explorer::Text::GenerateCodeSecretsRevealed,
                cx.theme().danger,
            )
        } else if self.withheld.is_empty() {
            (
                api_explorer::Text::GenerateCodeCarriesValues,
                cx.theme().muted_foreground,
            )
        } else {
            (
                api_explorer::Text::GenerateCodeSecretsWithheld(self.withheld.join(", ")),
                cx.theme().warning,
            )
        };

        h_flex()
            .w_full()
            .items_start()
            .gap_1p5()
            .text_xs()
            .text_color(colour)
            .child(
                Icon::new(AppIcon::AlertTriangle)
                    .size(px(12.))
                    .flex_shrink_0(),
            )
            // `flex_1` and `min_w_0` so the sentence wraps inside the card
            // instead of setting its width.
            .child(div().flex_1().min_w_0().child(t(message, cx)))
    }

    fn reveal_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Checkbox::new("generate-code-reveal")
            .checked(self.reveal_secrets)
            .label(t(api_explorer::Text::GenerateCodeRevealSecrets, cx))
            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                this.set_reveal_secrets(*checked, window, cx);
            }))
    }

    /// The substitution failure, in the same wording the send path uses.
    fn error_banner(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let error = self.error.clone()?;
        Some(
            h_flex()
                .w_full()
                .items_start()
                .gap_1p5()
                .px_2()
                .py_1p5()
                .rounded(cx.theme().radius)
                .text_xs()
                .text_color(cx.theme().danger)
                .bg(cx.theme().danger.opacity(0.1))
                .child(
                    Icon::new(AppIcon::AlertTriangle)
                        .size(px(12.))
                        .flex_shrink_0(),
                )
                .child(div().flex_1().min_w_0().child(t(error, cx)))
                .into_any_element(),
        )
    }
}
