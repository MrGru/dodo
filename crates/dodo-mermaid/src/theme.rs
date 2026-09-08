//! What a tab's diagram is drawn *in*: a preset, the handful of general
//! fields a person may change on top of it, and the rules for merging,
//! resetting and validating the two.
//!
//! **No GPUI here and no `mermaid_rs_renderer` either**, which is one boundary
//! more than [`crate::workspace`], [`crate::templates`] and [`crate::zoom`]
//! keep. The renderer's `Theme` is a struct of ~40 `String` fields; this module
//! never names it. [`crate::render`] is the only file that does, and it hands
//! this one a [`ThemeDefaults`] — the ten values the *chosen preset*
//! happens to carry — so every rule below can be asserted with a synthetic
//! table and no renderer at all. That matters more in this crate than most:
//! [`crate::view`]'s module doc records that a `#[gpui_kit::test]` cannot be added
//! here, so a rule left in the view is a rule nothing can assert.
//!
//! [`view`]: crate::view
//!
//! # Ten fields, not forty
//!
//! The renderer's theme is mostly *diagram-specific*: `sequence_*` styles an
//! actor box, `git_*` an eight-colour branch palette, `pie_*` a twelve-colour
//! wheel and its three text sizes. None of those means anything to a person
//! looking at a flowchart, and offering forty rows would bury the ten that do.
//! [`ThemeField`] is therefore the *general* set — the fields the renderer
//! consults whatever the diagram is — and everything else follows the preset.
//! The panel says so out loud ([`mermaid::Text::ThemeDiagramSpecificNote`]) so
//! their absence reads as a decision rather than an omission.
//!
//! **Which fields those are was measured, not assumed**, by rendering one
//! source per diagram family against renderer 0.3.1 twice — once on the plain
//! preset, once with the field moved — and asking whether the SVG changed at
//! all. That is also what `crate::render`'s
//! `every_editable_field_changes_the_rendered_svg` now holds the set to, and
//! it moved the set twice from the one this feature was specified with:
//!
//! - **`edge_label_background`, `cluster_background` and `cluster_border` are
//!   in.** All three are flowchart furniture — the box behind an edge label,
//!   and the fill and border of a `subgraph` — and a flowchart is the diagram
//!   people actually draw. Without them a user could recolour every node and
//!   watch the subgraph around them stay on the preset's colours, which reads
//!   as a bug rather than as a boundary.
//! - **`text_color` is out.** It sounds like the most general field in the
//!   struct and is one of the least: at 0.3.1 it reaches an xychart axis tick,
//!   a git tag's hole, and the stylesheet of the renderer's own *error* SVG —
//!   nothing else. Node labels, edge labels and subgraph titles are
//!   `primary_text_color`, which is why "Shape text" is the row that exists.
//!   Moving `text_color` changes not one byte of a flowchart, class, sequence,
//!   state or ER diagram, so a row for it would be a control that visibly does
//!   nothing.
//! - **`secondary_color` is out** for the same measured reason: it reaches a
//!   journey diagram's actor dots and a treemap's second tier, and none of the
//!   five common families. It is the presets' own second base colour, which is
//!   what makes it *look* general — but what the presets derive from it is the
//!   pie palette, and that derivation happens while the preset is being
//!   constructed, so an override merged on afterwards cannot reach it either.
//! - **`tertiary_color` was never in**, and the same measurement says why: one
//!   call site (a treemap's third tier), plus the same construction-time pie
//!   derivation an override arrives too late for.
//!
//! # A preset is a base, an override is a delta
//!
//! [`TabTheme`] holds a preset and a sparse set of overrides, never a copy of
//! all ten values. That is what makes "this field is still on the preset's
//! default" a fact rather than a comparison the UI has to guess at, and it is
//! why [`TabTheme::set`] *removes* an override that has been typed back to the
//! default rather than storing a value equal to it. Switching preset keeps the
//! overrides, because an override only exists when the user deliberately moved
//! a field off its default.
//!
//! # Following the app's appearance, until the user says otherwise
//!
//! [`TabTheme::preset`] is an `Option`, and `None` — the state every new tab
//! starts in — means *follow dodo's own light/dark appearance*, exactly as the
//! workspace behaved before this module existed. Only an explicit choice pins
//! a tab, and the two ways to make one are picking a preset and changing any
//! field: [`TabTheme::set`] pins the tab to whatever base was in effect at the
//! moment it stored the first override. Not pinning there is the surprising
//! behaviour, not the safe one — a `primary_text_color` chosen to read against
//! the light base would otherwise be silently re-hosted on the dark base the
//! next time the user switched appearance, and dark text on a dark fill is
//! invisible rather than merely wrong.
//!
//! **A pinned tab is then unaffected by dodo switching light↔dark.** Its
//! diagram keeps exactly the look the user chose and only the app's own chrome
//! changes around it, which is what a per-document setting is expected to do —
//! the alternative, re-basing a hand-picked palette every time the OS crosses
//! sunset, is a diagram that silently stops matching the one the user built.
//! The view acts on the same distinction: `MermaidView::sync_appearance_render`
//! re-renders a tab whose base moved and leaves a pinned one alone.
//!
//! Going back is [`TabTheme::follow_appearance`], which the preset picker
//! offers as its first entry.

use std::collections::BTreeMap;

use crate::i18n::mermaid;

/// The renderer's five named presets, in the order the picker lists them.
///
/// Deliberately an enum over `Theme::from_name`'s accepted strings rather than
/// a `String`: a preset that no longer exists upstream must be a compile error
/// here, not a silent fall back to `modern()` inside the renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MermaidThemePreset {
    /// `Theme::modern()` — the renderer's own light preset, and what dodo drew
    /// every light-appearance diagram with before this module existed.
    #[default]
    Modern,
    /// `Theme::mermaid_default()` — mermaid-js's `default`/`base` look.
    MermaidDefault,
    /// `Theme::dark()`, and what dodo's dark appearance resolves to.
    Dark,
    /// `Theme::forest()`.
    Forest,
    /// `Theme::neutral()`.
    Neutral,
}

