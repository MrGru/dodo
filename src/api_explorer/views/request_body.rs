//! The Body tab: a type picker, and the editor that type is edited with.
//!
//! Three editing surfaces sit under one picker — the code editor for the
//! text-shaped types, the key/value table for the two form types, and a stated
//! "no body" panel for the rest. Which one is shown is the only thing the type
//! decides here; how it is encoded and what `Content-Type` it implies is the
//! service layer's business (`services::http::request_body`), so this file has
//! no opinion about the wire.

use gpui::prelude::FluentBuilder as _;
use gpui::{Context, Entity, IntoElement, ParentElement as _, Styled as _, div};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, h_flex, v_flex,
};

use crate::api_explorer::components::empty_state::empty_state;
use crate::api_explorer::components::key_value_table::key_value_table;
use crate::api_explorer::components::later_step::later_step;
use crate::api_explorer::models::body::BodyType;
use crate::api_explorer::state::request::RowTable;
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn request_body_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let state = tab.read(cx);
        let body_type = state.request.body_type;
        let method = state.request.method;

        v_flex()
            .size_full()
            .child(self.body_toolbar(tab, body_type, cx))
            // A method with no body semantics still shows the editor — the
            // document is kept, and switching to POST sends it — but says
            // plainly that this request will not carry it.
            .when(
                !method.carries_body() && body_type != BodyType::None,
                |this| {
                    this.child(
                        div()
                            .w_full()
                            .px_3()
                            .py_1p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .bg(cx.theme().muted.opacity(0.4))
                            .child(t(Str::MethodSendsNoBody(method.as_str().to_string()), cx)),
                    )
                },
            )
            .child(div().flex_1().min_h_0().child(self.body_editor(tab, cx)))
            .into_any_element()
    }

    /// The type selector on the left; format and copy on the right.
    fn body_toolbar(
        &self,
        tab: &Entity<RequestTabState>,
        body_type: BodyType,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let format_tab = tab.clone();
        let copy_tab = tab.clone();

        h_flex()
            // `items_start`: the selector wraps to a second line at a narrow
            // window, and the format/copy controls stay aligned to its top row.
            .w_full()
            .min_w_0()
            .items_start()
            .gap_2()
            .px_2()
            .py_1p5()
            .child(
                // The selector takes the leftover width and wraps rather than
                // scrolling, so no type is ever hidden; the format/copy controls
                // live in their own pinned slot beside it, never overlapping.
                div()
                    .flex_1()
                    .min_w_0()
                    .child(self.body_type_selector(tab, body_type, cx)),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .when(body_type.is_formattable(), |this| {
                        this.child(
                            Button::new("format-body")
                                .ghost()
                                .xsmall()
                                .label(t(Str::FormatButton, cx))
                                .on_click(cx.listener(move |_, _, window, cx| {
                                    format_tab.update(cx, |state, cx| {
                                        state.format_body(window, cx);
                                    });
                                    cx.notify();
                                })),
                        )
                    })
                    .when(body_type.is_text(), |this| {
                        this.child(
                            Button::new("copy-body")
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Copy)
                                .tooltip(t(Str::Copy, cx))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    // Read here rather than each frame: pulling
                                    // the whole document out of the rope is
                                    // linear in its length.
                                    let text = copy_tab
                                        .read(cx)
                                        .request
                                        .body_editor
                                        .read(cx)
                                        .value()
                                        .to_string();
                                    if !text.is_empty() {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            text,
                                        ));
                                    }
                                })),
                        )
                    }),
            )
    }

    /// The body-type selector: a wrapping segmented control, one button per kind.
    ///
    /// It wraps to as many lines as it needs so every kind stays visible and
    /// selectable at any width — nothing is pushed off-screen. Binary is shown
    /// disabled with the reason attached, the honest placeholder for a kind this
    /// build cannot build yet.
    fn body_type_selector(
        &self,
        tab: &Entity<RequestTabState>,
        current: BodyType,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Built inline rather than through a helper: an edition-2024 `impl
        // IntoElement` return would capture `cx`, which cannot escape the map
        // closure.
        let buttons = BodyType::ALL.map(|candidate| {
            let available = candidate.is_available();
            let switch_tab = tab.clone();
            Button::new(("body-type", candidate as usize))
                .ghost()
                .xsmall()
                .selected(candidate == current)
                // Binary is shown disabled with the reason attached rather than
                // hidden, the honest placeholder for a kind this build cannot
                // build yet.
                .disabled(!available)
                .when(!available, |this| this.tooltip(t(Str::BinaryBodyLater, cx)))
                .label(t(candidate.label(), cx))
                .on_click(cx.listener(move |_, _, _, cx| {
                    switch_tab.update(cx, |state, cx| {
                        state.request.body_type = candidate;
                        state.request.apply_body_language(cx);
                        state.request.dirty = true;
                        cx.notify();
                    });
                    cx.notify();
                }))
        });

        h_flex().w_full().flex_wrap().gap_1().children(buttons)
    }

    fn body_editor(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let body_type = state.request.body_type;

        if body_type.is_form() {
            return key_value_table(RowTable::BodyFields, tab, cx).into_any_element();
        }

        if body_type.is_text() {
            let editor = state.request.body_editor.clone();
            return div()
                .size_full()
                .child(
                    Input::new(&editor)
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(cx.theme().mono_font_size)
                        .size_full(),
                )
                .into_any_element();
        }

        match body_type {
            // Not reachable through the picker, which shows Binary disabled —
            // stated here too so the pane can never be blank.
            BodyType::Binary => later_step(
                AppIcon::Binary,
                t(Str::BodyTypeBinary, cx),
                t(Str::BinaryBodyLater, cx),
                cx,
            )
            .into_any_element(),
            _ => empty_state(
                AppIcon::SquareCode,
                t(Str::NoBodyTitle, cx),
                Some(t(Str::NoBodyHint, cx)),
                cx,
            )
            .into_any_element(),
        }
    }
}
