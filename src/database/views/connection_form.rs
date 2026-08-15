//! The add / edit connection dialog.
//!
//! # It is an entity, and it is seeded with plain data
//!
//! Two rules from `settings::open` and `docker::views::detail`, both of which
//! cost a debugging session when broken:
//!
//! - The dialog body **must be an entity**. `Root::render_dialog_layer` builds
//!   the dialog from its own closure, so nothing there observes the page and a
//!   page `cx.notify()` does not repaint the dialog.
//! - That entity **may not read the page while it is being constructed**.
//!   [`open`] is reached from a click listener, so the page is leased for the
//!   whole call; a `page.read(cx)` inside the body's `new` panics at runtime
//!   with no compile error. So [`open`] takes the profile as plain data that
//!   the caller has already read for it.
//!
//! # Why there is no dropdown here
//!
//! The engine picker has two options and the TLS picker has three. A segmented
//! row of buttons shows every option at once, needs no popup, and cannot be
//! half-open when the dialog closes. `Select` is for lists too long to show.
//!
//! # The password notice is never absent
//!
//! [`db_connection::Text::PasswordStorageNotice`] is rendered whenever a password field is on
//! screen, not behind a disclosure and not only on first use. dodo stores the
//! password in plain text in its own data directory, and the one thing that
//! makes that acceptable is that the user is told every time.
//!
//! # Filling the form from a URI
//!
//! The first row is a connection URI and a button that fills the rest of the
//! form in from it. Three things about it are decisions:
//!
//! - **It is a button, not a paste handler.** Nothing about the manual flow
//!   changes, and a URI in the clipboard cannot rewrite fields the user is in
//!   the middle of typing.
//! - **It fills, and stops there.** Nothing is saved, connected or tested as a
//!   side effect: the whole feature is putting the fields on screen for the
//!   user to look at, which is why the success notice says to check them.
//! - **A name the user typed is never overwritten.** An empty name gets
//!   [`ParsedUri::suggested_name`]'s answer; anything else is left alone.
//!
//! The parse itself is [`crate::database::models::uri`], where it is unit
//! tested; this file only moves the answer into the inputs. A failure fills
//! nothing at all, so the user is never left guessing which fields came from
//! their paste.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, Styled as _, Task, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::database::components::notice::{Tone, notice};
use crate::database::models::connection::{ConnectionProfile, SslMode};
use crate::database::models::engine::{Address, Engine};
use crate::database::models::error::DbError;
use crate::database::models::uri::{self, ParsedUri, UriError};
use crate::database::services;
use crate::i18n::{Language, Str, db_connection, t};

/// The word for one TLS mode. Beside `SslMode::ALL` rather than on the enum
/// itself: `models/` has no opinion about how a mode is worded, only about what
/// it means.
fn ssl_label(mode: SslMode) -> Str {
    match mode {
        SslMode::Disable => db_connection::Text::SslDisable.into(),
        SslMode::Prefer => db_connection::Text::SslPrefer.into(),
        SslMode::Require => db_connection::Text::SslRequire.into(),
    }
}

/// The dialog's width. Stated rather than `w_full`, because a percentage width
/// inside `Dialog`'s wrappers resolves to `auto` and content-sizes the body.
const DIALOG_WIDTH: Pixels = px(520.);

/// `Dialog`'s own default padding, which the body has to subtract to sit inside
/// the card rather than overflow it.
const DIALOG_PADDING: Pixels = px(32.);

/// The width of a field's label column, so every field's input starts in the
/// same place.
const LABEL_WIDTH: Pixels = px(96.);

/// What the form tells the page.
pub enum FormEvent {
    /// Save was pressed, with the profile as edited.
    Saved(Box<ConnectionProfile>),
}

/// Where Test connection is.
#[derive(Default)]
enum TestState {
    #[default]
    Idle,
    Running,
    Passed,
    Failed(DbError),
}

/// What the last press of Fill from URI did. Held rather than rendered on the
/// spot so the answer survives the repaints that follow it, and cleared the
/// moment the engine picker or a Save moves the form on.
#[derive(Default)]
enum UriState {
    #[default]
    Idle,
    /// The form was filled in. Carries what the URI said that dodo did not
    /// apply, so the notice can name it.
    Filled {
        ignored: Vec<String>,
        tls_unsupported: bool,
    },
    Failed(UriError),
}

