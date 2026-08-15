//! The environments editor: a modal dialog over the whole window.
//!
//! # Why a `window.open_dialog` and not a panel
//!
//! The same reason `docker::views::detail` gives, and the same precedent
//! (`settings::open`): editing environments is a task you finish and leave, and
//! a hand-rolled scrim cannot actually block the page behind it. `Dialog`
//! brings the occluding backdrop, Escape, the focus trap and focus restoration
//! for free. Two consequences of following it, both load-bearing here:
//!
//! - **The body is an entity.** `Root::render_dialog_layer` builds the dialog
//!   from its own closure, so the *page's* `cx.notify()` does not repaint it.
//!   [`EnvironmentsEditor`] is the entity, and its own `cx.notify()` is what
//!   redraws a row.
//! - **The body's width is stated**, not `w_full`: a percentage width resolves
//!   to `auto` inside the dialog's wrappers and content-sizes the card.
//!
//! # Where the truth lives while this is open
//!
//! The rows own live `InputState`s, so *they* are authoritative while the
//! dialog is up. Every edit is pushed straight back into the page's
//! [`EnvironmentState`] ([`EnvironmentsEditor::commit`]) so that pressing Send
//! behind the dialog resolves against what is on screen — but the **disk** is
//! only written on a structural change and on blur, not on every keystroke.
//! Persisting per character would spawn one background write per key with
//! nothing to show for it.
//!
//! # The secret notice is not optional
//!
//! `decision-secret-variable-storage` requires the editor to say, where the
//! user can see it, that secret values are stored unencrypted on this machine.
//! That is [`api_variables::Text::SecretStorageWarning`], drawn in the warning colour above the
//! table rather than tucked into a tooltip. Masking is display only — the value
//! goes to disk in plain text like every other one.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
    v_flex,
};

use crate::api_explorer::models::variables::Variable;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Language, api_variables, t};

/// The card's preferred width and the body's preferred height, shrunk to fit a
/// small window by [`card_size`] before the dialog is built — `Dialog` computes
/// its `left` from the width it is given, so an over-wide card is pushed
/// off-centre rather than clipped.
const PANEL_W: Pixels = px(720.);
const PANEL_H: Pixels = px(440.);
/// Margin left around the card at a window too small for the preferred size.
const PANEL_MARGIN: Pixels = px(24.);
/// `Dialog`'s own left and right padding (`Edges::all(16)`), subtracted to get
/// the body's width from the card's.
const DIALOG_PADDING_X: Pixels = px(32.);
/// The scope list down the left: its preferred width, and the width below
/// which it will not shrink.
///
/// It gives way rather than holding 184px, because at a narrow window the card
/// itself shrinks ([`card_size`]) and a fixed list would take that width out of
/// the KEY and VALUE cells — which is where it hurts, since an environment name
/// truncates gracefully and a variable value does not.
const SCOPE_LIST_W: Pixels = px(184.);
const SCOPE_LIST_MIN_W: Pixels = px(112.);
/// The enable checkbox column, matching the request tables' own.
const ENABLE_COLUMN: Pixels = px(24.);
/// The SECRET column: a checkbox and the reveal toggle beside it.
const SECRET_COLUMN: Pixels = px(64.);
/// The trailing column holding delete.
const ACTIONS_COLUMN: Pixels = px(32.);

/// Which scope the right-hand pane is editing.
///
/// `None` is the collection scope, addressed the same way
/// [`EnvironmentState::variables`] addresses it, so there is one convention
/// rather than two.
///
/// [`EnvironmentState::variables`]: crate::api_explorer::state::environment::EnvironmentState::variables
type Scope = Option<u64>;

