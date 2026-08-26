//! Deciding, from clipboard text alone, which tool should have it.
//!
//! This is the whole of quick navigation's judgement: a `&str` in, an
//! `Option<Route>` out, no GPUI, no clipboard, no window. Everything that can
//! go wrong with the feature goes wrong here, so everything here is a unit test.
//!
//! # The order, and why it is that order
//!
//! [`Detector::ORDER`] is the one authoritative list. It is **most specific
//! first**, and every position in it is forced by an overlap with something
//! below it:
//!
//! 1. **cURL** — first, because a `curl` command usually *contains* JSON in its
//!    `--data`. Tested after JSON, every `curl -d '{…}'` would open the
//!    formatter and the API Explorer would never see one. The command word at
//!    the front is also the single most specific signal any of these formats
//!    has, so nothing below can be mistaken for it.
//! 2. **Database URI** — second, because a URI is structurally unmistakable
//!    (`scheme://`) and because getting it wrong in either direction is
//!    expensive: a missed one is a connection the user has to retype, and a
//!    false one is a saved connection they did not ask for. It is therefore the
//!    strictest detector here — see [`Detector::SCHEMES`].
//! 3. **JWT** — before Base64, because **a JWT is Base64**: three dot-separated
//!    base64url segments. With Base64 first, every token would go to the wrong
//!    view. This is the ordering the whole list exists for.
//! 4. **JSON** — before Base64, so that a JSON document which is *entirely* a
//!    quoted Base64 string (`"aGVsbG8="`) formats rather than decodes. The
//!    quotes are not Base64 characters, so the two do not really compete, but
//!    the order states which reading wins.
//! 5. **Base64** — last, and the loosest by far. Plenty of ordinary words match
//!    a naive Base64 pattern, so this one is not allowed to accept on shape: it
//!    must decode canonically *and* produce readable UTF-8. See
//!    [`detect_base64`].
//! 6. **Mermaid** — after Base64, and the one position in this list *not*
//!    forced by an overlap: Mermaid source shares no shape with any format
//!    above it. It is never a single token (database URI's gate), never
//!    starts with `{`/`[`/`"` (JSON's gate), never starts with the word
//!    `curl`, and its own syntax — `-->`, `{`, `}` — falls outside both
//!    Base64 alphabets, so [`detect_base64`]'s canonical-decode step already
//!    refuses it before Mermaid is ever reached. Appended last is therefore
//!    the safe default: nothing above it can be affected by an addition
//!    nothing above it overlaps with, and no existing test in this module had
//!    to change for Mermaid to be added.
//!
//! When nothing matches confidently the answer is `None` and dodo does nothing.
//! A wrong jump throws away whatever the user was looking at; not jumping costs
//! them one click.
//!
//! **This order is not the sidebar's**, and the two must never be conflated.
//! The Features settings page lets the user drag the sidebar's tools into any
//! order they like; that is a preference. This list is a correctness property —
//! every position above is forced by an overlap with something below it, save
//! Mermaid's, argued where it sits — so [`detect_among`] iterates `ORDER`
//! whatever order its caller's slice of
//! allowed detectors is in. What the sidebar *does* decide is which detectors
//! are allowed at all: a tool the user switched off is not a paste target, so
//! its detector is skipped and the text falls through to the next one.
//!
//! # Patterns versus parsers — the design judgement in this module
//!
//! The captain asked for user-editable regexes. For cURL, database URIs and
//! JSON, dodo already owns a **tested parser** for exactly that format, and
//! every one of them is strictly more accurate than any pattern could be:
//! `services::curl::parse` understands shell quoting, `models::uri::parse`
//! understands four engines' URI dialects across 36 tests, and `serde_json` is
//! the definition of the format. Replacing one of those with a regex would be a
//! regression wearing configurability as a disguise. Mermaid joined this group
//! rather than the pattern-shaped one below for the same reason: `dodo-mermaid`
//! already owns a real parser (`mermaid-rs-renderer`, embedded — see that
//! crate's own module docs), and a regex could only ever approximate what a
//! successful render already proves.
//!
//! So the rule here, uniformly, is:
//!
//! > **A pattern selects candidates; the parser confirms.**
//!
//! - Where a real parser exists (cURL, database URI, JSON, Mermaid) the user's
//!   pattern is an optional **gate**: set it and the text must match before the
//!   parser is even attempted. Leave it empty — the default — and the parser is
//!   attempted directly. A gate can only ever narrow, so a pattern cannot make
//!   detection *wronger*, only quieter. Mermaid's own stage-1 keyword check
//!   ([`looks_like_mermaid`]) runs regardless of the gate — it is not
//!   user-editable, the same way `curl::parse`'s internal shell-quoting rules
//!   are not — and exists only to keep an obviously-not-Mermaid clipboard from
//!   paying for a render at all.
//! - Where the format genuinely is pattern-shaped (JWT, Base64) the pattern is
//!   the **shape test** and carries a real default the user may replace. The
//!   confirming step is still there: the JWT header must decode to a JOSE header
//!   and Base64 must decode canonically to readable text, so even a wide-open
//!   user pattern cannot produce a false jump.
//!
//! An invalid pattern falls back to the detector's default and is reported in
//! the Settings dialog — see [`super::pattern`], which also records what bounds
//! an untrusted pattern's cost.

use base64::alphabet;
use base64::engine::{DecodePaddingMode, Engine as _, GeneralPurpose, GeneralPurposeConfig};
use regex::Regex;

use crate::api_explorer::services::curl;
use crate::database::models::uri;
use crate::i18n::{Str, quick_nav};
use crate::mermaid::{DefaultMermaidRenderer, MermaidRenderer, MermaidTheme};