pub struct ConnectionForm {
    /// The profile being edited. Every field but the text ones is edited here
    /// directly; the text ones live in their `InputState` until Save.
    profile: ConnectionProfile,

    /// The pasted URI. Not part of the profile and never saved: it is an input
    /// to the form, not a field of a connection.
    uri: Entity<InputState>,
    uri_state: UriState,

    name: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    database: Entity<InputState>,
    user: Entity<InputState>,
    password: Entity<InputState>,
    file: Entity<InputState>,

    password_revealed: bool,
    test: TestState,
    test_task: Option<Task<()>>,
    /// The language the placeholders were built for. `InputState::placeholder`
    /// takes its text at construction and caches it, so switching language has
    /// to push new ones in — the same trap `docker`'s search boxes have.
    language: Language,
}

impl EventEmitter<FormEvent> for ConnectionForm {}

/// Opens the dialog for `profile`.
///
/// `editing` only chooses the title; a new connection and an existing one are
/// the same form, because they are the same thing.
pub fn open(
    profile: ConnectionProfile,
    editing: bool,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ConnectionForm> {
    let form = cx.new(|cx| ConnectionForm::new(profile, window, cx));

    let body = form.clone();
    let title = if editing {
        t(db_connection::Text::EditConnectionTitle, cx)
    } else {
        t(db_connection::Text::NewConnection, cx)
    };

    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(title.clone())
            .w(DIALOG_WIDTH)
            // `.content()` rather than `.child()`: a plain child is wrapped in
            // an `overflow_y_scrollbar` box, and the stated width is what keeps
            // the body from content-sizing itself.
            .content({
                let body = body.clone();
                move |content, _, _| {
                    content.child(div().w(DIALOG_WIDTH - DIALOG_PADDING).child(body.clone()))
                }
            })
    });

    form
}