impl MermaidThemePreset {
    /// Picker order, which is also `ALL`'s indexing contract for the menu.
    pub(crate) const ALL: [MermaidThemePreset; 5] = [
        MermaidThemePreset::Modern,
        MermaidThemePreset::MermaidDefault,
        MermaidThemePreset::Dark,
        MermaidThemePreset::Forest,
        MermaidThemePreset::Neutral,
    ];

    /// The preset dodo's own appearance resolves to.
    ///
    /// The one place light/dark is turned into a Mermaid preset. Before this
    /// module it was a `MermaidTheme::{Light, Dark}` decided inline in the
    /// view; keeping it here means the "Automatic" entry in the picker and the
    /// render path cannot disagree about what automatic *means*.
    pub(crate) fn for_appearance(dark: bool) -> Self {
        if dark {
            MermaidThemePreset::Dark
        } else {
            MermaidThemePreset::Modern
        }
    }

    /// The preset's name in the picker.
    pub(crate) fn label(self) -> mermaid::Text {
        match self {
            MermaidThemePreset::Modern => mermaid::Text::ThemePresetModern,
            MermaidThemePreset::MermaidDefault => mermaid::Text::ThemePresetDefault,
            MermaidThemePreset::Dark => mermaid::Text::ThemePresetDark,
            MermaidThemePreset::Forest => mermaid::Text::ThemePresetForest,
            MermaidThemePreset::Neutral => mermaid::Text::ThemePresetNeutral,
        }
    }
}

/// Which of the renderer's general theme fields a row edits.
///
/// `Ord` is derived because [`TabTheme`]'s override set is a `BTreeMap` keyed
/// by this: a deterministic iteration order is what makes a theme's [`Hash`]
/// stable, and the render path hashes the theme to decide whether a re-render
/// is needed at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ThemeField {
    FontFamily,
    FontSize,
    PrimaryColor,
    PrimaryTextColor,
    PrimaryBorderColor,
    LineColor,
    Background,
    EdgeLabelBackground,
    ClusterBackground,
    ClusterBorder,
}

/// The kind of control a field needs, which is the only thing the view has to
/// switch on when it draws a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThemeFieldKind {
    /// A free-text font stack.
    FontStack,
    /// A point size, stepped rather than typed — see [`stepped_font_size`].
    Size,
    /// A CSS colour, edited through a colour picker.
    Colour,
}

impl ThemeField {
    /// Panel order: the two typographic fields, then colours from the most
    /// visible outwards — a node's fill, its text, its border, then the lines
    /// between nodes, then the page, then the containers.
    pub(crate) const ALL: [ThemeField; 10] = [
        ThemeField::FontFamily,
        ThemeField::FontSize,
        ThemeField::PrimaryColor,
        ThemeField::PrimaryTextColor,
        ThemeField::PrimaryBorderColor,
        ThemeField::LineColor,
        ThemeField::Background,
        ThemeField::EdgeLabelBackground,
        ThemeField::ClusterBackground,
        ThemeField::ClusterBorder,
    ];

    /// How many fields there are — the width of a [`ThemeDefaults`] table.
    pub(crate) const COUNT: usize = ThemeField::ALL.len();

    /// This field's slot in a [`ThemeDefaults`] table.
    ///
    /// A `match` rather than a search through [`Self::ALL`]: the table is read
    /// once per row per frame while the panel is open, and a linear scan for a
    /// value that is a constant of the type is the shape root `AGENTS.md`'s
    /// cheap-`render` rule exists to catch.
    const fn index(self) -> usize {
        match self {
            ThemeField::FontFamily => 0,
            ThemeField::FontSize => 1,
            ThemeField::PrimaryColor => 2,
            ThemeField::PrimaryTextColor => 3,
            ThemeField::PrimaryBorderColor => 4,
            ThemeField::LineColor => 5,
            ThemeField::Background => 6,
            ThemeField::EdgeLabelBackground => 7,
            ThemeField::ClusterBackground => 8,
            ThemeField::ClusterBorder => 9,
        }
    }

    /// The row's label.
    ///
    /// Named for what the field *does to a diagram*, not for the renderer's
    /// own field name: "Shape fill" is what a person is looking for, and
    /// `primaryColor` is a name only somebody who has read `theme.rs` could
    /// map onto it.
    pub(crate) fn label(self) -> mermaid::Text {
        match self {
            ThemeField::FontFamily => mermaid::Text::ThemeFieldFontFamily,
            ThemeField::FontSize => mermaid::Text::ThemeFieldFontSize,
            ThemeField::PrimaryColor => mermaid::Text::ThemeFieldShapeFill,
            ThemeField::PrimaryTextColor => mermaid::Text::ThemeFieldShapeText,
            ThemeField::PrimaryBorderColor => mermaid::Text::ThemeFieldShapeBorder,
            ThemeField::LineColor => mermaid::Text::ThemeFieldLines,
            ThemeField::Background => mermaid::Text::ThemeFieldBackground,
            ThemeField::EdgeLabelBackground => mermaid::Text::ThemeFieldEdgeLabel,
            ThemeField::ClusterBackground => mermaid::Text::ThemeFieldSubgraphFill,
            ThemeField::ClusterBorder => mermaid::Text::ThemeFieldSubgraphBorder,
        }
    }

