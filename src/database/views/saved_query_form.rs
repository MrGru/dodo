//! The one small dialog used to create and edit a saved query.

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};

use crate::database::components::notice::{Tone, notice};
use crate::database::models::library::SavedQuery;
use crate::database::views::database::DatabaseView;
use crate::i18n::{Str, db_connection, db_query, t};

const WIDTH: gpui::Pixels = px(680.);
const HEIGHT: gpui::Pixels = px(440.);
const PADDING: gpui::Pixels = px(32.);

pub fn open(
    page: Entity<DatabaseView>,
    draft: SavedQuery,
    editing: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let title = if editing {
        db_query::Text::SavedQueryEditTitle
    } else {
        db_query::Text::SavedQueryCreateTitle
    };
    let editor = cx.new(|cx| SavedQueryForm::new(page, draft, window, cx));
    let body = editor.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        dialog.title(t(title.clone(), cx)).w(WIDTH).content({
            let body = body.clone();
            move |content, _, _| {
                content.child(div().w(WIDTH - PADDING).h(HEIGHT).child(body.clone()))
            }
        })
    });
}

struct SavedQueryForm {
    page: Entity<DatabaseView>,
    draft: SavedQuery,
    name: Entity<InputState>,
    statement: Entity<InputState>,
    error: Option<Str>,
}

impl SavedQueryForm {
    fn new(
        page: Entity<DatabaseView>,
        draft: SavedQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t(db_query::Text::SavedQueryNamePlaceholder, cx))
                .default_value(draft.name.clone())
        });
        let statement = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor(draft.scope.engine.editor_language())
                .multi_line(true)
                .line_number(true)
                .soft_wrap(false)
                .default_value(draft.statement.clone())
        });
        name.update(cx, |state, cx| state.focus(window, cx));
        Self {
            page,
            draft,
            name,
            statement,
            error: None,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().to_string();
        let statement = self.statement.read(cx).value().to_string();
        if name.trim().is_empty() {
            self.error = Some(db_query::Text::SavedQueryNameRequired.into());
            cx.notify();
            return;
        }
        if statement.trim().is_empty() {
            self.error = Some(db_query::Text::SavedQueryStatementRequired.into());
            cx.notify();
            return;
        }

        let mut query = self.draft.clone();
        query.name = name;
        query.statement = statement;
        let saved = self
            .page
            .update(cx, |page, cx| page.save_saved_query(query, cx));
        if saved {
            window.close_dialog(cx);
        }
    }
}

impl Render for SavedQueryForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_3()
            .child(notice(
                Tone::Warning,
                t(db_query::Text::SavedQueryPlaintextNotice, cx),
                cx,
            ))
            .child(
                v_flex()
                    .gap_1()
                    .child(t(db_query::Text::SavedQueryScope, cx))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} · {}",
                                self.draft.scope.connection_name, self.draft.scope.target
                            )),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(t(db_query::Text::SavedQueryName, cx))
                    .child(Input::new(&self.name).small()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_1()
                    .child(t(db_query::Text::SavedQueryStatement, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(
                                Input::new(&self.statement)
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_size(cx.theme().mono_font_size)
                                    .size_full(),
                            ),
                    ),
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
                        Button::new("db-saved-query-cancel")
                            .ghost()
                            .small()
                            .label(t(db_connection::Text::Cancel, cx))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("db-saved-query-save")
                            .primary()
                            .small()
                            .label(t(db_connection::Text::Save, cx))
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}
