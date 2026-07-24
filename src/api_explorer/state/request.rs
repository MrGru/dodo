//! The editable state of one open request: method, URL, params, headers, body,
//! auth and scripts.
//!
//! This owns the `InputState` entities the editor renders, which is why it
//! needs a `Window` to build. The plain-data snapshot handed to the service
//! layer is [`RequestDraft`], taken at the moment Send is pressed.

use gpui::{AppContext as _, Context, Entity, SharedString, Window};
use gpui_component::input::InputState;

use crate::api_explorer::models::auth::{ApiKeyLocation, AuthDraft, AuthType};
use crate::api_explorer::models::body::{BodyDraft, BodyType};
use crate::api_explorer::models::key_value::KeyValue;
use crate::api_explorer::models::method::HttpMethod;
use crate::api_explorer::models::request::RequestDraft;
use crate::api_explorer::models::snapshot::RequestSnapshot;
use crate::i18n::{Str, t};

/// Which of the request tabs is showing.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestTab {
    #[default]
    Params,
    Headers,
    Body,
    Auth,
    Scripts,
}

impl RequestTab {
    pub const ALL: [RequestTab; 5] = [
        RequestTab::Params,
        RequestTab::Headers,
        RequestTab::Body,
        RequestTab::Auth,
        RequestTab::Scripts,
    ];

    pub fn label(self) -> Str {
        match self {
            RequestTab::Params => Str::RequestTabParams,
            RequestTab::Headers => Str::RequestTabHeaders,
            RequestTab::Body => Str::RequestTabBody,
            RequestTab::Auth => Str::RequestTabAuth,
            RequestTab::Scripts => Str::RequestTabScripts,
        }
    }
}

/// Which of the three key/value tables a row operation is about.
///
/// The tables differ only in which `Vec` they live in and what their empty
/// cells say, so every row operation takes one of these rather than being
/// written three times. Params and Headers go on the wire as they are; the
/// body fields become a form document.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowTable {
    Params,
    Headers,
    BodyFields,
}

impl RowTable {
    /// Position in the per-table arrays [`RequestState`] keeps (the bulk-edit
    /// flag and its editor). In [`RowTable`] declaration order.
    fn index(self) -> usize {
        match self {
            RowTable::Params => 0,
            RowTable::Headers => 1,
            RowTable::BodyFields => 2,
        }
    }

    /// The key, value and description placeholders a fresh row is given.
    fn placeholders(self) -> (Str, Str, Str) {
        match self {
            RowTable::Params => (
                Str::ParamKeyPlaceholder,
                Str::ParamValuePlaceholder,
                Str::DescriptionPlaceholder,
            ),
            RowTable::Headers => (
                Str::HeaderKeyPlaceholder,
                Str::HeaderValuePlaceholder,
                Str::DescriptionPlaceholder,
            ),
            RowTable::BodyFields => (
                Str::FieldKeyPlaceholder,
                Str::FieldValuePlaceholder,
                Str::DescriptionPlaceholder,
            ),
        }
    }
}

/// Which way a row is being moved.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MoveRow {
    Up,
    Down,
}

/// One row of an editable key/value table.
///
/// The three text inputs are entities so the row keeps its cursor, selection
/// and undo history across re-renders; the enabled flag is plain data.
pub struct KeyValueRow {
    /// Stable across reorders and deletions, so element ids do not collide when
    /// a row in the middle is removed.
    pub id: usize,
    pub enabled: bool,
    pub key: Entity<InputState>,
    pub value: Entity<InputState>,
    /// The user's note about the row.
    ///
    /// Documentation, not payload: it is deliberately absent from [`KeyValue`]
    /// and never reaches the wire, because no HTTP header or query parameter
    /// has a description. It travels with the row through duplicate and
    /// reorder, which is the whole of what it is for.
    pub description: Entity<InputState>,
}