/// One editable variable row.
///
/// The two text inputs are entities so the row keeps its cursor, selection and
/// undo history across re-renders; the flags are plain data. The subscriptions
/// are *held* rather than detached so that dropping the row — switching scope,
/// deleting it — drops its listeners with it.
struct VariableRow {
    /// Stable across deletions, so element ids do not collide when a row in the
    /// middle goes.
    id: usize,
    enabled: bool,
    secret: bool,
    /// Whether a secret row is currently showing its value. Reset to hidden
    /// whenever the rows are rebuilt, so leaving the scope re-masks it.
    revealed: bool,
    key: Entity<InputState>,
    value: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

pub struct EnvironmentsEditor {
    page: Entity<ApiExplorer>,
    scope: Scope,
    rows: Vec<VariableRow>,
    next_row_id: usize,
    /// The selected environment's name. Empty and unused in the collection
    /// scope, which cannot be renamed.
    name_input: Entity<InputState>,
    /// The language the widget-held placeholders were built for.
    language: Language,
    focus_handle: FocusHandle,
}

/// Opens the editor over the whole window, starting on `scope`.
///
/// `name` and `variables` are the starting scope's contents, read by the
/// **caller**. They cannot be read here: `open` is reached from a click
/// listener on the page, so the page entity is already leased and reading it
/// again panics with "cannot read … while it is already being updated". Every
/// later scope switch goes through [`EnvironmentsEditor::select`], which runs
/// in the editor's own context where the page is free.
pub fn open(
    page: Entity<ApiExplorer>,
    scope: Scope,
    name: String,
    variables: Vec<Variable>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| EnvironmentsEditor::new(page, scope, name, variables, window, cx));

    window.open_dialog(cx, move |dialog, window, cx| {
        let view = view.clone();
        let (card_w, body_h) = card_size(window);
        dialog
            .w(card_w)
            .title(t(api_variables::Text::Environments, cx))
            // `content`, not `child`: a plain child is wrapped in an
            // `overflow_y_scrollbar` box, which takes its width from its
            // content and collapses everything `w_full` inside.
            .content(move |content, _, _| {
                content.child(
                    div()
                        .w(card_w - DIALOG_PADDING_X)
                        .h(body_h)
                        .child(view.clone()),
                )
            })
    });
}

fn card_size(window: &Window) -> (Pixels, Pixels) {
    let viewport = window.viewport_size();
    let width = PANEL_W.min(viewport.width - PANEL_MARGIN * 2.);
    let height = PANEL_H.min(viewport.height - PANEL_MARGIN * 4.);
    (width, height)
}

impl EnvironmentsEditor {
    fn new(
        page: Entity<ApiExplorer>,
        scope: Scope,
        name: String,
        variables: Vec<Variable>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_placeholder = t(api_variables::Text::NamePlaceholder, cx);
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder(name_placeholder));

        // Renaming is live in the page as it is typed, so the picker's label
        // keeps up, and written to disk when the field is left.
        cx.subscribe(&name_input, |this, _, event: &InputEvent, cx| match event {
            InputEvent::Change => this.rename_from_field(cx),
            InputEvent::Blur => {
                this.rename_from_field(cx);
                this.persist(cx);
            }
            _ => {}
        })
        .detach();

