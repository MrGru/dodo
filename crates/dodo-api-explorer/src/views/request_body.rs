//! The Body tab: a type picker, and the editor that type is edited with.
//!
//! Four editing surfaces sit under one picker — the code editor for the
//! text-shaped types, the key/value table for the two form types, a single-file
//! picker for Binary, and a stated "no body" panel for `None`. Which one is
//! shown is the only thing the type decides here; how it is encoded and what
//! `Content-Type` it implies is the service layer's business
//! (`services::http::request_body`), so this file has no opinion about the wire.
//!
//! Nothing here reads a file. The picker hands back a path and a size through
//! `services::file_picker`, which does its `stat` on the background executor;
//! the bytes are read once, at send time, in `services::http::upload`.

use gpui::prelude::FluentBuilder as _;
use gpui::{Context, Entity, IntoElement, ParentElement as _, SharedString, Styled as _, div, px};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::components::empty_state::empty_state;
use crate::components::key_value_table::key_value_table;
use crate::i18n::{api_explorer, api_scripts, shared, t};
use crate::models::body::BodyType;
use crate::services::file_picker;
use crate::services::http::upload;
use crate::state::request::RowTable;
use crate::state::tab::RequestTabState;
use crate::views::explorer::ApiExplorer;

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
                            .child(t(
                                api_explorer::Text::MethodSendsNoBody(method.as_str().to_string()),
                                cx,
                            )),
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
                                .label(t(shared::Text::FormatButton, cx))
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
                                .tooltip(t(api_scripts::Text::Copy, cx))
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
    /// selectable at any width — nothing is pushed off-screen. Every kind is
    /// now buildable, so nothing here is disabled.
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
            let switch_tab = tab.clone();
            Button::new(("body-type", candidate as usize))
                .ghost()
                .xsmall()
                .selected(candidate == current)
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
            return key_value_table(RowTable::BodyFields, body_type.is_typed_form(), tab, cx)
                .into_any_element();
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

        if body_type.is_file() {
            return self.binary_pane(tab, cx);
        }

        empty_state(
            AppIcon::SquareCode,
            t(api_explorer::Text::NoBodyTitle, cx),
            Some(t(api_explorer::Text::NoBodyHint, cx)),
            cx,
        )
        .into_any_element()
    }

    /// The Binary body: one file, sent as the raw request body.
    ///
    /// Nothing is read here — only the name and the size the picker already
    /// learned are shown, and the `Content-Type` line states what the extension
    /// implies so the request is not a surprise. The bytes are read once, at
    /// send time, on the background executor.
    fn binary_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let state = tab.read(cx);
        let path = state.request.binary_path.clone();
        let size = state.request.binary_size.map(file_picker::format_size);
        let chosen = !path.trim().is_empty();
        let file = file_picker::ChosenFile {
            path: std::path::PathBuf::from(&path),
            size: state.request.binary_size,
        };
        let name = file.display_name();
        let media_type = upload::media_type_of(&file.path);

        let pick_tab = tab.clone();
        let clear_tab = tab.clone();

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .p_4()
            .child(
                Icon::new(AppIcon::Binary)
                    .size(px(28.))
                    .text_color(cx.theme().muted_foreground),
            )
            .when(chosen, |this| {
                this.child(
                    v_flex()
                        .max_w_full()
                        .min_w_0()
                        .items_center()
                        .gap_1()
                        // The name identifies the file; the full path sits
                        // under it, truncated, because this pane has the room
                        // that a table cell does not.
                        .child(div().font_bold().child(SharedString::from(name)))
                        .child(
                            div()
                                .max_w_full()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(SharedString::from(path.clone())),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .when_some(size, |this, size| this.child(SharedString::from(size)))
                                .child(SharedString::from(media_type)),
                        ),
                )
            })
            .when(!chosen, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(api_explorer::Text::BinaryBodyHint, cx)),
                )
            })
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("binary-choose-file")
                            .outline()
                            .small()
                            .icon(AppIcon::File)
                            .label(if chosen {
                                t(api_explorer::Text::ReplaceFile, cx)
                            } else {
                                t(api_explorer::Text::ChooseFile, cx)
                            })
                            .on_click(cx.listener(move |_, _, _, cx| {
                                file_picker::choose_file(
                                    pick_tab.clone(),
                                    cx,
                                    |state, chosen, cx| {
                                        state.request.binary_path =
                                            chosen.path.display().to_string();
                                        state.request.binary_size = chosen.size;
                                        state.request.dirty = true;
                                        cx.notify();
                                    },
                                );
                            })),
                    )
                    .when(chosen, |this| {
                        this.child(
                            Button::new("binary-clear-file")
                                .ghost()
                                .small()
                                .icon(AppIcon::Close)
                                .tooltip(t(api_explorer::Text::ClearFile, cx))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    clear_tab.update(cx, |state, cx| {
                                        state.request.binary_path.clear();
                                        state.request.binary_size = None;
                                        state.request.dirty = true;
                                        cx.notify();
                                    });
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}