    /// The control the row draws, and therefore which validator [`Self::canonical`] runs.
    pub(crate) fn kind(self) -> ThemeFieldKind {
        match self {
            ThemeField::FontFamily => ThemeFieldKind::FontStack,
            ThemeField::FontSize => ThemeFieldKind::Size,
            ThemeField::PrimaryColor
            | ThemeField::PrimaryTextColor
            | ThemeField::PrimaryBorderColor
            | ThemeField::LineColor
            | ThemeField::Background
            | ThemeField::EdgeLabelBackground
            | ThemeField::ClusterBackground
            | ThemeField::ClusterBorder => ThemeFieldKind::Colour,
        }
    }

    /// `raw` as this field would be stored, or why it cannot be.
    ///
    /// **Nothing reaches the renderer without passing through here.** A colour
    /// is normalised to `#RRGGBB`/`#RRGGBBAA` so that two spellings of one
    /// colour compare equal against the preset's default (see [`TabTheme::set`]),
    /// and a size to the shortest decimal that round-trips.
    pub(crate) fn canonical(self, raw: &str) -> Result<String, ThemeFieldError> {
        match self.kind() {
            ThemeFieldKind::FontStack => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    Err(ThemeFieldError::Empty)
                } else {
                    Ok(trimmed.to_string())
                }
            }
            ThemeFieldKind::Size => parse_font_size(raw)
                .map(format_font_size)
                .ok_or(ThemeFieldError::NotASize),
            ThemeFieldKind::Colour => canonical_colour(raw).ok_or(ThemeFieldError::NotAColour),
        }
    }
}

/// Why a value was refused.
///
/// No variant carries a user-facing string, and that is deliberate: every
/// control the panel draws is incapable of producing one of these. A colour
/// comes from a picker that only ever emits a real colour, a size from a pair
/// of step buttons that clamp, and an emptied font box is read as "reset this
/// field" rather than as an error. The type exists so that a *future* control
/// — a pasted hex box, a config file — cannot reach the renderer without
/// deciding what to do about invalid input, and so the tests below can assert
/// the refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThemeFieldError {
    /// A font stack of nothing but whitespace.
    Empty,
    /// Not a finite number inside [`FONT_SIZE_MIN`]..=[`FONT_SIZE_MAX`].
    NotASize,
    /// Not a CSS colour this module can parse — see [`canonical_colour`].
    NotAColour,
}

/// The ten values one preset carries, read out of the renderer's own theme
/// by [`crate::render::preset_defaults`].
///
/// A table rather than a `Theme`, because this module must not name the
/// renderer's type (see the module doc). It is also what lets the tests below
/// build a preset out of thin air.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ThemeDefaults([String; ThemeField::COUNT]);

impl ThemeDefaults {
    /// `values` are in [`ThemeField::ALL`] order — an array, so a field added
    /// to the enum stops this compiling until its default is supplied.
    pub(crate) fn new(values: [String; ThemeField::COUNT]) -> Self {
        Self(values)
    }

    /// `field`'s value in this preset, exactly as the renderer states it.
    ///
    /// Verbatim on purpose: the presets spell colours as `lightgrey`,
    /// `rgba(255, 255, 255, 0.25)` and `#ccc` as well as full hex, and the
    /// value that goes back to the renderer when a field is *not* overridden
    /// must be the one it shipped with. Canonicalising happens only when
    /// comparing ([`TabTheme::set`]) or when a swatch has to be painted.
    pub(crate) fn get(&self, field: ThemeField) -> &str {
        &self.0[field.index()]
    }
}

/// A resolved theme: which preset to start from, and what to change on top.
///
/// This is the value [`crate::render`] turns into the renderer's own `Theme`,
/// and the seam that used to be a two-variant `Light`/`Dark` enum. It is a
/// plain data type — no `Option` preset, no appearance — because by the time a
/// render is scheduled "follow the app's appearance" has already been answered
/// ([`TabTheme::resolve`]). `Hash` is part of its job: the render path hashes
/// the source *and* this to decide whether anything actually changed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MermaidTheme {
    preset: MermaidThemePreset,
    /// A `BTreeMap` so iteration — and therefore [`Hash`] — is deterministic.
    overrides: BTreeMap<ThemeField, String>,
}

impl Default for MermaidTheme {
    /// [`MermaidThemePreset::Modern`] with nothing changed: what dodo drew
    /// before per-tab theming existed, and what a caller with no window to
    /// read an appearance from should use (`quick_nav`'s Mermaid detection
    /// does exactly that).
    fn default() -> Self {
        Self::preset(MermaidThemePreset::default())
    }
}

impl MermaidTheme {
    /// An unmodified preset.
    pub fn preset(preset: MermaidThemePreset) -> Self {
        Self {
            preset,
            overrides: BTreeMap::new(),
        }
    }

    /// The preset this theme starts from.
    pub(crate) fn base(&self) -> MermaidThemePreset {
        self.preset
    }

    /// The changes to apply on top of it, in [`ThemeField`] order.
    pub(crate) fn overrides(&self) -> impl Iterator<Item = (ThemeField, &str)> {
        self.overrides
            .iter()
            .map(|(field, value)| (*field, value.as_str()))
    }
}

/// One tab's theme: a preset that may still be "whatever dodo's appearance
/// says", plus the fields the user has moved off it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TabTheme {
    /// `None` means follow dodo's light/dark appearance. See the module doc
    /// for why an explicit choice is the only thing that pins it.
    preset: Option<MermaidThemePreset>,
    overrides: BTreeMap<ThemeField, String>,
}

impl TabTheme {
    /// The preset actually in effect, given the preset dodo's current
    /// appearance resolves to ([`MermaidThemePreset::for_appearance`]).
    pub(crate) fn base(&self, appearance: MermaidThemePreset) -> MermaidThemePreset {
        self.preset.unwrap_or(appearance)
    }

