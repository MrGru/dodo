//! The Scripts tab: two editors, and an honest note about what they do.
//!
//! This phase stores and edits scripts and nothing else — there is no engine to
//! run them, and there is no way to run them by accident. The note at the top
//! of the pane says exactly that, and the placeholders inside both editors say
//! it again in the conditional ("would run"), because a user who never reads
//! the banner still reads the empty editor.
//!
//! Nothing here reaches `RequestDraft`: a field the service layer carries but
//! never uses would be the beginning of pretending.

use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme as _, Icon, StyledExt as _, h_flex, v_flex};

/// The least each script editor is given before the pair starts to scroll.
///
/// The two editors share the pane's height (each `flex_1`) so both are visible
/// at once, which is the point of the tab. This floor keeps each usable when
/// the request pane is dragged very short: below it, the pane scrolls rather
/// than squeezing the post-response editor down to a single reachable line.
const SCRIPT_MIN_HEIGHT: Pixels = px(150.);

/// The content height the two editors need before they stop shrinking and the
/// pane starts to scroll — both floors plus a little for the two headers.
const SCRIPTS_MIN_CONTENT: Pixels = px(330.);

use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn request_scripts_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let state = tab.read(cx);
        let pre = state.request.pre_request_script.clone();
        let post = state.request.post_response_script.clone();

        v_flex()
            .size_full()
            .child(self.scripts_notice(cx))
            .child(
                // Both panes on screen rather than behind a nested tab strip:
                // seeing the pair at once is the point of the tab. Each editor
                // takes half the available height; when the pane is dragged
                // shorter than the pair can fit, the inner column keeps its
                // minimum and this scrolls instead of hiding the second editor.
                div()
                    .id("script-panes")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .size_full()
                            .min_h(SCRIPTS_MIN_CONTENT)
                            .child(script_pane(t(Str::PreRequestScriptLabel, cx), &pre, cx))
                            .child(script_pane(t(Str::PostResponseScriptLabel, cx), &post, cx)),
                    ),
            )
            .into_any_element()
    }

    /// The one line that keeps this tab honest.
    fn scripts_notice(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .items_start()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().warning.opacity(0.08))
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(cx.theme().warning)
                    .child(Icon::new(AppIcon::SquareCode).size(px(14.))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(t(Str::ScriptsNotExecuted, cx)),
            )
    }
}

/// A titled editor that takes an equal share of the pane, down to
/// [`SCRIPT_MIN_HEIGHT`].
fn script_pane(
    label: SharedString,
    editor: &Entity<InputState>,
    cx: &mut Context<ApiExplorer>,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .flex_1()
        .min_h(SCRIPT_MIN_HEIGHT)
        .child(
            div()
                .w_full()
                .px_3()
                .py_1p5()
                .text_xs()
                .font_bold()
                .text_color(cx.theme().muted_foreground)
                .border_b_1()
                .border_color(cx.theme().border.opacity(0.5))
                .child(label),
        )
        .child(
            div().flex_1().min_h_0().child(
                Input::new(editor)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size)
                    .size_full(),
            ),
        )
}
