use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::highlighter::{Diagnostic, DiagnosticSeverity};
use gpui_component::input::{Input, InputState, Position};
use gpui_component::select::{Select, SelectState};
use gpui_component::{ActiveTheme, IndexPath, Sizable, h_flex, v_flex};

use serde::Serialize as _;

/// The indentation width options offered in the dropdown, in spaces.
const INDENT_OPTIONS: [usize; 3] = [2, 3, 4];

/// The JSON formatter view: a code editor plus a "Format" button and an
/// indent-width dropdown. Formatting pretty-prints the editor contents in
/// place; invalid JSON surfaces the parser error both as a message and as an
/// inline diagnostic (wavy underline) at the offending location.
pub struct JsonFormatter {
    input: Entity<InputState>,
    indent: Entity<SelectState<Vec<SharedString>>>,
    error: Option<SharedString>,
}

impl JsonFormatter {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .multi_line(true)
                .line_number(true)
                .placeholder("Paste JSON here, then click Format.")
        });

        let options: Vec<SharedString> = INDENT_OPTIONS
            .iter()
            .map(|n| SharedString::from(format!("{n} spaces")))
            .collect();
        let indent = cx.new(|cx| {
            SelectState::new(options, Some(IndexPath::default()), window, cx)
        });

        Self {
            input,
            indent,
            error: None,
        }
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
                let message = SharedString::from(format!(
                    "Invalid JSON at line {}, column {}: {}",
                    err.line(),
                    err.column(),
                    err
                ));
                self.error = Some(message.clone());

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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .label("Format")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.format(window, cx);
                            })),
                    )
                    .child(div().text_sm().child("Indent:"))
                    .child(Select::new(&self.indent).small().w(px(120.))),
            )
            .when_some(self.error.clone(), |this, error| {
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
            })
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