    /// Whether this tab is still following the app's appearance rather than a
    /// preset the user picked. The picker draws its first entry as selected
    /// when this is true.
    pub(crate) fn follows_appearance(&self) -> bool {
        self.preset.is_none()
    }

    /// Pins the tab to `preset`. Overrides survive: they are deltas the user
    /// asked for, not a snapshot of the preset they were made against.
    pub(crate) fn choose_preset(&mut self, preset: MermaidThemePreset) {
        self.preset = Some(preset);
    }

    /// Un-pins the tab, so it follows dodo's appearance again. Overrides
    /// survive for the same reason.
    pub(crate) fn follow_appearance(&mut self) {
        self.preset = None;
    }

    /// What `field` is currently set to: the override if there is one, and the
    /// preset's own default otherwise.
    pub(crate) fn value<'a>(&'a self, field: ThemeField, defaults: &'a ThemeDefaults) -> &'a str {
        self.overrides
            .get(&field)
            .map(String::as_str)
            .unwrap_or_else(|| defaults.get(field))
    }

    /// Whether the user has moved `field` off the preset's default — the one
    /// fact the panel draws a row's "changed" marker and reset button from.
    pub(crate) fn is_overridden(&self, field: ThemeField) -> bool {
        self.overrides.contains_key(&field)
    }

    /// Whether anything at all is off the preset's defaults, which is what
    /// enables the whole-theme reset.
    pub(crate) fn has_overrides(&self) -> bool {
        !self.overrides.is_empty()
    }

    /// Sets `field` to `raw`, validating first.
    ///
    /// Three rules live here rather than in the view, because all three are
    /// invisible when they go wrong:
    ///
    /// 1. **Nothing invalid is stored.** The error is returned rather than
    ///    swallowed, so the caller cannot accidentally write it through.
    /// 2. **A value equal to the preset's default is not an override.** Typing
    ///    a field back to where it started clears the marker and the reset
    ///    button, rather than leaving a row that claims to be changed and is
    ///    not. The comparison is canonical, so `lightgrey` and `#D3D3D3` are
    ///    the same value.
    /// 3. **Storing the first override pins the tab** to `appearance` if it
    ///    was still following the app. See the module doc for why the
    ///    alternative surprises people.
    pub(crate) fn set(
        &mut self,
        field: ThemeField,
        raw: &str,
        appearance: MermaidThemePreset,
        defaults: &ThemeDefaults,
    ) -> Result<(), ThemeFieldError> {
        let canonical = field.canonical(raw)?;
        let default = field.canonical(defaults.get(field)).ok();

        if default.as_deref() == Some(canonical.as_str()) {
            self.overrides.remove(&field);
            return Ok(());
        }

        self.overrides.insert(field, canonical);
        if self.preset.is_none() {
            self.preset = Some(appearance);
        }
        Ok(())
    }

    /// Puts one field back on the preset's default.
    pub(crate) fn reset_field(&mut self, field: ThemeField) {
        self.overrides.remove(&field);
    }

    /// Puts every field back on the preset's defaults.
    ///
    /// The *preset* is deliberately left alone: this is "reset to the preset's
    /// defaults", and a user who also wants the app's appearance back has the
    /// picker's first entry for that. One control, one effect.
    pub(crate) fn reset_overrides(&mut self) {
        self.overrides.clear();
    }

    /// The theme a render should actually use, with "follow the appearance"
    /// already resolved away.
    pub(crate) fn resolve(&self, appearance: MermaidThemePreset) -> MermaidTheme {
        MermaidTheme {
            preset: self.base(appearance),
            overrides: self.overrides.clone(),
        }
    }
}

/// The smallest and largest font size the stepper will reach, in points.
///
/// The presets ship 14 and 16, and this is generous either side of them
/// without letting a diagram be rendered at a size that is either unreadable
/// or so large that laying it out takes visible time.
pub(crate) const FONT_SIZE_MIN: f32 = 8.0;
pub(crate) const FONT_SIZE_MAX: f32 = 48.0;
/// One press of the `−`/`+` buttons.
pub(crate) const FONT_SIZE_STEP: f32 = 1.0;

/// `current` moved by `delta` steps, held inside the range.
///
/// The same shape as [`crate::zoom::scaled`] and for the same reason: every
/// change to the size goes through one clamp, so a later caller cannot forget
/// one. A non-finite input resolves to the range's low end rather than
/// propagating a NaN into the renderer's text metrics.
pub(crate) fn stepped_font_size(current: f32, delta: f32) -> f32 {
    let stepped = current + delta * FONT_SIZE_STEP;
    if !stepped.is_finite() {
        return FONT_SIZE_MIN;
    }
    stepped.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX)
}

/// A size as it is stored and shown: no trailing `.0`, since every preset ships
/// a whole number and a row reading "16" beats one reading "16.0".
pub(crate) fn format_font_size(size: f32) -> String {
    if size.fract() == 0.0 {
        format!("{}", size as i32)
    } else {
        format!("{size}")
    }
}

