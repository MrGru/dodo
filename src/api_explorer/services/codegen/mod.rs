//! Turning the request on screen into code somebody can run.
//!
//! # Why this is not in `services::curl`
//!
//! `services::curl` reads *one* language and produces a request; this reads a
//! request and produces *four* languages. They are the two directions of one
//! road, but they are shaped nothing alike: the parser is a tokenizer plus an
//! option table, the generators are four pure functions over a shared
//! normalized form ([`normalize`]) that the parser has no use for. Putting the
//! emitters here keeps them siblings, so the dialog dispatches on
//! [`CodeTarget`] rather than special-casing cURL, and leaves that 1,000-line
//! module doing one thing.
//!
//! The two are nonetheless joined by a property, and the join is asserted where
//! it belongs — [`curl`]'s own tests generate a command, hand it to
//! `services::curl::parse`, and require the result to normalize to the same
//! [`NormalizedRequest`]. That is the equivalence worth holding: the *wire
//! request*, not the editor state, which legitimately differs (a Bearer token is
//! an Auth-tab field on the way out and a header on the way back). The cases
//! where even that cannot hold are named in `curl`'s module doc.
//!
//! # Secrets: what is in the copied text, and what is not
//!
//! Generated code is only worth anything if it carries the *real* request, which
//! means running values through the same `{{name}}` substitution the send path
//! uses. But an environment holds API tokens, they are stored in plain text, and
//! a variable can be marked `secret` — and this text is about to go to the
//! clipboard, from where it reaches a shell history, a chat window or a pasted
//! bug report. Emitting a resolved secret there is a decision, so it is made
//! here and stated on screen:
//!
//! **By default, a reference to a variable marked `secret` is left as the
//! literal text `{{name}}`; everything else is resolved.** The mechanism is
//! [`VariableSet::with_secrets_masked`], which needs no new behaviour from the
//! substituter and covers nesting. [`Generated::withheld`] names what was left
//! out so the dialog can say so, and a **Resolve secret variables** toggle
//! resolves them for the case the feature exists for — a command you can
//! actually run — with the notice switching to say, in the danger colour, that
//! the code now contains those values in plain text.
//!
//! Two things this policy deliberately does *not* claim, both said out loud in
//! the dialog rather than only here:
//!
//! - **A token or password typed straight into the Auth tab is in the code.** It
//!   has no name to stand in for it, it is already visible in the tab it was
//!   typed into (the Basic password field's mask is a shoulder-surfing measure
//!   on a field being edited, not a promise about the request), and dodo already
//!   writes it to `collections.json` in plain text. The `secret` flag is the one
//!   place a user has *declared* a value sensitive, so it is the one thing this
//!   withholds.
//! - **Withholding is not reversible.** A snippet with `{{name}}` in it does not
//!   run until the reader supplies a value. That is the honest state of affairs
//!   and it is why the toggle exists.
//!
//! [`VariableSet::with_secrets_masked`]: crate::api_explorer::models::variables::VariableSet::with_secrets_masked

pub mod curl;
pub mod javascript;
pub mod normalize;

pub use normalize::{NormalizedBody, NormalizedPart, NormalizedRequest, normalize};

use crate::api_explorer::models::codegen::CodeTarget;
use crate::api_explorer::models::snapshot::RequestSnapshot;
use crate::api_explorer::models::variables::VariableSet;
use crate::i18n::{Str, api_explorer};

/// Why a request could not be turned into code.
///
/// Only substitution can fail. Everything else about a half-written request —
/// an illegal header name, an unfetchable scheme, a file that has moved — is the
/// send path's business, and refusing to show code for it would be unhelpful
/// exactly when the code is what would explain the problem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodegenError {
    UnresolvedVariable { name: String },
    RecursiveVariable { name: String },
}

impl CodegenError {
    /// The wording shown in place of the snippet.
    ///
    /// The same two sentences the send path uses for the same two failures: a
    /// user who has seen one of them once should not have to learn a second
    /// phrasing of it.
    pub fn message(self) -> Str {
        match self {
            CodegenError::UnresolvedVariable { name } => {
                api_explorer::Text::UnresolvedVariable(name).into()
            }
            CodegenError::RecursiveVariable { name } => {
                api_explorer::Text::RecursiveVariable(name).into()
            }
        }
    }
}

/// A snippet, with what it did not say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generated {
    pub code: String,
    /// The secret variables this snippet left as `{{name}}` placeholders, in the
    /// order they are defined. Empty when `reveal_secrets` was set, and empty
    /// when the request refers to no secret at all.
    pub withheld: Vec<String>,
}

