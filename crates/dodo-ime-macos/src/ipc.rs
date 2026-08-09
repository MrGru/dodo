//! Reading dodo's settings, and saying so.
//!
//! The bundle is launched by macOS, not by dodo, and the two never share a
//! process. `dodo_ime_ipc` is the whole contract; this module is the bundle's
//! half of it:
//!
//! 1. **At startup**, read `input-method.json` and write
//!    `input-method-status.json` — see [`start`].
//! 2. **On [`SETTINGS_CHANGED`](dodo_ime_ipc::SETTINGS_CHANGED)**, read it again.
//! 3. **In the controller**, hand the live configuration to whichever session is
//!    about to be used, which is what makes a change take effect without a
//!    restart.
//!
//! # Why the configuration is a process-wide global here, when the session is not
//!
//! `Session` is deliberately per-controller — two text fields compose
//! independently and a shared engine would leak one field's syllable into the
//! other. The *configuration* is the opposite: it is one user's one answer to
//! "how do I type", and macOS instantiates controllers whenever it likes. A
//! setting stored per controller would apply to whichever fields happened to
//! exist when the notification arrived and not to the ones created afterwards.
//!
//! So [`config`] is a `RwLock` read at the top of every controller call. The
//! read is uncontended in practice — InputMethodKit runs every one of those on
//! the main thread — but the notification callback is what makes the lock
//! necessary rather than decorative: it is the one writer, it runs on the run
//! loop, and it can land between two keystrokes.
//!
//! # Reading a file is not on the keystroke path
//!
//! The file is read exactly twice per settings change: once at launch, once when
//! the notification arrives. A controller asks this module for a value already in
//! memory. Nothing here touches a disk while composing, which matters for
//! latency and matters more for the privacy rule — a file read per keystroke
//! would be a typing log written in `atime`.
//!
//! # Failure is silence
//!
//! Every function here returns `()` or a value, never an error to report.
//! There is no user interface to report into and no log file (see the privacy
//! note on [`crate`]), and the fallback is always
//! [`DEFAULT_CONFIG`](crate::DEFAULT_CONFIG) — a working Telex input method. A
//! refused settings file means dodo is newer than this bundle, which dodo itself
//! can see and say; the bundle's job is to keep typing.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use dodo_ime_core::{LanguageId, VietnameseConfig};
use dodo_ime_ipc::paths;
use dodo_ime_ipc::settings::{Backend, SETTINGS_FILE, SettingsDocument};
use dodo_ime_ipc::status::{STATUS_FILE, StatusDocument};

use crate::DEFAULT_CONFIG;

/// The bundle's own version, which is what
/// [`StatusDocument::bundle_version`](dodo_ime_ipc::StatusDocument::bundle_version)
/// is for. Read here rather than in `dodo-ime-ipc`, which is linked by both
/// processes and would report dodo's version instead.
const BUNDLE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What this process is currently typing with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Live {
    pub backend: Backend,
    pub language: LanguageId,
    pub config: VietnameseConfig,
    /// The settings revision this came from. `0` means the compiled-in
    /// defaults — no file, or a file that was refused.
    pub revision: u64,
}

impl Live {
    const DEFAULT: Live = Live {
        backend: Backend::Native,
        language: LanguageId::English,
        config: DEFAULT_CONFIG,
        revision: 0,
    };
}

/// The one piece of process-wide state in the bundle. See the module docs for
/// why this is a global and `Session` is not.
static LIVE: RwLock<Live> = RwLock::new(Live::DEFAULT);

/// What the engine should be configured with right now.
///
/// A poisoned lock answers with the compiled-in defaults rather than panicking:
/// this is called from inside an Objective-C method, and unwinding across that
/// boundary would take the user's application down with it.
pub fn config() -> VietnameseConfig {
    LIVE.read()
        .map(|live| live.config)
        .unwrap_or(DEFAULT_CONFIG)
}

/// The language the native input method is using right now.
///
/// Event Tap is another host, so this bundle must pass every key through when it
/// owns the selection. The schema version makes an old bundle refuse that
/// selection rather than compose beside the tap.
pub fn language() -> LanguageId {
    LIVE.read()
        .map(|live| {
            if live.backend == Backend::Native {
                live.language
            } else {
                LanguageId::English
            }
        })
        .unwrap_or_default()
}

/// The settings revision in force, for the status file.
pub fn revision() -> u64 {
    LIVE.read().map(|live| live.revision).unwrap_or(0)
}

