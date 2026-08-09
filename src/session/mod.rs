//! Session restoration: dodo comes back the way it was left.
//!
//! The captain asked for this on 2026-08-06 — *"setting is not keep when reopen
//! app, save the setting and remember open window size, current tab, to make
//! sure it will be there when reopen app"* — and it overturns the sentence at
//! the top of [`crate::settings`], which said nothing was persisted. Not all of
//! it: see *What still resets* below, which is the half of the old design that
//! was right.
//!
//! [`models::document`] is the file, [`models::geometry`] and
//! [`models::features`] are the two hard parts of it, and
//! [`services::session_store`] is the seam onto disk. This file is the global
//! that holds the live document and decides when it is written.
//!
//! # One file, not three
//!
//! `session.json` carries the appearance settings, the window rectangle, the
//! open tool and the sidebar's tool list together, and that is a deliberate
//! choice over one file each:
//!
//! - They are **read at the same instant** — all of them before the window
//!   exists, because the theme has to be applied and the geometry known before
//!   the first frame. Three files would be three loads to sequence on the path
//!   that decides how fast dodo opens. The tool list joined them for the same
//!   reason and one more: which tool to open and whether that tool is still
//!   visible are one question, and answering it from two files that can
//!   disagree is how a window opens on a tool with no sidebar row.
//! - They are **written at the same instant**. Picking a theme in the Settings
//!   dialog and dragging the window are both "the user arranged their session";
//!   one coalescing timer covers both, where three stores would each need their
//!   own.
//! - One atomic rename means the restored session is **internally consistent**.
//!   Three files can disagree after a crash mid-write, and a window restored to
//!   a tool that a half-written second file no longer names is a bug with no
//!   good recovery.
//!
//! The counter-argument — that a corrupt `session.json` loses all three at once
//! — is real and is exactly what the version check answers: the file is refused
//! whole and dodo opens on its defaults, which is what losing one of three
//! would have done anyway.
//!
//! # What still resets, and why that is not an oversight
//!
//! **The Run scripts policy is not persisted.** It goes back to the cautious
//! `Ask for imported` at every launch. It is not a preference that happens to
//! live in the Settings dialog; it is the gate in front of running code that
//! arrived inside someone else's collection file, and "I allowed this once" is
//! not "allow it every morning from now on". The approvals themselves *are*
//! persisted, per script, in `script-consent.json` — which is the right
//! granularity for that memory. `models::document`'s module doc states the same
//! thing, and changing it is the captain's call.
//!
//! Also unrestored, for a much smaller reason: the Docker rail's page and the
//! Database and API Explorer tabs. The captain asked for the current *tool*.
//!
//! # Coalescing
//!
//! A resize drag emits a geometry change every frame. Writing on each would be
//! hundreds of writes for one gesture, so a change **schedules** a save rather
//! than performing one, and each new change replaces the pending task — a
//! `Task` cancels when it is dropped, so only the last change in a burst
//! reaches the disk. This is the shape [`crate::quick_nav::QuickNav::set_pattern`]
//! already used for per-keystroke pattern edits; [`SAVE_DELAY`] is shorter here
//! because a drag has no natural end the way releasing a key does.
//!
//! The hole a trailing-edge debounce leaves is *quit within the delay*, which
//! is exactly what someone who resizes and then closes the window does. It is
//! closed by [`flush_on_quit`], registered with `App::on_app_quit`: gpui awaits
//! those futures before the process ends, so the pending document still gets
//! written — on the background executor, like every other write here.
//!
//! # An unreadable file is never overwritten
//!
//! If the load failed — a version from a newer dodo, or an unreadable file —
//! dodo **stops saving** for the rest of the run. Refusing to read a newer
//! file and then flattening it on the first window move would make the refusal
//! pointless. The Settings dialog says so; see
//! [`services::session_store`]'s module doc.

pub mod models;
pub mod services;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AsyncApp, BorrowAppContext as _, Bounds, DisplayId, Global, Pixels, PlatformDisplay, Task,
    WindowBounds, px, size,
};

use crate::i18n::Str;
use crate::session::models::document::{SessionDocument, ToolRecord, WindowMode, WindowRecord};
use crate::session::models::geometry;
use crate::session::services::session_store::{DiskSessionStore, SessionStore, SessionStoreError};