use super::config::QuickNavDocument;
use super::pattern::{self, PatternError};
use super::route::Route;

/// The largest clipboard this feature will look at, in bytes.
///
/// A clipboard can hold a whole file. Detection runs on the UI thread from a
/// key handler, so it is bounded rather than merely fast: above this, quick
/// navigation does nothing at all and the keystroke is the user's again. One
/// mebibyte is far larger than anything anyone pastes to *format*, and small
/// enough that the worst case here — one `serde_json` parse plus one linear
/// regex pass — stays imperceptible.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// The shortest Base64 that is taken seriously, in characters.
///
/// Below this the evidence is too thin to act on: four characters decode to
/// three bytes, and three readable bytes happen by accident.
const MIN_BASE64_LEN: usize = 8;

/// The shortest decoded Base64 payload that counts as a payload.
const MIN_BASE64_DECODED: usize = 3;

/// Canonical Base64, standard and URL-safe alphabets.
///
/// **Canonical on purpose**: `GeneralPurposeConfig::new` requires correct
/// padding, so a string whose length is not a multiple of four is refused. That
/// is the "correct padding/length" evidence Base64 detection needs, and it is
/// enforced by the decoder rather than by the pattern — so a user who loosens
/// the pattern still cannot make `abcdef` decode.
const B64_STANDARD: GeneralPurpose = GeneralPurpose::new(&alphabet::STANDARD, CANONICAL);
const B64_URL_SAFE: GeneralPurpose = GeneralPurpose::new(&alphabet::URL_SAFE, CANONICAL);
const CANONICAL: GeneralPurposeConfig = GeneralPurposeConfig::new();

/// A JWT's segments are base64url and conventionally **unpadded**, so the
/// header is decoded with a separate, padding-indifferent engine. This is the
/// same reading `encoder_decoder` uses to display the token.
const B64_JWT: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// One format quick navigation can recognise.
///
/// The enum is the extension point the captain asked for: a new tool that can
/// accept a paste adds a variant here, a row in [`Detector::ORDER`], its arm in
/// [`Detector::detect`], a [`Route`] variant, and an arm in
/// `Layout::apply_route`. There is no registry to register with and no dynamic
/// dispatch — an ordered list of six is not a plugin system and should not
/// grow into one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Detector {
    Curl,
    DatabaseUri,
    Jwt,
    Json,
    Base64,
    Mermaid,
}

impl Detector {
    /// **The detection order.** The module doc above is the reasoning; this is
    /// the list, and it is the only place the order is written down.
    pub const ORDER: [Detector; 6] = [
        Detector::Curl,
        Detector::DatabaseUri,
        Detector::Jwt,
        Detector::Json,
        Detector::Base64,
        Detector::Mermaid,
    ];

    /// The id a saved pattern is filed under in `quick-nav.json`, and the value
    /// the settings dropdowns would use if there ever were one. Stable
    /// identifiers, never localized labels — the same rule the other settings
    /// follow.
    pub fn code(self) -> &'static str {
        match self {
            Detector::Curl => "curl",
            Detector::DatabaseUri => "database-uri",
            Detector::Jwt => "jwt",
            Detector::Json => "json",
            Detector::Base64 => "base64",
            Detector::Mermaid => "mermaid",
        }
    }

    /// The Settings dialog's label for this detector's pattern field.
    pub fn label(self) -> Str {
        match self {
            Detector::Curl => quick_nav::Text::CurlPattern.into(),
            Detector::DatabaseUri => quick_nav::Text::DatabasePattern.into(),
            Detector::Jwt => quick_nav::Text::JwtPattern.into(),
            Detector::Json => quick_nav::Text::JsonPattern.into(),
            Detector::Base64 => quick_nav::Text::Base64Pattern.into(),
            Detector::Mermaid => quick_nav::Text::MermaidPattern.into(),
        }
    }

    /// Whether this detector's pattern is a **gate** in front of a real parser
    /// or the **shape test** itself. It decides which description the settings
    /// field carries, and it is the module doc's rule made checkable.
    pub fn has_parser(self) -> bool {
        self.default_pattern().is_none()
    }

    /// The pattern used when the user has set none — or has set one that does
    /// not compile.
    ///
    /// `None` for the four detectors backed by a real parser: their default is
    /// *no gate at all*, because attempting the parser is already the most
    /// accurate test available.
    pub fn default_pattern(self) -> Option<&'static str> {
        match self {
            Detector::Curl | Detector::DatabaseUri | Detector::Json | Detector::Mermaid => None,
            // Three dot-separated base64url segments. The signature may be
            // empty — that is what an `alg: none` token looks like — and `=` is
            // allowed because some encoders pad. The confirming decode is what
            // makes this safe to keep loose.
            Detector::Jwt => Some(r"^[A-Za-z0-9_=-]+\.[A-Za-z0-9_=-]+\.[A-Za-z0-9_=-]*$"),
            // One alphabet or the other, never a mixture. Length and padding are
            // deliberately *not* expressed here; the canonical decoder enforces
            // them and cannot be edited away.
            Detector::Base64 => Some(r"^(?:[A-Za-z0-9+/]+={0,2}|[A-Za-z0-9_-]+={0,2})$"),
        }
    }

    /// The URI schemes quick navigation will act on.
    ///
    /// Narrower than [`uri::parse`] accepts, and deliberately so. That function
    /// serves the connection form, where the user has already said "this is a
    /// connection URI" by pasting into a box labelled so; it therefore also
    /// accepts bare `file:`, SQLite's own URI-filename scheme. Here the text
    /// arrives with no such statement, and `file:///Users/me/report.pdf` copied
    /// out of a browser must not become a SQLite connection. So: the four
    /// engines' network schemes plus the explicit `sqlite` spellings, and
    /// nothing else.
    ///
    /// `http`/`https` were never in reach — `uri::parse` refuses them by name —
    /// but this list is what makes that a decision rather than an accident.
    const SCHEMES: [&'static str; 10] = [
        "postgres",
        "postgresql",
        "mysql",
        "mariadb",
        "redis",
        "rediss",
        "valkey",
        "valkeys",
        "sqlite",
        "sqlite3",
    ];

    /// The id handed to [`uri::parse`] during detection. The real id is assigned
    /// by the Database Explorer when it decides whether this is a connection it
    /// already has, so anything here would do; naming it says it is a
    /// placeholder rather than a meaningful zero.
    pub const PLACEHOLDER_ID: u64 = 0;

    /// Tries this one detector. `pattern` is the effective pattern from
    /// [`Patterns::pattern`] — the user's, or the default, or `None`.
    pub fn detect(self, text: &str, pattern: Option<&Regex>) -> Option<Route> {
        match self {
            Detector::Curl => detect_curl(text, pattern),
            Detector::DatabaseUri => detect_database_uri(text, pattern),
            Detector::Jwt => detect_jwt(text, pattern?),
            Detector::Json => detect_json(text, pattern),
            Detector::Base64 => detect_base64(text, pattern?),
            Detector::Mermaid => detect_mermaid(text, pattern),
        }
    }
}

