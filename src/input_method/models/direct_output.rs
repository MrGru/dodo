//! The direct-typing subset of an engine result.
//!
//! Global event hooks do not receive a marked-text client. They may only rewrite
//! an original key as a bounded delete/insert plan, otherwise they reset and let
//! the original event through. Keeping that security rule independent of macOS
//! or Windows makes both fallback hosts prove the same fail-closed behaviour.

#[cfg(any(target_os = "windows", test))]
use dodo_ime_core::EngineAction;
use dodo_ime_core::core::{grapheme_count, grapheme_prefix};

/// One atomic direct-output attempt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputPlan {
    pub delete_before: usize,
    pub insert: Option<String>,
    pub pass_through: bool,
}

impl OutputPlan {
    /// Returns `None` for a composition, candidate, or contradictory sequence.
    #[cfg(any(target_os = "windows", test))]
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

    /// The least tail a direct-output host must replace to turn `before` into
    /// `after` at an end cursor. It cannot preserve a matching suffix: a
    /// Backspace-only host must delete through that suffix before inserting it.
    pub fn minimal(before: &str, after: &str) -> OutputPlan {
        let before_count = grapheme_count(before);
        let after_count = grapheme_count(after);
        let mut prefix = 0;
        while prefix < before_count
            && prefix < after_count
            && grapheme_at(before, prefix) == grapheme_at(after, prefix)
        {
            prefix += 1;
        }

        let inserted_start = grapheme_prefix(after, prefix).len();
        OutputPlan {
            delete_before: before_count - prefix,
            insert: (!after[inserted_start..].is_empty())
                .then(|| after[inserted_start..].to_owned()),
            pass_through: false,
        }
    }
}

fn grapheme_at(text: &str, at: usize) -> &str {
    let start = grapheme_prefix(text, at).len();
    let end = grapheme_prefix(text, at + 1).len();
    &text[start..end]
}

#[cfg(test)]
mod tests {
    use super::OutputPlan;
    use dodo_ime_core::EngineAction;
    use dodo_ime_core::core::truncate_graphemes;

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

    #[test]
    fn minimal_replacement_applies_at_an_end_cursor() {
        for (before, after) in [
            ("tiêng", "tiếng"),
            ("hoiư", "hơi"),
            ("e\u{0302}\u{0301}", ""),
        ] {
            let plan = OutputPlan::minimal(before, after);
            let mut document = truncate_graphemes(before, plan.delete_before);
            if let Some(insert) = plan.insert {
                document.push_str(&insert);
            }
            assert_eq!(document, after, "{before:?} -> {after:?}");
        }

        let tone = OutputPlan::minimal("tiêng", "tiếng");
        assert_eq!(tone.delete_before, 3);
        assert_eq!(tone.insert.as_deref(), Some("ếng"));
    }
}