/// How long a change waits before it is written.
///
/// Shorter than quick navigation's 600ms because the burst this coalesces is a
/// pointer drag rather than typing: frames arrive ~16ms apart, so 400ms of
/// quiet reliably means the gesture is over, and the shorter the delay the less
/// a hard kill can lose.
const SAVE_DELAY: Duration = Duration::from_millis(400);

/// The live session: what will be saved, and the seam onto the file.
pub struct Session {
    document: SessionDocument,
    store: Arc<dyn SessionStore>,
    /// The pending coalesced save, if any. Dropping it cancels it.
    save: Option<Task<()>>,
    /// Bumped on every accepted change, and copied into `saved_revision` once a
    /// write of that revision succeeds. A counter rather than a `dirty` flag
    /// because a change can land while a write is in flight, and clearing a
    /// flag on completion would forget it.
    revision: u64,
    saved_revision: u64,
    /// Set when **loading** failed, which makes the file untouchable for the
    /// rest of the run. See the module doc.
    read_failed: bool,
    /// What went wrong reading or writing, for the Settings dialog to show.
    store_error: Option<SessionStoreError>,
}

impl Global for Session {}

impl Session {
    fn new(store: Arc<dyn SessionStore>) -> Self {
        Self {
            document: SessionDocument::new(),
            store,
            save: None,
            revision: 0,
            saved_revision: 0,
            read_failed: false,
            store_error: None,
        }
    }

    /// The appearance choices the user has made, if any. `None` per field means
    /// *never chosen*, which is not the same as choosing the default — see
    /// [`models::document`].
    pub fn language(cx: &App) -> Option<String> {
        Self::read(cx, |document| document.appearance.language.clone())
    }

    pub fn theme(cx: &App) -> Option<String> {
        Self::read(cx, |document| document.appearance.theme.clone())
    }

    pub fn font_size(cx: &App) -> Option<f32> {
        Self::read(cx, |document| document.appearance.font_size)
    }

    pub fn border_radius(cx: &App) -> Option<f32> {
        Self::read(cx, |document| document.appearance.border_radius)
    }

    /// The tool that was open, by [`crate::layout::View`] code. An unknown code
    /// is the caller's to fall back on.
    pub fn active_tool(cx: &App) -> Option<String> {
        Self::read(cx, |document| document.workspace.active_tool.clone())
    }

    pub fn sidebar_collapsed(cx: &App) -> Option<bool> {
        Self::read(cx, |document| document.workspace.sidebar_collapsed)
    }

    /// The tool list as the **file** holds it: an order and a visibility flag
    /// each, or `None` before the user has ever opened the Features page.
    ///
    /// Deliberately raw. Turning it into something the sidebar can be built
    /// from means placing it against the tools this build actually has, and
    /// that is [`models::features::Features::resolve`]'s job — done once, in
    /// `Layout`, rather than by every caller in its own way.
    pub fn tools(cx: &App) -> Option<Vec<ToolRecord>> {
        Self::read(cx, |document| document.workspace.tools.clone())
    }

    pub fn set_tools(tools: Vec<ToolRecord>, cx: &mut App) {
        Self::edit(cx, |document| document.workspace.tools = Some(tools));
    }

    /// A keyboard language saved by a pre-IPC tray build.
    ///
    /// The menu now reads `input-method.json`; this is migration input only and
    /// is never written by current builds.
    pub fn legacy_input_language(cx: &App) -> Option<String> {
        Self::read(cx, |document| document.tray.input_language.clone())
    }

    /// What went wrong with `session.json`, if anything.
    pub fn store_error(cx: &App) -> Option<Str> {
        cx.try_global::<Session>()
            .and_then(|state| state.store_error.as_ref().map(SessionStoreError::message))
    }

