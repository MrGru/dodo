//! Pure policy for the Accessibility-gated Event Tap.
//!
//! The CoreGraphics callback is deliberately thin: it asks these rules whether
//! to pass, process, or re-enable, then performs the platform call. Keeping the
//! policy here proves the security defaults without needing Accessibility access.

use dodo_ime_core::LanguageId;
use dodo_ime_ipc::settings::Backend;

pub use crate::input_method::models::direct_output::OutputPlan;

/// What the pane can honestly say about Event Tap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EventTapStatus {
    /// Event Tap is not the selected Vietnamese backend.
    #[default]
    Inactive,
    /// A live native bundle has not yet adopted the Event Tap selection.
    WaitingForNative,
    /// macOS has not granted Accessibility permission to dodo.
    NeedsAccessibility,
    /// The tap is attached to dodo's main run loop.
    Running,
    /// CoreGraphics refused or later disabled the tap. Keys pass through.
    Failed,
}

/// The next lifecycle state before the platform service touches CoreGraphics.
pub fn desired_status(
    backend: Backend,
    language: LanguageId,
    native_is_live: bool,
    settings_applied: bool,
    accessibility_trusted: bool,
) -> EventTapStatus {
    if backend != Backend::EventTap || language != LanguageId::Vietnamese {
        EventTapStatus::Inactive
    } else if native_is_live && !settings_applied {
        EventTapStatus::WaitingForNative
    } else if !accessibility_trusted {
        EventTapStatus::NeedsAccessibility
    } else {
        EventTapStatus::Running
    }
}

/// Whether this eligible, untrusted state may ask macOS to show Dodo.
///
/// macOS owns the asynchronous request and the Accessibility list; dodo only
/// asks once per process, then keeps keys passing through until a later check
/// observes a grant.
pub fn should_request_accessibility(status: EventTapStatus, already_requested: bool) -> bool {
    status == EventTapStatus::NeedsAccessibility && !already_requested
}

/// The only three event classes Event Tap treats specially.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapEvent {
    KeyDown { autorepeat: bool },
    TapDisabled,
    Other,
}

/// What the callback must do with one event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handling {
    /// Feed a key down through the existing engine. `autorepeat` is retained by
    /// the cloned output event; it is never discarded or converted to a new key.
    ProcessKey { autorepeat: bool },
    /// Return CoreGraphics' event pointer unchanged.
    PassThrough,
    /// Re-enable once for CoreGraphics' explicit disabled notification.
    RecoverTap,
}

/// Security-first callback routing.
pub fn handling(event: TapEvent, secure_input: bool) -> Handling {
    match event {
        TapEvent::TapDisabled => Handling::RecoverTap,
        TapEvent::KeyDown { .. } if !secure_input => Handling::ProcessKey {
            autorepeat: matches!(event, TapEvent::KeyDown { autorepeat: true }),
        },
        TapEvent::KeyDown { .. } | TapEvent::Other => Handling::PassThrough,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EventTapStatus, Handling, OutputPlan, TapEvent, desired_status, handling,
        should_request_accessibility,
    };
    use dodo_ime_core::{EngineAction, LanguageId};
    use dodo_ime_ipc::settings::Backend;

    #[test]
    fn only_the_selected_backend_can_own_transformation() {
        assert_eq!(
            desired_status(Backend::Native, LanguageId::Vietnamese, false, false, true),
            EventTapStatus::Inactive
        );
        assert_eq!(
            desired_status(Backend::EventTap, LanguageId::English, false, false, true),
            EventTapStatus::Inactive
        );
        assert_eq!(
            desired_status(Backend::EventTap, LanguageId::Vietnamese, true, false, true),
            EventTapStatus::WaitingForNative
        );
        assert_eq!(
            desired_status(Backend::EventTap, LanguageId::Vietnamese, true, true, true),
            EventTapStatus::Running
        );
    }

    #[test]
    fn accessibility_request_is_once_and_a_returning_user_can_start_the_tap() {
        let inactive = desired_status(Backend::Native, LanguageId::Vietnamese, false, false, false);
        assert!(!should_request_accessibility(inactive, false));

        let waiting = desired_status(
            Backend::EventTap,
            LanguageId::Vietnamese,
            true,
            false,
            false,
        );
        assert_eq!(waiting, EventTapStatus::WaitingForNative);
        assert!(!should_request_accessibility(waiting, false));

        let untrusted = desired_status(
            Backend::EventTap,
            LanguageId::Vietnamese,
            false,
            false,
            false,
        );
        assert_eq!(untrusted, EventTapStatus::NeedsAccessibility);
        assert!(should_request_accessibility(untrusted, false));
        assert!(!should_request_accessibility(untrusted, true));

        let trusted_after_return = desired_status(
            Backend::EventTap,
            LanguageId::Vietnamese,
            false,
            false,
            true,
        );
        assert_eq!(trusted_after_return, EventTapStatus::Running);
        assert!(!should_request_accessibility(trusted_after_return, false));
    }

    #[test]
    fn secure_input_and_non_key_events_pass_through_but_repeats_survive() {
        assert_eq!(
            handling(TapEvent::KeyDown { autorepeat: true }, false),
            Handling::ProcessKey { autorepeat: true }
        );
        assert_eq!(
            handling(TapEvent::KeyDown { autorepeat: false }, true),
            Handling::PassThrough
        );
        assert_eq!(handling(TapEvent::Other, false), Handling::PassThrough);
    }

    #[test]
    fn a_disabled_notification_gets_one_recovery_action_not_a_retry_loop() {
        assert_eq!(handling(TapEvent::TapDisabled, false), Handling::RecoverTap);
    }

    #[test]
    fn direct_engine_actions_become_a_replacement_or_an_unchanged_event() {
        assert_eq!(
            OutputPlan::from_actions(&[EngineAction::ReplaceBeforeCursor {
                grapheme_count: 2,
                text: "tiếng".into(),
            }]),
            Some(OutputPlan {
                delete_before: 2,
                insert: Some("tiếng".into()),
                pass_through: false,
            })
        );
        assert_eq!(
            OutputPlan::from_actions(&[EngineAction::PassThrough]),
            Some(OutputPlan {
                pass_through: true,
                ..OutputPlan::default()
            })
        );
        assert!(
            OutputPlan::from_actions(&[EngineAction::SetComposition {
                text: "tiếng".into(),
                cursor: 5,
                selection: None,
            }])
            .is_none()
        );
    }
}
