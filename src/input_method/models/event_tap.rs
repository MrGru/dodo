//! Pure policy for the Accessibility-gated Event Tap.
//!
//! The CoreGraphics callback is deliberately thin: it asks these rules whether
//! to pass, process, or re-enable, then performs the platform call. Keeping the
//! policy here proves the security defaults without needing Accessibility access.

use dodo_ime_core::{EngineAction, LanguageId};
use dodo_ime_ipc::settings::Backend;

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

/// The direct-typing work Event Tap can safely perform for one original event.
///
/// It intentionally understands only the engine's direct actions. A marked-text
/// action or a future candidate action is uncertainty, so the caller resets the
/// engine and returns the original event unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputPlan {
    pub delete_before: usize,
    pub insert: Option<String>,
    pub pass_through: bool,
}

impl OutputPlan {
    /// Turns one direct engine result into one atomic output plan.
    pub fn from_actions(actions: &[EngineAction]) -> Option<OutputPlan> {
        let mut plan = OutputPlan::default();

        for action in actions {
            match action {
                EngineAction::InsertText(text)
                    if plan.insert.is_none() && plan.delete_before == 0 =>
                {
                    plan.insert = Some(text.clone());
                }
                EngineAction::DeleteBackward(count) if plan.insert.is_none() => {
                    plan.delete_before = plan.delete_before.checked_add(*count)?;
                }
                EngineAction::ReplaceBeforeCursor {
                    grapheme_count,
                    text,
                } if plan.insert.is_none() && plan.delete_before == 0 => {
                    plan.delete_before = *grapheme_count;
                    plan.insert = (!text.is_empty()).then(|| text.clone());
                }
                EngineAction::PassThrough if !plan.pass_through => plan.pass_through = true,
                _ => return None,
            }
        }

        (!actions.is_empty()).then_some(plan)
    }

    pub fn transforms(&self) -> bool {
        self.delete_before != 0 || self.insert.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{EventTapStatus, Handling, OutputPlan, TapEvent, desired_status, handling};
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
    fn missing_accessibility_never_claims_the_tap_is_running() {
        assert_eq!(
            desired_status(
                Backend::EventTap,
                LanguageId::Vietnamese,
                false,
                false,
                false
            ),
            EventTapStatus::NeedsAccessibility
        );
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
