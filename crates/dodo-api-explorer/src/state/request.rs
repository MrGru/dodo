//! The editable state of one open request: method, URL, params, headers, body,
//! auth and scripts.
//!
//! This owns the `InputState` entities the editor renders, which is why it
//! needs a `Window` to build. The plain-data snapshot handed to the service
//! layer is [`RequestDraft`], taken at the moment Send is pressed.

use gpui::{AppContext as _, Context, Entity, SharedString, Window};
use gpui_component::input::InputState;

use crate::i18n::{Str, api_explorer, api_scripts, t};
use crate::models::auth::{ApiKeyLocation, AuthDraft, AuthType};
use crate::models::body::{BodyDraft, BodyType};
use crate::models::collection::NodeId;
use crate::models::key_value::{FieldKind, KeyValue};
use crate::models::method::HttpMethod;
use crate::models::request::RequestDraft;
use crate::models::script::{ScriptOrigin, ScriptSyntaxError};
use crate::models::snapshot::RequestSnapshot;
use crate::models::tab_title;

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
            RequestTab::Params => api_explorer::Text::RequestTabParams.into(),
            RequestTab::Headers => api_explorer::Text::RequestTabHeaders.into(),
            RequestTab::Body => api_explorer::Text::RequestTabBody.into(),
            RequestTab::Auth => api_explorer::Text::RequestTabAuth.into(),
            RequestTab::Scripts => api_explorer::Text::RequestTabScripts.into(),
        }
    }
}

/// Which of the two script editors an operation is about.
///
/// Here rather than in the view because three layers need the same distinction:
/// the Scripts tab (which editor a templates menu or a Format button belongs
/// to), the tab state (which check task to replace) and the send path (which
/// hook a script is).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScriptSlot {
    Pre,
    Post,
}

impl ScriptSlot {
    pub const ALL: [ScriptSlot; 2] = [ScriptSlot::Pre, ScriptSlot::Post];