    /// Where to open the window, or `None` to use the caller's default.
    ///
    /// This is the whole of the geometry decision as far as the rest of dodo is
    /// concerned: the saved rectangle is placed against the displays that exist
    /// *now* by [`models::geometry::place_record`], so an unplugged monitor, a
    /// changed resolution and a size below the layout's minimum are all already
    /// dealt with by the time a `WindowBounds` comes back.
    ///
    /// The display the window was on comes first in the list handed to `place`,
    /// which is what makes it both the preferred display and the one an
    /// unplaceable rectangle is centred on. Gone, the primary takes its place.
    pub fn window_bounds(cx: &App) -> Option<WindowBounds> {
        let record = Self::read(cx, |document| document.window.clone())?;
        let bounds = geometry::place_record(
            &record,
            &displays(record.display.as_deref(), cx),
            crate::layout::window_min_size(),
        )?;

        Some(match record.mode {
            WindowMode::Windowed => WindowBounds::Windowed(bounds),
            WindowMode::Maximized => WindowBounds::Maximized(bounds),
            WindowMode::Fullscreen => WindowBounds::Fullscreen(bounds),
        })
    }

    /// The display to open on, for `WindowOptions::display_id`.
    ///
    /// **Not a nicety — on macOS it is the only thing that can put the window
    /// back on the right monitor.** Every macOS coordinate in the pinned gpui is
    /// display-*local*, so the saved rectangle says where on a screen the window
    /// was and nothing about which screen; [`models::document::WindowRecord`]
    /// has the four functions that prove it. Pairing the rectangle with the
    /// display named here is what makes it mean the same thing on the way back
    /// in as it did on the way out.
    ///
    /// `None` — no saved display, or one that is not attached any more — leaves
    /// gpui on the primary display, which is the unplugged-monitor case.
    pub fn window_display(cx: &App) -> Option<DisplayId> {
        let uuid = Self::read(cx, |document| {
            document.window.as_ref().and_then(|w| w.display.clone())
        })?;
        display_by_uuid(&uuid, cx).map(|display| display.id())
    }

    pub fn set_language(code: impl Into<String>, cx: &mut App) {
        let code = code.into();
        Self::edit(cx, |document| document.appearance.language = Some(code));
    }

    pub fn set_theme(name: impl Into<String>, cx: &mut App) {
        let name = name.into();
        Self::edit(cx, |document| document.appearance.theme = Some(name));
    }

    pub fn set_font_size(size: f32, cx: &mut App) {
        Self::edit(cx, |document| document.appearance.font_size = Some(size));
    }

    pub fn set_border_radius(radius: f32, cx: &mut App) {
        Self::edit(cx, |document| {
            document.appearance.border_radius = Some(radius)
        });
    }

    pub fn set_active_tool(code: &'static str, cx: &mut App) {
        Self::edit(cx, |document| {
            document.workspace.active_tool = Some(code.to_owned());
        });
    }

    pub fn set_sidebar_collapsed(collapsed: bool, cx: &mut App) {
        Self::edit(cx, |document| {
            document.workspace.sidebar_collapsed = Some(collapsed);
        });
    }

    /// Records where the window is now. Called from a bounds observer, so this
    /// is the burst [`SAVE_DELAY`] exists for.
    ///
    /// The rectangle stored is gpui's **restore** rectangle in every mode: for
    /// a maximized or fullscreen window that is the size it returns to, which
    /// is what makes unzooming after a restart land somewhere sensible. It is
    /// stored **with the display it is measured against** — see
    /// [`Session::window_display`] for why the rectangle is meaningless without
    /// it on macOS.
    pub fn set_window(bounds: WindowBounds, display: Option<String>, cx: &mut App) {
        let (mode, rect) = match bounds {
            WindowBounds::Windowed(rect) => (WindowMode::Windowed, rect),
            WindowBounds::Maximized(rect) => (WindowMode::Maximized, rect),
            WindowBounds::Fullscreen(rect) => (WindowMode::Fullscreen, rect),
        };
        let (x, y, width, height) = geometry::record_of(rect);

        Self::edit(cx, |document| {
            document.window = Some(WindowRecord {
                mode,
                x,
                y,
                width,
                height,
                display,
            });
        });
    }

    fn read<T>(cx: &App, get: impl FnOnce(&SessionDocument) -> T) -> T
    where
        T: Default,
    {
        cx.try_global::<Session>()
            .map(|state| get(&state.document))
            .unwrap_or_default()
    }