impl KeyValueRow {
    /// Placeholders are pushed in here rather than at render time: they live
    /// inside `InputState`, which is not rebuilt each frame, so they are also
    /// what [`RequestState::sync_placeholders`] has to refresh when the
    /// language changes.
    fn new(id: usize, table: RowTable, window: &mut Window, cx: &mut gpui::App) -> Self {
        let (key, value, description) = table.placeholders();
        Self {
            id,
            enabled: true,
            key: single_line(t(key, cx), window, cx),
            value: single_line(t(value, cx), window, cx),
            description: single_line(t(description, cx), window, cx),
        }
    }

    /// The row as plain data.
    pub fn snapshot(&self, cx: &gpui::App) -> KeyValue {
        KeyValue {
            enabled: self.enabled,
            key: self.key.read(cx).value().to_string(),
            value: self.value.read(cx).value().to_string(),
        }
    }
}

/// A single-line field with its placeholder already pushed in.
fn single_line(
    placeholder: SharedString,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
}

/// A plain multi-line field: the Bulk Edit text area. No code gutter — it holds
/// `Key: Value` lines, not source, so soft wrap and a placeholder are enough.
fn multi_line(
    placeholder: SharedString,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .multi_line(true)
            .soft_wrap(true)
            .placeholder(placeholder)
    })
}

/// Parses Bulk Edit text back into `(enabled, key, value)` rows.
///
/// One entry per non-blank line, `Key: Value`, splitting on the first colon so a
/// value may itself contain one (`Host: example.com:8080`). A line beginning
/// with `#` is a disabled entry; a line with no colon is a key with an empty
/// value. This is the inverse of [`RequestState::rows_to_bulk`].
fn parse_bulk_lines(text: &str) -> Vec<(bool, String, String)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (enabled, rest) = match line.strip_prefix('#') {
            Some(rest) => (false, rest.trim_start()),
            None => (true, line),
        };
        if rest.is_empty() {
            continue;
        }
        let (key, value) = match rest.split_once(':') {
            Some((key, value)) => (key.trim().to_string(), value.trim().to_string()),
            None => (rest.to_string(), String::new()),
        };
        out.push((enabled, key, value));
    }
    out
}

/// A multi-line code editor: the body document and both script panes.
///
/// `code_editor` comes first because it *replaces* the mode, and `line_number`
/// asserts in debug builds that the mode is already a code editor.
fn code_editor(
    language: &'static str,
    placeholder: SharedString,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .code_editor(language)
            .multi_line(true)
            .line_number(true)
            .soft_wrap(true)
            .placeholder(placeholder)
    })
}

/// The request half of one open tab.
pub struct RequestState {
    pub method: HttpMethod,
    pub url: Entity<InputState>,
    pub params: Vec<KeyValueRow>,
    pub headers: Vec<KeyValueRow>,
    pub active_tab: RequestTab,

    /// Whether each key/value table is showing its Bulk Edit text view instead
    /// of the row editor. Indexed by [`RowTable::index`].
    ///
    /// In Bulk Edit the editor at the same index is the source of truth; in
    /// Table mode the rows are. Switching modes serializes one into the other
    /// (see [`RequestState::set_edit_mode`]).
    bulk_edit: [bool; 3],
    /// The multiline editor behind each table's Bulk Edit view.
    bulk_editors: [Entity<InputState>; 3],

    // Body tab.
    pub body_type: BodyType,
    /// The document behind the text-shaped body types.
    ///
    /// One editor for all of them rather than one each: switching JSON to Raw
    /// and back has to keep what was typed, and re-pointing the highlighter
    /// (see [`RequestState::apply_body_language`]) is cheaper than rebuilding
    /// the widget and its rope.
    pub body_editor: Entity<InputState>,
    /// The rows behind the two form body types, shared for the same reason.
    pub body_fields: Vec<KeyValueRow>,

    // Auth tab.
    pub auth_type: AuthType,
    pub auth_token: Entity<InputState>,
    pub auth_username: Entity<InputState>,
    pub auth_password: Entity<InputState>,
    pub auth_key_name: Entity<InputState>,
    pub auth_key_value: Entity<InputState>,
    pub auth_key_location: ApiKeyLocation,