    /// The prefix element ids in this slot are built from.
    pub fn id_prefix(self) -> &'static str {
        match self {
            ScriptSlot::Pre => "pre-script",
            ScriptSlot::Post => "post-script",
        }
    }

    pub fn index(self) -> usize {
        match self {
            ScriptSlot::Pre => 0,
            ScriptSlot::Post => 1,
        }
    }

    pub fn label(self) -> Str {
        match self {
            ScriptSlot::Pre => api_scripts::Text::PreRequestScriptLabel.into(),
            ScriptSlot::Post => api_scripts::Text::PostResponseScriptLabel.into(),
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
                api_explorer::Text::ParamKeyPlaceholder.into(),
                api_explorer::Text::ParamValuePlaceholder.into(),
                api_explorer::Text::DescriptionPlaceholder.into(),
            ),
            RowTable::Headers => (
                api_explorer::Text::HeaderKeyPlaceholder.into(),
                api_explorer::Text::HeaderValuePlaceholder.into(),
                api_explorer::Text::DescriptionPlaceholder.into(),
            ),
            RowTable::BodyFields => (
                api_explorer::Text::FieldKeyPlaceholder.into(),
                api_explorer::Text::FieldValuePlaceholder.into(),
                api_explorer::Text::DescriptionPlaceholder.into(),
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
    /// Whether the row sends its text or a file. Only multipart reads this;
    /// every other table leaves it at [`FieldKind::Text`].
    pub kind: FieldKind,
    /// The file a [`FieldKind::File`] row sends. Plain data rather than an
    /// `InputState`: it is chosen through the platform picker, never typed.
    pub file_path: String,
    /// The chosen file's size, when the `stat` succeeded. Display only.
    pub file_size: Option<u64>,
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
            kind: FieldKind::default(),
            file_path: String::new(),
            file_size: None,
        }
    }

    /// The row as plain data.
    pub fn snapshot(&self, cx: &gpui::App) -> KeyValue {
        KeyValue {
            enabled: self.enabled,
            key: self.key.read(cx).value().to_string(),
            value: self.value.read(cx).value().to_string(),
            kind: self.kind,
            file_path: self.file_path.clone(),
        }
    }

    /// Whether this row is a file row that has been named but never given a
    /// file — the state the table marks and the encoder skips.
    ///
    /// Delegates to the model so the table and the encoder cannot drift apart
    /// about what "incomplete" means.
    pub fn is_incomplete_file(&self, cx: &gpui::App) -> bool {
        self.snapshot(cx).is_incomplete_file()
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
    /// The file [`BodyType::Binary`] sends. Empty means none chosen.
    pub binary_path: String,
    /// Its size, when known. Display only; the encoder re-reads the file.
    pub binary_size: Option<u64>,

    // Auth tab.
    pub auth_type: AuthType,
    pub auth_token: Entity<InputState>,
    pub auth_username: Entity<InputState>,
    pub auth_password: Entity<InputState>,
    pub auth_key_name: Entity<InputState>,
    pub auth_key_value: Entity<InputState>,
    pub auth_key_location: ApiKeyLocation,

    // Scripts tab.
    pub pre_request_script: Entity<InputState>,
    pub post_response_script: Entity<InputState>,
    /// The last parse failure in each editor, or `None` when it parses.
    ///
    /// Kept beside the editor as well as inside it: the wavy underline says
    /// *where*, and the strip under the header says *what*. A syntax error a
    /// user has to hover to read is one they will discover by pressing Send.
    pub pre_script_error: Option<ScriptSyntaxError>,
    pub post_script_error: Option<ScriptSyntaxError>,
    /// Where this request's scripts came from, which is what the consent gate
    /// reads. Set once — by an import, or by whatever snapshot the tab was
    /// opened from — and deliberately **not** changed by editing: see
    /// [`ScriptOrigin`].
    pub script_origin: ScriptOrigin,
    /// The collection node this tab was opened from, when it was opened from
    /// one. The other half of a consent key, and `None` for a new tab, a
    /// history reopen or a pasted cURL command.
    pub origin_node: Option<NodeId>,

    /// The tab's display name. `None` means "not named yet", and the strip
    /// shows the method and path instead.
    pub name: Option<SharedString>,
    /// Whether anything has been edited since the tab was last named. Drives
    /// the unsaved dot in the tab strip.
    pub dirty: bool,
    /// What the URL field held at the last change event.
    ///
    /// Kept so that a *paste* can be told apart from typing: see
    /// [`is_bulk_change`], which is what stops the cURL importer from firing
    /// while somebody types the word "curl".
    pub last_url: String,
    /// Source of [`KeyValueRow::id`]. Monotonic, never reused.
    next_row_id: usize,
}

/// Whether a change to a text field is a paste rather than a keystroke.
///
/// A keystroke changes at most a couple of characters at one place — the common
/// prefix and suffix account for everything else. A paste replaces a selection,
/// which may be the whole field, so length alone is not enough to tell them
/// apart; this compares the shape of the edit instead.
pub fn is_bulk_change(previous: &str, current: &str) -> bool {
    let previous: Vec<char> = previous.chars().collect();
    let current: Vec<char> = current.chars().collect();

    let prefix = previous
        .iter()
        .zip(&current)
        .take_while(|(a, b)| a == b)
        .count();
    let remaining = previous.len().min(current.len()) - prefix;
    let suffix = previous
        .iter()
        .rev()
        .zip(current.iter().rev())
        .take_while(|(a, b)| a == b)
        .take(remaining)
        .count();

    let removed = previous.len() - prefix - suffix;
    let inserted = current.len() - prefix - suffix;
    // Two characters of slack: an IME commit and a paired-bracket insertion
    // are both still typing.
    removed > 2 || inserted > 2
}

