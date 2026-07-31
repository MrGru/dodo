//! Semantic versions, their ordering, and which of them a channel will offer.
//!
//! # Why this is not the `semver` crate
//!
//! Same reasoning as [`sha256`](super::sha256), and the same conclusion: the
//! rules are short, exactly specified, and the interesting part is not the
//! parse but the *policy* on top of it, which no crate supplies. What is
//! implemented here is SemVer 2.0 §11 precedence, including the pre-release
//! rules that are the only subtle part:
//!
//! - a version *with* a pre-release is lower than the same version without one
//!   (`1.0.0-rc.1 < 1.0.0`);
//! - pre-release identifiers compare field by field, numerically when both are
//!   numeric and ASCII-lexically otherwise, with numeric always lower;
//!   `1.0.0-alpha.2 < 1.0.0-alpha.10` — the case a naive string compare gets
//!   backwards.
//!
//! Build metadata (`+abc`) is parsed and then **ignored** in comparison, as the
//! specification requires.
//!
//! # The channel is the policy, and the manifest's own `channel` is not a veto
//!
//! `docs/release.md` records the trap: `releases/latest/download/update.json`
//! excludes pre-releases, so a beta client polling that URL is handed *stable's*
//! manifest. The defence here is that acceptability is decided by the shape of
//! the offered **version**, through [`Channel::accepts`], and not by trusting
//! the document's `channel` field:
//!
//! - **stable** takes releases with no pre-release part at all. Handed a
//!   nightly manifest it still offers nothing, which is the protection.
//! - **beta** takes those, plus `-beta.N` and `-rc.N`. A beta user must keep
//!   getting stable releases — stable is what beta becomes — so this is a
//!   superset, not a different stream.
//! - **nightly** takes anything that parses.
//!
//! The document's `channel` is still read and shown; it just does not get a
//! vote, because a field that decided whether to update would make a
//! misconfigured publication path silently stop everyone's updates.

use std::cmp::Ordering;

/// A parsed semantic version. Build metadata is dropped: it does not
/// participate in precedence, and keeping it would invite a comparison that
/// wrongly used it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The dot-separated pre-release identifiers, empty for a final release.
    pub pre: Vec<PreRelease>,
}

/// One dot-separated pre-release identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreRelease {
    /// All-digits, compared as a number. `alpha.2 < alpha.10`.
    Numeric(u64),
    Alphanumeric(String),
}

impl Version {
    /// Parses `MAJOR.MINOR.PATCH[-PRE][+BUILD]`, tolerating a leading `v`
    /// because release tags carry one and manifests do not.
    ///
    /// Strict about the rest: a missing component, a non-numeric core field, a
    /// leading zero on a numeric field, or an empty identifier is `None`. The
    /// strictness is the point — an updater that guessed at `1.2` would be
    /// guessing about whether to replace the user's binary.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);

        // Build metadata first: it may itself contain `-`, so it has to come
        // off before the pre-release is split out.
        let (text, _build) = match text.split_once('+') {
            Some((head, build)) if !build.is_empty() => (head, Some(build)),
            Some(_) => return None,
            None => (text, None),
        };

        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };

        let mut parts = core.split('.');
        let major = numeric_field(parts.next()?)?;
        let minor = numeric_field(parts.next()?)?;
        let patch = numeric_field(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }

        let pre = match pre {
            None => Vec::new(),
            Some(pre) => {
                let mut identifiers = Vec::new();
                for part in pre.split('.') {
                    identifiers.push(parse_pre_identifier(part)?);
                }
                identifiers
            }
        };

        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }

    /// Whether this is a final release rather than a pre-release.
    pub fn is_stable(&self) -> bool {
        self.pre.is_empty()
    }

    /// The first pre-release identifier as text, which is what names the
    /// stream: `beta` in `1.2.0-beta.3`.
    fn pre_stream(&self) -> Option<&str> {
        match self.pre.first() {
            Some(PreRelease::Alphanumeric(name)) => Some(name.as_str()),
            _ => None,
        }
    }

    /// `MAJOR.MINOR.PATCH[-PRE]`, the form the manifest carries and the dialog
    /// shows. Round-trips through [`Version::parse`].
    pub fn to_display(&self) -> String {
        let mut text = format!("{}.{}.{}", self.major, self.minor, self.patch);
        if !self.pre.is_empty() {
            text.push('-');
            let parts: Vec<String> = self
                .pre
                .iter()
                .map(|id| match id {
                    PreRelease::Numeric(n) => n.to_string(),
                    PreRelease::Alphanumeric(s) => s.clone(),
                })
                .collect();
            text.push_str(&parts.join("."));
        }
        text
    }
}

