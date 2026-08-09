//! The direct-typing subset of an engine result.
//!
//! Global event hooks do not receive a marked-text client. They may only rewrite
//! an original key as a bounded delete/insert plan, otherwise they reset and let
//! the original event through. Keeping that security rule independent of macOS
//! or Windows makes both fallback hosts prove the same fail-closed behaviour.

use dodo_ime_core::EngineAction;

/// One atomic direct-output attempt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputPlan {
    pub delete_before: usize,
    pub insert: Option<String>,
    pub pass_through: bool,
}

impl OutputPlan {
    /// Returns `None` for a composition, candidate, or contradictory sequence.
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
    use super::OutputPlan;
    use dodo_ime_core::EngineAction;

    #[test]
    fn only_direct_actions_become_an_output_plan() {
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