    // Scripts tab. Edited and kept for the session; nothing runs them, which
    // the tab states on screen rather than leaving to be discovered.
    pub pre_request_script: Entity<InputState>,
    pub post_response_script: Entity<InputState>,

    /// The tab's display name. `None` means "not named yet", and the strip
    /// shows the method and path instead.
    pub name: Option<SharedString>,
    /// Whether anything has been edited since the tab was last named. Drives
    /// the unsaved dot in the tab strip.
    pub dirty: bool,
    /// Source of [`KeyValueRow::id`]. Monotonic, never reused.
    next_row_id: usize,
}

impl RequestState {
    pub fn new(window: &mut Window, cx: &mut Context<super::tab::RequestTabState>) -> Self {
        // Read before the struct literal: `t` borrows `cx`, which the entity
        // constructors below need mutably.
        let url_placeholder = t(Str::UrlPlaceholder, cx);
        let body_placeholder = t(Str::BodyPlaceholder, cx);
        let pre_placeholder = t(Str::PreRequestScriptPlaceholder, cx);
        let post_placeholder = t(Str::PostResponseScriptPlaceholder, cx);
        let token_placeholder = t(Str::AuthTokenPlaceholder, cx);
        let username_placeholder = t(Str::AuthUsernamePlaceholder, cx);
        let password_placeholder = t(Str::AuthPasswordPlaceholder, cx);
        let key_name_placeholder = t(Str::ApiKeyNamePlaceholder, cx);
        let key_value_placeholder = t(Str::ApiKeyValuePlaceholder, cx);
        let bulk_placeholder = t(Str::BulkEditPlaceholder, cx);

        let mut state = Self {
            method: HttpMethod::default(),
            url: single_line(url_placeholder, window, cx),
            params: Vec::new(),
            headers: Vec::new(),
            active_tab: RequestTab::default(),

            bulk_edit: [false; 3],
            bulk_editors: [
                multi_line(bulk_placeholder.clone(), window, cx),
                multi_line(bulk_placeholder.clone(), window, cx),
                multi_line(bulk_placeholder, window, cx),
            ],

            body_type: BodyType::default(),
            body_editor: code_editor("text", body_placeholder, window, cx),
            body_fields: Vec::new(),

            auth_type: AuthType::default(),
            auth_token: single_line(token_placeholder, window, cx),
            auth_username: single_line(username_placeholder, window, cx),
            // Masked so a password is not left legible on a shared screen. The
            // mask is display only; `value()` still returns what was typed.
            auth_password: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(password_placeholder)
                    .masked(true)
            }),
            auth_key_name: single_line(key_name_placeholder, window, cx),
            auth_key_value: single_line(key_value_placeholder, window, cx),
            auth_key_location: ApiKeyLocation::default(),

            pre_request_script: code_editor("text", pre_placeholder, window, cx),
            post_response_script: code_editor("text", post_placeholder, window, cx),