/// A core version field: digits only, and no leading zero (SemVer §2).
fn numeric_field(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if text.len() > 1 && text.starts_with('0') {
        return None;
    }
    text.parse().ok()
}

fn parse_pre_identifier(text: &str) -> Option<PreRelease> {
    if text.is_empty() {
        return None;
    }
    if text.bytes().all(|b| b.is_ascii_digit()) {
        // A numeric identifier may not carry a leading zero either.
        if text.len() > 1 && text.starts_with('0') {
            return None;
        }
        return text.parse().ok().map(PreRelease::Numeric);
    }
    if text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Some(PreRelease::Alphanumeric(text.to_owned()));
    }
    None
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| compare_pre(&self.pre, &other.pre))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// SemVer §11.3–11.4. An *absent* pre-release outranks a present one; beyond
/// that, identifiers compare field by field and a longer list wins a tie.
fn compare_pre(left: &[PreRelease], right: &[PreRelease]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }

    for (a, b) in left.iter().zip(right.iter()) {
        let ordering = match (a, b) {
            (PreRelease::Numeric(a), PreRelease::Numeric(b)) => a.cmp(b),
            (PreRelease::Alphanumeric(a), PreRelease::Alphanumeric(b)) => a.cmp(b),
            // "Numeric identifiers always have lower precedence than
            // alphanumeric identifiers."
            (PreRelease::Numeric(_), PreRelease::Alphanumeric(_)) => Ordering::Less,
            (PreRelease::Alphanumeric(_), PreRelease::Numeric(_)) => Ordering::Greater,
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }

    left.len().cmp(&right.len())
}

/// Which stream of releases the app follows. Serialized as the same lowercase
/// strings `update.json` uses, so a manifest's `channel` and the user's
/// configured channel are the same vocabulary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
    Nightly,
}

impl Channel {
    pub const ALL: [Channel; 3] = [Channel::Stable, Channel::Beta, Channel::Nightly];

    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
            Channel::Nightly => "nightly",
        }
    }

    /// Unknown text falls back to `stable`, the most conservative stream. A
    /// hand-edited config with a typo must not silently subscribe anyone to
    /// nightlies.
    pub fn from_code(code: &str) -> Channel {
        Channel::ALL
            .into_iter()
            .find(|c| c.as_str() == code)
            .unwrap_or(Channel::Stable)
    }

    /// Whether a version's *shape* belongs to this channel. See the module doc.
    pub fn accepts(self, version: &Version) -> bool {
        match self {
            Channel::Stable => version.is_stable(),
            Channel::Beta => {
                version.is_stable() || matches!(version.pre_stream(), Some("beta" | "rc"))
            }
            Channel::Nightly => true,
        }
    }
}

/// What a check concluded about one candidate version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateDecision {
    /// Newer, on this channel, and not skipped.
    Offer,
    /// Same as, or older than, what is running.
    UpToDate,
    /// Newer, but the user pressed **Skip this version** for exactly it.
    Skipped,
    /// Newer, but a pre-release this channel does not follow.
    WrongChannel,
}