impl ConnectionForm {
    fn new(profile: ConnectionProfile, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name_placeholder = t(db_connection::Text::FieldNamePlaceholder, cx);
        let file_placeholder = t(db_connection::Text::FieldFilePlaceholder, cx);
        let uri_placeholder = t(db_connection::Text::FieldUriPlaceholder, cx);

        let text = |value: &str, window: &mut Window, cx: &mut Context<Self>| {
            let value = value.to_string();
            cx.new(|cx| InputState::new(window, cx).default_value(value))
        };

        Self {
            uri: cx.new(|cx| InputState::new(window, cx).placeholder(uri_placeholder)),
            uri_state: UriState::Idle,
            name: {
                let value = profile.name.clone();
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(name_placeholder)
                        .default_value(value)
                })
            },
            host: text(&profile.host, window, cx),
            port: text(&profile.port.to_string(), window, cx),
            database: text(&profile.database, window, cx),
            user: text(&profile.user, window, cx),
            password: {
                let value = profile.password.clone();
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .masked(true)
                        .default_value(value)
                })
            },
            file: {
                let value = profile.file.clone();
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(file_placeholder)
                        .default_value(value)
                })
            },
            profile,
            password_revealed: false,
            test: TestState::Idle,
            test_task: None,
            language: Language::current(cx),
        }
    }

    /// The profile as the form currently reads.
    fn collected(&self, cx: &App) -> ConnectionProfile {
        ConnectionProfile {
            name: self.name.read(cx).value().to_string(),
            host: self.host.read(cx).value().to_string(),
            // A port that is not a number is zero, which `ConnectionProfile::
            // problem` reports as missing — better than refusing the keystroke
            // and better than silently keeping the old value.
            port: self.port.read(cx).value().parse().unwrap_or(0),
            database: self.database.read(cx).value().to_string(),
            user: self.user.read(cx).value().to_string(),
            password: self.password.read(cx).value().to_string(),
            file: self.file.read(cx).value().to_string(),
            ..self.profile.clone()
        }
    }

    fn set_engine(&mut self, engine: Engine, window: &mut Window, cx: &mut Context<Self>) {
        if self.profile.engine == engine {
            return;
        }
        let mut next = self.collected(cx);
        next.set_engine(engine);

        // The port is the one text field the engine owns, so it is pushed back
        // into its input rather than left showing the previous engine's.
        let port = next.port.to_string();
        self.port.update(cx, |state, cx| {
            state.set_value(port, window, cx);
        });
        if self.user.read(cx).value().is_empty() && !next.user.is_empty() {
            let user = next.user.clone();
            self.user.update(cx, |state, cx| {
                state.set_value(user, window, cx);
            });
        }

        self.profile = next;
        self.test = TestState::Idle;
        // Whatever the URI said, it does not describe this engine any more.
        self.uri_state = UriState::Idle;
        cx.notify();
    }

    /// Reads the URI field and fills the rest of the form in from it.
    ///
    /// Nothing else happens: no save, no connect, no test. A parse failure
    /// leaves every field exactly as it was — the parser has no partial
    /// result — so the only thing that changes is the message under the row.
    fn fill_from_uri(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.uri.read(cx).value().to_string();
        let parsed = match uri::parse(&input, self.profile.id) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.uri_state = UriState::Failed(error);
                cx.notify();
                return;
            }
        };

        self.apply(&parsed, window, cx);
        self.uri_state = UriState::Filled {
            ignored: parsed.ignored,
            tls_unsupported: parsed.tls_unsupported,
        };
        // The fields are not the ones that were tested any more.
        self.test = TestState::Idle;
        cx.notify();
    }

    /// Pushes a parsed profile into every input, and into the two fields the
    /// profile itself owns (the engine and the TLS mode).
    ///
    /// Every input is written, including the ones this engine does not show:
    /// [`Self::collected`] reads them all, so leaving the previous engine's
    /// host behind would save it.
    fn apply(&mut self, parsed: &ParsedUri, window: &mut Window, cx: &mut Context<Self>) {
        let profile = &parsed.profile;

        // A name the user typed is theirs. Only a blank one is filled in.
        if self.name.read(cx).value().trim().is_empty() {
            self.set_text(&self.name.clone(), parsed.suggested_name(), window, cx);
        }
        self.set_text(&self.host.clone(), profile.host.clone(), window, cx);
        self.set_text(&self.port.clone(), profile.port.to_string(), window, cx);
        self.set_text(&self.database.clone(), profile.database.clone(), window, cx);
        self.set_text(&self.user.clone(), profile.user.clone(), window, cx);
        // The password lands in its field like any other, still masked and
        // still under the storage notice. A pasted one is not treated
        // differently from a typed one.
        self.set_text(&self.password.clone(), profile.password.clone(), window, cx);
        self.set_text(&self.file.clone(), profile.file.clone(), window, cx);

        self.profile = ConnectionProfile {
            name: self.name.read(cx).value().to_string(),
            ..profile.clone()
        };
    }

    fn set_text(
        &self,
        state: &Entity<InputState>,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        state.update(cx, |state, cx| {
            state.set_value(value, window, cx);
        });
    }

    fn set_ssl_mode(&mut self, mode: SslMode, cx: &mut Context<Self>) {
        self.profile.ssl_mode = mode;
        self.test = TestState::Idle;
        cx.notify();
    }

    fn toggle_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.password_revealed = !self.password_revealed;
        let masked = !self.password_revealed;
        self.password.update(cx, |state, cx| {
            state.set_masked(masked, window, cx);
        });
        cx.notify();
    }

    /// Runs the connection attempt on the background executor — never on the UI
    /// thread — and keeps the task so a second press replaces the first.
    fn test_connection(&mut self, cx: &mut Context<Self>) {
        let profile = self.collected(cx);
        self.test = TestState::Running;
        cx.notify();

        self.test_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { services::test_connection(&profile) })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.test = match result {
                    Ok(()) => TestState::Passed,
                    Err(error) => TestState::Failed(error),
                };
                this.test_task = None;
                cx.notify();
            });
        }));
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let profile = self.collected(cx);
        cx.emit(FormEvent::Saved(Box::new(profile)));
        window.close_dialog(cx);
    }

    /// Re-pushes the placeholders after a language change, because
    /// `InputState::placeholder` caches the text it was built with.
    fn sync_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = Language::current(cx);
        if self.language == language {
            return;
        }
        self.language = language;

        let name = t(db_connection::Text::FieldNamePlaceholder, cx);
        self.name.update(cx, |state, cx| {
            state.set_placeholder(name, window, cx);
        });
        let file = t(db_connection::Text::FieldFilePlaceholder, cx);
        self.file.update(cx, |state, cx| {
            state.set_placeholder(file, window, cx);
        });
        // The URI placeholder is an example URI, declared identical in every
        // language by `i18n`'s own `term()` guard, so re-pushing it would be a
        // provable no-op rather than a fix.
    }

    // ---- rendering -------------------------------------------------------

    fn field(&self, label: Str, control: AnyElement, cx: &Context<Self>) -> AnyElement {
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                div()
                    .w(LABEL_WIDTH)
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(label, cx)),
            )
            // `min_w_0` on the growing half: without it the widest control sets
            // the row's width and pushes the dialog's own padding off.
            .child(div().flex_1().min_w_0().child(control))
            .into_any_element()
    }

    fn text_field(&self, label: Str, state: &Entity<InputState>, cx: &Context<Self>) -> AnyElement {
        self.field(label, Input::new(state).small().into_any_element(), cx)
    }

    /// A row of buttons, one per option, with the selected one filled. Used for
    /// both pickers; see the module doc for why this is not a `Select`.
    fn segmented<T: PartialEq + Copy + 'static>(
        &self,
        id: &'static str,
        options: &[(T, Str)],
        selected: T,
        on_pick: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .gap_1()
            .children(options.iter().enumerate().map(|(index, (value, label))| {
                let value = *value;
                let on_pick = on_pick.clone();
                Button::new((id, index))
                    .small()
                    .map(|button| {
                        if value == selected {
                            button.primary()
                        } else {
                            button.outline()
                        }
                    })
                    .label(t(label.clone(), cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        on_pick(this, value, window, cx);
                    }))
            }))
            .into_any_element()
    }

    fn network_fields(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut fields = vec![
            self.text_field(
                db_connection::Text::FieldHost.into(),
                &self.host.clone(),
                cx,
            ),
            self.text_field(
                db_connection::Text::FieldPort.into(),
                &self.port.clone(),
                cx,
            ),
            self.text_field(
                db_connection::Text::FieldDatabase.into(),
                &self.database.clone(),
                cx,
            ),
            self.text_field(
                db_connection::Text::FieldUser.into(),
                &self.user.clone(),
                cx,
            ),
            self.password_field(cx),
        ];
        if self.profile.engine.supports_tls() {
            fields.push(self.field(
                db_connection::Text::FieldSsl.into(),
                self.segmented(
                    "db-ssl",
                    &SslMode::ALL.map(|mode| (mode, ssl_label(mode))),
                    self.profile.ssl_mode,
                    |this, mode, _, cx| this.set_ssl_mode(mode, cx),
                    cx,
                ),
                cx,
            ));
        }
        fields
    }

    fn password_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let revealed = self.password_revealed;
        self.field(
            db_connection::Text::FieldPassword.into(),
            h_flex()
                .w_full()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&self.password).small()),
                )
                .child(
                    Button::new("db-reveal-password")
                        .ghost()
                        .small()
                        .icon(if revealed {
                            AppIcon::EyeOff
                        } else {
                            AppIcon::Eye
                        })
                        .tooltip(if revealed {
                            t(db_connection::Text::HidePassword, cx)
                        } else {
                            t(db_connection::Text::RevealPassword, cx)
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_password(window, cx);
                        })),
                )
                .into_any_element(),
            cx,
        )
    }

    /// The URI row and whatever the last press of its button had to say.
    fn uri_row(&self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .w_full()
            .gap_2()
            .child(
                self.field(
                    db_connection::Text::FieldUri.into(),
                    h_flex()
                        .w_full()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&self.uri).small()),
                        )
                        .child(
                            Button::new("db-fill-from-uri")
                                .small()
                                .outline()
                                .label(t(db_connection::Text::FillFromUri, cx))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.fill_from_uri(window, cx);
                                })),
                        )
                        .into_any_element(),
                    cx,
                ),
            )
            .map(|this| match &self.uri_state {
                UriState::Idle => this,
                UriState::Failed(error) => {
                    this.child(notice(Tone::Danger, t(error.message(), cx), cx))
                }
                UriState::Filled {
                    ignored,
                    tls_unsupported,
                } => this
                    .child(notice(
                        Tone::Success,
                        t(db_connection::Text::UriFilled, cx),
                        cx,
                    ))
                    .when(!ignored.is_empty(), |this| {
                        // Named, not dropped: the user gets to see that dodo
                        // read the parameter and chose not to act on it.
                        this.child(notice(
                            Tone::Warning,
                            t(db_connection::Text::UriIgnored(ignored.join(", ")), cx),
                            cx,
                        ))
                    })
                    .when(*tls_unsupported, |this| {
                        this.child(notice(
                            Tone::Warning,
                            t(db_connection::Text::UriTlsNotApplied, cx),
                            cx,
                        ))
                    }),
            })
            .into_any_element()
    }

    fn file_fields(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        vec![self.text_field(
            db_connection::Text::FieldFile.into(),
            &self.file.clone(),
            cx,
        )]
    }

    fn test_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let running = matches!(self.test, TestState::Running);
        let label: SharedString = if running {
            t(db_connection::Text::Testing, cx)
        } else {
            t(db_connection::Text::TestConnection, cx)
        };

        v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex().w_full().child(
                    Button::new("db-test-connection")
                        .small()
                        .outline()
                        .disabled(running)
                        .label(label)
                        .on_click(cx.listener(|this, _, _, cx| this.test_connection(cx))),
                ),
            )
            .map(|this| match &self.test {
                TestState::Passed => this.child(notice(
                    Tone::Success,
                    t(db_connection::Text::TestSucceeded, cx),
                    cx,
                )),
                TestState::Failed(error) => {
                    this.child(notice(Tone::Danger, t(error.message(), cx), cx))
                }
                TestState::Idle | TestState::Running => this,
            })
            .into_any_element()
    }
}