impl RequestState {
    pub fn new(window: &mut Window, cx: &mut Context<super::tab::RequestTabState>) -> Self {
        // Read before the struct literal: `t` borrows `cx`, which the entity
        // constructors below need mutably.
        let url_placeholder = t(api_explorer::Text::UrlPlaceholder, cx);
        let body_placeholder = t(api_explorer::Text::BodyPlaceholder, cx);
        let pre_placeholder = t(api_explorer::Text::PreRequestScriptPlaceholder, cx);
        let post_placeholder = t(api_explorer::Text::PostResponseScriptPlaceholder, cx);
        let token_placeholder = t(api_explorer::Text::AuthTokenPlaceholder, cx);
        let username_placeholder = t(api_explorer::Text::AuthUsernamePlaceholder, cx);
        let password_placeholder = t(api_explorer::Text::AuthPasswordPlaceholder, cx);
        let key_name_placeholder = t(api_explorer::Text::ApiKeyNamePlaceholder, cx);
        let key_value_placeholder = t(api_explorer::Text::ApiKeyValuePlaceholder, cx);
        let bulk_placeholder = t(api_explorer::Text::BulkEditPlaceholder, cx);

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
            binary_path: String::new(),
            binary_size: None,

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

            // `"javascript"`, not `"text"`: these are the two editors where the
            // user writes code rather than reads a payload, and the grammar is
            // compiled in (`Cargo.toml`'s `syntax-highlighting` feature).
            pre_request_script: code_editor("javascript", pre_placeholder, window, cx),
            post_response_script: code_editor("javascript", post_placeholder, window, cx),
            pre_script_error: None,
            post_script_error: None,
            script_origin: ScriptOrigin::default(),
            origin_node: None,

            name: None,
            dirty: false,
            last_url: String::new(),
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

        let (enabled, key, value, description, kind, file_path, file_size) = {
            let row = &self.rows(table)[index];
            (
                row.enabled,
                row.key.read(cx).value(),
                row.value.read(cx).value(),
                row.description.read(cx).value(),
                row.kind,
                row.file_path.clone(),
                row.file_size,
            )
        };

        let mut copy = KeyValueRow::new(self.next_row_id, table, window, cx);
        self.next_row_id += 1;
        copy.enabled = enabled;
        // A duplicated file row points at the same file: the point of
        // duplicating one is usually to change the field name, not the upload.
        copy.kind = kind;
        copy.file_path = file_path;
        copy.file_size = file_size;
        for (field, text) in [
            (&copy.key, key),
            (&copy.value, value),
            (&copy.description, description),
        ] {
            field.update(cx, |state, cx| state.set_value(text, window, cx));
        }

        self.rows_mut(table).insert(index + 1, copy);
    }

    /// Switches a row between sending its text and sending a file.
    ///
    /// Both sides are kept: switching a file row back to text and back again
    /// finds the same file, for the same reason the Body tab keeps every
    /// editor's contents when its kind changes.
    pub fn set_row_kind(&mut self, table: RowTable, id: usize, kind: FieldKind) {
        if let Some(row) = self.rows_mut(table).iter_mut().find(|row| row.id == id) {
            row.kind = kind;
        }
    }

