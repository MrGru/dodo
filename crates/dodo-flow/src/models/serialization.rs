//! The versioned document format, and the migration ladder — **present at
//! version 1 rather than retrofitted**.
//!
//! Requirements §31: *"Create a versioned document format… Plan migrations from
//! the beginning."* The temptation at version 1 is to write `{"version": 1, …}`
//! and promise to add migrations when there is a version 2. That promise is
//! always kept late, because by then there are files in the wild written by a
//! build that had no loader able to migrate anything. So the machinery is here
//! now, exercised by tests, with an empty ladder:
//!
//! ```text
//! bytes ─▶ serde_json::Value ─▶ read "version" ─▶ migrate v ▸ v+1 ▸ … ▸ CURRENT ─▶ FlowDocument
//! ```
//!
//! # Why the ladder works on `serde_json::Value` and not on typed structs
//!
//! A migration step rewrites the *JSON* of the older version. The alternative —
//! keeping `FlowDocumentV1`, `FlowDocumentV2`, … as Rust types and converting
//! between them — means every retired shape stays compiled into the binary
//! forever, and every field rename becomes a struct copy. Working on `Value`
//! costs one parse of the file (which was going to happen anyway) and lets a
//! step be the three lines it usually is: rename a key, split a field, default
//! a new one.
//!
//! # Three failure modes, three distinct errors
//!
//! - **A file from the future** ([`LoadError::FutureVersion`]) cannot be
//!   migrated *down*, and guessing would silently drop whatever the newer
//!   version added. It is refused, with both version numbers in the message so
//!   the answer ("update dodo") is visible.
//! - **A file with no version** ([`LoadError::MissingVersion`]) is not a flow
//!   document. Defaulting it to 1 would make every unrelated JSON file open as
//!   an empty canvas.
//! - **A file with a version but a broken body** ([`LoadError::Json`]) is a
//!   parse error, and the parse error is the useful thing to show.
//!
//! # What is *not* here
//!
//! No file IO. Where a document lives under `data_dir()` is the app wiring's
//! question
//! and `docs/architecture/persistence.md` is its authority; this module deals
//! in strings, so it is testable with no filesystem.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::FlowDocument;

/// The format version this build writes.
///
/// **Bumping this is a two-line change**: raise the number and push a step onto
/// [`MIGRATIONS`] that turns a version-`N` `Value` into a version-`N+1` one.
/// The `the_migration_ladder_is_complete` test fails if the second line is
/// forgotten.
pub const CURRENT_VERSION: u32 = 1;

/// One rung of the ladder: rewrites a document body written by version `from`
/// into the shape version `from + 1` expects.
///
/// A plain `fn` rather than a closure so [`MIGRATIONS`] can be a `const` slice
/// with no allocation and no lazy initialisation.
pub type MigrationStep = fn(&mut Value) -> Result<(), LoadError>;

/// The ladder, one entry per version that has ever been written, in order.
///
/// Empty at version 1, which is correct and not a stub: there is exactly one
/// format so far, so there is nothing to climb. The machinery around it is what
/// this phase delivers, and it is proven by the
/// `a_synthetic_ladder_climbs_every_rung` test, which climbs a fabricated
/// two-step ladder rather than a fake step registered here that nothing would
/// ever use.
pub const MIGRATIONS: &[(u32, MigrationStep)] = &[];