/// The effective pattern for every detector, compiled once.
///
/// Built when the settings are loaded or edited, never from a key handler.
pub struct Patterns {
    entries: [Entry; Detector::ORDER.len()],
}

#[derive(Default)]
struct Entry {
    regex: Option<Regex>,
    error: Option<PatternError>,
}

impl Patterns {
    /// Compiles every detector's pattern from the saved settings.
    ///
    /// A pattern that does not compile is recorded in [`Patterns::error`] — the
    /// Settings dialog shows it — and the detector falls back to its default.
    /// It is never left with no pattern where it needs one, and the feature is
    /// never silently switched off.
    pub fn compile(document: &QuickNavDocument) -> Self {
        Self {
            entries: Detector::ORDER.map(|detector| {
                let (regex, error) = match pattern::compile(document.pattern(detector)) {
                    Ok(regex) => (regex, None),
                    Err(error) => (None, Some(error)),
                };

                let regex = regex.or_else(|| {
                    detector.default_pattern().map(|source| {
                        pattern::compile(source)
                            .ok()
                            .flatten()
                            .expect("a built-in default pattern must compile")
                    })
                });

                Entry { regex, error }
            }),
        }
    }

    fn entry(&self, detector: Detector) -> &Entry {
        let index = Detector::ORDER
            .iter()
            .position(|candidate| *candidate == detector)
            .expect("every detector is in ORDER");
        &self.entries[index]
    }

    pub fn pattern(&self, detector: Detector) -> Option<&Regex> {
        self.entry(detector).regex.as_ref()
    }

    /// What was wrong with the user's pattern for `detector`, if anything.
    pub fn error(&self, detector: Detector) -> Option<&PatternError> {
        self.entry(detector).error.as_ref()
    }
}

impl Default for Patterns {
    fn default() -> Self {
        Self::compile(&QuickNavDocument::default())
    }
}

/// Reads `text` and says where it should go, or `None` for "do nothing",
/// considering only the detectors in `allowed`.
///
/// **The sidebar's tool list is what narrows it.** A tool the user has switched
/// off is not a paste target: its detector is skipped entirely, so the text
/// falls through to the next one or nowhere at all. Re-enabling a tool the user
/// turned off — even for one keystroke, even helpfully — would be the app
/// overruling the setting, so the alternative was never on the table.
///
/// **`allowed` is a membership test and never an order.** The iteration is
/// [`Detector::ORDER`]'s, whatever order the caller's slice is in, because that
/// order is a correctness property — most specific first, every position forced
/// by an overlap below it — and the sidebar's is a preference. Dragging Base64
/// above JWT in the Features page must not change what a pasted token does.
pub fn detect_among(text: &str, patterns: &Patterns, allowed: &[Detector]) -> Option<Route> {
    if text.len() > MAX_INPUT_BYTES || text.trim().is_empty() {
        return None;
    }

    Detector::ORDER
        .into_iter()
        .filter(|detector| allowed.contains(detector))
        .find_map(|detector| detector.detect(text, patterns.pattern(detector)))
}

// ---- the six detectors ------------------------------------------------
//
// Each one normalizes the candidate its own way — trimmed, whitespace-stripped
// — and applies its pattern to *that*, so a user editing a Base64 pattern is
// writing about the same string the decoder will see.

/// Attempting the parser **is** the detector.
///
/// `curl::parse` starts with `looks_like_curl`, which examines only the first
/// word, so a command that is not one costs almost nothing. It returns `None`
/// for a command with no URL in it, which is the right answer here too: there
/// would be nothing to put in a tab.
fn detect_curl(text: &str, gate: Option<&Regex>) -> Option<Route> {
    if !passes(gate, text.trim()) {
        return None;
    }
    curl::parse(text).map(|snapshot| Route::Curl(Box::new(snapshot)))
}

/// Attempting the parser is the detector here too, behind two cheap guards that
/// keep prose out of it: the text must be a single token, and its scheme must be
/// one of [`Detector::SCHEMES`].
///
/// The single-token rule matters because `uri::parse` reads whatever follows the
/// authority as a path — `postgres://db/shop and then some notes` would parse,
/// with a database called `shop and then some notes`. A URI has no spaces in it.
fn detect_database_uri(text: &str, gate: Option<&Regex>) -> Option<Route> {
    let candidate = text.trim();
    if !passes(gate, candidate) || candidate.split_whitespace().count() != 1 {
        return None;
    }

    let scheme = candidate.split_once(':')?.0.to_ascii_lowercase();
    if !Detector::SCHEMES.contains(&scheme.as_str()) {
        return None;
    }

    uri::parse(candidate, Detector::PLACEHOLDER_ID)
        .ok()
        .map(|parsed| Route::Database(Box::new(parsed)))
}

