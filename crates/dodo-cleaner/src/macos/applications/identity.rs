//! App identity normalization (Phase 9).
//!
//! Pure domain logic, no filesystem access and no GPUI: given the raw fields a
//! bundle's `Info.plist` yields, derive the normalized forms that
//! [`super::locations`] and [`super::confidence`] match leftover paths against.
//!
//! `team_id` is threaded through as an `Option<String>` the caller supplies.
//! Real extraction would need code-signing inspection (the `Security`
//! framework or shelling out to `codesign`); dodo does not add a new
//! dependency or an external process for this phase, so every real scan
//! passes `None` today. The scoring and matching logic here still treats a
//! `Some` team id correctly and is unit-tested with one, so the gap is only in
//! *populating* the field — see `docs/cleaner/known-limitations.md`.

/// Normalized identity of an installed application, derived from its bundle
/// identifier and display name.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AppIdentity {
    pub bundle_id: Option<String>,
    /// `bundle_id` with a trailing `.helper` / `.Helper` / `-helper` segment
    /// removed, when it had one. `None` when `bundle_id` is `None` or does not
    /// end in a recognized helper suffix.
    pub bundle_id_without_helper_suffix: Option<String>,
    /// The last dot-separated component of `bundle_id`, lowercased (e.g.
    /// `com.acme.MyApp` -> `myapp`).
    pub final_bundle_component: Option<String>,
    /// Lowercased app name with punctuation collapsed to spaces and a
    /// trailing version-looking token stripped.
    pub normalized_name: String,
    /// The vendor segment guessed from the bundle identifier's reverse-DNS
    /// shape (e.g. `com.acme.MyApp` -> `acme`), lowercased.
    pub vendor: Option<String>,
    pub team_id: Option<String>,
}

impl AppIdentity {
    pub fn new(bundle_id: Option<&str>, display_name: &str, team_id: Option<String>) -> Self {
        Self {
            bundle_id: bundle_id.map(ToOwned::to_owned),
            bundle_id_without_helper_suffix: bundle_id.and_then(strip_helper_suffix),
            final_bundle_component: bundle_id.and_then(final_bundle_component),
            normalized_name: normalize_app_name(display_name),
            vendor: bundle_id.and_then(vendor_from_bundle_id),
            team_id,
        }
    }
}

/// Strips a trailing `.helper`, `.Helper` or `-helper` segment from a bundle
/// identifier, case-insensitively. Returns `None` when there is nothing to
/// strip, so callers can tell "no helper suffix" apart from "stripped to an
/// empty string" (which never happens: a bare `helper`/`-helper` with nothing
/// before it is left untouched).
pub fn strip_helper_suffix(bundle_id: &str) -> Option<String> {
    let lower = bundle_id.to_ascii_lowercase();
    for separator in ['.', '-'] {
        let suffix = format!("{separator}helper");
        if let Some(prefix_len) = lower.len().checked_sub(suffix.len())
            && prefix_len > 0
            && lower.ends_with(&suffix)
        {
            return Some(bundle_id[..prefix_len].to_string());
        }
    }
    None
}

/// The last dot-separated component of a bundle identifier, lowercased.
pub fn final_bundle_component(bundle_id: &str) -> Option<String> {
    bundle_id
        .rsplit('.')
        .next()
        .filter(|component| !component.is_empty())
        .map(str::to_ascii_lowercase)
}

/// Guesses the vendor segment from a reverse-DNS bundle identifier:
/// `<tld>.<vendor>.<app...>` when there are three or more components,
/// `<vendor>.<app>` when there are exactly two, and `None` for a single
/// component (nothing to call a vendor).
pub fn vendor_from_bundle_id(bundle_id: &str) -> Option<String> {
    let parts: Vec<&str> = bundle_id
        .split('.')
        .filter(|part| !part.is_empty())
        .collect();
    match parts.len() {
        0 | 1 => None,
        2 => Some(parts[0].to_ascii_lowercase()),
        _ => Some(parts[1].to_ascii_lowercase()),
    }
}

/// Lowercases `name`, collapses punctuation/whitespace runs to single spaces,
/// and drops a trailing version-looking token (`2`, `v2`, `3.2`, ...).
pub fn normalize_app_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    let mut tokens: Vec<String> = cleaned
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    while tokens.len() > 1 && tokens.last().is_some_and(|token| is_version_token(token)) {
        tokens.pop();
    }
    tokens.join(" ")
}

fn is_version_token(token: &str) -> bool {
    let digits = token.strip_prefix('v').unwrap_or(token);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_recognized_helper_suffixes() {
        assert_eq!(
            strip_helper_suffix("com.acme.MyApp.helper"),
            Some("com.acme.MyApp".to_string())
        );
        assert_eq!(
            strip_helper_suffix("com.acme.MyApp.Helper"),
            Some("com.acme.MyApp".to_string())
        );
        assert_eq!(
            strip_helper_suffix("com.acme.MyApp-helper"),
            Some("com.acme.MyApp".to_string())
        );
        assert_eq!(strip_helper_suffix("com.acme.MyApp"), None);
    }

    #[test]
    fn final_component_is_lowercased() {
        assert_eq!(
            final_bundle_component("com.acme.MyApp"),
            Some("myapp".to_string())
        );
        assert_eq!(final_bundle_component(""), None);
    }

    #[test]
    fn vendor_guess_handles_two_and_three_part_ids() {
        assert_eq!(
            vendor_from_bundle_id("com.acme.MyApp"),
            Some("acme".to_string())
        );
        assert_eq!(
            vendor_from_bundle_id("acme.MyApp"),
            Some("acme".to_string())
        );
        assert_eq!(vendor_from_bundle_id("acme"), None);
    }

    #[test]
    fn normalized_name_strips_version_tokens_and_punctuation() {
        assert_eq!(normalize_app_name("Acme Notes"), "acme notes");
        assert_eq!(normalize_app_name("Acme Notes 2"), "acme notes");
        assert_eq!(normalize_app_name("Acme Notes v3.2"), "acme notes");
        assert_eq!(normalize_app_name("Acme-Notes (2024)"), "acme notes");
    }

    #[test]
    fn versioned_display_names_normalize_to_the_same_identity() {
        let base = AppIdentity::new(Some("com.acme.notes"), "Acme Notes", None);
        let versioned = AppIdentity::new(Some("com.acme.notes"), "Acme Notes 2", None);
        assert_eq!(base.normalized_name, versioned.normalized_name);
    }
}