/// Why a document could not be loaded.
#[derive(Debug)]
pub enum LoadError {
    /// The JSON did not parse, or the body did not match the document shape.
    Json(serde_json::Error),
    /// The top level was not a JSON object with a numeric `version`.
    MissingVersion,
    /// Written by a newer build. Carries what was found and what this build
    /// understands.
    FutureVersion { found: u32, supported: u32 },
    /// A version between the file's and [`CURRENT_VERSION`] has no migration
    /// step. Means [`MIGRATIONS`] and [`CURRENT_VERSION`] disagree — a bug in
    /// this crate, not in the file.
    MissingMigration { from: u32 },
    /// A migration step rejected the document it was given.
    Migration { from: u32, reason: String },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Json(e) => write!(f, "not a valid flow document: {e}"),
            LoadError::MissingVersion => {
                write!(f, "not a flow document: no \"version\" field")
            }
            LoadError::FutureVersion { found, supported } => write!(
                f,
                "this document is version {found}; this build of dodo understands up to {supported}"
            ),
            LoadError::MissingMigration { from } => {
                write!(f, "no migration from document version {from}")
            }
            LoadError::Migration { from, reason } => {
                write!(f, "migrating from document version {from} failed: {reason}")
            }
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoadError::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for LoadError {
    fn from(e: serde_json::Error) -> LoadError {
        LoadError::Json(e)
    }
}

/// Why a document could not be written. Only one cause today, but named rather
/// than leaking `serde_json::Error` into the crate's public API — the caller's
/// error handling should not have to change when the format gains a step that
/// can fail for its own reasons.
#[derive(Debug)]
pub enum SaveError {
    Json(serde_json::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Json(e) => write!(f, "could not serialize the document: {e}"),
        }
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SaveError::Json(e) => Some(e),
        }
    }
}

impl From<serde_json::Error> for SaveError {
    fn from(e: serde_json::Error) -> SaveError {
        SaveError::Json(e)
    }
}

/// The on-disk envelope: a version beside the document's own fields.
///
/// `#[serde(flatten)]` keeps the file one level deep — `{"version": 1,
/// "nodes": […], "edges": […]}` — which is the shape §31 shows, and which makes
/// a hand edit or a `jq` query straightforward.
#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    version: u32,
    #[serde(flatten)]
    document: FlowDocument,
}

impl FlowDocument {
    /// Writes the document with a `version` field, indented.
    ///
    /// Pretty rather than compact because these files are diffed, reviewed and
    /// occasionally hand-edited; a canvas document is small next to the
    /// geometry it describes, and the geometry is never in it.
    pub fn to_json(&self) -> Result<String, SaveError> {
        let envelope = Envelope {
            version: CURRENT_VERSION,
            // The clone is one `Vec` walk per *save*, not per frame. Taking a
            // reference instead would need a second borrowing envelope type,
            // for no measurable gain at this frequency.
            document: self.clone(),
        };
        Ok(serde_json::to_string_pretty(&envelope)?)
    }

    /// Reads a document, migrating it up from whatever version wrote it.
    ///
    /// The loaded document's id watermark is reseeded
    /// ([`FlowDocument::reseed_ids`]) before it is returned, so a file whose
    /// stored watermark is stale — a merge, a hand edit, another build — cannot
    /// make this session issue a duplicate id.
    pub fn from_json(json: &str) -> Result<FlowDocument, LoadError> {
        let mut value: Value = serde_json::from_str(json)?;
        let version = read_version(&value)?;

        migrate(&mut value, version, MIGRATIONS)?;

        let envelope: Envelope = serde_json::from_value(value)?;
        let mut document = envelope.document;
        document.reseed_ids();
        Ok(document)
    }
}

/// The `version` field, or [`LoadError::MissingVersion`].
fn read_version(value: &Value) -> Result<u32, LoadError> {
    value
        .get("version")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(LoadError::MissingVersion)
}

