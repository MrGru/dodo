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

use crate::{
    geometry::Vec2,
    models::{Connector, ElementKind, FlowDocument, LinearKind},
};

/// The format version this build writes.
///
/// **Bumping this is a two-line change**: raise the number and push a step onto
/// [`MIGRATIONS`] that turns a version-`N` `Value` into a version-`N+1` one.
/// The `the_migration_ladder_is_complete` test fails if the second line is
/// forgotten.
///
/// Version 2 is Phase 10's: text became first-class and
/// [`FontStyle`](crate::models::FontStyle) stopped carrying a continuous
/// `size`. See `fonts_became_four_steps` below, this ladder's first rung.
///
/// **Version 3 is Phase 12's, and it rewrites nothing** — see
/// `images_arrived`, and the paragraph below for the rule that decides when a
/// version has to move at all.
///
/// # When a new field costs a version, and when it does not
///
/// Phase 11 added five style fields and left the version alone; this phase adds
/// two and moves it. The difference is **what an older build silently does with
/// the new data**, and it is worth stating as a rule because both answers are
/// right:
///
/// - A build that does not know `fill_style` drops it, writes the document back
///   without it, and the shape is drawn solid. Something was lost and the user
///   can put it back with one press.
/// - A build that does not know `images` drops **the only copy of a
///   photograph**. Nothing on screen says so, the element stays where it was,
///   and no press brings it back.
///
/// So the version rises when an older build's silent discard would lose
/// something unrecoverable. Past that line, [`LoadError::FutureVersion`] is the
/// whole point of the envelope — it is what turns a quiet data loss into a
/// sentence that names both version numbers.
pub const CURRENT_VERSION: u32 = 4;

/// One rung of the ladder: rewrites a document body written by version `from`
/// into the shape version `from + 1` expects.
///
/// A plain `fn` rather than a closure so [`MIGRATIONS`] can be a `const` slice
/// with no allocation and no lazy initialisation.
pub type MigrationStep = fn(&mut Value) -> Result<(), LoadError>;

/// The ladder, one entry per version that has ever been written, in order.
///
/// **This was empty for nine phases and the machinery around it was still
/// right.** Version 2 is the first real rung, and it cost three lines plus a
/// test — which is exactly the argument for having built the ladder at version
/// 1 rather than promising it: had `from_json` been `serde_json::from_str` all
/// along, every document written before this phase would now fail to load with
/// a type error about a float, and there would be nowhere to put the fix.
pub const MIGRATIONS: &[(u32, MigrationStep)] = &[
    (1, fonts_became_four_steps),
    (2, images_arrived),
    (3, connectors_gained_ordered_endpoints),
];

/// **Version 1 ▸ 2**: a font's continuous `size` became one of four steps, and
/// its `family` stopped being a free font name.
///
/// Both fields changed *type*, which is the migration case that cannot be
/// handled by `#[serde(default)]`: a default fills a field that is **missing**,
/// and these are present with the wrong shape. A version-1 document reaching a
/// version-2 `FontStyle` without this step fails to parse entirely — one
/// malformed font on one element and the whole file refuses to open.
///
/// The rewrite is deliberately lenient in one direction and strict in none: a
/// number becomes the nearest
/// [`FontSize`](crate::models::FontSize) step, and anything else — a string, a
/// null, a value some other build wrote — is **removed**, so `#[serde(default)]`
/// answers and the element loads at Medium. Refusing the document instead would
/// be choosing a parse error over a legible diagram.
fn fonts_became_four_steps(value: &mut Value) -> Result<(), LoadError> {
    let Some(nodes) = value.get_mut("nodes").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for node in nodes {
        migrate_font(node);
    }

    let Some(edges) = value.get_mut("edges").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for edge in edges {
        migrate_font(edge);
    }

    Ok(())
}