    /// Applies one change and schedules the save.
    ///
    /// Two things it declines to do, both of which would otherwise show up as
    /// disk traffic nobody asked for: it does nothing at all while the file is
    /// untouchable (see the module doc), and it does nothing when the change
    /// left the document exactly as it was — which is the common case for a
    /// sidebar click on the tool that is already open.
    fn edit(cx: &mut App, change: impl FnOnce(&mut SessionDocument)) {
        if cx.try_global::<Session>().is_none() {
            return;
        }
        cx.update_global::<Session, _>(|state, cx| {
            if state.read_failed {
                return;
            }

            let before = state.document.clone();
            change(&mut state.document);
            if state.document == before {
                return;
            }

            state.revision += 1;
            let revision = state.revision;
            let store = state.store.clone();
            let document = state.document.clone();

            state.save = Some(cx.spawn(async move |cx| {
                cx.background_executor().timer(SAVE_DELAY).await;
                let result = cx
                    .background_executor()
                    .spawn(async move { store.persist(&document) })
                    .await;

                cx.update(|cx| match result {
                    Ok(()) => Self::mark_saved(revision, cx),
                    Err(error) => Self::report(error, cx),
                });
            }));
        });
    }

    fn mark_saved(revision: u64, cx: &mut App) {
        if cx.try_global::<Session>().is_none() {
            return;
        }
        cx.update_global::<Session, _>(|state, _| {
            state.saved_revision = state.saved_revision.max(revision);
            state.store_error = None;
        });
    }

    fn report(error: SessionStoreError, cx: &mut App) {
        eprintln!("session.json: {error:?}");
        if cx.try_global::<Session>().is_some() {
            cx.update_global::<Session, _>(|state, _| state.store_error = Some(error));
        }
        cx.refresh_windows();
    }

    /// Adopts what the store read at launch, or refuses to touch the file.
    fn adopt(loaded: Result<SessionDocument, SessionStoreError>, cx: &mut App) {
        if cx.try_global::<Session>().is_none() {
            return;
        }
        cx.update_global::<Session, _>(|state, _| match loaded {
            Ok(document) => {
                state.document = document;
                state.read_failed = false;
                state.store_error = None;
            }
            Err(error) => {
                eprintln!("session.json: {error:?}");
                state.read_failed = true;
                state.store_error = Some(error);
            }
        });
    }
}

/// The display with this UUID, if it is still attached.
fn display_by_uuid(uuid: &str, cx: &App) -> Option<Rc<dyn PlatformDisplay>> {
    cx.displays()
        .into_iter()
        .find(|display| display.uuid().is_ok_and(|found| found.to_string() == uuid))
}

/// Every attached display's usable area, **most preferred first**.
///
/// The order is [`models::geometry::place`]'s contract — the first entry is the
/// display a window with nowhere else to go is centred on, and `App::displays`
/// promises no order of its own — so `remembered` leads when it is still
/// attached, and the primary display otherwise.
///
/// `visible_bounds` rather than `bounds` excludes the macOS menu bar and the
/// Windows taskbar, so a clamped window lands under neither; it is what gpui's
/// own `default_bounds` uses.
fn displays(remembered: Option<&str>, cx: &App) -> Vec<Bounds<Pixels>> {
    let first = remembered
        .and_then(|uuid| display_by_uuid(uuid, cx))
        .or_else(|| cx.primary_display());
    let first_id = first.as_ref().map(|display| display.id());

    first
        .iter()
        .map(|display| display.visible_bounds())
        .chain(
            cx.displays()
                .iter()
                .filter(|display| Some(display.id()) != first_id)
                .map(|display| display.visible_bounds()),
        )
        .collect()
}

/// Installs the session global and the quit-time flush.
///
/// Nothing is read here: [`load`] does that, and it has to be awaited before
/// the window opens so the theme and the geometry are known for the first
/// frame. Same post-`gpui_component::init` position as every other `init` in
/// `main.rs`, though this one binds no keys.
pub fn init(cx: &mut App) {
    cx.set_global(Session::new(Arc::new(DiskSessionStore::new())));
    cx.on_app_quit(flush_on_quit).detach();
}

/// Reads `session.json` on the background executor and adopts it.
///
/// Awaited from `main` before the window is opened. A failure leaves the
/// defaults in place — dodo opens exactly as it did before this feature — and
/// stops the file being written for the rest of the run.
pub async fn load(cx: &mut AsyncApp) {
    let store = cx.update(|cx| cx.global::<Session>().store.clone());

    let loaded = cx
        .background_executor()
        .spawn(async move { store.load() })
        .await;

    cx.update(|cx| Session::adopt(loaded, cx));
}

