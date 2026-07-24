use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::highlighter::{Diagnostic, DiagnosticSeverity};
use gpui_component::input::{Input, InputState, Position};
use gpui_component::select::{Select, SelectState};
use gpui_component::{ActiveTheme, IndexPath, Sizable, h_flex, v_flex};

use serde::Serialize as _;

use crate::i18n::{Language, Str, t};

/// The indentation width options offered in the dropdown, in spaces.
const INDENT_OPTIONS: [usize; 3] = [2, 3, 4];

/// The JSON formatter view: a code editor plus a "Format" button and an
/// indent-width dropdown. Formatting pretty-prints the editor contents in
/// place; invalid JSON surfaces the parser error both as a message and as an
/// inline diagnostic (wavy underline) at the offending location.
///
/// The error is kept as a [`Str`] rather than a rendered string so that it is
/// re-translated when the language changes while it is on screen.
pub struct JsonFormatter {
    input: Entity<InputState>,
    indent: Entity<SelectState<Vec<SharedString>>>,
    error: Option<Str>,
    /// The language the editor placeholder and dropdown labels were built for.
    /// Those live inside library state rather than being rebuilt every frame,
    /// so [`Self::sync_language`] pushes new text into them when this goes stale.
    language: Language,
}

impl JsonFormatter {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let language = Language::current(cx);
        let placeholder = t(Str::JsonPlaceholder, cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .multi_line(true)
                .line_number(true)
                .placeholder(placeholder)
        });

        let options = indent_options(cx);
        let indent = cx.new(|cx| SelectState::new(options, Some(IndexPath::default()), window, cx));

        Self {
            input,
            indent,
            error: None,
            language,
        }
    }

    /// Re-pushes the localized strings that library widgets hold internally.
    /// Cheap and idempotent: it does nothing unless the language changed.
    fn sync_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = Language::current(cx);
        if language == self.language {
            return;
        }
        self.language = language;

        let placeholder = t(Str::JsonPlaceholder, cx);
        self.input.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx);
        });

        let options = indent_options(cx);
        self.indent.update(cx, |state, cx| {
            let selected = state.selected_index(cx);
            state.set_items(options, window, cx);
            // `set_items` swaps the item list but leaves the trigger showing the
            // old item; re-selecting refreshes it from the new list.
            state.set_selected_index(selected, window, cx);
            cx.notify();
        });
    }

    /// The currently selected indent width in spaces (defaults to 2).
    fn indent_width(&self, cx: &App) -> usize {
        self.indent
            .read(cx)
            .selected_index(cx)
            .and_then(|ip| INDENT_OPTIONS.get(ip.row).copied())
            .unwrap_or(INDENT_OPTIONS[0])
    }

    fn pretty_print(value: &serde_json::Value, width: usize) -> String {
        let indent = " ".repeat(width);
        let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
        let mut buf = Vec::new();
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        // Serializing a `serde_json::Value` into a String is infallible.
        value
            .serialize(&mut ser)
            .expect("serializing serde_json::Value cannot fail");
        String::from_utf8(buf).expect("serde_json emits valid UTF-8")
    }

    fn format(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.input.read(cx).value().to_string();
        let width = self.indent_width(cx);

        match serde_json::from_str::<serde_json::Value>(&source) {
            Ok(value) => {
                let formatted = Self::pretty_print(&value, width);
                self.error = None;
                self.input.update(cx, |state, cx| {
                    state.set_value(formatted, window, cx);
                    if let Some(diagnostics) = state.diagnostics_mut() {
                        diagnostics.clear();
                    }
                    cx.notify();
                });
            }
            Err(err) => {
                // serde_json reports 1-based line/column; diagnostics use 0-based.
                let line = (err.line().max(1) - 1) as u32;
                let column = (err.column().max(1) - 1) as u32;
                // `err`'s own wording is serde_json's, in English; the frame
                // around it is ours and is translated.
                let error = Str::InvalidJson {
                    line: err.line(),
                    column: err.column(),
                    detail: err.to_string(),
                };
                let message = t(error.clone(), cx);
                self.error = Some(error);

                self.input.update(cx, |state, cx| {
                    let text = state.text().clone();
                    if let Some(diagnostics) = state.diagnostics_mut() {
                        diagnostics.reset(&text);
                        let start = Position::new(line, column);
                        let end = Position::new(line, column.saturating_add(1));
                        diagnostics.push(
                            Diagnostic::new(start..end, message.clone())
                                .with_severity(DiagnosticSeverity::Error),
                        );
                    }
                    cx.notify();
                });
            }
        }

        cx.notify();
    }
}

impl Render for JsonFormatter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_language(window, cx);

        v_flex()
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Button::new("format-json")
                            .primary()
                            .label(t(Str::FormatButton, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.format(window, cx);
                            })),
                    )
                    .child(div().text_sm().child(t(Str::IndentLabel, cx)))
                    .child(Select::new(&self.indent).small().w(px(120.))),
            )
            .when_some(
                self.error.clone().map(|error| t(error, cx)),
                |this, error| {
                    this.child(
                        div()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().danger)
                            .bg(cx.theme().danger.opacity(0.1))
                            .text_color(cx.theme().danger)
                            .text_sm()
                            .px_3()
                            .py_2()
                            .child(error),
                    )
                },
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        Input::new(&self.input)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .size_full(),
                    ),
            )
    }
}

/// The indent-width dropdown labels in the active language, in `INDENT_OPTIONS`
/// order so a row index still maps to a width.
fn indent_options(cx: &App) -> Vec<SharedString> {
    INDENT_OPTIONS
        .iter()
        .map(|n| t(Str::IndentSpaces(*n), cx))
        .collect()
}