/// Reads the settings file in `dir`, adopts it, and returns what is now live.
///
/// Pure enough to test: everything about *which* directory is the caller's
/// problem, and the read/parse/version rules belong to `dodo_ime_ipc`. A refused
/// or missing file adopts the defaults, which is what makes a hand-mangled file
/// a working input method rather than a broken one.
pub fn adopt_from(dir: &Path) -> Live {
    let (document, _refused) = SettingsDocument::read_or_default(&dir.join(SETTINGS_FILE));
    let live = Live {
        backend: document.backend,
        language: document.language,
        config: document.vietnamese.to_config(),
        // A refused file reports revision 0, because `read_or_default` hands
        // back the defaults: dodo then sees "the bundle is on its defaults" and
        // can say so, which is the honest reading of "I could not understand
        // your settings".
        revision: document.revision,
    };
    if let Ok(mut held) = LIVE.write() {
        *held = live;
    }
    live
}

/// Writes the status file into `dir`, reporting what is live.
///
/// Best effort: the only consequence of failing is that dodo cannot tell whether
/// its settings arrived.
pub fn report_into(dir: &Path) {
    let status = StatusDocument::now(BUNDLE_VERSION, revision());
    let _ = status.write(&dir.join(STATUS_FILE));
}

/// dodo's data directory, or `None` when the environment names no home.
fn dir() -> Option<PathBuf> {
    paths::support_dir_from_env()
}

/// Read the settings, then say what was read. The whole exchange, in the order
/// it has to happen.
fn refresh() {
    if let Some(dir) = dir() {
        adopt_from(&dir);
        report_into(&dir);
    }
}

/// Adopt dodo's settings and start listening for changes.
///
/// Called once from `main`, **before** `NSApplication::run`: the observer has to
/// be installed on the thread whose run loop will deliver to it, and the initial
/// read has to happen before the first keystroke rather than on it.
#[cfg(target_os = "macos")]
pub fn start() {
    refresh();
    observer::install();
}

/// The distributed-notification observer.
///
/// # Why `CFNotificationCenter` and not `NSDistributedNotificationCenter`
///
/// The Objective-C class wants either a selector on a real Objective-C object or
/// a `block2` closure. Both are more machinery than this needs: the C function
/// below takes no captured state, because there is none to capture — the
/// notification carries no payload and the answer is always "read the file
/// again". `CFNotificationCenterAddObserver` takes exactly that, a plain
/// function pointer, so the bundle gains no class and no block runtime.
///
/// # Why the observer is never removed
///
/// It lives as long as the process, and the process is a system agent macOS
/// starts and stops as it pleases. There is no shutdown path to remove it on —
/// `NSApplication::run` does not return — and removing it at exit would be
/// registering a callback to unregister a callback.
#[cfg(target_os = "macos")]
mod observer {
    use std::ffi::c_void;
    use std::ptr::null;

    use objc2_core_foundation::{
        CFDictionary, CFNotificationCenter, CFNotificationName, CFNotificationSuspensionBehavior,
        CFString,
    };

    /// dodo wrote the settings file. Read it again, and say what was read.
    ///
    /// Runs on the main run loop, which is the same thread every controller call
    /// arrives on, so the only synchronisation it needs is the one `RwLock` in
    /// the parent module.
    ///
    /// # Safety
    ///
    /// Called by CoreFoundation with pointers this function does not read. It
    /// deliberately ignores every argument — including `user_info`, which a
    /// distributed notification can carry from *any* process on the machine.
    /// Nothing here parses attacker-supplied structure; the file is the only
    /// input, and it is read with the version rule.
    unsafe extern "C-unwind" fn settings_changed(
        _center: *mut CFNotificationCenter,
        _observer: *mut c_void,
        _name: *const CFNotificationName,
        _object: *const c_void,
        _user_info: *const CFDictionary,
    ) {
        super::refresh();
    }

