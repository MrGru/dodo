//! The Scripts tab: two editors, a per-editor templates menu, and an honest
//! note about what they do.
//!
//! This phase stores and edits scripts and nothing else — there is no engine to
//! run them, and there is no way to run them by accident. The note at the top
//! of the pane says exactly that, and the placeholders inside both editors say
//! it again in the conditional ("would run"), because a user who never reads
//! the banner still reads the empty editor. The templates menu is an editing
//! convenience only: it inserts snippets, it does not run them.
//!
//! Nothing here reaches `RequestDraft`: a field the service layer carries but
//! never uses would be the beginning of pretending.

use gpui::{
    Context, Entity, IntoElement, ParentElement as _, Pixels, SharedString, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::popover::Popover;
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};

/// A small floor under each script editor, so a very short pane still shows two
/// usable editors rather than squeezing one to a line. The two editors share
/// the pane's height (each `flex_1`), so both are always on screen at once —
/// which is the whole point of the tab — and grow as the request pane grows.
const SCRIPT_MIN_HEIGHT: Pixels = px(64.);

use crate::api_explorer::models::script_template::ScriptTemplate;
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

/// Which of the two editors a templates menu belongs to. Selects the editor,
/// the popover's open flag, and the element ids.
#[derive(Clone, Copy)]
enum ScriptSlot {
    Pre,
    Post,
}

impl ScriptSlot {
    fn id_prefix(self) -> &'static str {
        match self {
            ScriptSlot::Pre => "pre-script",
            ScriptSlot::Post => "post-script",
        }
    }
}

impl ApiExplorer {
    pub(super) fn request_scripts_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let state = tab.read(cx);
        let pre = state.request.pre_request_script.clone();
        let post = state.request.post_response_script.clone();
        let pre_open = self.pre_template_menu_open;
        let post_open = self.post_template_menu_open;

        v_flex()
            .size_full()
            .child(self.scripts_notice(cx))
            .child(
                // Both editors on screen rather than behind a nested tab strip:
                // seeing the pair at once is the point of the tab. Each takes an
                // equal share of the available height and both stay visible even
                // when the request pane is short.
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.script_pane(
                        ScriptSlot::Pre,
                        t(Str::PreRequestScriptLabel, cx),
                        &pre,
                        ScriptTemplate::PRE_REQUEST,
                        pre_open,
                        cx,
                    ))
                    .child(self.script_pane(
                        ScriptSlot::Post,
                        t(Str::PostResponseScriptLabel, cx),
                        &post,
                        ScriptTemplate::POST_RESPONSE,
                        post_open,
                        cx,
                    )),
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

    /// A titled editor that takes an equal share of the pane, down to
    /// [`SCRIPT_MIN_HEIGHT`], with a templates menu in its header.
    fn script_pane(
        &self,
        slot: ScriptSlot,
        label: SharedString,
        editor: &Entity<InputState>,
        templates: &'static [ScriptTemplate],
        menu_open: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .w_full()
            .flex_1()
            .min_h(SCRIPT_MIN_HEIGHT)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_3()
                    .py_1p5()
                    .border_b_1()
                    .border_color(cx.theme().border.opacity(0.5))
                    .child(
                        div()
                            .text_xs()
                            .font_bold()
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .child(self.templates_menu(slot, editor, templates, menu_open, cx)),
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

    /// The templates popover for one editor: a menu of snippets, each inserted
    /// at the cursor when picked.
    fn templates_menu(
        &self,
        slot: ScriptSlot,
        editor: &Entity<InputState>,
        templates: &'static [ScriptTemplate],
        menu_open: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let prefix = slot.id_prefix();

        let items = templates.iter().map(|template| {
            let template = *template;
            let editor = editor.clone();
            Button::new((prefix, template as usize))
                .ghost()
                .w_full()
                .justify_start()
                .label(t(template.label(), cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    // Insert at the cursor, undoably, with a trailing newline so
                    // the next line starts clean.
                    let snippet = format!("{}\n", template.snippet());
                    editor.update(cx, |state, cx| state.insert(snippet, window, cx));
                    this.set_template_menu_open(slot, false);
                    cx.notify();
                }))
        });

        Popover::new(SharedString::from(format!("{prefix}-templates")))
            .open(menu_open)
            .on_open_change(cx.listener(move |this, open, _, cx| {
                this.set_template_menu_open(slot, *open);
                cx.notify();
            }))
            .trigger(
                Button::new(SharedString::from(format!("{prefix}-templates-trigger")))
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::SquareCode)
                    .label(t(Str::InsertTemplate, cx)),
            )
            .w(px(240.))
            .child(
                v_flex()
                    .gap_1()
                    .p_1()
                    .child(
                        div()
                            .text_xs()
                            .font_bold()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(Str::InsertTemplate, cx)),
                    )
                    .children(items),
            )
    }

    /// Sets the open flag for the given editor's templates popover.
    fn set_template_menu_open(&mut self, slot: ScriptSlot, open: bool) {
        match slot {
            ScriptSlot::Pre => self.pre_template_menu_open = open,
            ScriptSlot::Post => self.post_template_menu_open = open,
        }
    }
}