/// Shape, then evidence.
///
/// The evidence is the **JOSE header**: RFC 7515 requires the first segment to
/// decode to a JSON object carrying `alg`, and that is precisely what tells a
/// real token from three base64url words separated by dots. The payload is not
/// checked — a token whose payload is not JSON is still a token, and the JWT
/// view says so far better than silence would.
fn detect_jwt(text: &str, shape: &Regex) -> Option<Route> {
    let token = text.trim();
    if !shape.is_match(token) {
        return None;
    }

    let header = token.split('.').next()?;
    let bytes = B64_JWT.decode(header.as_bytes()).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get("alg")?.as_str()?;

    Some(Route::Jwt(token.to_owned()))
}

/// `serde_json` is the parser, behind one guard: the text must *start* like a
/// JSON document.
///
/// Without that guard `42`, `true` and `null` are all valid JSON documents, and
/// routing a bare number to the formatter would be a jump that achieves nothing.
/// `"` is included so that a document which is entirely a quoted string — the
/// quoted-Base64 case — still formats.
fn detect_json(text: &str, gate: Option<&Regex>) -> Option<Route> {
    let candidate = text.trim();
    if !passes(gate, candidate) {
        return None;
    }
    if !candidate.starts_with(['{', '[', '"']) {
        return None;
    }

    serde_json::from_str::<serde_json::Value>(candidate).ok()?;
    Some(Route::Json(candidate.to_owned()))
}

/// The loosest format, and therefore the one asked for the most evidence.
///
/// All four conditions have to hold:
///
/// 1. at least [`MIN_BASE64_LEN`] characters once wrapping whitespace is gone;
/// 2. the shape pattern matches — one alphabet, not a mixture;
/// 3. it decodes under a **canonical** engine, so the length and padding are
///    right rather than merely plausible;
/// 4. the bytes are UTF-8 and read as text.
///
/// Condition 4 is what rejects `password`, `computer` and every other
/// eight-letter word: they decode to bytes that are not valid UTF-8. Conditions
/// 3 and 4 live in code rather than in the pattern, so loosening the pattern
/// cannot produce a false jump.
fn detect_base64(text: &str, shape: &Regex) -> Option<Route> {
    let candidate: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if candidate.chars().count() < MIN_BASE64_LEN || !shape.is_match(&candidate) {
        return None;
    }

    // Which alphabet the text is written in, where it says so. A string using
    // only `[A-Za-z0-9]` decodes identically under both, so standard is tried
    // first and url-safe is the fallback rather than a guess.
    let url_safe = candidate.contains(['-', '_']);
    let engine = if url_safe {
        &B64_URL_SAFE
    } else {
        &B64_STANDARD
    };

    let bytes = engine.decode(candidate.as_bytes()).ok()?;
    let decoded = String::from_utf8(bytes).ok()?;
    if decoded.chars().count() < MIN_BASE64_DECODED || !decoded.chars().all(is_ordinary_text) {
        return None;
    }

    Some(Route::Base64 {
        text: candidate,
        url_safe,
    })
}

/// Mermaid's own first-line keywords — the candidates [`looks_like_mermaid`]
/// tests for, aligned with `mermaid-rs-renderer` 0.3.1. Not required to be
/// exhaustive of every reserved word the renderer understands: this is a
/// candidate filter, not the test, so a keyword this list misses only costs a
/// render that would have succeeded anyway — the same "quieter, never
/// wronger" property a user's gate pattern has.
const MERMAID_KEYWORDS: [&str; 28] = [
    "flowchart",
    "graph",
    "sequenceDiagram",
    "classDiagram",
    "stateDiagram-v2",
    "stateDiagram",
    "erDiagram",
    "gantt",
    "journey",
    "timeline",
    "mindmap",
    "gitGraph",
    "pie",
    "quadrantChart",
    "xychart-beta",
    "sankey-beta",
    "kanban",
    "C4Context",
    "C4Container",
    "C4Component",
    "C4Deployment",
    "block-beta",
    "architecture-beta",
    "requirementDiagram",
    "zenuml",
    "packet-beta",
    "radar-beta",
    "treemap",
];

/// Mermaid's direction tokens — the only words `graph`/`flowchart` may be
/// followed by on their opening line.
///
/// Without this, `graph database is unavailable` would pass a bare prefix
/// check: it does start with `graph`. `mermaid-rs-renderer` is lenient enough
/// — a bare `flowchart` with nothing else is a valid, empty diagram — that
/// stage 2 cannot be trusted to refuse it either, so the direction check is
/// what actually keeps this detector's promise of "low false-positive".
const MERMAID_DIRECTIONS: [&str; 5] = ["TB", "TD", "BT", "RL", "LR"];

/// Stage 1: does `candidate` open the way a Mermaid diagram does.
///
/// A word-boundary prefix match — `graphical` must not match `graph` — with
/// one extra rule for `graph`/`flowchart` alone, argued at
/// [`MERMAID_DIRECTIONS`]. Cheap on purpose: this runs before the parser in
/// [`detect_mermaid`], on every clipboard string quick navigation considers.
fn looks_like_mermaid(candidate: &str) -> bool {
    for keyword in MERMAID_KEYWORDS {
        let Some(rest) = candidate.strip_prefix(keyword) else {
            continue;
        };
        if rest.starts_with(|c: char| c.is_alphanumeric() || c == '-' || c == '_') {
            continue; // `keyword` was only a prefix of a longer word.
        }
        if keyword != "graph" && keyword != "flowchart" {
            return true;
        }
        let opening_line = rest.split(['\n', '\r']).next().unwrap_or("").trim();
        return opening_line.is_empty() || MERMAID_DIRECTIONS.contains(&opening_line);
    }
    false
}