        let mut editor = Self {
            page,
            scope,
            rows: Vec::new(),
            next_row_id: 0,
            name_input,
            language: Language::current(cx),
            focus_handle: cx.focus_handle(),
        };
        editor
            .name_input
            .update(cx, |state, cx| state.set_value(name, window, cx));
        editor.load_rows(&variables, window, cx);
        editor
    }

    // ---- Scope selection and the rows behind it -----------------------------

    /// Switches the pane to another scope, rebuilding its rows.
    ///
    /// Nothing has to be flushed first: every edit was already committed to the
    /// page as it was made, so the scope being left is already saved.
    fn select(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        self.scope = scope;

        let name = scope
            .and_then(|id| {
                self.page
                    .read(cx)
                    .environments
                    .environments()
                    .iter()
                    .find(|environment| environment.id == id)
                    .map(|environment| environment.name.clone())
            })
            .unwrap_or_default();
        self.name_input
            .update(cx, |state, cx| state.set_value(name, window, cx));

        let variables = self.page.read(cx).environments.variables(scope).to_vec();
        self.load_rows(&variables, window, cx);
    }

    /// Rebuilds the row list from plain data. Split out of [`Self::select`] so
    /// that construction — which may not read the page, see [`open`] — and a
    /// later scope switch share one path.
    fn load_rows(&mut self, variables: &[Variable], window: &mut Window, cx: &mut Context<Self>) {
        self.rows = Vec::with_capacity(variables.len().max(1));
        for variable in variables {
            let row = self.build_row(variable, window, cx);
            self.rows.push(row);
        }
        if self.rows.is_empty() {
            // One empty row to type into, the same invariant the request
            // tables keep.
            let row = self.build_row(&Variable::default(), window, cx);
            self.rows.push(row);
        }
        cx.notify();
    }

    /// Builds one row and wires its two fields back to [`Self::commit`].
    ///
    /// `Change` commits to the page but not to disk; `Blur` writes. The row id
    /// is taken here rather than by the caller so that no path can hand two
    /// rows the same id — which would collide their element ids and make the
    /// wrong row respond to a click.
    fn build_row(
        &mut self,
        variable: &Variable,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> VariableRow {
        let key_placeholder = t(api_variables::Text::KeyPlaceholder, cx);
        let value_placeholder = t(api_variables::Text::ValuePlaceholder, cx);

        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(key_placeholder)
                .default_value(variable.key.clone())
        });
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(value_placeholder)
                // A secret starts masked; revealing is an explicit act.
                .masked(variable.secret)
                .default_value(variable.value.clone())
        });

        let subscriptions = [&key, &value]
            .map(|field| {
                cx.subscribe(field, |this, _, event: &InputEvent, cx| match event {
                    InputEvent::Change => this.commit(cx),
                    InputEvent::Blur => {
                        this.commit(cx);
                        this.persist(cx);
                    }
                    _ => {}
                })
            })
            .into_iter()
            .collect();

        let id = self.next_row_id;
        self.next_row_id += 1;

        VariableRow {
            id,
            enabled: variable.enabled,
            secret: variable.secret,
            revealed: false,
            key,
            value,
            _subscriptions: subscriptions,
        }
    }

    /// Appends an empty row.
    fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.build_row(&Variable::default(), window, cx);
        self.rows.push(row);
        self.commit(cx);
        self.persist(cx);
        cx.notify();
    }

    fn remove_row(&mut self, id: usize, cx: &mut Context<Self>) {
        self.rows.retain(|row| row.id != id);
        self.commit(cx);
        self.persist(cx);
        cx.notify();
    }

    /// The rows as plain data.
    ///
    /// A row with neither a name nor a value contributes nothing — that is the
    /// trailing "type here" row — so it is dropped rather than written to disk
    /// as an empty variable.
    fn harvest(&self, cx: &App) -> Vec<Variable> {
        self.rows
            .iter()
            .map(|row| Variable {
                key: row.key.read(cx).value().to_string(),
                value: row.value.read(cx).value().to_string(),
                enabled: row.enabled,
                secret: row.secret,
            })
            .filter(|variable| {
                !(variable.key.trim().is_empty() && variable.value.trim().is_empty())
            })
            .collect()
    }

    /// Pushes the rows into the page's state, without touching the disk.
    ///
    /// Refuses while the page's own load has not landed: the rows would have
    /// been built from an empty placeholder document, and writing them back
    /// would erase the file that is still being read. See
    /// [`EnvironmentState::is_loaded`].
    ///
    /// [`EnvironmentState::is_loaded`]: crate::api_explorer::state::environment::EnvironmentState::is_loaded
    fn commit(&mut self, cx: &mut Context<Self>) {
        if !self.page.read(cx).environments.is_loaded() {
            return;
        }
        let variables = self.harvest(cx);
        let scope = self.scope;
        self.page.update(cx, |page, cx| {
            page.environments.set_variables(scope, variables);
            page.environments.set_error(None);
            cx.notify();
        });
    }

    fn persist(&mut self, cx: &mut Context<Self>) {
        if !self.page.read(cx).environments.is_loaded() {
            return;
        }
        self.page.update(cx, |page, cx| {
            page.persist_environments(cx);
        });
    }

    // ---- Environment-level actions ------------------------------------------

    fn create_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = t(api_variables::Text::DefaultEnvironmentName, cx).to_string();
        let id = self.page.update(cx, |page, cx| {
            let id = page.environments.create(name);
            page.persist_environments(cx);
            cx.notify();
            id
        });
        self.select(Some(id), window, cx);
        // Straight into the name field: a new environment is always renamed.
        self.name_input.update(cx, |state, cx| {
            state.focus(window, cx);
        });
    }

    fn duplicate_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.scope else {
            return;
        };
        let suffix = t(api_variables::Text::EnvironmentCopySuffix, cx).to_string();
        let copy = self.page.update(cx, |page, cx| {
            let copy = page.environments.duplicate(id, &suffix);
            page.persist_environments(cx);
            cx.notify();
            copy
        });
        if let Some(copy) = copy {
            self.select(Some(copy), window, cx);
        }
    }

    fn delete_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.scope else {
            return;
        };
        self.page.update(cx, |page, cx| {
            page.environments.remove(id);
            page.persist_environments(cx);
            cx.notify();
        });
        // Back to the collection scope, which always exists.
        self.select(None, window, cx);
    }

    fn rename_from_field(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.scope else {
            return;
        };
        let name = self.name_input.read(cx).value().to_string();
        self.page.update(cx, |page, cx| {
            page.environments.rename(id, name);
            cx.notify();
        });
    }

    /// Picks an environment file and merges it in, then selects what arrived.
    ///
    /// The page owns the picker and the read (both off the UI thread); this
    /// only says what to do with the result. The editor entity is captured
    /// rather than `self`, because the callback lands in the *page's* context
    /// long after this method has returned.
    fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.entity();
        self.page.update(cx, |page, cx| {
            page.import_environments(window, cx, move |_, selected, window, cx| {
                if let Some(selected) = selected {
                    editor.update(cx, |editor, cx| {
                        editor.select(Some(selected), window, cx);
                    });
                }
                // A failed import leaves the pane where it was; the page holds
                // the error and the editor redraws it below.
                editor.update(cx, |_, cx| cx.notify());
            });
        });
    }

    /// Re-pushes the placeholders the widgets cache after a language change.
    fn sync_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = Language::current(cx);
        if language == self.language {
            return;
        }
        self.language = language;

        let name = t(api_variables::Text::NamePlaceholder, cx);
        self.name_input
            .update(cx, |state, cx| state.set_placeholder(name, window, cx));

        let key = t(api_variables::Text::KeyPlaceholder, cx);
        let value = t(api_variables::Text::ValuePlaceholder, cx);
        for row in &self.rows {
            let (key, value) = (key.clone(), value.clone());
            row.key
                .update(cx, |state, cx| state.set_placeholder(key, window, cx));
            row.value
                .update(cx, |state, cx| state.set_placeholder(value, window, cx));
        }
    }
}

