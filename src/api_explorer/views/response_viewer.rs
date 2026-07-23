//! The response half: status metadata, response tabs, and the body.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::tag::Tag;
use gpui_component::{
    ActiveTheme as _, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::api_explorer::components::empty_state::empty_state;
use crate::api_explorer::components::later_step::later_step;
use crate::api_explorer::components::status_tag::{metric, status_tag};
use crate::api_explorer::models::exchange::{BodyKind, StatusClass, format_duration, format_size};
use crate::api_explorer::models::json_tree::{RowContent, RowLabel, ScalarKind};
use crate::api_explorer::services::http::cookies::{Cookie, cookies_from_headers};
use crate::api_explorer::state::response::{BodyView, Outcome, ResponseTab};
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn render_response_viewer(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(tab) = self.active_tab().cloned() else {
            return div().size_full().into_any_element();
        };

        let state = tab.read(cx);
        let collapsed = state.response.collapsed;

        v_flex()
            .size_full()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(self.response_meta_row(&tab, cx))
            .when(!collapsed, |this| {
                this.child(self.response_tab_bar(&tab, cx)).child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.response_pane(&tab, cx)),
                )
            })
            .into_any_element()
    }

    /// Status, class caption, timing, size, and the collapse control.
    fn response_meta_row(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let collapsed = state.response.collapsed;
        let tab = tab.clone();

        let summary = match &state.response.outcome {
            Outcome::Idle => h_flex()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t(Str::NoResponseYet, cx))
                .into_any_element(),
            Outcome::InFlight => h_flex()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t(Str::Sending, cx))
                .into_any_element(),
            Outcome::Failed(_) => h_flex()
                .items_center()
                .gap_2()
                .child(
                    Tag::custom(
                        cx.theme().danger.opacity(0.15),
                        cx.theme().danger,
                        cx.theme().danger.opacity(0.4),
                    )
                    .small()
                    .rounded(cx.theme().radius)
                    .child(div().font_bold().child(t(Str::RequestFailed, cx))),
                )
                .into_any_element(),
            // Copied out by value: `exchange` borrows from `state`, which
            // borrows `cx`, and the summary needs `cx` mutably.
            Outcome::Received(exchange) => {
                let (status, class, elapsed, size) = (
                    exchange.status,
                    exchange.status_class(),
                    exchange.elapsed,
                    exchange.size_bytes,
                );
                Self::received_summary(status, class, elapsed, size, cx)
            }
        };

        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .h(px(38.))
            .px_3()
            .gap_2()
            .child(summary)
            .child(
                Button::new("toggle-response")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::PanelBottom)
                    .tooltip(t(
                        if collapsed {
                            Str::ExpandResponse
                        } else {
                            Str::CollapseResponse
                        },
                        cx,
                    ))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        tab.update(cx, |state, cx| {
                            state.response.collapsed = !state.response.collapsed;
                            cx.notify();
                        });
                        cx.notify();
                    })),
            )
    }

    /// The status badge, class caption, duration and size of a real response.
    fn received_summary(
        status: u16,
        class: StatusClass,
        elapsed: Duration,
        size_bytes: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .items_center()
            .gap_3()
            .child(status_tag(status, class, cx))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(class.label(), cx)),
            )
            .child(metric(AppIcon::Clock, format_duration(elapsed).into(), cx))
            .child(metric(AppIcon::HardDrive, format_size(size_bytes).into(), cx))
            .into_any_element()
    }

    fn response_tab_bar(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let active = state.response.active_tab;
        let header_count = state.response.header_count();
        let body_view = state.response.body_view;
        let has_body = state.response.exchange().is_some();
        let kind = state
            .response
            .exchange()
            .map_or(BodyKind::Text, |exchange| exchange.kind);
        let selected = ResponseTab::ALL
            .iter()
            .position(|candidate| *candidate == active)
            .unwrap_or(0);

        let switch_tab = tab.clone();

        // The tab strip gets the leftover width and scrolls its own tabs when
        // there is not enough of it; the body controls are pinned so that a
        // narrow window can never push Copy off the edge.
        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_2()
            .px_2()
            // A divider above the strip separates it from the status row, and
            // the existing one below separates it from the response pane.
            .border_t_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                // `TabBar` sizes itself to its tabs and has no width of its
                // own, so it needs a slot that both bounds it (`flex_1` +
                // `min_w_0`) and clips it. Without `overflow_hidden` it spills
                // out of the slot and paints over the controls beside it.
                div().flex_1().min_w_0().overflow_hidden().child(
                // Denser than the request strip: this row also carries the
                // Pretty/Raw/Copy controls, and both have to fit dodo's
                // 900px-wide default window without either being clipped.
                TabBar::new("response-panes")
                    .xsmall()
                    .selected_index(selected)
                    .children(ResponseTab::ALL.map(|pane| {
                        let tab = Tab::new().label(t(pane.label(), cx));
                        // The count badge the reference shows beside Headers.
                        if pane == ResponseTab::Headers && header_count > 0 {
                            tab.suffix(
                                Tag::secondary()
                                    .small()
                                    .rounded_full()
                                    .child(format!("{header_count}")),
                            )
                        } else {
                            tab
                        }
                    }))
                    .on_click(cx.listener(move |_, index: &usize, _, cx| {
                        let Some(pane) = ResponseTab::ALL.get(*index).copied() else {
                            return;
                        };
                        switch_tab.update(cx, |state, cx| {
                            state.response.active_tab = pane;
                            cx.notify();
                        });
                        cx.notify();
                    })),
                ),
            )
            .when(active == ResponseTab::Body && has_body, |this| {
                this.child(
                    h_flex()
                        .flex_shrink_0()
                        .child(self.body_controls(tab, body_view, kind, cx)),
                )
            })
    }

    /// Pretty / Raw, plus Preview for HTML and Tree for JSON, then Copy and
    /// Save-to-file. The mode buttons that only make sense for a given body kind
    /// are shown only for that kind rather than as dead controls.
    fn body_controls(
        &self,
        tab: &Entity<RequestTabState>,
        body_view: BodyView,
        kind: BodyKind,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let copy_tab = tab.clone();

        h_flex()
            .items_center()
            .gap_1()
            .child(self.body_mode_button(tab, body_view, BodyView::Pretty, Str::BodyPretty, "body-pretty", cx))
            .child(self.body_mode_button(tab, body_view, BodyView::Raw, Str::BodyRaw, "body-raw", cx))
            .when(kind == BodyKind::Html, |this| {
                this.child(self.body_mode_button(
                    tab,
                    body_view,
                    BodyView::Preview,
                    Str::BodyPreview,
                    "body-preview",
                    cx,
                ))
            })
            .when(kind == BodyKind::Json, |this| {
                this.child(self.body_mode_button(
                    tab,
                    body_view,
                    BodyView::Tree,
                    Str::BodyTree,
                    "body-tree",
                    cx,
                ))
            })
            .child(
                Button::new("body-copy")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Copy)
                    .tooltip(t(Str::Copy, cx))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        // Copies the whole body, not just the windowed part —
                        // the window is a rendering limit, not a data limit.
                        let body = copy_tab
                            .read(cx)
                            .response
                            .exchange()
                            .map(|exchange| exchange.body.clone());
                        if let Some(body) = body {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(body));
                        }
                    })),
            )
            .child(
                Button::new("body-save-file")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Save)
                    .tooltip(t(Str::SaveToFile, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.save_body_to_file(cx))),
            )
    }

    /// One body-view mode toggle. Every mode but Tree refreshes the shared
    /// editor; Tree renders its own element, so it only flips the mode.
    fn body_mode_button(
        &self,
        tab: &Entity<RequestTabState>,
        current: BodyView,
        target: BodyView,
        label: Str,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tab = tab.clone();
        Button::new(id)
            .ghost()
            .xsmall()
            .selected(current == target)
            .label(t(label, cx))
            .on_click(cx.listener(move |_, _, window, cx| {
                tab.update(cx, |state, cx| {
                    state.response.body_view = target;
                    if target != BodyView::Tree {
                        state.refresh_body(window, cx);
                    }
                    cx.notify();
                });
                cx.notify();
            }))
    }

    fn response_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);

        // A failure replaces the whole pane: there are no headers or body to
        // show, and the reason is the only useful thing on screen.
        if let Outcome::Failed(error) = &state.response.outcome {
            return self.error_banner(error.clone(), cx).into_any_element();
        }

        if state.response.exchange().is_none() {
            return empty_state(
                AppIcon::Send,
                t(Str::NoResponseYet, cx),
                Some(t(Str::NoResponseHint, cx)),
                cx,
            )
            .into_any_element();
        }

        let pane = state.response.active_tab;
        // Read before the immutable borrow of `cx` is needed mutably below.
        let body_view = state.response.body_view;
        if !pane.is_implemented() {
            return later_step(
                AppIcon::SquareCode,
                t(pane.label(), cx),
                t(Str::ArrivesLater, cx),
                cx,
            )
            .into_any_element();
        }

        match pane {
            ResponseTab::Headers => self.headers_pane(tab, cx).into_any_element(),
            ResponseTab::Cookies => self.cookies_pane(tab, cx).into_any_element(),
            // Body is the default pane. In Tree mode it renders the parsed JSON
            // tree; every other mode renders through the shared editor.
            _ => {
                if body_view == BodyView::Tree {
                    self.json_tree_pane(tab, cx).into_any_element()
                } else {
                    self.body_pane(tab, cx).into_any_element()
                }
            }
        }
    }

    /// The same calm banner the other tools use for errors.
    fn error_banner(&self, error: Str, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_3().child(
            div()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().danger)
                .bg(cx.theme().danger.opacity(0.1))
                .text_color(cx.theme().danger)
                .text_sm()
                .px_3()
                .py_2()
                .child(t(error, cx)),
        )
    }

    fn body_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let body = state.response.body.clone();
        let shown = state.response.visible_lines.min(state.response.total_lines);
        let total = state.response.total_lines;
        let has_more = state.response.has_more_lines();
        let truncated = state
            .response
            .exchange()
            .is_some_and(|exchange| exchange.truncated);
        // The HTML preview is honestly a text rendering, not a real page; the
        // note says so at the top of the pane.
        let preview_note = state.response.body_view == BodyView::Preview
            && state
                .response
                .exchange()
                .is_some_and(|exchange| exchange.kind == BodyKind::Html);
        let more_tab = tab.clone();

        v_flex()
            .size_full()
            .when(preview_note, |this| {
                this.child(
                    div()
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .bg(cx.theme().muted.opacity(0.4))
                        .child(t(Str::HtmlPreviewNote, cx)),
                )
            })
            .child(
                div().flex_1().min_h_0().child(
                    Input::new(&body)
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(cx.theme().mono_font_size)
                        .size_full(),
                ),
            )
            .child(
                // The footer states exactly what is on screen and what is not,
                // so a capped render is never a silent one.
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().child(t(Str::LineRange { shown, total }, cx)))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .when(truncated, |this| {
                                this.child(div().child(t(Str::BodyTruncated, cx)))
                            })
                            .when(has_more, |this| {
                                this.child(
                                    Button::new("load-more-lines")
                                        .ghost()
                                        .xsmall()
                                        .label(t(Str::LoadMoreLines, cx))
                                        .on_click(cx.listener(move |_, _, window, cx| {
                                            more_tab.update(cx, |state, cx| {
                                                state.response.show_more_lines();
                                                state.refresh_body(window, cx);
                                                cx.notify();
                                            });
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
    }

    /// The JSON tree view: an expand/collapse outline of the parsed body.
    ///
    /// The tree is parsed once and cached on the response state; rendering asks
    /// only for the visible rows, so a large document costs a screenful of
    /// elements, not one per node. Deep nodes start collapsed.
    fn json_tree_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let source = tab
            .read(cx)
            .response
            .exchange()
            .map(|exchange| exchange.body.clone())
            .unwrap_or_default();

        // Build the tree lazily and read out its visible rows in one update.
        // A body that is not JSON, or is a bare scalar with no tree worth
        // showing, falls back to the plain text body.
        let visible = tab.update(cx, |state, _| {
            state.response.json_tree(&source).and_then(|tree| {
                tree.is_expandable().then(|| tree.visible_rows())
            })
        });

        let Some(visible) = visible else {
            return self.body_pane(tab, cx).into_any_element();
        };

        let truncated = visible.truncated;

        // Built eagerly so the row elements do not keep `cx` borrowed as a lazy
        // iterator would.
        let mut row_elements: Vec<gpui::AnyElement> = Vec::new();
        for (index, row) in visible.rows.into_iter().enumerate() {
            row_elements.push(
                self.json_tree_row(tab, &source, index, row, cx)
                    .into_any_element(),
            );
        }

        v_flex()
            .size_full()
            .child(
                div()
                    .id("json-tree")
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .p_1()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size)
                    .children(row_elements),
            )
            .when(truncated, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .py_1()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(
                            Str::JsonTreeTruncated(
                                crate::api_explorer::models::json_tree::ROW_BUDGET,
                            ),
                            cx,
                        )),
                )
            })
            .into_any_element()
    }

    /// One line of the JSON tree: a disclosure control for containers, the key,
    /// and the value (or a bracket-and-count for a collapsed container).
    fn json_tree_row(
        &self,
        tab: &Entity<RequestTabState>,
        source: &str,
        index: usize,
        row: crate::api_explorer::models::json_tree::JsonRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tab = tab.clone();
        let indent = px(4. + row.depth as f32 * 14.);

        let key_element = match &row.label {
            RowLabel::Root => div().into_any_element(),
            RowLabel::Index(i) => div()
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{i}:"))
                .into_any_element(),
            RowLabel::Field(name) => div()
                .flex_shrink_0()
                .text_color(cx.theme().info)
                .child(format!("{name}:"))
                .into_any_element(),
        };

        let (disclosure, value_element) = match row.content {
            RowContent::Scalar { text, kind } => (
                div().w(px(16.)).flex_shrink_0().into_any_element(),
                div()
                    .min_w_0()
                    .text_color(scalar_color(kind, cx))
                    .child(scalar_text(&text, kind))
                    .into_any_element(),
            ),
            RowContent::Container {
                open,
                close,
                count,
                expanded,
            } => {
                let path = row.path.clone();
                let source = source.to_string();
                let toggle = Button::new(("json-toggle", index))
                    .ghost()
                    .xsmall()
                    .icon(if expanded {
                        AppIcon::ChevronDown
                    } else {
                        AppIcon::ChevronRight
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let source = source.clone();
                        let path = path.clone();
                        tab.update(cx, |state, cx| {
                            if let Some(tree) = state.response.json_tree(&source) {
                                tree.toggle(&path);
                                cx.notify();
                            }
                        });
                        cx.notify();
                    }))
                    .into_any_element();
                let summary = if expanded {
                    format!("{open}")
                } else {
                    format!("{open} … {close} {count}")
                };
                (
                    toggle,
                    div()
                        .min_w_0()
                        .text_color(cx.theme().muted_foreground)
                        .child(summary)
                        .into_any_element(),
                )
            }
        };

        h_flex()
            .w_full()
            .items_center()
            .gap_1()
            .pl(indent)
            .pr_2()
            .py(px(1.))
            .child(disclosure)
            .child(key_element)
            .child(value_element)
    }

    /// The Cookies pane: the `Set-Cookie` headers parsed into name, value and
    /// attributes.
    fn cookies_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let cookies = tab
            .read(cx)
            .response
            .exchange()
            .map(|exchange| cookies_from_headers(&exchange.headers))
            .unwrap_or_default();

        if cookies.is_empty() {
            return empty_state(
                AppIcon::File,
                t(Str::NoCookies, cx),
                Some(t(Str::NoCookiesHint, cx)),
                cx,
            )
            .into_any_element();
        }

        let rows: Vec<_> = cookies
            .into_iter()
            .enumerate()
            .map(|(index, cookie)| cookie_row(index, cookie, cx))
            .collect();

        div()
            .id("response-cookies")
            .size_full()
            .overflow_y_scroll()
            .child(v_flex().w_full().children(rows))
            .into_any_element()
    }

    fn headers_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let headers = state
            .response
            .exchange()
            .map(|exchange| exchange.headers.clone())
            .unwrap_or_default();

        div()
            .id("response-headers")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .children(headers.into_iter().enumerate().map(|(index, (name, value))| {
                        h_flex()
                            .w_full()
                            .items_start()
                            .gap_3()
                            .px_3()
                            .py_1p5()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.5))
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .when(index % 2 == 1, |this| this.bg(cx.theme().list_even))
                            .child(
                                div()
                                    .w(px(220.))
                                    .flex_shrink_0()
                                    .font_bold()
                                    .child(name),
                            )
                            .child(div().flex_1().min_w_0().child(value))
                    })),
            )
    }
}

