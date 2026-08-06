//! Session restoration: dodo comes back the way it was left.
//!
//! The captain asked for this on 2026-08-06 — *"setting is not keep when reopen
//! app, save the setting and remember open window size, current tab, to make
//! sure it will be there when reopen app"* — and it overturns the sentence at
//! the top of [`crate::settings`], which said nothing was persisted. Not all of
//! it: see *What still resets* below, which is the half of the old design that
//! was right.
//!
//! [`models::document`] is the file and [`services::session_store`] is the seam
//! onto disk. This file is the global that holds the live document and decides
//! when it is written.
//!
//! # One file, not three
//!
//! `session.json` carries the appearance settings, the window rectangle and the
//! open tool together, and that is a deliberate choice over one file each:
//!
//! - They are **read at the same instant** — all three before the window
//!   exists, because the theme has to be applied and the geometry known before
//!   the first frame. Three files would be three loads to sequence on the path
//!   that decides how fast dodo opens.
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
//! granularity for that memory. [`models::document`]'s module doc states the
//! same thing, and changing it is the captain's call.
//!
//! # Coalescing
//!
//! Nothing here writes on the spot. A change **schedules** a save, and each new
//! change replaces the pending task — a `Task` cancels when it is dropped, so
//! only the last change in a burst reaches the disk. This is the shape
//! [`crate::quick_nav::QuickNav::set_pattern`] already used for per-keystroke
//! pattern edits, and it is what will keep a resize drag from writing the file
//! once per frame when the window joins this file.
//!
//! The hole a trailing-edge debounce leaves is *quit within the delay*, which
//! is exactly what someone who changes a setting and then closes the window
//! does. It is closed by [`flush_on_quit`], registered with `App::on_app_quit`:
//! gpui awaits those futures before the process ends, so the pending document
//! still gets written — on the background executor, like every other write
//! here.
//!
//! # An unreadable file is never overwritten
//!
//! If the load failed — a version from a newer dodo, or an unreadable file —
//! dodo **stops saving** for the rest of the run. Refusing to read a newer file
//! and then flattening it on the first settings change would make the refusal
//! pointless. The Settings dialog says so; see [`services::session_store`]'s
//! module doc.

pub mod models;
pub mod services;

use std::sync::Arc;
use std::time::Duration;

use gpui::{App, AsyncApp, BorrowAppContext as _, Global, Task};

use crate::i18n::Str;
use crate::session::models::document::SessionDocument;
use crate::session::services::session_store::{DiskSessionStore, SessionStore, SessionStoreError};

/// How long a change waits before it is written.
///
/// Shorter than quick navigation's 600ms because the burst this will have to
/// coalesce is a pointer drag rather than typing: frames arrive ~16ms apart, so
/// 400ms of quiet reliably means the gesture is over, and the shorter the delay
/// the less a hard kill can lose.
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

    /// What went wrong with `session.json`, if anything.
    pub fn store_error(cx: &App) -> Option<Str> {
        cx.try_global::<Session>()
            .and_then(|state| state.store_error.as_ref().map(SessionStoreError::message))
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
    /// left the document exactly as it was.
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

/// Installs the session global and the quit-time flush.
///
/// Nothing is read here: [`load`] does that, and it has to be awaited before
/// the window opens so the theme is known for the first frame. Same
/// post-`gpui_component::init` position as every other `init` in `main.rs`,
/// though this one binds no keys.
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
/// `App`. That is also what keeps the write off the UI thread: the future hands
/// it to the background executor and awaits that.
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::TestAppContext;

    use super::{SAVE_DELAY, Session, flush_on_quit};
    use crate::session::models::document::SessionDocument;
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
}