/// Generates `target`'s code for `snapshot`.
///
/// `reveal_secrets` is the dialog's toggle; see this module's doc for what each
/// setting puts in the copied text.
pub fn generate(
    target: CodeTarget,
    snapshot: &RequestSnapshot,
    variables: &VariableSet,
    reveal_secrets: bool,
) -> Result<Generated, CodegenError> {
    let resolved = if reveal_secrets {
        variables.clone()
    } else {
        variables.with_secrets_masked()
    };
    let request = normalize(snapshot, &resolved)?;

    let code = match target {
        CodeTarget::Curl => curl::generate(&request),
        CodeTarget::JsFetch => javascript::fetch(&request),
        CodeTarget::JsAxios => javascript::axios(&request),
        CodeTarget::JsXhr => javascript::xhr(&request),
    };

    // Which placeholders actually *survived* into the snippet, rather than which
    // secrets happen to be defined: a request that refers to none of them has
    // nothing to warn about, and the dialog's notice would be a lie.
    let withheld = if reveal_secrets {
        Vec::new()
    } else {
        variables
            .secret_names()
            .into_iter()
            .filter(|name| code.contains(&format!("{{{{{name}}}}}")))
            .collect()
    };

    Ok(Generated { code, withheld })
}

#[cfg(test)]
mod tests {
    use super::{CodegenError, generate};
    use crate::api_explorer::models::auth::{AuthDraft, AuthType};
    use crate::api_explorer::models::codegen::CodeTarget;
    use crate::api_explorer::models::key_value::KeyValue;
    use crate::api_explorer::models::snapshot::RequestSnapshot;
    use crate::api_explorer::models::variables::{Variable, VariableScope, VariableSet};

    /// A request whose token and host both come from variables, one of them
    /// secret.
    fn snapshot() -> RequestSnapshot {
        RequestSnapshot {
            url: "https://{{host}}/v1/things".into(),
            params: vec![KeyValue::text("q", "rust")],
            auth: AuthDraft {
                kind: AuthType::Bearer,
                token: "{{apiToken}}".into(),
                ..AuthDraft::default()
            },
            ..RequestSnapshot::default()
        }
    }

    fn variables() -> VariableSet {
        let mut set = VariableSet::default();
        set.push_layer(
            VariableScope::Environment,
            vec![
                Variable::new("host", "api.example.com"),
                Variable::secret("apiToken", "s3cr3t-value"),
            ],
        );
        set
    }

    #[test]
    fn a_secret_is_withheld_as_its_own_placeholder_in_every_target() {
        for target in CodeTarget::ALL {
            let generated = generate(target, &snapshot(), &variables(), false).expect("generates");
            assert!(
                generated.code.contains("{{apiToken}}"),
                "{target:?} did not leave the placeholder:\n{}",
                generated.code
            );
            assert!(
                !generated.code.contains("s3cr3t-value"),
                "{target:?} leaked the secret value:\n{}",
                generated.code
            );
            // A public variable in the same request still resolves — withholding
            // is per value, not a switch that stops substitution.
            assert!(
                generated.code.contains("api.example.com"),
                "{target:?} failed to resolve a public variable:\n{}",
                generated.code
            );
            assert_eq!(generated.withheld, ["apiToken".to_string()]);
        }
    }

    #[test]
    fn revealing_resolves_the_secret_and_reports_nothing_withheld() {
        for target in CodeTarget::ALL {
            let generated = generate(target, &snapshot(), &variables(), true).expect("generates");
            assert!(
                generated.code.contains("s3cr3t-value"),
                "{target:?} did not resolve the secret when asked to:\n{}",
                generated.code
            );
            assert!(!generated.code.contains("{{apiToken}}"));
            assert!(generated.withheld.is_empty());
        }
    }

    #[test]
    fn a_request_that_refers_to_no_secret_reports_none_withheld() {
        // The notice must not appear merely because an environment happens to
        // hold a secret somewhere.
        let mut plain = snapshot();
        plain.auth = AuthDraft::default();
        for target in CodeTarget::ALL {
            let generated = generate(target, &plain, &variables(), false).expect("generates");
            assert!(
                generated.withheld.is_empty(),
                "{target:?} warned about a secret this request never used"
            );
        }
    }

    #[test]
    fn a_token_typed_into_the_auth_tab_is_in_the_code() {
        // Stated as a test because it is the half of the policy that withholds
        // nothing, and a future change that quietly started masking it would be
        // a change to what the dialog promises.
        let mut typed = snapshot();
        typed.auth.token = "typed-by-hand".into();
        let generated = generate(CodeTarget::Curl, &typed, &variables(), false).expect("generates");
        assert!(generated.code.contains("Bearer typed-by-hand"));
        assert!(generated.withheld.is_empty());
    }

    #[test]
    fn an_unresolved_variable_stops_every_target_with_the_same_error() {
        let mut missing = snapshot();
        missing.url = "https://{{nowhere}}/x".into();
        // The auth token is a reference too, and headers are resolved before the
        // URL; clearing it makes the assertion about the one variable it names.
        missing.auth = AuthDraft::default();
        for target in CodeTarget::ALL {
            assert_eq!(
                generate(target, &missing, &VariableSet::default(), false),
                Err(CodegenError::UnresolvedVariable {
                    name: "nowhere".into()
                }),
                "{target:?}"
            );
        }
    }
}
