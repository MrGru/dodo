//! The editable state of one open request: method, URL, params, headers.
//!
//! This owns the `InputState` entities the editor renders, which is why it
//! needs a `Window` to build. The plain-data snapshot handed to the service
//! layer is [`RequestDraft`], taken at the moment Send is pressed.

use gpui::{AppContext as _, Context, Entity, SharedString, Window};
use gpui_component::input::InputState;

use crate::api_explorer::models::key_value::KeyValue;
use crate::api_explorer::models::method::HttpMethod;
use crate::api_explorer::models::request::RequestDraft;
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

    /// Whether this phase implements the tab. The three that do not render an
    /// honest placeholder naming the phase they arrive in.
    pub fn is_implemented(self) -> bool {
        matches!(self, RequestTab::Params | RequestTab::Headers)
    }
}

/// One row of an editable key/value table.
///
/// The two text inputs are entities so the row keeps its cursor, selection and
/// undo history across re-renders; the enabled flag is plain data.
pub struct KeyValueRow {
    /// Stable across reorders and deletions, so element ids do not collide when
    /// a row in the middle is removed.
    pub id: usize,
    pub enabled: bool,
    pub key: Entity<InputState>,
    pub value: Entity<InputState>,
}

impl KeyValueRow {
    /// Placeholders are pushed in here rather than at render time: they live
    /// inside `InputState`, which is not rebuilt each frame, so they are also
    /// what [`RequestState::sync_row_placeholders`] has to refresh when the
    /// language changes.
    fn new(
        id: usize,
        key_placeholder: SharedString,
        value_placeholder: SharedString,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> Self {
        Self {
            id,
            enabled: true,
            key: cx.new(|cx| InputState::new(window, cx).placeholder(key_placeholder)),
            value: cx.new(|cx| InputState::new(window, cx).placeholder(value_placeholder)),
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

/// The request half of one open tab.
pub struct RequestState {
    pub method: HttpMethod,
    pub url: Entity<InputState>,
    pub params: Vec<KeyValueRow>,
    pub headers: Vec<KeyValueRow>,
    pub active_tab: RequestTab,
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
        let placeholder = t(Str::UrlPlaceholder, cx);
        let url = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));

        let mut state = Self {
            method: HttpMethod::default(),
            url,
            params: Vec::new(),
            headers: Vec::new(),
            active_tab: RequestTab::default(),
            name: None,
            dirty: false,
            next_row_id: 0,
        };
        // Both tables open with one empty row to type into, which is what the
        // reference shows and what saves a click on every new request.
        state.add_param(window, cx);
        state.add_header(window, cx);
        state
    }

    pub fn add_param(&mut self, window: &mut Window, cx: &mut gpui::App) {
        let row = KeyValueRow::new(
            self.next_row_id,
            t(Str::ParamKeyPlaceholder, cx),
            t(Str::ParamValuePlaceholder, cx),
            window,
            cx,
        );
        self.next_row_id += 1;
        self.params.push(row);
    }

    pub fn add_header(&mut self, window: &mut Window, cx: &mut gpui::App) {
        let row = KeyValueRow::new(
            self.next_row_id,
            t(Str::HeaderKeyPlaceholder, cx),
            t(Str::HeaderValuePlaceholder, cx),
            window,
            cx,
        );
        self.next_row_id += 1;
        self.headers.push(row);
    }

    /// Re-pushes every row's placeholders after a language change.
    pub fn sync_row_placeholders(&self, window: &mut Window, cx: &mut gpui::App) {
        for (rows, key, value) in [
            (
                &self.params,
                Str::ParamKeyPlaceholder,
                Str::ParamValuePlaceholder,
            ),
            (
                &self.headers,
                Str::HeaderKeyPlaceholder,
                Str::HeaderValuePlaceholder,
            ),
        ] {
            let key = t(key, cx);
            let value = t(value, cx);
            for row in rows {
                row.key.update(cx, |state, cx| {
                    state.set_placeholder(key.clone(), window, cx);
                });
                row.value.update(cx, |state, cx| {
                    state.set_placeholder(value.clone(), window, cx);
                });
            }
        }
    }

    /// Removes a row by its stable id. Unknown ids are ignored rather than
    /// panicking: a stale click from a re-render is not an error.
    pub fn remove_param(&mut self, id: usize) {
        self.params.retain(|row| row.id != id);
    }

    pub fn remove_header(&mut self, id: usize) {
        self.headers.retain(|row| row.id != id);
    }

    /// An owned copy of everything the service layer needs, so the request can
    /// run on a background thread while the user keeps editing.
    pub fn draft(&self, cx: &gpui::App) -> RequestDraft {
        RequestDraft {
            method: self.method,
            url: self.url.read(cx).value().to_string(),
            params: self.params.iter().map(|row| row.snapshot(cx)).collect(),
            headers: self.headers.iter().map(|row| row.snapshot(cx)).collect(),
        }
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