/// A stored or default size back as a number, or `None` if it is not one this
/// module would ever have stored.
pub(crate) fn parse_font_size(raw: &str) -> Option<f32> {
    let size: f32 = raw.trim().parse().ok()?;
    (size.is_finite() && (FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(&size)).then_some(size)
}

/// `raw` as `#RRGGBB`, or `#RRGGBBAA` when it is not fully opaque — or `None`
/// if it is not a colour this module understands.
///
/// **The accepted set is driven by the presets, not by CSS.** The five presets
/// spell their general fields as 3- and 6-digit hex, `rgba(…)` with a
/// fractional alpha, `hsl(…)` with fractional percentages, and four bare
/// colour names; a parser that handled less could not show a swatch for
/// `Theme::dark()`'s `line_color`, and `crate::render`'s
/// `every_preset_default_parses` is what keeps that true rather than
/// remembered. Named colours are the handful the presets use plus their
/// obvious spellings, deliberately not the full CSS list: a name nobody ships
/// is a name nobody can reach.
pub(crate) fn canonical_colour(raw: &str) -> Option<String> {
    let (r, g, b, a) = parse_colour(raw)?;
    if a == u8::MAX {
        Some(format!("#{r:02X}{g:02X}{b:02X}"))
    } else {
        Some(format!("#{r:02X}{g:02X}{b:02X}{a:02X}"))
    }
}

/// `raw` as 8-bit RGBA.
fn parse_colour(raw: &str) -> Option<(u8, u8, u8, u8)> {
    let value = raw.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some((prefix, args)) = split_function(value) {
        return match prefix.as_str() {
            "rgb" | "rgba" => parse_rgb_arguments(&args),
            "hsl" | "hsla" => parse_hsl_arguments(&args),
            _ => None,
        };
    }
    parse_named(&value.to_ascii_lowercase())
}

/// `name(a, b, c)` split into its lowercased name and its comma-separated
/// arguments. Whitespace-separated CSS Color 4 syntax (`rgb(0 0 0 / 50%)`) is
/// deliberately not accepted: no preset uses it, and guessing at a syntax
/// nothing in the tree produces is how a parser grows cases nothing tests.
fn split_function(value: &str) -> Option<(String, Vec<String>)> {
    let open = value.find('(')?;
    let close = value.rfind(')')?;
    if close < open {
        return None;
    }
    let name = value[..open].trim().to_ascii_lowercase();
    let args = value[open + 1..close]
        .split(',')
        .map(|part| part.trim().to_string())
        .collect();
    Some((name, args))
}

fn parse_hex(hex: &str) -> Option<(u8, u8, u8, u8)> {
    if !hex.is_ascii() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |index: usize| u8::from_str_radix(&hex[index..index + 1], 16).ok();
    let pair = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
    match hex.len() {
        3 | 4 => {
            let expand = |value: u8| value * 17;
            Some((
                expand(digit(0)?),
                expand(digit(1)?),
                expand(digit(2)?),
                if hex.len() == 4 {
                    expand(digit(3)?)
                } else {
                    u8::MAX
                },
            ))
        }
        6 | 8 => Some((
            pair(0)?,
            pair(2)?,
            pair(4)?,
            if hex.len() == 8 { pair(6)? } else { u8::MAX },
        )),
        _ => None,
    }
}

/// `rgb`/`rgba` arguments: three 0–255 channels (the presets spell some of
/// them fractionally, e.g. `rgb(146.5000000001, 0, 0)`) and an optional 0–1
/// alpha.
fn parse_rgb_arguments(args: &[String]) -> Option<(u8, u8, u8, u8)> {
    if args.len() < 3 || args.len() > 4 {
        return None;
    }
    let channel = |index: usize| -> Option<u8> {
        let value: f32 = args[index].parse().ok()?;
        value
            .is_finite()
            .then(|| value.round().clamp(0.0, 255.0) as u8)
    };
    Some((
        channel(0)?,
        channel(1)?,
        channel(2)?,
        parse_alpha(args.get(3))?,
    ))
}

/// `hsl`/`hsla` arguments: degrees, then two percentages, then an optional
/// 0–1 alpha. The `%` is optional because the presets are not consistent about
/// writing it.
fn parse_hsl_arguments(args: &[String]) -> Option<(u8, u8, u8, u8)> {
    if args.len() < 3 || args.len() > 4 {
        return None;
    }
    let number = |index: usize| -> Option<f32> {
        let value: f32 = args[index].trim_end_matches('%').trim().parse().ok()?;
        value.is_finite().then_some(value)
    };
    let hue = number(0)?.rem_euclid(360.0) / 360.0;
    let saturation = (number(1)? / 100.0).clamp(0.0, 1.0);
    let lightness = (number(2)? / 100.0).clamp(0.0, 1.0);
    let (r, g, b) = hsl_to_rgb(hue, saturation, lightness);
    Some((r, g, b, parse_alpha(args.get(3))?))
}

/// A 0–1 alpha argument, or fully opaque when there is none.
fn parse_alpha(arg: Option<&String>) -> Option<u8> {
    let Some(arg) = arg else {
        return Some(u8::MAX);
    };
    let value: f32 = arg.trim_end_matches('%').trim().parse().ok()?;
    // A trailing `%` means 0–100 rather than 0–1; nothing in the presets uses
    // it, but accepting it costs one branch and refusing it would be arbitrary.
    let scaled = if arg.trim_end().ends_with('%') {
        value / 100.0
    } else {
        value
    };
    scaled
        .is_finite()
        .then(|| (scaled.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The standard HSL→RGB conversion, over `h`/`s`/`l` already normalised to 0–1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let sector = h * 6.0;
    let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let match_value = l - chroma / 2.0;
    let byte = |value: f32| ((value + match_value).clamp(0.0, 1.0) * 255.0).round() as u8;
    (byte(r), byte(g), byte(b))
}

/// The bare colour names the five presets actually spell out, plus the
/// alternative spelling of each — `Theme::dark()` writes `lightgrey`,
/// `Theme::forest()` writes `green`, `Theme::neutral()` writes `white` and
/// several write `black`.
fn parse_named(name: &str) -> Option<(u8, u8, u8, u8)> {
    let (r, g, b) = match name {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "grey" | "gray" => (128, 128, 128),
        "lightgrey" | "lightgray" => (211, 211, 211),
        "darkgrey" | "darkgray" => (169, 169, 169),
        "green" => (0, 128, 0),
        "red" => (255, 0, 0),
        "blue" => (0, 0, 255),
        "transparent" => return Some((0, 0, 0, 0)),
        _ => return None,
    };
    Some((r, g, b, u8::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic preset table — the point of [`ThemeDefaults`] being a table
    /// rather than the renderer's own type. Values are deliberately unlike the
    /// real presets so a test that passes by accident is unlikely.
    fn defaults() -> ThemeDefaults {
        ThemeDefaults::new([
            "Test Sans".to_string(),
            "16".to_string(),
            "#101010".to_string(),
            "#202020".to_string(),
            "#303030".to_string(),
            "lightgrey".to_string(),
            "#606060".to_string(),
            "#808080".to_string(),
            "#909090".to_string(),
            "rgba(255, 255, 255, 0.25)".to_string(),
        ])
    }

    #[test]
    fn a_field_indexes_its_own_slot_in_the_defaults_table() {
        for (index, field) in ThemeField::ALL.into_iter().enumerate() {
            assert_eq!(field.index(), index, "{field:?}");
        }
    }

    #[test]
    fn a_new_tab_follows_the_apps_appearance() {
        let theme = TabTheme::default();
        assert!(theme.follows_appearance());
        assert_eq!(
            theme.base(MermaidThemePreset::Dark),
            MermaidThemePreset::Dark
        );
        assert_eq!(
            theme.base(MermaidThemePreset::Modern),
            MermaidThemePreset::Modern
        );
    }

    #[test]
    fn choosing_a_preset_pins_the_tab_against_the_appearance() {
        let mut theme = TabTheme::default();
        theme.choose_preset(MermaidThemePreset::Forest);
        assert!(!theme.follows_appearance());
        assert_eq!(
            theme.base(MermaidThemePreset::Dark),
            MermaidThemePreset::Forest
        );
    }

    #[test]
    fn following_the_appearance_again_un_pins_the_tab() {
        let mut theme = TabTheme::default();
        theme.choose_preset(MermaidThemePreset::Forest);
        theme.follow_appearance();
        assert_eq!(
            theme.base(MermaidThemePreset::Dark),
            MermaidThemePreset::Dark
        );
    }

    #[test]
    fn an_untouched_field_reads_the_presets_own_default() {
        let theme = TabTheme::default();
        assert_eq!(
            theme.value(ThemeField::PrimaryColor, &defaults()),
            "#101010"
        );
        assert!(!theme.is_overridden(ThemeField::PrimaryColor));
        assert!(!theme.has_overrides());
    }

    #[test]
    fn an_override_wins_over_the_default_and_is_visible_as_one() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        theme
            .set(
                ThemeField::PrimaryColor,
                "#ff0000",
                MermaidThemePreset::Modern,
                &defaults,
            )
            .expect("a hex colour is valid");

        assert_eq!(theme.value(ThemeField::PrimaryColor, &defaults), "#FF0000");
        assert!(theme.is_overridden(ThemeField::PrimaryColor));
        assert!(theme.has_overrides());
        // Only the field that was set moved.
        assert!(!theme.is_overridden(ThemeField::LineColor));
        assert_eq!(theme.value(ThemeField::LineColor, &defaults), "lightgrey");
    }

    /// The pinning rule from the module doc: an edit made while the tab was
    /// following the appearance nails it down, so a later appearance switch
    /// cannot re-host the chosen colour on the opposite base.
    #[test]
    fn the_first_override_pins_the_tab_to_the_base_it_was_made_against() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        theme
            .set(
                ThemeField::PrimaryColor,
                "#ff0000",
                MermaidThemePreset::Dark,
                &defaults,
            )
            .unwrap();

        assert!(!theme.follows_appearance());
        assert_eq!(
            theme.base(MermaidThemePreset::Modern),
            MermaidThemePreset::Dark,
            "the appearance switching to light must not move a pinned tab"
        );
    }

    #[test]
    fn an_override_does_not_re_pin_a_tab_that_already_chose_a_preset() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        theme.choose_preset(MermaidThemePreset::Neutral);
        theme
            .set(
                ThemeField::LineColor,
                "#123456",
                MermaidThemePreset::Dark,
                &defaults,
            )
            .unwrap();
        assert_eq!(
            theme.base(MermaidThemePreset::Dark),
            MermaidThemePreset::Neutral
        );
    }

    /// Rule 2 of [`TabTheme::set`], and the reason the comparison is canonical
    /// rather than textual: the default here is spelled `lightgrey`.
    #[test]
    fn setting_a_field_back_to_the_default_clears_the_override() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        theme
            .set(
                ThemeField::LineColor,
                "#000000",
                MermaidThemePreset::Modern,
                &defaults,
            )
            .unwrap();
        assert!(theme.is_overridden(ThemeField::LineColor));

        theme
            .set(
                ThemeField::LineColor,
                "#D3D3D3",
                MermaidThemePreset::Modern,
                &defaults,
            )
            .unwrap();
        assert!(
            !theme.is_overridden(ThemeField::LineColor),
            "a value equal to the preset default is not an override, however it is spelled"
        );
    }

    /// …and a value that only *equals the default* must not pin a tab either,
    /// or opening the panel and clicking the colour already shown would
    /// silently stop the tab following the appearance.
    #[test]
    fn a_value_equal_to_the_default_does_not_pin_the_tab() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        theme
            .set(
                ThemeField::LineColor,
                "lightgrey",
                MermaidThemePreset::Dark,
                &defaults,
            )
            .unwrap();
        assert!(theme.follows_appearance());
    }

    #[test]
    fn resetting_one_field_leaves_the_others_alone() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        for field in [ThemeField::PrimaryColor, ThemeField::Background] {
            theme
                .set(field, "#abcdef", MermaidThemePreset::Modern, &defaults)
                .unwrap();
        }

        theme.reset_field(ThemeField::PrimaryColor);
        assert!(!theme.is_overridden(ThemeField::PrimaryColor));
        assert_eq!(
            theme.value(ThemeField::PrimaryColor, &defaults),
            "#101010",
            "a reset field reads the preset default again"
        );
        assert!(theme.is_overridden(ThemeField::Background));
    }

    #[test]
    fn resetting_the_whole_theme_clears_every_override_but_keeps_the_preset() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        theme.choose_preset(MermaidThemePreset::Forest);
        for field in ThemeField::ALL {
            let value = match field.kind() {
                ThemeFieldKind::FontStack => "Other Sans",
                ThemeFieldKind::Size => "20",
                ThemeFieldKind::Colour => "#abcdef",
            };
            theme
                .set(field, value, MermaidThemePreset::Modern, &defaults)
                .unwrap();
        }
        assert!(theme.has_overrides());

        theme.reset_overrides();
        assert!(!theme.has_overrides());
        for field in ThemeField::ALL {
            assert!(!theme.is_overridden(field), "{field:?}");
            assert_eq!(theme.value(field, &defaults), defaults.get(field));
        }
        assert_eq!(
            theme.base(MermaidThemePreset::Dark),
            MermaidThemePreset::Forest,
            "resetting the fields is not the same request as un-choosing the preset"
        );
    }

    #[test]
    fn invalid_colours_are_refused_and_never_stored() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        for raw in [
            "",
            "   ",
            "#12",
            "#12345",
            "not-a-colour",
            "#gggggg",
            "rgb(1, 2)",
            "rgb(1, 2, 3, 4, 5)",
            "hsl(1, 2)",
            "#1234567",
            "rgb(a, b, c)",
        ] {
            assert_eq!(
                theme.set(
                    ThemeField::PrimaryColor,
                    raw,
                    MermaidThemePreset::Modern,
                    &defaults
                ),
                Err(ThemeFieldError::NotAColour),
                "{raw:?} must not reach the renderer"
            );
        }
        assert!(!theme.has_overrides());
        assert!(
            theme.follows_appearance(),
            "a refused value must not pin the tab either"
        );
    }

    #[test]
    fn invalid_sizes_are_refused() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        for raw in ["", "big", "0", "7.9", "48.1", "1000", "NaN", "inf", "-12"] {
            assert_eq!(
                theme.set(
                    ThemeField::FontSize,
                    raw,
                    MermaidThemePreset::Modern,
                    &defaults
                ),
                Err(ThemeFieldError::NotASize),
                "{raw:?}"
            );
        }
        assert!(!theme.has_overrides());
    }

    #[test]
    fn an_empty_font_stack_is_refused() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        assert_eq!(
            theme.set(
                ThemeField::FontFamily,
                "   ",
                MermaidThemePreset::Modern,
                &defaults
            ),
            Err(ThemeFieldError::Empty)
        );
        assert!(!theme.has_overrides());
    }

    #[test]
    fn a_font_stack_is_stored_trimmed() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        theme
            .set(
                ThemeField::FontFamily,
                "  Comic Sans MS, sans-serif  ",
                MermaidThemePreset::Modern,
                &defaults,
            )
            .unwrap();
        assert_eq!(
            theme.value(ThemeField::FontFamily, &defaults),
            "Comic Sans MS, sans-serif"
        );
    }

    /// The per-tab requirement, asserted on the model rather than on the view:
    /// two themes are two values and share nothing.
    #[test]
    fn two_tabs_hold_independent_themes() {
        let defaults = defaults();
        let mut first = TabTheme::default();
        let mut second = TabTheme::default();

        first.choose_preset(MermaidThemePreset::Forest);
        first
            .set(
                ThemeField::PrimaryColor,
                "#ff0000",
                MermaidThemePreset::Modern,
                &defaults,
            )
            .unwrap();

        assert!(second.follows_appearance());
        assert!(!second.is_overridden(ThemeField::PrimaryColor));
        assert_eq!(second.value(ThemeField::PrimaryColor, &defaults), "#101010");

        second.choose_preset(MermaidThemePreset::Neutral);
        assert_eq!(
            first.base(MermaidThemePreset::Dark),
            MermaidThemePreset::Forest
        );
        assert_eq!(
            second.base(MermaidThemePreset::Dark),
            MermaidThemePreset::Neutral
        );
        assert_eq!(first.value(ThemeField::PrimaryColor, &defaults), "#FF0000");
        assert_eq!(second.value(ThemeField::PrimaryColor, &defaults), "#101010");
    }

    #[test]
    fn resolving_answers_follow_the_appearance_before_the_render_path_sees_it() {
        let defaults = defaults();
        let mut theme = TabTheme::default();
        assert_eq!(
            theme.resolve(MermaidThemePreset::Dark).base(),
            MermaidThemePreset::Dark
        );

        theme
            .set(
                ThemeField::Background,
                "#ff0000",
                MermaidThemePreset::Dark,
                &defaults,
            )
            .unwrap();
        let resolved = theme.resolve(MermaidThemePreset::Modern);
        assert_eq!(resolved.base(), MermaidThemePreset::Dark);
        assert_eq!(
            resolved.overrides().collect::<Vec<_>>(),
            vec![(ThemeField::Background, "#FF0000")]
        );
    }

    /// The render path skips work when the source hash is unchanged, so the
    /// theme has to be part of what is hashed — which is only true if two
    /// different themes actually hash differently.
    #[test]
    fn two_different_themes_are_not_equal() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let hash = |theme: &MermaidTheme| {
            let mut hasher = DefaultHasher::new();
            theme.hash(&mut hasher);
            hasher.finish()
        };

        let defaults = defaults();
        let modern = TabTheme::default().resolve(MermaidThemePreset::Modern);
        let dark = TabTheme::default().resolve(MermaidThemePreset::Dark);
        assert_ne!(hash(&modern), hash(&dark));

        let mut tweaked = TabTheme::default();
        tweaked
            .set(
                ThemeField::PrimaryColor,
                "#ff0000",
                MermaidThemePreset::Modern,
                &defaults,
            )
            .unwrap();
        assert_ne!(
            hash(&modern),
            hash(&tweaked.resolve(MermaidThemePreset::Modern))
        );
    }

    /// The overrides map is ordered, so a theme built by setting the same
    /// fields in a different order is the *same* theme — otherwise the render
    /// key would change for no reason and every edit would re-render.
    #[test]
    fn override_order_does_not_change_the_resolved_theme() {
        let defaults = defaults();
        let build = |fields: [ThemeField; 2]| {
            let mut theme = TabTheme::default();
            for field in fields {
                theme
                    .set(field, "#abcdef", MermaidThemePreset::Modern, &defaults)
                    .unwrap();
            }
            theme.resolve(MermaidThemePreset::Modern)
        };
        assert_eq!(
            build([ThemeField::PrimaryColor, ThemeField::Background]),
            build([ThemeField::Background, ThemeField::PrimaryColor])
        );
    }

    #[test]
    fn hex_colours_of_every_length_canonicalise() {
        assert_eq!(canonical_colour("#abc").as_deref(), Some("#AABBCC"));
        assert_eq!(canonical_colour("#ABC").as_deref(), Some("#AABBCC"));
        assert_eq!(canonical_colour("  #a1b2c3 ").as_deref(), Some("#A1B2C3"));
        assert_eq!(canonical_colour("#ccc").as_deref(), Some("#CCCCCC"));
        assert_eq!(canonical_colour("#12345678").as_deref(), Some("#12345678"));
        assert_eq!(canonical_colour("#abcd").as_deref(), Some("#AABBCCDD"));
    }

    /// A fully opaque colour must canonicalise without an alpha pair, or the
    /// default-comparison in [`TabTheme::set`] would see `#FFFFFF` and
    /// `#FFFFFFFF` as different values.
    #[test]
    fn a_fully_opaque_colour_drops_its_alpha() {
        assert_eq!(canonical_colour("#ffffffff").as_deref(), Some("#FFFFFF"));
        assert_eq!(
            canonical_colour("rgba(0,0,0,1)").as_deref(),
            Some("#000000")
        );
    }

    #[test]
    fn the_function_syntaxes_the_presets_use_all_parse() {
        // `Theme::mermaid_default()`'s edge label background.
        assert_eq!(
            canonical_colour("rgba(248,250,252, 0.92)").as_deref(),
            Some("#F8FAFCEB")
        );
        // `Theme::dark()`'s cluster border.
        assert_eq!(
            canonical_colour("rgba(255, 255, 255, 0.25)").as_deref(),
            Some("#FFFFFF40")
        );
        // A fractional channel, as `MERMAID_GIT_INV_COLORS` spells them.
        assert_eq!(
            canonical_colour("rgb(146.5000000001, 73.2500000001, 0)").as_deref(),
            Some("#934900")
        );
        // `hsl` with fractional percentages, as `adjust_color` emits them.
        assert_eq!(
            canonical_colour("hsl(240, 100%, 46.2745098039%)").as_deref(),
            Some("#0000EC")
        );
        assert_eq!(
            canonical_colour("hsl(0, 0%, 0%)").as_deref(),
            Some("#000000")
        );
        assert_eq!(
            canonical_colour("hsl(120, 100%, 50%)").as_deref(),
            Some("#00FF00")
        );
    }

    #[test]
    fn the_bare_names_the_presets_use_all_parse() {
        for name in ["lightgrey", "LightGrey", "green", "white", "black"] {
            assert!(canonical_colour(name).is_some(), "{name}");
        }
        assert_eq!(canonical_colour("green").as_deref(), Some("#008000"));
        assert_eq!(
            canonical_colour("transparent").as_deref(),
            Some("#00000000")
        );
        assert!(canonical_colour("rebeccapurple").is_none());
    }

    #[test]
    fn a_size_round_trips_through_its_stored_form() {
        assert_eq!(format_font_size(16.0), "16");
        assert_eq!(format_font_size(14.5), "14.5");
        assert_eq!(parse_font_size("16"), Some(16.0));
        assert_eq!(parse_font_size(" 14.5 "), Some(14.5));
    }

    #[test]
    fn stepping_a_size_stays_inside_the_range() {
        assert_eq!(stepped_font_size(16.0, 1.0), 17.0);
        assert_eq!(stepped_font_size(16.0, -1.0), 15.0);
        assert_eq!(stepped_font_size(FONT_SIZE_MIN, -1.0), FONT_SIZE_MIN);
        assert_eq!(stepped_font_size(FONT_SIZE_MAX, 1.0), FONT_SIZE_MAX);
        assert_eq!(stepped_font_size(f32::NAN, 1.0), FONT_SIZE_MIN);
    }

    #[test]
    fn every_preset_has_a_label_and_a_slot_in_the_picker() {
        for preset in MermaidThemePreset::ALL {
            assert!(
                MermaidThemePreset::ALL.contains(&preset),
                "{preset:?} must be reachable from the picker"
            );
            let _ = preset.label();
        }
        assert_eq!(MermaidThemePreset::ALL.len(), 5);
    }

    #[test]
    fn the_appearance_presets_are_the_two_the_workspace_used_to_hard_code() {
        assert_eq!(
            MermaidThemePreset::for_appearance(false),
            MermaidThemePreset::Modern
        );
        assert_eq!(
            MermaidThemePreset::for_appearance(true),
            MermaidThemePreset::Dark
        );
    }
}