    /// Records the file a row will upload. An empty path clears the choice.
    pub fn set_row_file(&mut self, table: RowTable, id: usize, path: String, size: Option<u64>) {
        if let Some(row) = self.rows_mut(table).iter_mut().find(|row| row.id == id) {
            row.file_path = path;
            row.file_size = size;
        }
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
                    ..KeyValue::text(key, value)
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
    ///
    /// The `refresh` is not optional. `set_highlighter` drops the highlighter
    /// and cancels its parse task without scheduling a replacement, so on its
    /// own it leaves the editor uncoloured until the user's next keystroke;
    /// `refresh` is what marks the next render as the one that re-parses. See
    /// the module doc of `database::state::editor` for the full diagnosis, and
    /// `state::tab::refresh_body`, which gets the same effect from the
    /// `set_value` it happens to make straight afterwards.
    pub fn apply_body_language(&self, cx: &mut gpui::App) {
        let Some(language) = self.body_type.editor_language() else {
            return;
        };
        self.body_editor.update(cx, |state, cx| {
            state.set_highlighter(language, cx);
            state.refresh(cx);
        });
    }

    /// The editor behind one script slot.
    pub fn script_editor(&self, slot: ScriptSlot) -> &Entity<InputState> {
        match slot {
            ScriptSlot::Pre => &self.pre_request_script,
            ScriptSlot::Post => &self.post_response_script,
        }
    }

    /// The last parse failure in one script slot.
    pub fn script_error(&self, slot: ScriptSlot) -> Option<&ScriptSyntaxError> {
        match slot {
            ScriptSlot::Pre => self.pre_script_error.as_ref(),
            ScriptSlot::Post => self.post_script_error.as_ref(),
        }
    }

    pub fn set_script_error(&mut self, slot: ScriptSlot, error: Option<ScriptSyntaxError>) {
        match slot {
            ScriptSlot::Pre => self.pre_script_error = error,
            ScriptSlot::Post => self.post_script_error = error,
        }
    }

    /// Re-pushes every placeholder the widgets hold internally after a language
    /// change.
    ///
    /// `InputState` takes its placeholder once and caches it, so none of them
    /// re-translate on their own; this is the sweep that makes them.
    pub fn sync_placeholders(&self, window: &mut Window, cx: &mut gpui::App) {
        for (field, str) in [
            (&self.url, api_explorer::Text::UrlPlaceholder),
            (&self.body_editor, api_explorer::Text::BodyPlaceholder),
            (&self.auth_token, api_explorer::Text::AuthTokenPlaceholder),
            (
                &self.auth_username,
                api_explorer::Text::AuthUsernamePlaceholder,
            ),
            (
                &self.auth_password,
                api_explorer::Text::AuthPasswordPlaceholder,
            ),
            (
                &self.auth_key_name,
                api_explorer::Text::ApiKeyNamePlaceholder,
            ),
            (
                &self.auth_key_value,
                api_explorer::Text::ApiKeyValuePlaceholder,
            ),
            (
                &self.pre_request_script,
                api_explorer::Text::PreRequestScriptPlaceholder,
            ),
            (
                &self.post_response_script,
                api_explorer::Text::PostResponseScriptPlaceholder,
            ),
        ] {
            let text = t(str, cx);
            field.update(cx, |state, cx| {
                state.set_placeholder(text, window, cx);
            });
        }

        let bulk_placeholder = t(api_explorer::Text::BulkEditPlaceholder, cx);
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
                file_path: self.binary_path.clone(),
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
            script_origin: self.script_origin,
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
        self.last_url = url.clone();
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
        self.binary_path = snapshot.body.file_path.clone();
        // The size is not saved — it is a property of the file, not of the
        // request — so the view asks for it again through
        // `services::file_picker::refresh_size` after a restore.
        self.binary_size = None;

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
        // Provenance travels with the request: restoring an imported request
        // into a tab does not make its script one the user wrote.
        self.script_origin = snapshot.script_origin;

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
            row.kind = value.kind;
            row.file_path = value.file_path.clone();
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

    /// What the request tab strip shows.
    ///
    /// An explicit name — typed into the save popover, or carried in from the
    /// collection the request was opened from — always wins. Everything else is
    /// [`tab_title::derive`]'s job, including what to do with a URL that does
    /// not parse yet; a request with nothing typed at all falls back to the one
    /// piece of wording, which is why this needs `cx`.
    pub fn display_name(&self, cx: &gpui::App) -> SharedString {
        if let Some(name) = &self.name {
            return name.clone();
        }

        match tab_title::derive(&self.url.read(cx).value()) {
            Some(title) => SharedString::from(title),
            None => t(api_explorer::Text::UntitledRequest, cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_bulk_change, parse_bulk_lines};

    #[test]
    fn typing_one_character_at_a_time_is_not_a_paste() {
        let typed = "curl https://example.com";
        for split in 1..typed.len() {
            assert!(
                !is_bulk_change(&typed[..split - 1], &typed[..split]),
                "typing up to {:?} read as a paste",
                &typed[..split]
            );
        }
        // …and so is deleting backwards through it.
        assert!(!is_bulk_change("curl h", "curl "));
    }

    #[test]
    fn an_edit_in_the_middle_is_still_typing() {
        assert!(!is_bulk_change("https://a.b/xy", "https://a.b/x1y"));
    }

    #[test]
    fn a_paste_over_a_selection_is_a_bulk_change_even_when_it_shortens() {
        let long_url = "https://example.com/a/very/long/path/that/was/already/here";
        let pasted = "curl -X POST https://a.b/x";
        assert!(is_bulk_change(long_url, pasted));
        assert!(is_bulk_change("", pasted));
        // Clearing the field is bulk too; the caller's other checks decide what
        // that means.
        assert!(is_bulk_change(long_url, ""));
    }

    #[test]
    fn no_change_at_all_is_not_a_change() {
        assert!(!is_bulk_change("same", "same"));
        assert!(!is_bulk_change("", ""));
    }

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
