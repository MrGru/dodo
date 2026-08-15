//! Nub — deliberately reports nothing, on purpose.
//!
//! The ticket hedges this one tool explicitly: "Because Nub may evolve:
//! prefer configuration or CLI discovery when supported... Fall back to
//! scan-only when uncertain." Unlike nvm (`~/.nvm`), fnm (`~/.fnm` or
//! `$XDG_DATA_HOME/fnm`), or Volta (`~/.volta`), this phase has no
//! well-known, version-stable filesystem convention for a tool named "Nub"
//! to check with any confidence — there is no cache/version-store split to
//! point at that isn't a guess. Guessing one would risk exactly what the
//! ticket forbids for *any* Node-version manager: misclassifying a
//! provisioned Node version as ordinary, safe-to-delete cache junk
//! ("Provisioned Node versions are user-managed tools, not ordinary cache.
//! Never select them by default.").
//!
//! So this provider takes the ticket's own escape hatch literally: it checks
//! one defensive, generically-named override (`NUB_HOME`, following the
//! `<TOOL>_HOME` convention every other provider here already uses) via
//! [`detect_home`], and then reports nothing at all rather than inventing a
//! directory layout underneath it. "Fall back to scan-only when uncertain"
//! resolves here to "fall back to reporting nothing", because there is no
//! subpath under an unknown layout that could be marked scan-only with any
//! justification — scan-only still requires knowing *what* is being
//! scanned. `detect_home` exists so a future session with real knowledge of
//! Nub's on-disk shape has one already-wired place to extend, and so this
//! module's tests can show detection itself degrades gracefully across
//! different absence shapes without ever turning into a fabricated
//! location.

use std::path::PathBuf;

use crate::core::node_tool_provider::{
    NodeCacheLocation, NodeToolCacheProvider, NodeToolEnvironment,
};

pub(crate) struct NubProvider;

/// Best-effort, read-only check for a plausible `NUB_HOME`-style override —
/// present only so this module's tests can exercise different "absence"
/// shapes (unset, set-but-missing, set-and-present) and confirm none of them
/// ever turns into a location from [`NubProvider::discover`].
fn detect_home(environment: &NodeToolEnvironment) -> Option<PathBuf> {
    environment
        .nub_home
        .as_ref()
        .filter(|path| path.is_dir())
        .cloned()
}

impl NodeToolCacheProvider for NubProvider {
    fn id(&self) -> &'static str {
        "nub"
    }

    fn display_name(&self) -> &'static str {
        "Nub"
    }

    fn discover(&self, environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation> {
        // See this module's doc comment: detection is checked so a future
        // session knows where to extend this, but nothing is ever turned
        // into a reported location today, confirmed or not.
        let _ = detect_home(environment);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn detect_home_is_none_when_nub_home_is_not_set() {
        let environment = NodeToolEnvironment::default();
        assert_eq!(detect_home(&environment), None);
    }

    #[test]
    fn detect_home_is_none_when_nub_home_is_set_but_missing() {
        let environment = NodeToolEnvironment {
            nub_home: Some(PathBuf::from("/nonexistent/nub-home")),
            ..Default::default()
        };
        assert_eq!(detect_home(&environment), None);
    }

    #[test]
    fn detect_home_finds_an_existing_override_but_discover_still_reports_nothing() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-nub-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates a plausible Nub home");

        let environment = NodeToolEnvironment {
            nub_home: Some(temp.clone()),
            ..Default::default()
        };
        assert_eq!(detect_home(&environment), Some(temp.clone()));

        let provider = NubProvider;
        assert!(
            provider.discover(&environment).is_empty(),
            "Nub must never report a location even when a plausible home is detected"
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn discover_never_panics_and_never_fabricates_a_location_across_every_shape() {
        let shapes = [
            NodeToolEnvironment::default(),
            NodeToolEnvironment {
                nub_home: Some(PathBuf::from("/nonexistent/nub-home")),
                ..Default::default()
            },
            NodeToolEnvironment {
                home: Some(PathBuf::from("/Users/example")),
                nub_home: None,
                ..Default::default()
            },
        ];
        let provider = NubProvider;
        for environment in shapes {
            assert!(provider.discover(&environment).is_empty());
        }
    }
}