            name: None,
            dirty: false,
            next_row_id: 0,
        };

        // Every table opens with one empty row to type into, which is what the
        // reference shows and what saves a click on every new request.
        for table in [RowTable::Params, RowTable::Headers, RowTable::BodyFields] {
            state.add_row(table, window, cx);
        }
        state
    }

    pub fn rows(&self, table: RowTable) -> &[KeyValueRow] {
        match table {
            RowTable::Params => &self.params,
            RowTable::Headers => &self.headers,
            RowTable::BodyFields => &self.body_fields,
        }
    }

    fn rows_mut(&mut self, table: RowTable) -> &mut Vec<KeyValueRow> {
        match table {
            RowTable::Params => &mut self.params,
            RowTable::Headers => &mut self.headers,
            RowTable::BodyFields => &mut self.body_fields,
        }
    }

    /// Appends an empty row.
    pub fn add_row(&mut self, table: RowTable, window: &mut Window, cx: &mut gpui::App) {
        let row = KeyValueRow::new(self.next_row_id, table, window, cx);
        self.next_row_id += 1;
        self.rows_mut(table).push(row);
    }

    /// Removes a row by its stable id. Unknown ids are ignored rather than
    /// panicking: a stale click from a re-render is not an error.
    pub fn remove_row(&mut self, table: RowTable, id: usize) {
        self.rows_mut(table).retain(|row| row.id != id);
    }

    /// Copies a row, inserting the copy directly beneath the original.
    ///
    /// The copy gets its own `InputState` entities seeded with the original's
    /// text — sharing them would give two rows one cursor and one undo
    /// history, so editing either would edit both.
    pub fn duplicate_row(
        &mut self,
        table: RowTable,
        id: usize,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let Some(index) = self.index_of(table, id) else {
            return;
        };

        let (enabled, key, value, description) = {
            let row = &self.rows(table)[index];
            (
                row.enabled,
                row.key.read(cx).value(),
                row.value.read(cx).value(),
                row.description.read(cx).value(),
            )
        };

        let mut copy = KeyValueRow::new(self.next_row_id, table, window, cx);
        self.next_row_id += 1;
        copy.enabled = enabled;
        for (field, text) in [
            (&copy.key, key),
            (&copy.value, value),
            (&copy.description, description),
        ] {
            field.update(cx, |state, cx| state.set_value(text, window, cx));
        }

        self.rows_mut(table).insert(index + 1, copy);
    }

    /// Swaps a row with its neighbour. A row already at the end it is moving
    /// towards stays where it is, so neither button has to be disabled per row.
    pub fn move_row(&mut self, table: RowTable, id: usize, direction: MoveRow) {
        let Some(index) = self.index_of(table, id) else {
            return;
        };
        let rows = self.rows_mut(table);
        let target = match direction {
            MoveRow::Up => index.checked_sub(1),
            MoveRow::Down => (index + 1 < rows.len()).then_some(index + 1),
        };
        if let Some(target) = target {
            rows.swap(index, target);
        }
    }

    pub fn set_row_enabled(&mut self, table: RowTable, id: usize, enabled: bool) {
        if let Some(row) = self.rows_mut(table).iter_mut().find(|row| row.id == id) {
            row.enabled = enabled;
        }
    }

    /// Whether every row of a table is enabled — the checked state of the
    /// toggle-all control. An empty table reads as not-all-on.
    pub fn all_rows_enabled(&self, table: RowTable) -> bool {
        let rows = self.rows(table);
        !rows.is_empty() && rows.iter().all(|row| row.enabled)
    }

    /// Enables or disables every row of a table at once (the toggle-all control).
    pub fn set_all_rows_enabled(&mut self, table: RowTable, enabled: bool) {
        for row in self.rows_mut(table) {
            row.enabled = enabled;
        }
    }

    /// Whether a table is showing its Bulk Edit text view.
    pub fn is_bulk_edit(&self, table: RowTable) -> bool {
        self.bulk_edit[table.index()]
    }

    /// The multiline editor behind a table's Bulk Edit view.
    pub fn bulk_editor(&self, table: RowTable) -> &Entity<InputState> {
        &self.bulk_editors[table.index()]
    }

    /// Switches a table between Table and Bulk Edit, carrying the data across
    /// losslessly. Entering Bulk Edit serializes the rows into the editor;
    /// leaving it parses the editor back into rows, reusing the existing row
    /// entities by position so each row keeps its description.
    pub fn set_edit_mode(
        &mut self,
        table: RowTable,
        bulk: bool,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        if self.bulk_edit[table.index()] == bulk {
            return;
        }
        if bulk {
            let text = self.rows_to_bulk(table, cx);
            let editor = self.bulk_editors[table.index()].clone();
            editor.update(cx, |state, cx| state.set_value(text, window, cx));
        } else {
            let text = self.bulk_editors[table.index()]
                .read(cx)
                .value()
                .to_string();
            self.apply_bulk_text(table, &text, window, cx);
        }
        self.bulk_edit[table.index()] = bulk;
    }

    /// Serializes a table's rows into Bulk Edit text: one `Key: Value` per row,
    /// disabled rows prefixed with `# `. Fully empty rows (the trailing "type
    /// here" row) contribute nothing.
    fn rows_to_bulk(&self, table: RowTable, cx: &gpui::App) -> String {
        let mut lines = Vec::new();
        for row in self.rows(table) {
            let key = row.key.read(cx).value();
            let value = row.value.read(cx).value();
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() && value.is_empty() {
                continue;
            }
            let prefix = if row.enabled { "" } else { "# " };
            lines.push(format!("{prefix}{key}: {value}"));
        }
        lines.join("\n")
    }

    /// Rebuilds a table's rows from Bulk Edit text, reusing existing row entities
    /// positionally so descriptions (which Bulk Edit cannot express) survive the
    /// round trip when rows are only toggled or their values edited.
    fn apply_bulk_text(
        &mut self,
        table: RowTable,
        text: &str,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let parsed = parse_bulk_lines(text);
        let mut existing = std::mem::take(self.rows_mut(table)).into_iter();
        let mut rows = Vec::with_capacity(parsed.len().max(1));
        for (enabled, key, value) in parsed {
            let mut row = existing.next().unwrap_or_else(|| {
                let row = KeyValueRow::new(self.next_row_id, table, window, cx);
                self.next_row_id += 1;
                row
            });
            row.enabled = enabled;
            row.key
                .update(cx, |state, cx| state.set_value(key, window, cx));
            row.value
                .update(cx, |state, cx| state.set_value(value, window, cx));
            rows.push(row);
        }
        if rows.is_empty() {
            rows.push(KeyValueRow::new(self.next_row_id, table, window, cx));
            self.next_row_id += 1;
        }
        *self.rows_mut(table) = rows;
    }

    /// A table's rows as plain [`KeyValue`] data, taken from whichever view is
    /// authoritative: the Bulk Edit editor when that view is open, the rows
    /// otherwise. This is how [`RequestState::draft`] stays correct even when a
    /// table is left in Bulk Edit at Send time.
    fn table_key_values(&self, table: RowTable, cx: &gpui::App) -> Vec<KeyValue> {
        if self.is_bulk_edit(table) {
            let text = self.bulk_editors[table.index()].read(cx).value();
            parse_bulk_lines(&text)
                .into_iter()
                .map(|(enabled, key, value)| KeyValue {
                    enabled,
                    key,
                    value,
                })
                .collect()
        } else {
            self.rows(table)
                .iter()
                .map(|row| row.snapshot(cx))
                .collect()
        }
    }

    fn index_of(&self, table: RowTable, id: usize) -> Option<usize> {
        self.rows(table).iter().position(|row| row.id == id)
    }

    /// Points the body editor's highlighter at the grammar the current body
    /// type is written in.
    ///
    /// Called when the body type changes rather than at render time, because
    /// re-pointing the highlighter re-parses the document — cheap once, wasteful
    /// every frame.
    pub fn apply_body_language(&self, cx: &mut gpui::App) {
        let Some(language) = self.body_type.editor_language() else {
            return;
        };
        self.body_editor.update(cx, |state, cx| {
            state.set_highlighter(language, cx);
        });
    }

    /// Re-pushes every placeholder the widgets hold internally after a language
    /// change.
    ///
    /// `InputState` takes its placeholder once and caches it, so none of them
    /// re-translate on their own; this is the sweep that makes them.
    pub fn sync_placeholders(&self, window: &mut Window, cx: &mut gpui::App) {
        for (field, str) in [
            (&self.url, Str::UrlPlaceholder),
            (&self.body_editor, Str::BodyPlaceholder),
            (&self.auth_token, Str::AuthTokenPlaceholder),
            (&self.auth_username, Str::AuthUsernamePlaceholder),
            (&self.auth_password, Str::AuthPasswordPlaceholder),
            (&self.auth_key_name, Str::ApiKeyNamePlaceholder),
            (&self.auth_key_value, Str::ApiKeyValuePlaceholder),
            (&self.pre_request_script, Str::PreRequestScriptPlaceholder),
            (
                &self.post_response_script,
                Str::PostResponseScriptPlaceholder,
            ),
        ] {
            let text = t(str, cx);
            field.update(cx, |state, cx| {
                state.set_placeholder(text, window, cx);
            });
        }

        let bulk_placeholder = t(Str::BulkEditPlaceholder, cx);
        for editor in &self.bulk_editors {
            editor.update(cx, |state, cx| {
                state.set_placeholder(bulk_placeholder.clone(), window, cx);
            });
        }

        for table in [RowTable::Params, RowTable::Headers, RowTable::BodyFields] {
            let (key, value, description) = table.placeholders();
            let placeholders = [t(key, cx), t(value, cx), t(description, cx)];
            for row in self.rows(table) {
                for (field, text) in [&row.key, &row.value, &row.description]
                    .into_iter()
                    .zip(&placeholders)
                {
                    field.update(cx, |state, cx| {
                        state.set_placeholder(text.clone(), window, cx);
                    });
                }
            }
        }
    }

    /// An owned copy of everything the service layer needs, so the request can
    /// run on a background thread while the user keeps editing.
    ///
    /// This is the only place the body document is read out in full, which is
    /// what keeps a large body off the render path: nothing calls
    /// `InputState::value` on it per frame.
    pub fn draft(&self, cx: &gpui::App) -> RequestDraft {
        RequestDraft {
            method: self.method,
            url: self.url.read(cx).value().to_string(),
            params: self.table_key_values(RowTable::Params, cx),
            headers: self.table_key_values(RowTable::Headers, cx),
            body: BodyDraft {
                kind: self.body_type,
                text: self.body_editor.read(cx).value().to_string(),
                fields: self.table_key_values(RowTable::BodyFields, cx),
            },
            auth: AuthDraft {
                kind: self.auth_type,
                token: self.auth_token.read(cx).value().to_string(),
                username: self.auth_username.read(cx).value().to_string(),
                password: self.auth_password.read(cx).value().to_string(),
                key_name: self.auth_key_name.read(cx).value().to_string(),
                key_value: self.auth_key_value.read(cx).value().to_string(),
                key_location: self.auth_key_location,
            },
        }
    }

    /// A full plain-data capture of this request, including the scripts the
    /// wire-facing [`RequestDraft`] drops. This is what a saved collection entry
    /// and a history entry store.
    pub fn snapshot(&self, cx: &gpui::App) -> RequestSnapshot {
        let draft = self.draft(cx);
        RequestSnapshot {
            method: draft.method,
            url: draft.url,
            params: draft.params,
            headers: draft.headers,
            body: draft.body,
            auth: draft.auth,
            pre_request_script: self.pre_request_script.read(cx).value().to_string(),
            post_response_script: self.post_response_script.read(cx).value().to_string(),
        }
    }

    /// Restores this request from a saved snapshot — the reverse of
    /// [`RequestState::snapshot`], used when a saved request or a history entry
    /// is reopened into a tab. `name` is the tab's display name (the collection
    /// node's name, or `None` for a history reopen).
    pub fn apply_snapshot(
        &mut self,
        snapshot: &RequestSnapshot,
        name: Option<SharedString>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        self.method = snapshot.method;
        let url = snapshot.url.clone();
        self.url
            .update(cx, |state, cx| state.set_value(url, window, cx));

        self.load_rows(RowTable::Params, &snapshot.params, window, cx);
        self.load_rows(RowTable::Headers, &snapshot.headers, window, cx);

        self.body_type = snapshot.body.kind;
        let body_text = snapshot.body.text.clone();
        self.body_editor
            .update(cx, |state, cx| state.set_value(body_text, window, cx));
        self.apply_body_language(cx);
        self.load_rows(RowTable::BodyFields, &snapshot.body.fields, window, cx);

        self.auth_type = snapshot.auth.kind;
        self.auth_key_location = snapshot.auth.key_location;
        for (field, text) in [
            (&self.auth_token, &snapshot.auth.token),
            (&self.auth_username, &snapshot.auth.username),
            (&self.auth_password, &snapshot.auth.password),
            (&self.auth_key_name, &snapshot.auth.key_name),
            (&self.auth_key_value, &snapshot.auth.key_value),
        ] {
            let text = text.clone();
            field.update(cx, |state, cx| state.set_value(text, window, cx));
        }

        let pre = snapshot.pre_request_script.clone();
        self.pre_request_script
            .update(cx, |state, cx| state.set_value(pre, window, cx));
        let post = snapshot.post_response_script.clone();
        self.post_response_script
            .update(cx, |state, cx| state.set_value(post, window, cx));

        self.name = name;
        // A freshly restored request matches what is saved, so no unsaved dot.
        self.dirty = false;
    }

    /// Replaces a table's rows with ones seeded from saved key/value pairs,
    /// keeping the "one empty row to type into" invariant when the list is
    /// empty.
    fn load_rows(
        &mut self,
        table: RowTable,
        values: &[KeyValue],
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let mut rows = Vec::with_capacity(values.len().max(1));
        for value in values {
            let mut row = KeyValueRow::new(self.next_row_id, table, window, cx);
            self.next_row_id += 1;
            row.enabled = value.enabled;
            let key = value.key.clone();
            let val = value.value.clone();
            row.key
                .update(cx, |state, cx| state.set_value(key, window, cx));
            row.value
                .update(cx, |state, cx| state.set_value(val, window, cx));
            rows.push(row);
        }
        if rows.is_empty() {
            rows.push(KeyValueRow::new(self.next_row_id, table, window, cx));
            self.next_row_id += 1;
        }
        *self.rows_mut(table) = rows;
        // A restored request opens in Table view; its rows are authoritative.
        self.bulk_edit[table.index()] = false;
    }

    /// What the request tab strip shows: the given name, or a summary of the
    /// URL's path, or just the method for an empty request.
    pub fn display_name(&self, cx: &gpui::App) -> SharedString {
        if let Some(name) = &self.name {
            return name.clone();
        }

        let url = self.url.read(cx).value();
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return SharedString::new_static("/");
        }

        // Show the path if the URL parses, and the raw text while it is still
        // being typed — a half-typed URL should not blank the tab title.
        match reqwest::Url::parse(trimmed) {
            Ok(parsed) => {
                let path = parsed.path();
                let host = parsed.host_str().unwrap_or_default();
                if path == "/" || path.is_empty() {
                    SharedString::from(host.to_string())
                } else {
                    SharedString::from(format!("{host}{path}"))
                }
            }
            Err(_) => SharedString::from(trimmed.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_bulk_lines;

    #[test]
    fn parses_key_value_lines() {
        let parsed = parse_bulk_lines("Accept: application/json\nX-Trace: abc");
        assert_eq!(
            parsed,
            vec![
                (true, "Accept".to_string(), "application/json".to_string()),
                (true, "X-Trace".to_string(), "abc".to_string()),
            ]
        );
    }

    #[test]
    fn a_leading_hash_marks_a_disabled_entry() {
        let parsed = parse_bulk_lines("# Authorization: Bearer x");
        assert_eq!(
            parsed,
            vec![(false, "Authorization".to_string(), "Bearer x".to_string())]
        );
    }

    #[test]
    fn only_the_first_colon_splits_so_values_may_contain_one() {
        let parsed = parse_bulk_lines("Host: example.com:8080");
        assert_eq!(
            parsed,
            vec![(true, "Host".to_string(), "example.com:8080".to_string())]
        );
    }

    #[test]
    fn blank_lines_and_bare_hashes_are_skipped_and_missing_values_are_empty() {
        let parsed = parse_bulk_lines("\n  \n#\nflag\n");
        assert_eq!(parsed, vec![(true, "flag".to_string(), String::new())]);
    }
}
