//! The Tests tab: what this request's scripts asserted about the response.
//!
//! # Four situations, four different things to say
//!
//! Collapsing these into one empty state is the failure mode this tab was
//! designed against, because each one has a different next action:
//!
//! | Situation | What it says |
//! |---|---|
//! | No response yet | Handled a level up, by the pane's own `NoResponseYet` state — there is nothing to have tested |
//! | The request has no post-response script | *"This request has no tests"*, and a button that opens the Scripts tab with the assertion template inserted |
//! | The script ran and defined none | *"The script ran and defined no tests"* — a different sentence, because the fix is different |
//! | The script failed | The failure, first and prominently. It is the actionable case and must not be buried under a "0 tests" empty state |
//!
//! # Rows, not a table
//!
//! Hand-rolled `v_flex`/`h_flex` rows, matching the Headers and Cookies panes,
//! rather than `gpui_component::table`: a test row is a glyph, a wrapping name,
//! a right-aligned duration and — when it failed — a message under it, which is
//! not a grid.
//!
//! A **failed** row and an **errored** row look different on purpose. Postman
//! paints both red; the distinction is what tells the user whether their API is
//! wrong or their script is.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};

use crate::api_explorer::components::empty_state::empty_state;
use crate::api_explorer::models::exchange::format_duration;
use crate::api_explorer::models::script::is_runnable;
use crate::api_explorer::models::script_template::ScriptTemplate;
use crate::api_explorer::models::test_result::{ScriptPhase, TestOutcome, TestResult, TestSummary};
use crate::api_explorer::state::request::{RequestTab, ScriptSlot};
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn tests_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let state = tab.read(cx);
        let report = &state.response.tests;
        let has_script = is_runnable(&state.request.post_response_script.read(cx).value());

        // Copied out before `cx` is needed mutably below.
        let error = report.error.clone();
        let summary = report.summary();
        let elapsed = report.elapsed;
        let dropped = report.dropped;
        let grouped = report.spans_both_phases();
        let ran = report.ran;
        let nothing_to_show = report.is_empty();
        // Grouped by phase when both hooks produced tests, in phase order.
        let results: Vec<TestResult> = if grouped {
            ScriptPhase::ALL
                .into_iter()
                .flat_map(|phase| report.phase(phase).cloned().collect::<Vec<_>>())
                .collect()
        } else {
            report.results.clone()
        };

        if nothing_to_show {
            return if has_script {
                // The script ran and asserted nothing. Different words from "no
                // script", because the fix is to write a `pm.test`, not to write
                // a script.
                empty_state(
                    AppIcon::SquareCode,
                    t(
                        if ran {
                            Str::TestsScriptDefinedNone
                        } else {
                            Str::TestsNotRun
                        },
                        cx,
                    ),
                    Some(t(Str::TestsScriptDefinedNoneHint, cx)),
                    cx,
                )
                .into_any_element()
            } else {
                self.no_tests_state(tab, cx)
            };
        }

        // Built eagerly rather than in a `children` closure: every row needs
        // `cx` mutably, and a closure that captured it could not hand the
        // element back out.
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        if grouped {
            for phase in ScriptPhase::ALL {
                let of_phase: Vec<&TestResult> = results
                    .iter()
                    .filter(|result| result.phase == phase)
                    .collect();
                if of_phase.is_empty() {
                    continue;
                }
                rows.push(self.phase_heading(phase, cx).into_any_element());
                for (index, result) in of_phase.into_iter().enumerate() {
                    rows.push(self.test_row(index, result, cx).into_any_element());
                }
            }
        } else {
            for (index, result) in results.iter().enumerate() {
                rows.push(self.test_row(index, result, cx).into_any_element());
            }
        }

        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .when_some(error, |this, error| {
                this.child(self.test_error_banner(error, cx))
            })
            .when(!summary.is_empty(), |this| {
                this.child(self.test_summary_bar(summary, elapsed, cx))
            })
            .child(
                v_flex()
                    .id("test-rows")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .when(dropped > 0, |this| {
                this.child(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(Str::TestsDropped(dropped), cx)),
                )
            })
            .into_any_element()
    }

    /// "This request has no tests", and the one click that changes that.
    fn no_tests_state(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let tab = tab.clone();

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .p_6()
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(AppIcon::SquareCode).size(px(28.)))
                    .child(div().text_sm().child(t(Str::TestsNone, cx)))
                    .child(
                        div()
                            .text_xs()
                            .text_center()
                            .max_w(px(260.))
                            .child(t(Str::TestsNoneHint, cx)),
                    ),
            )
            .child(
                Button::new("tests-add")
                    .small()
                    .icon(AppIcon::SquareCode)
                    .label(t(Str::TestsAddOne, cx))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        // Open the editor the test would go in *and* seed it, so
                        // the button lands the user somewhere they can type
                        // rather than somewhere they have to look around.
                        tab.update(cx, |state, cx| {
                            state.request.active_tab = RequestTab::Scripts;
                            let editor = state.request.script_editor(ScriptSlot::Post).clone();
                            let snippet = format!("{}\n", ScriptTemplate::AssertStatus.snippet());
                            editor.update(cx, |editor, cx| editor.insert(snippet, window, cx));
                            cx.notify();
                        });
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// The script itself failed. First, and loud: it is the actionable case.
    fn test_error_banner(&self, error: Str, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_start()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().danger.opacity(0.08))
            .text_xs()
            .text_color(cx.theme().danger)
            .child(
                div()
                    .flex_shrink_0()
                    .child(Icon::new(AppIcon::AlertTriangle).size(px(14.))),
            )
            .child(div().flex_1().min_w_0().child(t(error, cx)))
    }

    /// `3 passed · 1 failed · 24 ms`, coloured by the worst outcome.
    fn test_summary_bar(
        &self,
        summary: TestSummary,
        elapsed: std::time::Duration,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colour = summary_colour(&summary, cx);

        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .text_xs()
            .child(
                div()
                    .font_bold()
                    .text_color(colour)
                    .child(t(Str::TestsPassedCount(summary.passed), cx)),
            )
            .when(summary.failed > 0, |this| {
                this.child(
                    div()
                        .text_color(cx.theme().danger)
                        .child(t(Str::TestsFailedCount(summary.failed), cx)),
                )
            })
            .when(summary.errored > 0, |this| {
                this.child(
                    div()
                        .text_color(cx.theme().warning)
                        .child(t(Str::TestsErroredCount(summary.errored), cx)),
                )
            })
            .child(div().flex_1().min_w_0())
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(cx.theme().muted_foreground)
                    .child(format_duration(elapsed)),
            )
    }

    fn phase_heading(&self, phase: ScriptPhase, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .px_3()
            .pt_2()
            .pb_1()
            .text_xs()
            .font_bold()
            .text_color(cx.theme().muted_foreground)
            .child(t(phase.label(), cx))
    }

    /// One result: a glyph, the name the script gave it, its duration, and — for
    /// anything but a pass — the message under it.
    fn test_row(
        &self,
        index: usize,
        result: &TestResult,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (icon, colour) = match &result.outcome {
            TestOutcome::Passed => (AppIcon::CircleCheck, cx.theme().success),
            TestOutcome::Failed { .. } => (AppIcon::CircleX, cx.theme().danger),
            TestOutcome::Errored { .. } => (AppIcon::AlertTriangle, cx.theme().warning),
        };
        let message = result.outcome.message().map(str::to_string);

        v_flex()
            .w_full()
            .min_w_0()
            .px_3()
            .py_1p5()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.4))
            .when(index % 2 == 1, |this| {
                this.bg(cx.theme().list_even.opacity(0.5))
            })
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(colour)
                            .child(Icon::new(icon).size(px(14.))),
                    )
                    .child(
                        // The name a script wrote, verbatim and wrapping: it is
                        // user content, and truncating it would hide which
                        // assertion failed.
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .child(result.name.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format_duration(result.elapsed)),
                    ),
            )
            .when_some(message, |this, message| {
                this.child(
                    div()
                        // Lined up under the name, not under the glyph.
                        .pl(px(22.))
                        .min_w_0()
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(cx.theme().muted_foreground)
                        .child(message),
                )
            })
    }
}

/// The colour a summary reads as: its worst outcome.
fn summary_colour(summary: &TestSummary, cx: &Context<ApiExplorer>) -> gpui::Hsla {
    if summary.failed > 0 {
        cx.theme().danger
    } else if summary.errored > 0 {
        cx.theme().warning
    } else {
        cx.theme().success
    }
}
