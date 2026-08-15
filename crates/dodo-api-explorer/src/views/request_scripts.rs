//! The Scripts tab: two editors, a per-editor templates menu and Format
//! button, and an honest note about what they do.
//!
//! Both editors are now live — the pre-request script runs before every send,
//! the post-response one after every response — so the note at the top no longer
//! says either of them is stored and not run. What it says instead is the part a
//! user cannot see from the editor: **what the sandbox denies**. The same
//! element, in the same place, with a truer sentence; the honesty rule the tab
//! was built around has not changed, only the fact it reports.
//!
//! Three things make the editors worth working in rather than merely present:
//! JavaScript highlighting (the grammar is compiled in — see `Cargo.toml`),
//! a syntax error underlined **where it was typed** with its message on a strip
//! under the header, and Format. What Format does is deliberately narrow, and
//! [`script_format`](crate::models::script_format) is where that
//! choice is argued.

use gpui::prelude::FluentBuilder as _;
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

use crate::app_icon::AppIcon;
use crate::i18n::{api_scripts, shared, t};
use crate::models::script_template::ScriptTemplate;
use crate::state::request::ScriptSlot;
use crate::state::tab::RequestTabState;
use crate::views::explorer::ApiExplorer;

impl ApiExplorer {
    pub(super) fn request_scripts_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let pre_open = self.pre_template_menu_open;
        let post_open = self.post_template_menu_open;

        v_flex()
            .size_full()
            .min_w_0()
            .child(self.scripts_notice(cx))
            .child(
                // Both editors on screen rather than behind a nested tab strip:
                // seeing the pair at once is the point of the tab. Each takes an
                // equal share of the available height and both stay visible even
                // when the request pane is short.
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.script_pane(
                        tab,
                        ScriptSlot::Pre,
                        ScriptTemplate::PRE_REQUEST,
                        pre_open,
                        cx,
                    ))
                    .child(self.script_pane(
                        tab,
                        ScriptSlot::Post,
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
            .min_w_0()
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
                    .child(t(api_scripts::Text::ScriptsSandboxNotice, cx)),
            )
    }

    /// A titled editor that takes an equal share of the pane, down to
    /// [`SCRIPT_MIN_HEIGHT`], with Format and a templates menu in its header and
    /// its parse error, if any, on a strip below.
    fn script_pane(
        &self,
        tab: &Entity<RequestTabState>,
        slot: ScriptSlot,
        templates: &'static [ScriptTemplate],
        menu_open: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let editor = state.request.script_editor(slot).clone();
        // Copied out: `state` borrows `cx`, which the header needs mutably.
        let error = state
            .request
            .script_error(slot)
            .map(|error| (error.line, error.detail.clone()));

        v_flex()
            .w_full()
            .min_w_0()
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
                            .flex_shrink_0()
                            .text_xs()
                            .font_bold()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(slot.label(), cx)),
                    )
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_1()
                            .child(self.format_button(tab, slot, cx))
                            .child(self.templates_menu(slot, &editor, templates, menu_open, cx)),
                    ),
            )
            .when_some(error, |this, (line, detail)| {
                this.child(self.syntax_error_strip(line, detail, cx))
            })
            .child(
                div().flex_1().min_h_0().min_w_0().overflow_hidden().child(
                    Input::new(&editor)
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(cx.theme().mono_font_size)
                        .size_full(),
                ),
            )
    }

    /// The parse failure, named and placed.
    ///
    /// The editor already draws a wavy underline at the spot; this says what is
    /// wrong without asking the user to hover, and states the line so the two
    /// agree. `line` arrives 0-based from the engine's own reporting and is
    /// shown 1-based, as every editor numbers lines.
    fn syntax_error_strip(
        &self,
        line: usize,
        detail: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .w_full()
            .min_w_0()
            .items_start()
            .gap_2()
            .px_3()
            .py_1()
            .bg(cx.theme().danger.opacity(0.08))
            .text_xs()
            .text_color(cx.theme().danger)
            .child(
                div()
                    .flex_shrink_0()
                    .child(Icon::new(AppIcon::AlertTriangle).size(px(12.))),
            )
            .child(div().flex_1().min_w_0().child(t(
                api_scripts::Text::SyntaxErrorAt {
                    line: line + 1,
                    detail,
                },
                cx,
            )))
    }

    /// Re-indents one editor. The same affordance the JSON formatter and the
    /// request body offer, under the same label.
    fn format_button(
        &self,
        tab: &Entity<RequestTabState>,
        slot: ScriptSlot,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tab = tab.clone();
        Button::new((slot.id_prefix(), 0u64))
            .ghost()
            .xsmall()
            .label(t(shared::Text::FormatButton, cx))
            .on_click(cx.listener(move |_, _, window, cx| {
                let editor = tab.read(cx).request.script_editor(slot).clone();
                tab.update(cx, |state, cx| {
                    state.format_script(editor, window, cx);
                });
                cx.notify();
            }))
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
                    .label(t(api_scripts::Text::InsertTemplate, cx)),
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
                            .child(t(api_scripts::Text::InsertTemplate, cx)),
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