impl Render for ConnectionForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_language(window, cx);

        let engine = self.profile.engine;
        let address = engine.address();
        let fields = match address {
            Address::Network => self.network_fields(cx),
            Address::File => self.file_fields(cx),
        };
        let problem = self.collected(cx).problem();

        v_flex()
            .w_full()
            .gap_3()
            .child(self.uri_row(cx))
            .child(self.text_field(
                db_connection::Text::FieldName.into(),
                &self.name.clone(),
                cx,
            ))
            .child(self.render_engine_picker(engine, cx))
            .children(fields)
            .when(address == Address::Network, |this| {
                // Never hidden, never behind a disclosure: the password is
                // stored unencrypted and the user is told every time.
                this.child(notice(
                    Tone::Info,
                    t(db_connection::Text::PasswordStorageNotice, cx),
                    cx,
                ))
            })
            .child(self.test_row(cx))
            .when_some(problem, |this, problem| {
                this.child(notice(Tone::Warning, t(problem.message(), cx), cx))
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("db-form-cancel")
                            .small()
                            .ghost()
                            .label(t(db_connection::Text::Cancel, cx))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("db-form-save")
                            .small()
                            .primary()
                            .label(t(db_connection::Text::Save, cx))
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}

impl ConnectionForm {
    /// The engine picker. Its own function rather than [`Self::segmented`]
    /// because a product name is a proper noun, not a [`Str`] — "PostgreSQL"
    /// reads the same in every language and has no translation to look up.
    fn render_engine_picker(&self, selected: Engine, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                div()
                    .w(LABEL_WIDTH)
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(db_connection::Text::FieldEngine, cx)),
            )
            .child(h_flex().flex_1().min_w_0().gap_1().children(
                Engine::ALL.into_iter().enumerate().map(|(index, engine)| {
                    Button::new(("db-engine", index))
                        .small()
                        .map(|button| {
                            if engine == selected {
                                button.primary()
                            } else {
                                button.outline()
                            }
                        })
                        .label(SharedString::new_static(engine.display_name()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_engine(engine, window, cx);
                        }))
                }),
            ))
            .into_any_element()
    }
}