    pub fn install() {
        let Some(center) = CFNotificationCenter::distributed_center() else {
            return;
        };
        let name = CFString::from_str(dodo_ime_ipc::SETTINGS_CHANGED);

        // SAFETY: the observer pointer is null and never dereferenced (nothing
        // is ever removed by it — see the module docs), the callback matches
        // `CFNotificationCallback`, the name is a live `CFString` for the
        // duration of the call, and the object filter is null, which is the
        // documented "any sender".
        unsafe {
            center.add_observer(
                null(),
                Some(settings_changed),
                Some(&name),
                null(),
                // The input method is a background agent that macOS may consider
                // suspended when nothing is typing at it. Coalescing or dropping
                // would mean a settings change that never arrives, so this asks
                // for the one behaviour that always delivers.
                CFNotificationSuspensionBehavior::DeliverImmediately,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BUNDLE_VERSION, LIVE, Live, adopt_from, config, language, report_into, revision};
    use crate::{DEFAULT_CONFIG, Session};
    use dodo_ime_core::{InputScheme, KeyEvent, LanguageId, TonePlacement};
    use dodo_ime_ipc::settings::{
        Backend, SETTINGS_FILE, Scheme, SettingsDocument, Tone, VietnameseSettings,
    };
    use dodo_ime_ipc::status::{STATUS_FILE, StatusDocument};

    /// A directory of this test's own. These tests write the *real* file names,
    /// so they must never be pointed at a real data directory.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dodo-ime-macos-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// [`LIVE`] is process-wide and libtest runs tests on threads of one
    /// process, so every test that adopts a file takes this first. Without it
    /// they would race on the global and fail each other's assertions about
    /// [`config`], which is exactly the bug the lock in the real code prevents.
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Holds the lock, and puts the global back the way it was found.
    struct Held {
        _guard: std::sync::MutexGuard<'static, ()>,
        live: Live,
    }

    impl Held {
        fn take() -> Held {
            let guard = GUARD
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Held {
                live: *LIVE.read().unwrap(),
                _guard: guard,
            }
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            if let Ok(mut live) = LIVE.write() {
                *live = self.live;
            }
        }
    }

    #[test]
    fn nothing_on_disk_means_the_compiled_in_defaults() {
        let _held = Held::take();
        let dir = scratch("absent");

        let live = adopt_from(&dir);
        assert_eq!(live.backend, Backend::Native);
        assert_eq!(live.language, LanguageId::English);
        assert_eq!(live.config, DEFAULT_CONFIG);
        assert_eq!(live.revision, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dodos_settings_reach_the_engine_configuration() {
        let _held = Held::take();
        let dir = scratch("adopt");

        let document = SettingsDocument::next(
            &SettingsDocument::default(),
            LanguageId::Vietnamese,
            VietnameseSettings {
                scheme: Scheme::Vni,
                tone_placement: Tone::Traditional,
                spell_check: false,
                bracket_shortcuts: false,
            },
        );
        document.write(&dir.join(SETTINGS_FILE)).unwrap();

        let live = adopt_from(&dir);
        assert_eq!(live.backend, Backend::Native);
        assert_eq!(live.language, LanguageId::Vietnamese);
        assert_eq!(language(), LanguageId::Vietnamese);
        assert_eq!(live.config.scheme, InputScheme::Vni);
        assert_eq!(live.config.tone_placement, TonePlacement::Traditional);
        assert!(!live.config.spell_check);
        assert!(!live.config.bracket_shortcuts);
        assert_eq!(live.revision, 1);
        // And it is what a controller would now read.
        assert_eq!(config(), live.config);
        assert_eq!(revision(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file from a newer dodo. The bundle keeps typing, on its defaults, and
    /// reports revision 0 so dodo can tell that its settings did *not* arrive.
    #[test]
    fn a_refused_file_leaves_a_working_input_method() {
        let _held = Held::take();
        let dir = scratch("refused");
        std::fs::write(dir.join(SETTINGS_FILE), br#"{"version":99}"#).unwrap();

        let live = adopt_from(&dir);
        assert_eq!(live.language, LanguageId::English);
        assert_eq!(live.config, DEFAULT_CONFIG);
        assert_eq!(live.revision, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_selected_language_reaches_the_native_session() {
        let _held = Held::take();
        let dir = scratch("language");
        let document = SettingsDocument::next(
            &SettingsDocument::default(),
            LanguageId::English,
            VietnameseSettings::default(),
        );
        document.write(&dir.join(SETTINGS_FILE)).unwrap();

        let live = adopt_from(&dir);
        let mut session = Session::new(live.language, live.config);
        assert!(
            !session.key(&KeyEvent::character('d')).handled,
            "English is passed through instead of entering the Vietnamese engine"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn event_tap_selection_leaves_the_native_host_passive() {
        let _held = Held::take();
        let dir = scratch("event-tap");
        let document = SettingsDocument::next_with_backend(
            &SettingsDocument::default(),
            Backend::EventTap,
            LanguageId::Vietnamese,
            VietnameseSettings::default(),
        );
        document.write(&dir.join(SETTINGS_FILE)).unwrap();

        let live = adopt_from(&dir);
        assert_eq!(live.backend, Backend::EventTap);
        assert_eq!(live.language, LanguageId::Vietnamese);
        assert_eq!(language(), LanguageId::English);
        assert!(
            !Session::new(language(), config())
                .key(&KeyEvent::character('d'))
                .handled,
            "the native host must not transform beside Event Tap"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_status_file_names_this_build_and_the_revision_in_force() {
        let _held = Held::take();
        let dir = scratch("status");

        SettingsDocument {
            version: dodo_ime_ipc::settings::SETTINGS_SCHEMA_VERSION,
            backend: Backend::Native,
            language: LanguageId::Vietnamese,
            revision: 12,
            vietnamese: VietnameseSettings::default(),
        }
        .write(&dir.join(SETTINGS_FILE))
        .unwrap();

        adopt_from(&dir);
        report_into(&dir);

        let status = StatusDocument::read(&dir.join(STATUS_FILE))
            .unwrap()
            .expect("the status file was written");
        assert_eq!(status.bundle_version, BUNDLE_VERSION);
        assert_eq!(status.settings_revision, 12);
        assert_eq!(status.pid, std::process::id());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
