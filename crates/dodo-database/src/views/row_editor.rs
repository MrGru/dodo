//! The smallest input surface shared by cell edit, Add Row, and Duplicate Row.
//!
//! It is an entity because it owns `InputState`s and lives in a real
//! `window.open_dialog`. Every field has an explicit NULL toggle, so an empty
//! string is never guessed to mean NULL.

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _,
    WindowExt as _, h_flex, v_flex,
};

use crate::components::notice::{Tone, notice};
use crate::i18n::{Str, db_connection, db_query, t};
use crate::models::value::{ColumnMeta, Row, Value};
use crate::views::database::DatabaseView;

const WIDTH: gpui::Pixels = px(640.);
const PADDING: gpui::Pixels = px(32.);

#[derive(Clone, Copy)]
pub enum Action {
    EditCell { row: usize, column: usize },
    Insert,
}

pub struct Draft {
    pub title: Str,
    pub columns: Vec<ColumnMeta>,
    pub values: Row,
    pub included: Vec<usize>,
    pub required_identity: Vec<(usize, String)>,
    pub action: Action,
}

struct Field {
    column: usize,
    name: String,
    type_name: String,
    original: Value,
    input: Entity<InputState>,
    is_null: bool,
    fixed: bool,
}

pub fn open(page: Entity<DatabaseView>, draft: Draft, window: &mut Window, cx: &mut App) {
    let title = draft.title.clone();
    let editor = cx.new(|cx| RowEditor::new(page, draft, window, cx));
    let body = editor.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        dialog.title(t(title.clone(), cx)).w(WIDTH).content({
            let body = body.clone();
            move |content, _, _| content.child(div().w(WIDTH - PADDING).child(body.clone()))
        })
    });
}

struct RowEditor {
    page: Entity<DatabaseView>,
    fields: Vec<Field>,
    required_identity: Vec<(usize, String)>,
    action: Action,
    error: Option<Str>,
}

impl RowEditor {
    fn new(
        page: Entity<DatabaseView>,
        draft: Draft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Draft {
            columns,
            values,
            included,
            required_identity,
            action,
            ..
        } = draft;
        let fields = included
            .into_iter()
            .filter_map(|column| {
                let meta = columns.get(column)?;
                let original = values.get(column)?.clone();
                let fixed = matches!(original, Value::Bytes(_) | Value::Truncated { .. });
                let text = original.display();
                Some(Field {
                    column,
                    name: meta.name.clone(),
                    type_name: meta.type_name.clone(),
                    is_null: matches!(original, Value::Null),
                    original,
                    fixed,
                    input: cx.new(|cx| InputState::new(window, cx).default_value(text)),
                })
            })
            .collect();
        Self {
            page,
            fields,
            required_identity,
            action,
            error: None,
        }
    }

    fn toggle_null(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(field) = self.fields.get_mut(index)
            && !field.fixed
        {
            field.is_null = !field.is_null;
            self.error = None;
            cx.notify();
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let values = self
            .fields
            .iter()
            .map(|field| {
                let value = if field.is_null {
                    Value::Null
                } else if field.fixed {
                    field.original.clone()
                } else {
                    edited_value(&field.original, field.input.read(cx).value().as_ref())
                };
                (field.column, value)
            })
            .collect::<Vec<_>>();

        let missing = self
            .required_identity
            .iter()
            .filter(|(required, _)| {
                values
                    .iter()
                    .any(|(column, value)| column == required && matches!(value, Value::Null))
            })
            .map(|(_, name)| name.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            self.error = Some(db_query::Text::IdentityRequired(missing.join(", ")).into());
            cx.notify();
            return;
        }

        let page = self.page.clone();
        let action = self.action;
        window.close_dialog(cx);
        page.update(cx, |page, cx| match action {
            Action::EditCell { row, column } => {
                if let Some((_, value)) = values.into_iter().find(|(index, _)| *index == column) {
                    page.apply_cell_edit(row, column, value, cx);
                }
            }
            Action::Insert => page.apply_insert(values, cx),
        });
    }
}

impl Render for RowEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .max_h(px(520.))
            .gap_3()
            .children((!self.required_identity.is_empty()).then(|| {
                notice(
                    Tone::Info,
                    t(
                        db_query::Text::IdentityRequired(
                            self.required_identity
                                .iter()
                                .map(|(_, name)| name.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ),
                        cx,
                    ),
                    cx,
                )
            }))
            .child(
                v_flex()
                    .id("db-row-fields")
                    .w_full()
                    .max_h(px(410.))
                    .overflow_y_scroll()
                    .gap_2()
                    .children(self.fields.iter().enumerate().map(|(index, field)| {
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(
                                h_flex()
                                    .w_full()
                                    .justify_between()
                                    .child(
                                        div()
                                            .font_semibold()
                                            .child(SharedString::from(field.name.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(SharedString::from(field.type_name.clone())),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            Input::new(&field.input)
                                                .small()
                                                .disabled(field.fixed || field.is_null),
                                        ),
                                    )
                                    .child(
                                        Button::new(("db-row-null", index))
                                            .ghost()
                                            .small()
                                            .selected(field.is_null)
                                            .disabled(field.fixed)
                                            .label(t(db_query::Text::SetNull, cx))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.toggle_null(index, cx)
                                            })),
                                    ),
                            )
                    })),
            )
            .children(
                self.error
                    .clone()
                    .map(|error| notice(Tone::Warning, t(error, cx), cx)),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("db-row-cancel")
                            .ghost()
                            .small()
                            .label(t(db_connection::Text::Cancel, cx))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("db-row-save")
                            .primary()
                            .small()
                            .label(t(db_connection::Text::Save, cx))
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}

fn edited_value(original: &Value, text: &str) -> Value {
    match original {
        Value::Bool(_) => text
            .parse::<bool>()
            .map(Value::Bool)
            .unwrap_or_else(|_| Value::Text(text.into())),
        Value::Int(_) => text
            .parse::<i64>()
            .map(Value::Int)
            .unwrap_or_else(|_| Value::Text(text.into())),
        Value::Float(_) => text
            .parse::<f64>()
            .map(Value::Float)
            .unwrap_or_else(|_| Value::Text(text.into())),
        Value::Json(_) => Value::Json(text.into()),
        Value::Null | Value::Text(_) => Value::Text(text.into()),
        Value::Bytes(_) | Value::Truncated { .. } => original.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::edited_value;
    use crate::models::value::Value;

    #[test]
    fn editing_preserves_scalar_types_and_empty_text_is_not_null() {
        assert_eq!(edited_value(&Value::Int(1), "2"), Value::Int(2));
        assert_eq!(
            edited_value(&Value::Text("x".into()), ""),
            Value::Text(String::new())
        );
        assert_eq!(edited_value(&Value::Null, ""), Value::Text(String::new()));
    }
}