/// The whole "should we offer this?" rule, in one pure function.
///
/// Order matters and is deliberate: **channel first, then skip, then
/// newness.** A skip records "not this one", so it must not also suppress the
/// version after it; and a version this channel would never take is reported as
/// such rather than as "up to date", which is a different thing to tell someone.
pub fn decide(
    current: &Version,
    candidate: &Version,
    channel: Channel,
    skipped: Option<&str>,
) -> UpdateDecision {
    if !channel.accepts(candidate) {
        return UpdateDecision::WrongChannel;
    }
    if candidate <= current {
        return UpdateDecision::UpToDate;
    }
    if skipped
        .and_then(Version::parse)
        .is_some_and(|skipped| skipped == *candidate)
    {
        return UpdateDecision::Skipped;
    }
    UpdateDecision::Offer
}

#[cfg(test)]
mod tests {
    use super::{Channel, PreRelease, UpdateDecision, Version, decide};

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap_or_else(|| panic!("`{text}` should parse"))
    }

    #[test]
    fn parses_the_ordinary_shapes() {
        assert_eq!(
            v("1.2.3"),
            Version {
                major: 1,
                minor: 2,
                patch: 3,
                pre: Vec::new()
            }
        );
        // A release tag carries a `v`; a manifest's `version` does not.
        assert_eq!(v("v0.1.6"), v("0.1.6"));
        assert_eq!(
            v("1.0.0-beta.2").pre,
            vec![
                PreRelease::Alphanumeric("beta".into()),
                PreRelease::Numeric(2)
            ]
        );
        // Build metadata parses and is then dropped, per SemVer §10.
        assert_eq!(v("1.0.0+build.5"), v("1.0.0"));
        assert_eq!(v("1.0.0-rc.1+build.5"), v("1.0.0-rc.1"));
    }

    #[test]
    fn refuses_what_it_cannot_be_sure_of() {
        for text in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "a.b.c",
            "1.2.-3",
            "01.2.3",
            "1.0.0-",
            "1.0.0-..1",
            "1.0.0+",
            "1.0.0-beta.01",
            "-1.0.0",
        ] {
            assert!(
                Version::parse(text).is_none(),
                "`{text}` must not parse — an updater that guesses is guessing about \
                 replacing the user's binary"
            );
        }
    }

    #[test]
    fn ordering_follows_the_core_fields_first() {
        assert!(v("2.0.0") > v("1.9.9"));
        assert!(v("1.10.0") > v("1.9.0"), "minor is numeric, not lexical");
        assert!(v("1.0.10") > v("1.0.9"));
        assert_eq!(v("1.0.0"), v("1.0.0"));
    }

    /// The subtle half of SemVer §11, and the reason this is not a string
    /// compare.
    #[test]
    fn a_pre_release_is_lower_than_the_release_it_leads_to() {
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"));
        assert!(v("1.0.0-alpha.1") < v("1.0.0-alpha.beta"));
        assert!(v("1.0.0-alpha.beta") < v("1.0.0-beta"));
        assert!(
            v("1.0.0-beta.2") < v("1.0.0-beta.11"),
            "numeric, not lexical"
        );
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
    }

    #[test]
    fn round_trips_through_display() {
        for text in [
            "0.1.6",
            "1.2.3",
            "1.0.0-rc.1",
            "2.0.0-beta.11",
            "1.0.0-x-y.3",
        ] {
            assert_eq!(v(text).to_display(), text);
            assert_eq!(v(&v(text).to_display()), v(text));
        }
    }

    // ---- Channels ----------------------------------------------------------

    #[test]
    fn stable_takes_only_final_releases() {
        assert!(Channel::Stable.accepts(&v("1.2.3")));
        for pre in ["1.2.3-rc.1", "1.2.3-beta.1", "1.2.3-nightly.20260730"] {
            assert!(
                !Channel::Stable.accepts(&v(pre)),
                "{pre} must not reach a stable user — this is the defence against \
                 being handed the wrong channel's manifest"
            );
        }
    }

    #[test]
    fn beta_is_a_superset_of_stable() {
        assert!(
            Channel::Beta.accepts(&v("1.2.3")),
            "stable is what beta becomes"
        );
        assert!(Channel::Beta.accepts(&v("1.2.3-beta.4")));
        assert!(Channel::Beta.accepts(&v("1.2.3-rc.1")));
        assert!(!Channel::Beta.accepts(&v("1.2.3-nightly.7")));
        assert!(!Channel::Beta.accepts(&v("1.2.3-alpha.1")));
    }

    #[test]
    fn nightly_takes_anything_that_parses() {
        for text in ["1.2.3", "1.2.3-beta.1", "1.2.3-nightly.7", "1.2.3-alpha.0"] {
            assert!(Channel::Nightly.accepts(&v(text)), "{text}");
        }
    }

    #[test]
    fn an_unknown_channel_name_falls_back_to_the_cautious_one() {
        assert_eq!(Channel::from_code("beta"), Channel::Beta);
        assert_eq!(Channel::from_code("nightly"), Channel::Nightly);
        assert_eq!(
            Channel::from_code("Nightly"),
            Channel::Stable,
            "case matters"
        );
        assert_eq!(Channel::from_code("bananas"), Channel::Stable);
        assert_eq!(Channel::from_code(""), Channel::Stable);
    }

    // ---- The decision ------------------------------------------------------

    #[test]
    fn a_newer_release_is_offered_on_every_channel_that_accepts_it() {
        for channel in Channel::ALL {
            assert_eq!(
                decide(&v("0.1.6"), &v("0.2.0"), channel, None),
                UpdateDecision::Offer,
                "{channel:?}"
            );
        }
    }

    #[test]
    fn the_same_or_an_older_version_is_up_to_date() {
        for channel in Channel::ALL {
            assert_eq!(
                decide(&v("0.1.6"), &v("0.1.6"), channel, None),
                UpdateDecision::UpToDate,
                "{channel:?}"
            );
            assert_eq!(
                decide(&v("0.1.6"), &v("0.1.5"), channel, None),
                UpdateDecision::UpToDate,
                "{channel:?} — a downgrade is not an update"
            );
        }
    }

    #[test]
    fn a_pre_release_reaches_only_the_channels_that_follow_it() {
        let current = v("0.1.6");
        let candidate = v("0.2.0-beta.1");
        assert_eq!(
            decide(&current, &candidate, Channel::Stable, None),
            UpdateDecision::WrongChannel
        );
        assert_eq!(
            decide(&current, &candidate, Channel::Beta, None),
            UpdateDecision::Offer
        );
        assert_eq!(
            decide(&current, &candidate, Channel::Nightly, None),
            UpdateDecision::Offer
        );
    }

    #[test]
    fn a_skip_suppresses_exactly_the_version_it_named() {
        let current = v("0.1.6");
        assert_eq!(
            decide(&current, &v("0.2.0"), Channel::Stable, Some("0.2.0")),
            UpdateDecision::Skipped
        );
        assert_eq!(
            decide(&current, &v("0.2.1"), Channel::Stable, Some("0.2.0")),
            UpdateDecision::Offer,
            "skipping one release must not silence the next"
        );
        // A tag-shaped skip records the same version.
        assert_eq!(
            decide(&current, &v("0.2.0"), Channel::Stable, Some("v0.2.0")),
            UpdateDecision::Skipped
        );
        // Junk in the config does not suppress anything.
        assert_eq!(
            decide(
                &current,
                &v("0.2.0"),
                Channel::Stable,
                Some("not-a-version")
            ),
            UpdateDecision::Offer
        );
    }

    /// Channel is checked before the skip, so a skipped pre-release on a stable
    /// channel reports the reason it would never have been offered anyway.
    #[test]
    fn the_channel_verdict_wins_over_the_skip() {
        assert_eq!(
            decide(
                &v("0.1.6"),
                &v("0.2.0-beta.1"),
                Channel::Stable,
                Some("0.2.0-beta.1")
            ),
            UpdateDecision::WrongChannel
        );
    }
}