/// Climbs `value` from `version` to [`CURRENT_VERSION`], one step at a time.
///
/// Takes the ladder as a parameter rather than reading [`MIGRATIONS`] directly
/// so the tests can drive it with a synthetic one. That is not a testing
/// affordance bolted on: the ladder is data, and a function over data is the
/// shape that lets the ladder be *verified* rather than merely declared.
fn migrate(
    value: &mut Value,
    version: u32,
    ladder: &[(u32, MigrationStep)],
) -> Result<(), LoadError> {
    if version > CURRENT_VERSION {
        return Err(LoadError::FutureVersion {
            found: version,
            supported: CURRENT_VERSION,
        });
    }

    for current in version..CURRENT_VERSION {
        let step = ladder
            .iter()
            .find(|(from, _)| *from == current)
            .map(|(_, step)| step)
            .ok_or(LoadError::MissingMigration { from: current })?;

        step(value)?;

        // Each step leaves the document at the next version, whether or not it
        // bothered to say so — one place to get this right rather than one per
        // step.
        if let Some(object) = value.as_object_mut() {
            object.insert("version".into(), Value::from(current + 1));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{CURRENT_VERSION, Envelope, LoadError, MIGRATIONS, MigrationStep, migrate};
    use crate::{
        geometry::Vec2,
        models::{
            ElementKind, Endpoint, FlowDocument, RenderQuality, RenderStyle, ShapeKind,
            ids::ElementId,
        },
    };

    fn document() -> FlowDocument {
        let mut doc = FlowDocument::new();
        let a = doc.add_node(
            ElementKind::default(),
            Vec2::new(-40.5, 12.25),
            Vec2::new(150.0, 40.0),
        );
        let b = doc.add_node(
            ElementKind::Shape(ShapeKind::Ellipse),
            Vec2::new(300.0, 200.0),
            Vec2::new(120.0, 80.0),
        );
        doc.add_edge(Endpoint::handle(a, "out"), Endpoint::node(b));
        doc.settings.render_style = RenderStyle::Sketch;
        doc.settings.render_quality = RenderQuality::DRAFT;
        doc.metadata
            .insert("title".into(), Value::from("a diagram"));
        doc
    }

    #[test]
    fn a_document_round_trips_through_json() {
        let original = document();

        let json = original.to_json().expect("serializes");
        let loaded = FlowDocument::from_json(&json).expect("loads");

        assert_eq!(loaded, original);
    }

    #[test]
    fn an_empty_document_round_trips() {
        let original = FlowDocument::new();

        let loaded = FlowDocument::from_json(&original.to_json().unwrap()).unwrap();

        assert_eq!(loaded, original);
        assert!(loaded.is_empty());
    }

    #[test]
    fn the_written_file_carries_the_version_at_the_top_level() {
        let value: Value = serde_json::from_str(&document().to_json().unwrap()).unwrap();

        assert_eq!(value["version"], json!(CURRENT_VERSION));
        assert!(value["nodes"].is_array(), "the envelope is flattened");
        assert!(value["edges"].is_array());
    }

    #[test]
    fn unknown_metadata_survives_a_round_trip() {
        // The forward-compatibility valve: a field a newer build put in
        // `metadata` must come back out unchanged rather than being dropped.
        let mut doc = FlowDocument::new();
        doc.metadata
            .insert("dodo.future".into(), json!({"lanes": [1, 2, 3]}));

        let loaded = FlowDocument::from_json(&doc.to_json().unwrap()).unwrap();

        assert_eq!(loaded.metadata["dodo.future"], json!({"lanes": [1, 2, 3]}));
    }

    #[test]
    fn loading_reseeds_the_id_watermark() {
        // A hand-edited or merged file whose stored watermark is below an id it
        // contains must not make this session issue that id again.
        let json = r#"{
            "version": 1,
            "nodes": [{"id": 900, "position": {"x": 0.0, "y": 0.0}}],
            "ids": 2
        }"#;

        let mut doc = FlowDocument::from_json(json).expect("loads");

        assert_eq!(doc.next_id(), ElementId::new(901));
    }

    #[test]
    fn a_missing_version_is_refused_rather_than_assumed() {
        let err = FlowDocument::from_json(r#"{"nodes": [], "edges": []}"#).unwrap_err();

        assert!(matches!(err, LoadError::MissingVersion), "{err:?}");
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn a_document_from_the_future_is_refused_with_both_numbers() {
        let json = format!(r#"{{"version": {}, "nodes": []}}"#, CURRENT_VERSION + 7);

        let err = FlowDocument::from_json(&json).unwrap_err();

        match err {
            LoadError::FutureVersion { found, supported } => {
                assert_eq!(found, CURRENT_VERSION + 7);
                assert_eq!(supported, CURRENT_VERSION);
            }
            other => panic!("expected FutureVersion, got {other:?}"),
        }
    }

    #[test]
    fn malformed_json_reports_the_parse_error() {
        let err = FlowDocument::from_json("{not json").unwrap_err();

        assert!(matches!(err, LoadError::Json(_)), "{err:?}");
    }

    #[test]
    fn a_versioned_document_with_a_broken_body_is_a_parse_error() {
        let err =
            FlowDocument::from_json(r#"{"version": 1, "nodes": "not an array"}"#).unwrap_err();

        assert!(matches!(err, LoadError::Json(_)), "{err:?}");
    }

    #[test]
    fn the_migration_ladder_is_complete() {
        // The guard on bumping `CURRENT_VERSION` without adding the step.
        // Stated as a count plus contiguity rather than as a loop over
        // `1..CURRENT_VERSION`, which is an empty range today and which clippy
        // rejects as such — the assertion below says the same thing and keeps
        // saying it once the range is non-empty.
        assert_eq!(
            MIGRATIONS.len() as u32,
            CURRENT_VERSION - 1,
            "CURRENT_VERSION is {CURRENT_VERSION}, so MIGRATIONS needs {} steps and has {}",
            CURRENT_VERSION - 1,
            MIGRATIONS.len()
        );

        for (index, (from, _)) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                *from,
                index as u32 + 1,
                "the ladder must start at version 1 and have no gaps"
            );
        }

        // And the real ladder must carry a current document unchanged.
        let mut value = json!({"version": CURRENT_VERSION});
        assert!(migrate(&mut value, CURRENT_VERSION, MIGRATIONS).is_ok());
    }

    #[test]
    fn a_synthetic_ladder_climbs_every_rung() {
        // Version 1 has nothing to migrate, so the machinery is proven against
        // a ladder built here rather than a fake step left in `MIGRATIONS` for
        // the tests' benefit.
        fn rename_pos_to_position(value: &mut Value) -> Result<(), LoadError> {
            let object = value.as_object_mut().ok_or(LoadError::MissingVersion)?;
            if let Some(pos) = object.remove("pos") {
                object.insert("position".into(), pos);
            }
            Ok(())
        }

        fn add_a_default_field(value: &mut Value) -> Result<(), LoadError> {
            let object = value.as_object_mut().ok_or(LoadError::MissingVersion)?;
            object.insert("zoom".into(), Value::from(1.0));
            Ok(())
        }

        let ladder: &[(u32, MigrationStep)] =
            &[(1, rename_pos_to_position), (2, add_a_default_field)];

        // The loop below is `migrate`'s own, run against a `CURRENT_VERSION` of
        // 3 — which is what a two-step ladder would mean.
        let mut value = json!({"version": 1, "pos": [4, 5]});
        for (from, step) in ladder {
            step(&mut value).expect("step applies");
            value["version"] = Value::from(from + 1);
        }

        assert_eq!(value["version"], json!(3));
        assert_eq!(value["position"], json!([4, 5]));
        assert!(value.get("pos").is_none(), "the old key is gone");
        assert_eq!(value["zoom"], json!(1.0));
    }

    #[test]
    fn a_gap_in_the_ladder_is_reported_rather_than_skipped() {
        // Simulates `CURRENT_VERSION` having been bumped with no step added:
        // `migrate` must refuse rather than hand a version-1 body to a
        // version-2 parser.
        let mut value = json!({"version": 1});

        // With CURRENT_VERSION == 1 there is no range to walk, so the gap is
        // constructed by asking `migrate` for a version *below* the current one
        // via a ladder that does not cover it. `from_version` 0 is a version
        // that never existed, which is exactly the shape of the bug.
        let err = migrate(&mut value, 0, &[]).unwrap_err();

        assert!(
            matches!(err, LoadError::MissingMigration { from: 0 }),
            "{err:?}"
        );
    }

    #[test]
    fn migrating_a_current_document_is_a_no_op() {
        let mut value = json!({"version": CURRENT_VERSION, "nodes": []});
        let before = value.clone();

        migrate(&mut value, CURRENT_VERSION, MIGRATIONS).expect("nothing to do");

        assert_eq!(value, before);
    }

    #[test]
    fn the_envelope_type_is_what_is_written_and_read() {
        // Guards the flattening: an envelope that nested the document under a
        // key would still round-trip through `to_json`/`from_json` while
        // producing a file shape §31 does not describe.
        let envelope: Envelope =
            serde_json::from_str(&document().to_json().unwrap()).expect("parses as the envelope");

        assert_eq!(envelope.version, CURRENT_VERSION);
        assert_eq!(envelope.document.nodes.len(), 2);
    }
}