/// Writes the pending document, if there is one, before the process ends.
///
/// gpui builds these futures while the app is still whole and then awaits them
/// with a timeout, so everything the write needs — the store, the document, the
/// executor — is taken **now**, synchronously, and the future itself touches no
/// `App`. That is also what keeps the write off the UI thread: the future
/// hands it to the background executor and awaits that.
fn flush_on_quit(cx: &mut App) -> impl Future<Output = ()> + use<> {
    let executor = cx.background_executor().clone();
    let pending = cx.try_global::<Session>().and_then(|state| {
        let unsaved = state.revision != state.saved_revision;
        (unsaved && !state.read_failed).then(|| (state.store.clone(), state.document.clone()))
    });

    async move {
        let Some((store, document)) = pending else {
            return;
        };
        if let Err(error) = executor
            .spawn(async move { store.persist(&document) })
            .await
        {
            eprintln!("session.json: {error:?}");
        }
    }
}

/// The default window dodo opens when nothing has been saved — and the size the
/// restored one is measured against.
pub fn default_window_bounds(cx: &App) -> WindowBounds {
    WindowBounds::centered(size(px(900.), px(620.)), cx)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::{Bounds, Pixels, TestAppContext, WindowBounds, point, px, size};

    use super::{SAVE_DELAY, Session, flush_on_quit};
    use crate::session::models::document::{SessionDocument, ToolRecord, WindowMode, WindowRecord};
    use crate::session::services::session_store::{
        InMemorySessionStore, SessionStore as _, SessionStoreError,
    };

    /// Installs a session backed by an in-memory store, having "loaded"
    /// `loaded`.
    fn install(
        cx: &mut TestAppContext,
        loaded: Result<SessionDocument, SessionStoreError>,
    ) -> Arc<InMemorySessionStore> {
        let store = Arc::new(InMemorySessionStore::default());
        let handle = store.clone();
        cx.update(|cx| {
            cx.set_global(Session::new(handle));
            Session::adopt(loaded, cx);
        });
        store
    }

    /// Lets the coalescing timer fire. The test clock does not move on its own,
    /// so `run_until_parked` alone would only ever see the *pending* save.
    fn settle(cx: &mut TestAppContext) {
        cx.executor().advance_clock(SAVE_DELAY * 2);
        cx.run_until_parked();
    }

    #[gpui::test]
    fn a_change_reaches_the_store_once_the_delay_has_passed(cx: &mut TestAppContext) {
        let store = install(cx, Ok(SessionDocument::new()));

        cx.update(|cx| Session::set_theme("Ayu Dark", cx));
        assert_eq!(store.writes(), 0, "nothing may be written on the spot");

        settle(cx);
        assert_eq!(
            store.load().expect("loads").appearance.theme.as_deref(),
            Some("Ayu Dark"),
        );
    }

    /// The coalescing claim, as the disk sees it: a burst of changes is **one**
    /// write, holding the last of them.
    #[gpui::test]
    fn a_burst_of_changes_is_written_once(cx: &mut TestAppContext) {
        let store = install(cx, Ok(SessionDocument::new()));

        cx.update(|cx| {
            for size in [14., 16., 18., 14., 18.] {
                Session::set_font_size(size, cx);
            }
        });
        settle(cx);

        assert_eq!(store.writes(), 1, "one burst has to be one write");
        assert_eq!(store.load().expect("loads").appearance.font_size, Some(18.));
    }

    /// The rule that makes refusing a newer file mean anything: a session dodo
    /// could not read is a session dodo does not write over.
    #[gpui::test]
    async fn an_unreadable_file_is_never_written_back(cx: &mut TestAppContext) {
        let store = install(
            cx,
            Err(SessionStoreError::UnsupportedVersion {
                found: 99,
                understood: 1,
            }),
        );

        cx.update(|cx| {
            Session::set_theme("Ayu Dark", cx);
            Session::set_font_size(18., cx);
        });
        settle(cx);

        assert_eq!(store.writes(), 0, "the store must not have been touched");
        assert!(cx.update(|cx| Session::store_error(cx)).is_some());

        // …and the quit flush must not undo the refusal either, which is the
        // path that would otherwise flatten a newer dodo's file on the way out.
        cx.update(flush_on_quit).await;
        assert_eq!(store.writes(), 0);
    }

    /// Closing the window within the coalescing delay is the ordinary way to
    /// quit, so it must not be the way to lose the last change.
    #[gpui::test]
    async fn quitting_inside_the_delay_still_writes(cx: &mut TestAppContext) {
        let store = install(cx, Ok(SessionDocument::new()));

        let flush = cx.update(|cx| {
            Session::set_language("vi", cx);
            flush_on_quit(cx)
        });
        flush.await;

        assert_eq!(
            store.load().expect("loads").appearance.language.as_deref(),
            Some("vi"),
        );
    }

    /// …and a session with nothing pending writes nothing, so quitting does not
    /// touch the disk for its own sake.
    #[gpui::test]
    async fn quitting_with_nothing_pending_writes_nothing(cx: &mut TestAppContext) {
        let store = install(cx, Ok(SessionDocument::new()));

        cx.update(|cx| Session::set_border_radius(0., cx));
        settle(cx);
        assert_eq!(store.writes(), 1);

        cx.update(|cx| {
            // The same value again: no change, so nothing to flush.
            Session::set_border_radius(0., cx)
        });
        cx.update(flush_on_quit).await;

        assert_eq!(
            store.writes(),
            1,
            "quitting must not write for the sake of writing",
        );
    }

    /// A burst of *geometry* changes is the case the delay really exists for:
    /// a resize drag emits one every frame, and it must still be one write.
    #[gpui::test]
    fn a_resize_drag_is_written_once(cx: &mut TestAppContext) {
        let store = install(cx, Ok(SessionDocument::new()));

        cx.update(|cx| {
            for width in [800., 810., 820., 830., 900.] {
                Session::set_window(
                    WindowBounds::Windowed(Bounds {
                        origin: point(px(0.), px(0.)),
                        size: size(px(width), px(620.)),
                    }),
                    None,
                    cx,
                );
            }
        });
        settle(cx);

        assert_eq!(store.writes(), 1, "one gesture has to be one write");
        let window = store.load().expect("loads").window.expect("a window");
        assert_eq!(window.width, 900., "the last change is the one that lands");
        assert_eq!(window.mode, WindowMode::Windowed);
    }

    /// A maximized or fullscreen window keeps its **restore** rectangle, so
    /// unzooming after a restart lands where it did before.
    #[gpui::test]
    fn the_window_mode_survives_with_its_restore_rectangle(cx: &mut TestAppContext) {
        let store = install(cx, Ok(SessionDocument::new()));
        let restore = Bounds {
            origin: point(px(60.), px(40.)),
            size: size(px(900.), px(620.)),
        };

        cx.update(|cx| Session::set_window(WindowBounds::Fullscreen(restore), None, cx));
        settle(cx);

        let window = store.load().expect("loads").window.expect("a window");
        assert_eq!(window.mode, WindowMode::Fullscreen);
        assert_eq!((window.x, window.y), (60., 40.));
        assert_eq!((window.width, window.height), (900., 620.));
    }

    #[gpui::test]
    fn with_no_saved_window_there_is_nothing_to_restore(cx: &mut TestAppContext) {
        install(cx, Ok(SessionDocument::new()));
        assert!(cx.update(|cx| Session::window_bounds(cx)).is_none());
    }

    /// A session holding `record` was loaded.
    fn with_window(record: WindowRecord) -> Result<SessionDocument, SessionStoreError> {
        let mut document = SessionDocument::new();
        document.window = Some(record);
        Ok(document)
    }

    /// End to end through the global, on gpui's test display (1920x1080 at the
    /// origin): the rectangle comes back untouched and in the mode it was saved
    /// in. `models::geometry` argues each placement rule on its own; this is the
    /// wiring above it.
    #[gpui::test]
    fn a_saved_window_is_restored_in_the_mode_it_was_saved_in(cx: &mut TestAppContext) {
        for (mode, expected) in [
            (
                WindowMode::Windowed,
                WindowBounds::Windowed as fn(Bounds<Pixels>) -> WindowBounds,
            ),
            (WindowMode::Maximized, WindowBounds::Maximized),
            (WindowMode::Fullscreen, WindowBounds::Fullscreen),
        ] {
            install(
                cx,
                with_window(WindowRecord {
                    mode,
                    x: 100.,
                    y: 60.,
                    width: 1000.,
                    height: 700.,
                    display: None,
                }),
            );

            let rect = Bounds {
                origin: point(px(100.), px(60.)),
                size: size(px(1000.), px(700.)),
            };
            assert_eq!(
                cx.update(|cx| Session::window_bounds(cx)),
                Some(expected(rect)),
                "{mode:?} did not survive the round trip",
            );
        }
    }

    /// The awkward case the captain will want to try: quit on a second display,
    /// unplug it, reopen. The window must not come back where the monitor was.
    #[gpui::test]
    fn a_window_saved_on_a_display_that_is_gone_comes_back_on_screen(cx: &mut TestAppContext) {
        install(
            cx,
            with_window(WindowRecord {
                mode: WindowMode::Windowed,
                x: 3000.,
                y: 400.,
                width: 1000.,
                height: 700.,
                display: None,
            }),
        );

        let restored = cx
            .update(|cx| Session::window_bounds(cx))
            .expect("something to open");
        let rect = restored.get_bounds();

        assert_eq!(rect.size, size(px(1000.), px(700.)), "the size is kept");
        assert!(
            rect.origin.x >= px(0.) && rect.right() <= px(1920.),
            "{rect:?} is off the only display there is",
        );
        assert!(rect.origin.y >= px(0.) && rect.bottom() <= px(1080.));
    }

    /// A hand-edited or corrupt rectangle is "no saved geometry", so `main`
    /// opens its own default window rather than a zero-sized one.
    #[gpui::test]
    fn an_impossible_saved_rectangle_leaves_the_default_window(cx: &mut TestAppContext) {
        install(
            cx,
            with_window(WindowRecord {
                mode: WindowMode::Windowed,
                x: 0.,
                y: 0.,
                width: 0.,
                height: 0.,
                display: None,
            }),
        );
        assert!(cx.update(|cx| Session::window_bounds(cx)).is_none());
    }

    #[gpui::test]
    fn the_open_tool_reaches_the_store(cx: &mut TestAppContext) {
        let store = install(cx, Ok(SessionDocument::new()));

        cx.update(|cx| {
            Session::set_active_tool("docker", cx);
            Session::set_sidebar_collapsed(false, cx);
        });
        settle(cx);

        let workspace = store.load().expect("loads").workspace;
        assert_eq!(workspace.active_tool.as_deref(), Some("docker"));
        assert_eq!(workspace.sidebar_collapsed, Some(false));
    }

    /// The Features page's two changes reach the file, in order and with each
    /// tool's own flag. `models::features` argues the rules; this is the wiring
    /// above them.
    #[gpui::test]
    fn the_tool_list_reaches_the_store(cx: &mut TestAppContext) {
        let store = install(cx, Ok(SessionDocument::new()));

        cx.update(|cx| {
            Session::set_tools(
                vec![
                    ToolRecord {
                        code: "docker".to_owned(),
                        enabled: true,
                    },
                    ToolRecord {
                        code: "cleaner".to_owned(),
                        enabled: false,
                    },
                ],
                cx,
            )
        });
        settle(cx);

        let tools = store
            .load()
            .expect("loads")
            .workspace
            .tools
            .expect("a list");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].code, "docker");
        assert!(!tools[1].enabled);
    }

    /// Before the Features page has ever been opened there is nothing stored,
    /// and that is what `Features::resolve` reads as "every tool, in order".
    #[gpui::test]
    fn an_untouched_session_has_no_tool_list(cx: &mut TestAppContext) {
        install(cx, Ok(SessionDocument::new()));
        assert_eq!(cx.update(|cx| Session::tools(cx)), None);
    }

    /// Clicking the tool already open is the common case, and it must not cost
    /// a write. `layout::View::shown` is where an unknown or switched-off code
    /// is turned back into a tool; that fallback is tested there.
    #[gpui::test]
    fn re_selecting_the_open_tool_writes_nothing(cx: &mut TestAppContext) {
        let store = install(cx, Ok(SessionDocument::new()));

        cx.update(|cx| Session::set_active_tool("docker", cx));
        settle(cx);
        assert_eq!(store.writes(), 1);

        for _ in 0..5 {
            cx.update(|cx| Session::set_active_tool("docker", cx));
            settle(cx);
        }
        assert_eq!(store.writes(), 1, "an unchanged document is not a change");
    }
}