impl Focusable for EnvironmentsEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EnvironmentsEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_language(window, cx);

        h_flex()
            .size_full()
            .items_start()
            .track_focus(&self.focus_handle)
            .child(self.scope_list(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl_3()
                    .child(self.scope_header(cx))
                    .children(self.store_error(cx))
                    .child(self.secret_notice(cx))
                    .child(self.table(cx)),
            )
    }
}

impl EnvironmentsEditor {
    /// The collection scope, then every environment, then New and Import.
    fn scope_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let environments = self.page.read(cx).environments.environments().to_vec();
        let active = self.page.read(cx).environments.active_id();
        let selected = self.scope;

        let rows: Vec<gpui::AnyElement> = environments
            .iter()
            .map(|environment| {
                let id = environment.id;
                Button::new(("environment-scope", id as usize))
                    .ghost()
                    .small()
                    .w_full()
                    .justify_start()
                    .selected(selected == Some(id))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .child(SharedString::from(environment.name.clone())),
                            )
                            // The dot marks the environment requests currently
                            // resolve against, which is not necessarily the one
                            // being edited.
                            .when(active == Some(id), |this| {
                                this.child(div().size(px(6.)).rounded_full().bg(cx.theme().primary))
                            }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select(Some(id), window, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        v_flex()
            // A share of the card rather than a fixed width, clamped both ways.
            // `flex_shrink` alone does nothing here: the pane beside this one is
            // `flex_1` with `min_w_0`, so it absorbs every pixel of shrinking
            // and this would never give any back.
            .w(gpui::relative(0.32))
            .min_w(SCOPE_LIST_MIN_W)
            .max_w(SCOPE_LIST_W)
            .flex_shrink_0()
            .h_full()
            .gap_1()
            .pr_3()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("collection-variables-scope")
                    .ghost()
                    .small()
                    .w_full()
                    .justify_start()
                    .selected(selected.is_none())
                    .label(t(api_variables::Text::CollectionVariables, cx))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.select(None, window, cx);
                    })),
            )
            .child(
                div()
                    .pt_1()
                    .px_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(api_variables::Text::Environments, cx)),
            )
            .child(
                div()
                    .id("environment-scope-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(rows.is_empty(), |this| {
                        this.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t(api_variables::Text::NoEnvironmentsYet, cx)),
                        )
                    })
                    .children(rows),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        Button::new("environment-new")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Plus)
                            .label(t(api_variables::Text::NewEnvironment, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_environment(window, cx);
                            })),
                    )
                    .child(
                        Button::new("environment-import")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Import)
                            .tooltip(t(api_variables::Text::ImportEnvironment, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.import(window, cx);
                            })),
                    ),
            )
    }

    /// The name field (or the collection scope's fixed title) and the two
    /// environment-level actions.
    fn scope_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_environment = self.scope.is_some();
        let name_input = self.name_input.clone();

        v_flex()
            .w_full()
            .gap_1()
            .pb_2()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .when(is_environment, |this| {
                        this.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&name_input).small()),
                        )
                        .child(
                            Button::new("environment-duplicate")
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Copy)
                                .tooltip(t(api_variables::Text::DuplicateEnvironment, cx))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.duplicate_environment(window, cx);
                                })),
                        )
                        .child(
                            Button::new("environment-delete")
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Trash)
                                .tooltip(t(api_variables::Text::DeleteEnvironment, cx))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.delete_environment(window, cx);
                                })),
                        )
                    })
                    .when(!is_environment, |this| {
                        this.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .font_bold()
                                .child(t(api_variables::Text::CollectionVariables, cx)),
                        )
                    }),
            )
            .when(!is_environment, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(api_variables::Text::CollectionVariablesNote, cx)),
                )
            })
    }

    /// The last store or import failure, shown where the action that caused it
    /// happened. Held as a [`Str`] on the page, so it re-translates live.
    fn store_error(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let error = self.page.read(cx).environments.error().cloned()?;
        Some(
            h_flex()
                .w_full()
                .items_start()
                .gap_1p5()
                .px_2()
                .py_1p5()
                .mb_2()
                .rounded(cx.theme().radius)
                .text_xs()
                .text_color(cx.theme().danger)
                .bg(cx.theme().danger.opacity(0.1))
                .child(
                    Icon::new(AppIcon::AlertTriangle)
                        .size(px(12.))
                        .flex_shrink_0(),
                )
                .child(div().flex_1().min_w_0().child(t(error, cx)))
                .into_any_element(),
        )
    }

    /// The unencrypted-storage notice. Always on screen while the editor is
    /// open — see this module's doc; it is a decision, not a nicety.
    fn secret_notice(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .items_start()
            .gap_1p5()
            .px_2()
            .py_1p5()
            .mb_2()
            .rounded(cx.theme().radius)
            .text_xs()
            .text_color(cx.theme().warning)
            .bg(cx.theme().warning.opacity(0.1))
            .child(
                Icon::new(AppIcon::AlertTriangle)
                    .size(px(12.))
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(t(api_variables::Text::SecretStorageWarning, cx)),
            )
    }

    /// The variables table: a count, the column rule, the rows, and Add.
    fn table(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self
            .rows
            .iter()
            .filter(|row| row.enabled && !row.key.read(cx).value().trim().is_empty())
            .count();
        let summary = if active == 0 {
            api_variables::Text::NoActiveVariables
        } else {
            api_variables::Text::ActiveVariables(active)
        };

        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .pb_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(summary, cx)),
                    )
                    .child(
                        Button::new("variable-add-top")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Plus)
                            .label(t(api_variables::Text::AddVariable, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_row(window, cx);
                            })),
                    ),
            )
            .child(self.column_header(cx))
            .child(
                div()
                    .id("variable-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(
                        self.rows
                            .iter()
                            .map(|row| self.render_row(row, cx).into_any_element()),
                    ),
            )
    }

    fn column_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .py_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(div().w(ENABLE_COLUMN).flex_shrink_0())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(t(api_variables::Text::ColumnKey, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(t(api_variables::Text::ColumnValue, cx)),
            )
            .child(
                div()
                    .w(SECRET_COLUMN)
                    .flex_shrink_0()
                    .child(t(api_variables::Text::ColumnSecret, cx)),
            )
            .child(div().w(ACTIONS_COLUMN).flex_shrink_0())
    }

    fn render_row(&self, row: &VariableRow, cx: &mut Context<Self>) -> impl IntoElement {
        let id = row.id;
        let secret = row.secret;
        let revealed = row.revealed;

        h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.5))
            .child(
                div().w(ENABLE_COLUMN).flex_shrink_0().child(
                    Checkbox::new(("variable-enabled", id))
                        .checked(row.enabled)
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let checked = *checked;
                            if let Some(row) = this.rows.iter_mut().find(|row| row.id == id) {
                                row.enabled = checked;
                            }
                            this.commit(cx);
                            this.persist(cx);
                            cx.notify();
                        })),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&row.key).appearance(false).small()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&row.value).appearance(false).small()),
            )
            .child(
                h_flex()
                    .w(SECRET_COLUMN)
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .child(
                        Checkbox::new(("variable-secret", id))
                            .checked(secret)
                            .tooltip(t(api_variables::Text::MarkSecret, cx))
                            .on_click(cx.listener(move |this, checked: &bool, window, cx| {
                                this.set_secret(id, *checked, window, cx);
                            })),
                    )
                    // The reveal toggle only exists on a row that is masked, so
                    // an ordinary variable's cell is not half a control.
                    .when(secret, |this| {
                        this.child(
                            Button::new(("variable-reveal", id))
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Eye)
                                .selected(revealed)
                                .tooltip(t(
                                    if revealed {
                                        api_variables::Text::HideSecret
                                    } else {
                                        api_variables::Text::RevealSecret
                                    },
                                    cx,
                                ))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.toggle_reveal(id, window, cx);
                                })),
                        )
                    }),
            )
            .child(
                div().w(ACTIONS_COLUMN).flex_shrink_0().child(
                    Button::new(("variable-delete", id))
                        .ghost()
                        .xsmall()
                        .icon(AppIcon::Close)
                        .tooltip(t(api_variables::Text::DeleteRow, cx))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_row(id, cx);
                        })),
                ),
            )
    }

    /// Marks a row secret or ordinary. Turning the flag *on* also re-masks the
    /// field, so a value that was revealed does not stay legible.
    fn set_secret(&mut self, id: usize, secret: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            row.secret = secret;
            row.revealed = false;
            row.value
                .update(cx, |state, cx| state.set_masked(secret, window, cx));
        }
        self.commit(cx);
        self.persist(cx);
        cx.notify();
    }

    fn toggle_reveal(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            row.revealed = !row.revealed;
            let masked = !row.revealed;
            row.value
                .update(cx, |state, cx| state.set_masked(masked, window, cx));
        }
        cx.notify();
    }
}
