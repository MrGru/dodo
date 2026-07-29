//! The environment picker and the resolved-URL preview beside it.
//!
//! # Why this is its own row and not part of the URL row
//!
//! The request bar already carries the method picker, the URL, code generation,
//! save and Send. At a ~520px window the URL field is the only thing that can
//! give way, and it is already the narrowest it can usefully be; a sixth
//! control there would squeeze it to a few characters. So the picker sits on a
//! slim row of its own directly beneath, still inside the request bar block —
//! and that row is also where the preview belongs, since the preview is about
//! the URL directly above it.
//!
//! # The preview is the variable-aware affordance
//!
//! The brief asked for a way to see what a `{{token}}` currently resolves to,
//! ideally on hover or focus of the token itself. **That is not reachable with
//! this component set**: `InputState` renders its own rope through a custom
//! element and exposes no per-token hit region, no character-range hover and no
//! inline decoration API — only whole-field diagnostics, which are anchored to
//! parse positions rather than to hover. Building it would mean re-implementing
//! text layout outside the widget.
//!
//! What is here instead costs one row and answers the same question: whenever
//! the URL contains a reference, the row shows the URL **as it will be sent**,
//! with each unresolved name called out by name in the warning colour, and a
//! tooltip naming the scope each value came from. It appears only when there is
//! something to preview, so a request with no variables pays nothing.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::popover::Popover;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _, h_flex, v_flex};

use crate::api_explorer::models::interpolate::{has_reference, interpolate, resolve_all};
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::environments_editor;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

/// How wide the picker trigger may grow before its label truncates. Fixed so
/// that a long environment name cannot push the preview off the row.
const PICKER_MAX_W: gpui::Pixels = px(180.);

impl ApiExplorer {
    /// The row under the URL: the environment picker, then the preview.
    pub(super) fn environment_row(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_2()
            .px_2()
            .pb_2()
            .child(div().flex_shrink_0().child(self.environment_picker(cx)))
            .children(self.resolved_preview(tab, cx))
    }

    /// The picker itself: every environment, the "no environment" state, and
    /// the way into the editor.
    ///
    /// A `Popover` rather than a `Select` for the same reason the method picker
    /// is one: the list ends with an action ("Manage environments…") that is
    /// not a choice, and `Select` has nowhere to put it.
    fn environment_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let environments = self.environments.environments().to_vec();
        let active = self.environments.active_id();
        let label = self
            .environments
            .active()
            .map(|environment| SharedString::from(environment.name.clone()))
            .unwrap_or_else(|| t(Str::NoEnvironment, cx));