/// Strips a UTF-8 BOM and surrounding whitespace, then — only when the fence's
/// language is exactly `mermaid` or `mmd` — one outer Markdown code fence.
///
/// Every other fenced block is left alone: a ` ```rust ` fence around some
/// other language is not Mermaid source with an unlucky wrapper, and
/// stripping it on a hunch would turn ordinary code snippets into false
/// positives.
fn strip_mermaid_fence(text: &str) -> &str {
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    let Some(opened) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let Some((language, rest)) = opened.split_once('\n') else {
        return trimmed;
    };
    if !matches!(language.trim(), "mermaid" | "mmd") {
        return trimmed;
    }
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

/// Attempting the parser is the detector, same shape as cURL and the database
/// URI — except the "parser" here is a full `mermaid-rs-renderer` render
/// rather than a validate-only call, because `dodo-mermaid` exposes no cheaper
/// one (see its own module docs). That is safe for the same reason
/// [`MAX_INPUT_BYTES`] is safe for JSON: a render measures in microseconds on
/// `dodo-mermaid`'s own benchmarks, and [`looks_like_mermaid`] has already
/// turned away everything that obviously is not a candidate, so this only
/// runs for text that already looks like an opening Mermaid line.
fn detect_mermaid(text: &str, gate: Option<&Regex>) -> Option<Route> {
    if !passes(gate, text.trim()) {
        return None;
    }
    let candidate = strip_mermaid_fence(text);
    if !looks_like_mermaid(candidate) {
        return None;
    }
    // This module is deliberately GPUI-free (see its own doc), so there is no
    // window to read dodo's active appearance from — and it does not matter:
    // confirmation only needs a successful render, and `open_tab`'s own first
    // real render (`MermaidView::schedule_render`) is what actually chooses
    // light or dark for the pane the user will see.
    DefaultMermaidRenderer
        .render(candidate, MermaidTheme::default())
        .ok()?;
    Some(Route::Mermaid(candidate.to_owned()))
}

/// Whether a decoded character reads as text rather than as a byte that
/// happened to be valid UTF-8. Tab, newline and carriage return are text; every
/// other control character is not.
fn is_ordinary_text(c: char) -> bool {
    !c.is_control() || matches!(c, '\t' | '\n' | '\r')
}

/// Applies an optional gate. No gate is an open gate — the parser behind it is
/// the real test.
fn passes(gate: Option<&Regex>, candidate: &str) -> bool {
    gate.is_none_or(|regex| regex.is_match(candidate))
}

#[cfg(test)]
mod tests {
    use super::{Detector, MAX_INPUT_BYTES, Patterns, detect_among};
    use crate::i18n::Language;
    use crate::quick_nav::models::config::QuickNavDocument;
    use crate::quick_nav::models::route::Route;

    /// Detection with every detector in play — what the app does when the user
    /// has switched nothing off, and the baseline the narrowing tests below are
    /// measured against.
    fn detect(text: &str, patterns: &Patterns) -> Option<Route> {
        detect_among(text, patterns, &Detector::ORDER)
    }

    /// A real token: `{"alg":"HS256","typ":"JWT"}` over
    /// `{"sub":"1234567890","name":"Ada Lovelace","iat":1516239022}`.
    const JWT: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
                       eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkFkYSBMb3ZlbGFjZSIsImlhdCI6MTUxNjIzOTAyMn0.\
                       SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";

    fn route(text: &str) -> Option<Route> {
        detect(text, &Patterns::default())
    }

    /// Which detector claimed the text — the thing most of these tests are
    /// really asserting.
    fn detector_of(text: &str) -> Option<Detector> {
        route(text).map(|route| route.detector())
    }

    // ---- the ambiguous cases, which are the point of the order ------------

    /// The case the whole ordering exists for: a JWT *is* Base64, so Base64
    /// first would send every token to the wrong view.
    #[test]
    fn a_jwt_goes_to_the_jwt_view_and_not_to_base64() {
        assert_eq!(detector_of(JWT), Some(Detector::Jwt));
        assert_eq!(route(JWT), Some(Route::Jwt(JWT.to_owned())));
        assert!(
            Detector::ORDER.iter().position(|d| *d == Detector::Jwt)
                < Detector::ORDER.iter().position(|d| *d == Detector::Base64),
            "JWT must be tried before Base64",
        );
    }

    /// The other case the ordering exists for: a cURL command carrying a JSON
    /// body must open the API Explorer, not the formatter.
    #[test]
    fn a_curl_command_with_a_json_body_goes_to_the_api_explorer() {
        let text = r#"curl -X POST https://api.example.com/v1/orders \
  -H 'Content-Type: application/json' \
  -d '{"item":"widget","quantity":3}'"#;

        let Some(Route::Curl(snapshot)) = route(text) else {
            panic!("expected a cURL route, got {:?}", detector_of(text));
        };
        assert_eq!(snapshot.url, "https://api.example.com/v1/orders");
        assert!(
            snapshot.body.text.contains("widget"),
            "the JSON body has to survive into the request: {:?}",
            snapshot.body,
        );
        assert!(
            Detector::ORDER.iter().position(|d| *d == Detector::Curl)
                < Detector::ORDER.iter().position(|d| *d == Detector::Json),
            "cURL must be tried before JSON",
        );
    }

    /// A JSON document that is entirely a quoted Base64 string formats; the
    /// quotes are not Base64 characters, so nothing decodes it by accident.
    #[test]
    fn a_json_document_that_is_only_a_quoted_base64_string_formats() {
        assert_eq!(
            route("\"aGVsbG8gd29ybGQ=\""),
            Some(Route::Json("\"aGVsbG8gd29ybGQ=\"".to_owned()))
        );
    }

    /// A plain web URL is not a database URI and must not create a connection —
    /// nor be mistaken for anything else.
    #[test]
    fn a_plain_https_url_routes_nowhere() {
        for url in [
            "https://example.com",
            "https://example.com/some/path?q=1",
            "http://localhost:8080/health",
            "mongodb://db.example.com/app",
            "file:///Users/me/Downloads/report.pdf",
        ] {
            assert_eq!(route(url), None, "{url} must not route anywhere");
        }
    }

    /// The Base64 detector is the loose one, so this is the test that keeps it
    /// honest.
    #[test]
    fn a_bare_word_that_looks_like_base64_routes_nowhere() {
        for word in [
            "password",
            "computer",
            "deadbeef",
            "abcdefgh",
            "testtest",
            "keyboard",
            "language",
            "overflow",
            "selected",
            "AAAAAAAA",
            "documents",
        ] {
            assert_eq!(route(word), None, "{word} must not decode");
        }
    }

    // ---- each format on its own -------------------------------------------

    #[test]
    fn json_routes_to_the_formatter() {
        assert_eq!(
            route(r#"  {"name":"Ada","age":36}  "#),
            Some(Route::Json(r#"{"name":"Ada","age":36}"#.to_owned())),
            "the text is trimmed on the way through",
        );
        assert_eq!(detector_of("[1, 2, 3]"), Some(Detector::Json));
    }

    /// A bare scalar is valid JSON and formatting it achieves nothing, so it is
    /// not a jump worth making.
    #[test]
    fn a_bare_json_scalar_is_not_worth_a_jump() {
        for scalar in ["42", "true", "null", "-1.5e10"] {
            assert_eq!(route(scalar), None, "{scalar} must not route");
        }
    }

    #[test]
    fn base64_routes_to_the_decoder_with_the_alphabet_it_is_written_in() {
        assert_eq!(
            route("SGVsbG8sIHdvcmxkIQ=="),
            Some(Route::Base64 {
                text: "SGVsbG8sIHdvcmxkIQ==".to_owned(),
                url_safe: false,
            })
        );
        // `-` and `_` say url-safe. This is `{"a":"b?c>d"}` encoded url-safely.
        assert_eq!(
            route("eyJhIjoiYj9jPmQifQ=="),
            Some(Route::Base64 {
                text: "eyJhIjoiYj9jPmQifQ==".to_owned(),
                url_safe: false,
            }),
            "no url-safe-only character means the standard alphabet",
        );
        assert_eq!(
            route("Pz8_Pj4-Pz8_"),
            Some(Route::Base64 {
                text: "Pz8_Pj4-Pz8_".to_owned(),
                url_safe: true,
            })
        );
    }

    /// Base64 is routinely pasted with the wrapping a terminal or a PEM file put
    /// in. Newlines are not data.
    #[test]
    fn wrapped_base64_is_unwrapped_before_it_is_judged() {
        assert_eq!(
            route("SGVsbG8sIHdv\ncmxkIQ==\n"),
            Some(Route::Base64 {
                text: "SGVsbG8sIHdvcmxkIQ==".to_owned(),
                url_safe: false,
            })
        );
    }

    /// The canonical decoder is what enforces length and padding, and it is not
    /// reachable from the pattern.
    #[test]
    fn base64_with_the_wrong_length_or_padding_is_refused() {
        for text in ["SGVsbG8sIHdvcmxkIQ", "SGVsbG8sIHdvcmxkIQ=", "YWJjZGVmZ2g"] {
            assert_eq!(route(text), None, "{text} is not canonical Base64");
        }
    }

    #[test]
    fn base64_that_decodes_to_bytes_rather_than_text_is_refused() {
        // Eight bytes of 0xFF: canonical, decodes, and is not UTF-8.
        assert_eq!(route("//////////8="), None);
        // Valid UTF-8 but full of control characters.
        assert_eq!(route("AAECAwQFBgc="), None);
    }

    #[test]
    fn every_database_engine_routes_to_the_database_explorer() {
        for (uri, host) in [
            (
                "postgresql://alice:pw@db.example.com:6543/shop",
                "db.example.com",
            ),
            ("postgres://alice@db/shop", "db"),
            ("mysql://root:pw@127.0.0.1:3307/app", "127.0.0.1"),
            ("mariadb://root@127.0.0.1/app", "127.0.0.1"),
            ("redis://:pw@cache.internal:6380/3", "cache.internal"),
            ("rediss://cache.internal:6380/2", "cache.internal"),
            ("valkey://cache/1", "cache"),
        ] {
            let Some(Route::Database(parsed)) = route(uri) else {
                panic!("{uri} did not route to the database explorer");
            };
            assert_eq!(parsed.profile.host, host, "{uri}");
        }

        let Some(Route::Database(parsed)) = route("sqlite:///tmp/app.db") else {
            panic!("a sqlite: URI has to route");
        };
        assert_eq!(parsed.profile.file, "/tmp/app.db");
    }

    /// A URI has no spaces in it. Without this, prose that happens to start with
    /// a scheme would parse, and the trailing words would become a database
    /// name.
    #[test]
    fn a_sentence_that_starts_with_a_uri_is_not_a_uri() {
        assert_eq!(route("postgres://db/shop and then some notes"), None);
    }

    #[test]
    fn a_curl_command_with_no_url_routes_nowhere() {
        assert_eq!(route("curl --version"), None);
        assert_eq!(route("curl"), None);
    }

    // ---- Mermaid ------------------------------------------------------------

    #[test]
    fn a_flowchart_routes_to_the_mermaid_workspace() {
        let text = "flowchart LR\n  A[Request] --> B{Auth}\n  B --> C[API]";
        assert_eq!(route(text), Some(Route::Mermaid(text.to_owned())));
    }

    #[test]
    fn a_sequence_diagram_routes_to_the_mermaid_workspace() {
        let text = "sequenceDiagram\n  Alice->>Bob: Hello\n";
        assert_eq!(detector_of(text), Some(Detector::Mermaid));
    }

    /// A ```mermaid fence is very-high-confidence per the workspace plan: the
    /// fence itself says what the content is, so the stripped source — not
    /// the fence — is what lands in the new tab.
    #[test]
    fn a_fenced_mermaid_block_is_unwrapped_before_it_is_routed() {
        let fenced = "```mermaid\nerDiagram\n    USER ||--o{ ORDER : places\n```";
        assert_eq!(
            route(fenced),
            Some(Route::Mermaid(
                "erDiagram\n    USER ||--o{ ORDER : places".to_owned()
            )),
        );
    }

    #[test]
    fn an_mmd_fenced_block_is_also_unwrapped() {
        let fenced = "```mmd\ngraph TD\n  A --> B\n```";
        assert_eq!(
            route(fenced),
            Some(Route::Mermaid("graph TD\n  A --> B".to_owned())),
        );
    }

    /// A fence around any other language is left exactly as it is — it is not
    /// Mermaid source with an unlucky wrapper, and the fenced text here does
    /// not even look like Mermaid once the (unstripped) fence is considered.
    #[test]
    fn a_non_mermaid_fence_is_not_stripped_or_routed() {
        let fenced = "```rust\nflowchart LR\n  A --> B\n```";
        assert_eq!(route(fenced), None);
    }

    /// The plan's own negative examples: prose that merely mentions a diagram
    /// word, a bare arrow, and — the case stage 1's direction check exists
    /// for — `graph` followed by an ordinary sentence rather than a
    /// direction.
    #[test]
    fn mermaid_looking_prose_routes_nowhere() {
        for text in [
            "Please update this flowchart tomorrow.",
            "A --> B",
            "graph database is unavailable",
        ] {
            assert_eq!(route(text), None, "{text} must not route to Mermaid");
        }
    }

    /// Mermaid must not steal a paste that belongs to an existing detector.
    /// The two overlap-shaped ones, JSON and Base64, are the ones actually
    /// worth checking: neither's clipboard shape happens to start with a
    /// Mermaid keyword, but this is what would notice if it ever did.
    #[test]
    fn mermaid_does_not_steal_json_or_curl_or_database_pastes() {
        assert_eq!(
            detector_of("{\n  \"graph\": \"LR\"\n}"),
            Some(Detector::Json)
        );
        assert_eq!(
            detector_of("curl https://example.com"),
            Some(Detector::Curl),
            "a real cURL command must still win its own detector",
        );
        assert_eq!(
            detector_of("postgresql://alice@db/shop"),
            Some(Detector::DatabaseUri),
        );
    }

    #[test]
    fn nothing_at_all_routes_nowhere() {
        assert_eq!(route(""), None);
        assert_eq!(route("   \n\t "), None);
        assert_eq!(route("just some notes I copied"), None);
        assert_eq!(route("Lorem ipsum dolor sit amet."), None);
    }

    /// Detection runs on the UI thread, so it is bounded rather than merely
    /// fast.
    #[test]
    fn an_enormous_clipboard_is_left_alone() {
        let huge = format!("[{}]", "1,".repeat(MAX_INPUT_BYTES / 2 + 1));
        assert!(huge.len() > MAX_INPUT_BYTES);
        assert_eq!(detect(&huge, &Patterns::default()), None);
    }

    // ---- patterns ---------------------------------------------------------

    /// A gate can only narrow. With one set that the text fails, the parser is
    /// never attempted and the next detector gets its turn.
    #[test]
    fn a_gate_pattern_narrows_a_parser_backed_detector() {
        let mut document = QuickNavDocument::default();
        document.set_pattern(Detector::DatabaseUri, "^postgres");
        let patterns = Patterns::compile(&document);

        assert!(detect("postgresql://alice@db/shop", &patterns).is_some());
        assert_eq!(
            detect("mysql://root@db/app", &patterns),
            None,
            "the gate excluded it, so nothing else claimed it either",
        );
    }

    #[test]
    fn a_shape_pattern_replaces_the_default_for_jwt_and_base64() {
        let mut document = QuickNavDocument::default();
        // Only tokens whose header segment starts `eyJhbGciOiJIUzI1NiI` (HS256).
        document.set_pattern(Detector::Jwt, r"^eyJhbGciOiJIUzI1NiI[A-Za-z0-9_=-]*\..*$");
        let patterns = Patterns::compile(&document);

        assert!(detect(JWT, &patterns).is_some());
        // `{"alg":"RS512"}` as a header — a real token, excluded by the pattern.
        let rs512 = "eyJhbGciOiJSUzUxMiJ9.eyJzdWIiOiIxIn0.c2ln";
        assert!(detect(rs512, &Patterns::default()).is_some());
        assert_eq!(detect(rs512, &patterns), None);
    }

    /// A pattern that does not compile must not panic, must not switch the
    /// feature off, and must be reportable.
    #[test]
    fn an_invalid_pattern_falls_back_to_the_default_and_is_reported() {
        let mut document = QuickNavDocument::default();
        document.set_pattern(Detector::Jwt, "(unclosed");
        document.set_pattern(Detector::Curl, "(also unclosed");
        let patterns = Patterns::compile(&document);

        assert!(patterns.error(Detector::Jwt).is_some());
        assert!(patterns.error(Detector::Curl).is_some());
        assert!(patterns.error(Detector::Json).is_none());

        // …and detection still works exactly as it did with no pattern at all.
        assert_eq!(detect(JWT, &patterns), Some(Route::Jwt(JWT.to_owned())));
        assert!(detect("curl https://example.com", &patterns).is_some());
    }

    /// Even a wide-open shape pattern cannot produce a false jump, because the
    /// confirming decode is in code rather than in the pattern.
    #[test]
    fn a_wide_open_shape_pattern_still_cannot_decode_a_word() {
        let mut document = QuickNavDocument::default();
        document.set_pattern(Detector::Base64, ".*");
        document.set_pattern(Detector::Jwt, ".*");
        let patterns = Patterns::compile(&document);

        assert_eq!(detect("password", &patterns), None);
        assert_eq!(detect("not.a.token", &patterns), None);
        // The real ones still work.
        assert!(detect("SGVsbG8sIHdvcmxkIQ==", &patterns).is_some());
        assert!(detect(JWT, &patterns).is_some());
    }

    // ---- narrowing to the tools the sidebar still lists ---------------------

    /// A detector left out of `allowed` is not tried, and the text goes to the
    /// next one that can read it. `layout` decides what is allowed; this is the
    /// rule underneath that decision.
    #[test]
    fn a_detector_that_is_not_allowed_is_skipped_entirely() {
        let patterns = Patterns::default();
        let curl = "curl -X POST https://example.com -d '{\"a\":1}'";

        assert_eq!(
            detect_among(curl, &patterns, &Detector::ORDER).map(|r| r.detector()),
            Some(Detector::Curl),
        );

        let without_curl = [
            Detector::DatabaseUri,
            Detector::Jwt,
            Detector::Json,
            Detector::Base64,
        ];
        assert_eq!(detect_among(curl, &patterns, &without_curl), None);
        assert_eq!(
            detect_among("{\"a\":1}", &patterns, &without_curl).map(|r| r.detector()),
            Some(Detector::Json),
            "the body on its own still has somewhere to go",
        );
    }

    #[test]
    fn allowing_nothing_routes_nothing() {
        let patterns = Patterns::default();
        for text in [JWT, "{\"a\":1}", "SGVsbG8sIHdvcmxkIQ=="] {
            assert_eq!(detect_among(text, &patterns, &[]), None);
        }
    }

    /// **`allowed` is a membership test, never an order.** The Features page
    /// lets the user drag the sidebar into any order they like, and the caller
    /// may well hand its detectors over in that order — but a JWT is Base64,
    /// and only `ORDER` keeps it out of the decoder.
    #[test]
    fn the_allowed_list_cannot_reorder_detection() {
        let patterns = Patterns::default();
        let mut backwards = Detector::ORDER;
        backwards.reverse();

        assert_eq!(
            detect_among(JWT, &patterns, &backwards).map(|r| r.detector()),
            Some(Detector::Jwt),
        );
        assert_eq!(
            detect_among(
                "curl https://example.com -d '{\"a\":1}'",
                &patterns,
                &backwards
            )
            .map(|r| r.detector()),
            Some(Detector::Curl),
        );
        // Base64 listed first still does not get to claim a token.
        assert_eq!(
            detect_among(JWT, &patterns, &[Detector::Base64, Detector::Jwt]).map(|r| r.detector()),
            Some(Detector::Jwt),
        );
    }

    /// A duplicate in `allowed` is a caller's mistake, not a second attempt.
    #[test]
    fn a_repeated_detector_is_still_tried_once() {
        let patterns = Patterns::default();
        assert_eq!(
            detect_among(JWT, &patterns, &[Detector::Jwt, Detector::Jwt]),
            Some(Route::Jwt(JWT.to_owned())),
        );
    }

    // ---- the list itself --------------------------------------------------

    #[test]
    fn the_order_is_most_specific_first_and_lists_every_detector_once() {
        assert_eq!(
            Detector::ORDER,
            [
                Detector::Curl,
                Detector::DatabaseUri,
                Detector::Jwt,
                Detector::Json,
                Detector::Base64,
                Detector::Mermaid,
            ]
        );

        let mut codes: Vec<&str> = Detector::ORDER.iter().map(|d| d.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), Detector::ORDER.len(), "codes must be unique");
    }

    /// The parser-backed detectors are exactly the ones with no default
    /// pattern. That is the module's rule, stated once and checked here.
    #[test]
    fn only_the_pattern_shaped_formats_carry_a_default_pattern() {
        for detector in Detector::ORDER {
            let expected = matches!(detector, Detector::Jwt | Detector::Base64);
            assert_eq!(
                detector.default_pattern().is_some(),
                expected,
                "{} is on the wrong side of the parser/pattern rule",
                detector.code(),
            );
            assert_eq!(detector.has_parser(), !expected);
        }
    }

    #[test]
    fn every_built_in_default_pattern_compiles() {
        let patterns = Patterns::default();
        for detector in Detector::ORDER {
            assert!(patterns.error(detector).is_none());
            assert_eq!(
                patterns.pattern(detector).is_some(),
                detector.default_pattern().is_some(),
            );
        }
    }

    #[test]
    fn every_detector_is_named_in_every_language() {
        for detector in Detector::ORDER {
            for language in Language::ALL {
                assert!(
                    !detector.label().text(language).trim().is_empty(),
                    "{} has no label in {}",
                    detector.code(),
                    language.code(),
                );
            }
        }
    }
}