/// One element's `style.font`, rewritten in place. Absent at every level is
/// fine and common — `#[serde(default)]` has covered every one of these fields
/// since version 1, so most documents have no `font` object at all.
fn migrate_font(element: &mut Value) {
    let Some(font) = element
        .get_mut("style")
        .and_then(|style| style.get_mut("font"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };

    match font.get("size").and_then(Value::as_f64) {
        Some(size) => {
            let step = crate::models::FontSize::nearest(size as f32);
            // Serialized through serde rather than through a hand-written name,
            // so the rewritten value cannot drift from what the enum actually
            // reads back — a migration that writes a string the loader does not
            // recognise is worse than no migration.
            let encoded = serde_json::to_value(step).unwrap_or(Value::Null);
            font.insert("size".into(), encoded);
        }
        None => {
            font.remove("size");
        }
    }

    // A version-1 family was `Option<String>` — a font name, or the theme's.
    // There is no honest mapping from an arbitrary name onto three families, so
    // every one of them becomes the theme's font, which is what `None` meant.
    font.remove("family");
}

/// **Version 2 ▸ 3**: §10's images arrived, and there is nothing to rewrite.
///
/// A version-2 document has no `images` table and no `image` on any node, and
/// both fields default — an absent map is empty and an absent handle is `None`,
/// which is exactly what "this diagram has no pictures" means. So this step is
/// the identity, deliberately, and it is here rather than absent for two
/// reasons:
///
/// 1. [`MIGRATIONS`] is a contiguous ladder and `the_migration_ladder_is_complete`
///    holds it to that. A gap would be [`LoadError::MissingMigration`] on every
///    version-2 file in existence.
/// 2. **The version moved for what it tells an older build, not for what it
///    changes here** — see [`CURRENT_VERSION`]'s own doc. The rung is where that
///    decision is recorded, and it is the natural place for the next image
///    field's rewrite to go.
fn images_arrived(_value: &mut Value) -> Result<(), LoadError> {
    Ok(())
}

/// **Version 3 ▸ 4**: a straight line or arrow stopped deriving direction
/// from its normalized rectangle and gained ordered endpoints.
///
/// Old files never retained the initiating drag direction, so the only safe
/// migration is their existing visible diagonal: `position` to
/// `position + size`. From version 4 onward the connector field is the
/// authority and the rectangle is derived.
fn connectors_gained_ordered_endpoints(value: &mut Value) -> Result<(), LoadError> {
    let Some(nodes) = value.get_mut("nodes").and_then(Value::as_array_mut) else {
        return Ok(());
    };

    for node in nodes {
        if node.get("connector").is_some() {
            continue;
        }
        let Some(kind) = node
            .get("kind")
            .cloned()
            .and_then(|kind| serde_json::from_value::<ElementKind>(kind).ok())
        else {
            continue;
        };
        if !matches!(
            kind,
            ElementKind::Linear(LinearKind::Line | LinearKind::Arrow)
        ) {
            continue;
        }

        let position = node
            .get("position")
            .cloned()
            .and_then(|it| serde_json::from_value::<Vec2>(it).ok())
            .unwrap_or(Vec2::ZERO);
        let size = node
            .get("size")
            .cloned()
            .and_then(|it| serde_json::from_value::<Vec2>(it).ok())
            .unwrap_or(Vec2::new(150.0, 40.0));
        if let Some(object) = node.as_object_mut() {
            object.insert(
                "connector".into(),
                serde_json::to_value(Connector::from_rect(position, size))?,
            );
        }
    }

    Ok(())
}

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
            ElementKind, Endpoint, FlowDocument, FontFamily, FontSize, LinearKind, RenderQuality,
            RenderStyle, ShapeKind, ids::ElementId,
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
    fn version_three_rectangles_migrate_to_their_existing_connector_diagonal() {
        let mut old = document();
        let id = old.add_node(
            ElementKind::Linear(LinearKind::Arrow),
            Vec2::new(70.0, 90.0),
            Vec2::new(160.0, 40.0),
        );
        let mut value: Value = serde_json::from_str(&old.to_json().unwrap()).unwrap();
        value["version"] = json!(3);
        value["nodes"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("connector");

        let loaded = FlowDocument::from_json(&value.to_string()).unwrap();
        let connector = loaded.node(id).unwrap().connector.unwrap();
        assert_eq!(connector.start.point, Vec2::new(70.0, 90.0));
        assert_eq!(connector.end.point, Vec2::new(230.0, 130.0));
        assert!(connector.start.attachment.is_none());
        assert!(connector.end.attachment.is_none());
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

    /// **§9's whole vocabulary survives a save and a reload** — the text on a
    /// node, the label on an edge, a standalone text element, and every one of
    /// the properties Phase 11's panel will edit.
    ///
    /// Stated as a full round trip rather than as field-by-field assertions,
    /// because the failure this catches is a field that was added to the model
    /// and forgotten in the format: `#[serde(default)]` makes that silent, and
    /// the symptom is a document that opens with somebody's headings back at
    /// Medium.
    #[test]
    fn text_and_every_text_property_survive_a_round_trip() {
        use crate::models::{Color, TextAlign};

        let mut original = FlowDocument::new();
        let a = original.add_node(
            ElementKind::default(),
            Vec2::new(0.0, 0.0),
            Vec2::new(160.0, 60.0),
        );
        let b = original.add_node(
            ElementKind::Text,
            Vec2::new(300.0, 40.0),
            Vec2::new(200.0, 22.0),
        );
        let edge = original.add_edge(Endpoint::node(a), Endpoint::node(b));

        original.node_mut(a).unwrap().label = Some("a heading".into());
        {
            let font = &mut original.node_mut(a).unwrap().style.font;
            font.size = FontSize::ExtraLarge;
            font.family = FontFamily::HandDrawn;
            font.align = TextAlign::Center;
            font.color = Some(Color::from_rgba8(178, 242, 187, 255));
        }

        original.node_mut(b).unwrap().label = Some("standalone".into());
        original.node_mut(b).unwrap().style.font.size = FontSize::Small;
        original.node_mut(b).unwrap().style.font.family = FontFamily::Code;

        let edge_index = original
            .edges
            .iter()
            .position(|e| e.id == edge)
            .expect("the edge is there");
        original.edges[edge_index].label = Some("carries".into());
        original.edges[edge_index].style.font.align = TextAlign::Right;

        let loaded = FlowDocument::from_json(&original.to_json().unwrap()).expect("loads");
        assert_eq!(loaded, original);

        // And the four steps really are written as names rather than numbers,
        // which is what makes them readable in a diff and stable across a
        // change to the world sizes.
        let value: Value = serde_json::from_str(&original.to_json().unwrap()).unwrap();
        assert_eq!(
            value["nodes"][0]["style"]["font"]["size"],
            json!("ExtraLarge")
        );
        assert_eq!(value["nodes"][1]["style"]["font"]["family"], json!("Code"));
        assert_eq!(value["edges"][0]["label"], json!("carries"));
    }

    /// **The ladder's first real rung, driven through the real loader.**
    ///
    /// A version-1 document is what every build before Phase 10 wrote, and its
    /// `font.size` is a float where version 2 wants one of four names. Without
    /// the step this is not a wrong size — it is a document that will not open.
    #[test]
    fn a_version_one_document_loads_with_its_font_snapped_to_a_step() {
        let json = r#"{
            "version": 1,
            "nodes": [
                {"id": 1, "position": {"x": 0.0, "y": 0.0}, "size": {"x": 10.0, "y": 10.0},
                 "label": "old", "style": {"font": {"size": 20.0, "family": "Comic Sans MS"}}},
                {"id": 2, "position": {"x": 0.0, "y": 0.0}, "size": {"x": 10.0, "y": 10.0}}
            ],
            "edges": [
                {"id": 3, "source": {"node": 1, "handle": null},
                 "target": {"node": 2, "handle": null},
                 "style": {"font": {"size": 12.4}}}
            ]
        }"#;

        let document = FlowDocument::from_json(json).expect("a version-1 document still opens");

        assert_eq!(document.nodes[0].style.font.size, FontSize::Large);
        assert_eq!(
            document.nodes[0].style.font.family,
            FontFamily::Normal,
            "an arbitrary font name becomes the theme's font, which is what the \
             absent case always meant"
        );
        assert_eq!(
            document.nodes[1].style.font.size,
            FontSize::default(),
            "an element with no font object at all is untouched"
        );
        assert_eq!(document.edges[0].style.font.size, FontSize::Small);
    }

    /// The lenient half, stated on its own: a `size` this build cannot read is
    /// **dropped** so the default answers, rather than refusing the file.
    #[test]
    fn an_unreadable_font_size_defaults_instead_of_failing_the_load() {
        let json = r#"{
            "version": 1,
            "nodes": [{"id": 1, "style": {"font": {"size": "enormous"}}}]
        }"#;

        let document = FlowDocument::from_json(json).expect("one bad font must not lose the file");
        assert_eq!(document.nodes[0].style.font.size, FontSize::default());
    }

    /// **§10's picture survives a save and a reload, crop included** — and the
    /// bytes are written once however many elements show them.
    ///
    /// The round trip is the whole assertion, for the reason
    /// `text_and_every_text_property_survive_a_round_trip` gives: a field added
    /// to the model and forgotten in the format is silent, and the symptom is a
    /// diagram that opens with somebody's screenshot gone.
    #[test]
    fn an_image_and_its_crop_survive_a_round_trip_without_duplicating_the_bytes() {
        use crate::models::{ImageCrop, ImageFormat, ImageResource, NodeImage};

        let mut original = FlowDocument::new();
        let bytes: Vec<u8> = (0..96u8).collect();
        let handle = original.insert_image(ImageResource::new(
            ImageFormat::Png,
            1200,
            800,
            bytes.clone(),
        ));

        let a = original.add_node(
            ElementKind::Image,
            Vec2::new(-20.0, 40.0),
            Vec2::new(300.0, 200.0),
        );
        let b = original.add_node(
            ElementKind::Image,
            Vec2::new(400.0, 40.0),
            Vec2::new(150.0, 100.0),
        );
        original.node_mut(a).unwrap().image =
            Some(NodeImage::new(handle).with_crop(ImageCrop::new(0.1, 0.2, 0.5, 0.4)));
        original.node_mut(b).unwrap().image = Some(NodeImage::new(handle));

        let json = original.to_json().expect("serializes");
        let loaded = FlowDocument::from_json(&json).expect("loads");
        assert_eq!(loaded, original);

        let crop = loaded.nodes[0].image.unwrap().crop;
        assert!((crop.width - 0.5).abs() < 1e-6, "{crop:?}");
        assert_eq!(loaded.images.len(), 1, "the bytes were written twice");
        assert_eq!(loaded.image(handle).unwrap().bytes.as_ref(), &bytes[..]);

        // And the file says what it says: a table of resources beside the
        // elements, keyed by a handle, with the bytes in exactly one of them.
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["version"], json!(CURRENT_VERSION));
        assert_eq!(
            value["images"].as_object().map(|it| it.len()),
            Some(1),
            "one entry per distinct picture"
        );
        assert!(value["nodes"][0]["image"]["handle"].is_string());
        assert!(
            value["nodes"][0]["image"].get("bytes").is_none(),
            "an element must never carry the bytes"
        );
    }

    /// A version-2 document — everything written before this phase — still
    /// opens, and opens as a diagram with no pictures rather than as an error.
    #[test]
    fn a_version_two_document_loads_with_no_images() {
        let json = r#"{
            "version": 2,
            "nodes": [{"id": 1, "position": {"x": 0.0, "y": 0.0}}],
            "edges": []
        }"#;

        let document = FlowDocument::from_json(json).expect("a version-2 document still opens");

        assert!(document.images.is_empty());
        assert_eq!(document.nodes[0].image, None);
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