        let none_row = Button::new("environment-none")
            .ghost()
            .small()
            .w_full()
            .justify_start()
            .selected(active.is_none())
            .label(t(Str::NoEnvironment, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.environments.set_active(None);
                this.persist_environments(cx);
                this.environment_menu_open = false;
                cx.notify();
            }));

        let rows: Vec<gpui::AnyElement> = environments
            .iter()
            .map(|environment| {
                let id = environment.id;
                Button::new(("environment-option", id as usize))
                    .ghost()
                    .small()
                    .w_full()
                    .justify_start()
                    .selected(active == Some(id))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(SharedString::from(environment.name.clone())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.environments.set_active(Some(id));
                        this.persist_environments(cx);
                        this.environment_menu_open = false;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();

        let empty = rows.is_empty();

        Popover::new("environment-picker")
            .open(self.environment_menu_open)
            .on_open_change(cx.listener(|this, open, _, cx| {
                this.environment_menu_open = *open;
                cx.notify();
            }))
            .trigger(
                Button::new("environment-trigger")
                    .outline()
                    .xsmall()
                    .max_w(PICKER_MAX_W)
                    .tooltip(t(Str::SelectEnvironment, cx))
                    .child(
                        h_flex()
                            .min_w_0()
                            .items_center()
                            .gap_1()
                            .child(
                                Icon::new(AppIcon::Globe)
                                    .size(px(12.))
                                    .flex_shrink_0()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(div().min_w_0().truncate().text_xs().child(label))
                            .child(
                                Icon::new(AppIcon::ChevronDown)
                                    .size(px(10.))
                                    .flex_shrink_0(),
                            ),
                    ),
            )
            .p_1()
            .w(px(240.))
            .child(
                v_flex()
                    .w_full()
                    .gap_0p5()
                    .child(none_row)
                    // The hint replaces the list only when there is no list —
                    // an empty popover would leave "how do I make one?"
                    // unanswered.
                    .when(empty, |this| {
                        this.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t(Str::NoEnvironmentsYetHint, cx)),
                        )
                    })
                    .children(rows)
                    .child(
                        div()
                            .w_full()
                            .pt_1()
                            .mt_1()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("environment-manage")
                                    .ghost()
                                    .small()
                                    .w_full()
                                    .justify_start()
                                    .icon(AppIcon::Sliders)
                                    .label(t(Str::ManageEnvironments, cx))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.environment_menu_open = false;
                                        cx.notify();
                                        this.open_environments_editor(window, cx);
                                    })),
                            ),
                    ),
            )
    }

    /// Opens the editor on the environment currently active, or on the
    /// collection scope when there is none.
    ///
    /// The starting scope's name and variables are read **here** and handed
    /// over: this runs inside a click listener, so the page entity is leased
    /// and the editor cannot read it back during construction.
    pub(super) fn open_environments_editor(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let scope = self.environments.active_id();
        let name = self
            .environments
            .active()
            .map(|environment| environment.name.clone())
            .unwrap_or_default();
        let variables = self.environments.variables(scope).to_vec();
        environments_editor::open(cx.entity(), scope, name, variables, window, cx);
    }

    /// The "Resolves to …" line, drawn only when the URL holds a reference.
    fn resolved_preview(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let url = tab.read(cx).request.url.read(cx).value().to_string();
        if !has_reference(&url) {
            return None;
        }

        let variables = self.environments.variable_set();
        let missing: Vec<String> = resolve_all(&url, &variables)
            .into_iter()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name)
            .collect();

        // The tooltip names where each resolved value came from, which is the
        // second half of "what does this token mean right now".
        let sources: Vec<SharedString> = resolve_all(&url, &variables)
            .into_iter()
            .filter_map(|(name, value)| {
                value.is_some().then(|| {
                    let scope = variables
                        .lookup(&name)
                        .map(|(scope, _)| t(scope.label(), cx).to_string())
                        .unwrap_or_default();
                    t(Str::ResolvesFrom { name, scope }, cx)
                })
            })
            .collect();

        let (text, warn) = match missing.first() {
            // Named rather than counted: one name is enough to fix it, and it
            // is the same sentence the send-time failure uses.
            Some(name) => (t(Str::UnresolvedVariablePreview(name.clone()), cx), true),
            None => (
                interpolate(&url, &variables)
                    .map(SharedString::from)
                    // A recursion failure has no sensible preview text; the
                    // send-time banner explains it properly.
                    .unwrap_or_else(|_| t(Str::NoActiveVariables, cx)),
                false,
            ),
        };

        let colour = if warn {
            cx.theme().warning
        } else {
            cx.theme().muted_foreground
        };

        Some(
            h_flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(colour)
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(Str::ResolvedUrlLabel, cx)),
                )
                .child(
                    div()
                        .id("resolved-url-preview")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(text)
                        .when(!sources.is_empty(), |this| {
                            let sources = SharedString::from(
                                sources
                                    .iter()
                                    .map(SharedString::to_string)
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                            );
                            // A plain element's `tooltip` takes a builder, not
                            // a string — only `Button` has the string-shaped
                            // convenience this file uses elsewhere.
                            this.tooltip(move |window, cx| {
                                Tooltip::new(sources.clone()).build(window, cx)
                            })
                        }),
                )
                .into_any_element(),
        )
    }
}