/// The theme colour a scalar is drawn in, by type, matching the code editor's
/// own JSON highlighting closely enough to read as the same document.
fn scalar_color(kind: ScalarKind, cx: &Context<ApiExplorer>) -> gpui::Hsla {
    match kind {
        ScalarKind::String => cx.theme().success,
        ScalarKind::Number => cx.theme().info,
        ScalarKind::Bool => cx.theme().warning,
        ScalarKind::Null => cx.theme().muted_foreground,
    }
}

/// A scalar as it reads in the tree: strings quoted, everything else bare.
fn scalar_text(text: &str, kind: ScalarKind) -> String {
    match kind {
        ScalarKind::String => format!("\"{text}\""),
        _ => text.to_string(),
    }
}

/// One cookie: its `name = value`, then its attributes as small tags.
fn cookie_row(index: usize, cookie: Cookie, cx: &Context<ApiExplorer>) -> impl IntoElement {
    let attributes = cookie.attributes;
    v_flex()
        .w_full()
        .gap_1()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border.opacity(0.5))
        .when(index % 2 == 1, |this| this.bg(cx.theme().list_even))
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_1()
                .text_xs()
                .font_family(cx.theme().mono_font_family.clone())
                .child(div().flex_shrink_0().font_bold().child(cookie.name.clone()))
                .child(div().flex_shrink_0().text_color(cx.theme().muted_foreground).child("="))
                .child(div().flex_1().min_w_0().overflow_hidden().child(cookie.value.clone())),
        )
        .when(!attributes.is_empty(), |this| {
            this.child(
                h_flex().w_full().flex_wrap().gap_1().children(
                    attributes.into_iter().map(|attribute| {
                        let text = match attribute.value {
                            Some(value) => format!("{}={value}", attribute.name),
                            None => attribute.name,
                        };
                        Tag::secondary().small().child(text)
                    }),
                ),
            )
        })
}
