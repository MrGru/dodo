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

/// How tall each script editor is.
///
/// Fixed rather than a share of the pane: the request editor opens 240px tall,
/// and splitting that between two editors leaves each about one line — legible
/// as a screenshot, useless as an editor. At this height the pair overflows a
/// short pane and the tab scrolls, which is the honest trade. Dragging the
/// divider down still shows both at once.
const SCRIPT_HEIGHT: Pixels = px(180.);

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
                // seeing the pair at once is the point of the tab. They scroll
                // together when the request editor is short.
                div()
                    .id("script-panes")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
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

/// A titled editor, one [`SCRIPT_HEIGHT`] tall.
fn script_pane(
    label: SharedString,
    editor: &Entity<InputState>,
    cx: &mut Context<ApiExplorer>,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .flex_shrink_0()
        .h(SCRIPT_HEIGHT)
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
