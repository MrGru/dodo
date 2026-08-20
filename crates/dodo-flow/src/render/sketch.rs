//! §13's hand-drawn renderer: **a deterministic function from a canonical
//! outline to a wobbly one**, and nothing else.
//!
//! ```text
//! canonical Outline ──> perturb(style, seed, pass) ──> Outline
//!    (shapes.rs)          splitmix64, no clock,        (plan.rs, cache.rs)
//!                         no thread state, no RNG
//!                         owned by anything
//! ```
//!
//! # The rule the whole module exists to make unbreakable
//!
//! §40 rule 5 and §13, in as many words: **never fresh random values on
//! repaint.** The same element must produce the same sketch geometry until its
//! geometry or its style changes.
//!
//! That is not a caution here, it is the type signature. [`perturb`] takes the
//! seed as an argument and holds no state; [`SketchRng`] is seeded per call and
//! dropped at the end of it; nothing in this file reads a clock, a counter, an
//! atomic or a thread-local. There is no way to call it and get a different
//! answer, which is why the determinism property (§49) is a two-line test
//! rather than a soak test.
//!
//! The seed itself comes from [`element_seed`]: the document's
//! [`SketchStyle::seed`] mixed with the element's
//! [`ElementId`]. Both are serialized, so a reopened
//! document wobbles exactly as it did when it was saved — the id rather than
//! the runtime index on purpose, because an index is a slot number and a
//! reordered load would redraw every shape.
//!
//! # Geometric perturbation, not a bitmap
//!
//! §13 asks for geometric perturbation rather than rendering to textures, and
//! this module is only that: it walks an [`Outline`]'s commands and emits an
//! [`Outline`]. Everything downstream — the flattening estimate, the vertex
//! ceiling, the geometry cache, the clip — works on it unchanged, because it is
//! an ordinary outline. Sketch mode adds no primitive, no painter and no
//! second pipeline.
//!
//! **A straight segment becomes a cubic**, which is where the cost comes from
//! and is also the whole hand-drawn effect: a line that bows and misses its
//! corners. A cubic stays a cubic with its control points nudged.
//!
//! # What this costs, measured
//!
//! Apple M1, release, 2026-08-19, from `cargo run --release -p dodo-flow
//! --example flow_scene_bench --locked` — **real tessellations** through
//! `render::painter::build_path`, not estimates. Clean against sketch at the
//! default hand (roughness 1, bowing 1, **2 strokes**, jitter 2 px), each
//! column at the tolerance its own path would use:
//!
//! | primitive (160×64 unless stated) | clean verts | sketch verts | × | clean µs | sketch µs | × |
//! |---|---:|---:|---:|---:|---:|---:|
//! | rectangle, stroked | 24 | 132 | **5.5×** | 0.86 | 3.16 | 3.7× |
//! | rounded rectangle r8, stroked | 120 | 276 | 2.3× | 2.57 | 8.20 | 3.2× |
//! | diamond, stroked | 24 | 126 | 5.2× | 0.86 | 3.38 | 3.9× |
//! | ellipse, stroked | 264 | 312 | **1.2×** | 4.77 | 6.51 | 1.4× |
//! | line, 200 px | 6 | 24 | 4.0× | 0.42 | 1.11 | 2.6× |
//! | Bézier edge, 200 px | 144 | 168 | **1.2×** | 2.89 | 3.53 | 1.2× |
//! | arrow head, filled | 3 | 33 | 11.0× | 0.77 | 3.71 | 4.8× |
//!
//! **The ratios are not the point; the axis-aligned rows are.** A clean
//! rectangle in this engine is not a 24-vertex path at all — it is a *quad*, at
//! **zero** path vertices and no path batch (Phase 0 §1.7, and
//! [`shapes::prefers_quad`](crate::render::shapes::prefers_quad) is the one
//! place that decides it). A sketched rectangle cannot be a quad, because a
//! quad has four straight axis-aligned sides by definition. So sketch mode
//! moves **every rectangular node body from the cheapest primitive the engine
//! has onto two of its most expensive**, and that — not the 1.2× on an
//! ellipse — is what the budget has to survive. [`crate::render::lod`] is where
//! it is spent.
//!
//! Per scene, same run, clean against hand-drawn:
//!
//! | scene | paths | est. verts | painted | tessellate | hand |
//! |---|---:|---:|---:|---:|---|
//! | large, clean | 126 | 31,188 | 19,242 | 0.36 ms | — |
//! | **large, sketch** | 324 | 65,676 | **32,790** | 0.70 ms | kept |
//! | dense, clean | 2,986 | 59,720 | 17,916 | 1.11 ms | — |
//! | **dense, sketch** | 2,986 | 59,720 | 17,916 | 1.17 ms | **dropped by the ladder** |
//! | scattered, sketch | 4,998 | 126,799 | 39,414 | 2.22 ms | kept (36 bodies) |
//!
//! So a realistic frame that keeps the hand costs about **2.6× the paths and
//! 1.7× the painted vertices** of the same frame clean, and the launcher's own
//! scene agrees: 317 paths / 33,447 vertices sketched against 142 / 15,615
//! clean, in one path batch either way. A frame that cannot afford it is drawn
//! clean and says so.
//!
//! The two mitigations are both in [`SketchStyle`], and both were measured:
//!
//! - **[`SketchStyle::TOLERANCE_FACTOR`]** — sketch geometry is flattened at 3×
//!   the document's tolerance, because a deliberately imprecise line does not
//!   need a quarter-pixel bow. A sketched rectangle and ellipse together are
//!   **714 painted vertices at the document tolerance and 444 at 3×** (14.1 µs
//!   against 9.1 µs) — 38 % fewer vertices and 35 % less CPU, with no visible
//!   difference at any zoom the shape is legible at. 4× buys a further 13 % and
//!   starts to show.
//! - **[`SketchStyle::stroke_count`]** — a straight multiplier on both frame
//!   budgets, capped at [`SketchStyle::MAX_STROKES`]. Two is the look; three is
//!   50 % more of everything for very little more of it.
//!
//! # The estimate is much looser here than it is for clean geometry
//!
//! The rightmost two columns of the benchmark's primitive table are the vertex
//! *estimate* beside the reality, and they do not track:
//!
//! | | painted | estimated | ratio |
//! |---|---:|---:|---:|
//! | clean rectangle | 24 | 39 | 1.6× — the safety margin, as designed |
//! | **sketched rectangle** | 132 | 596 | **4.5×** |
//! | clean ellipse | 264 | 509 | 1.9× |
//! | sketched ellipse | 312 | 576 | 1.8× |
//!
//! The cause is in [`cubic_segments`](crate::geometry::curve::cubic_segments),
//! and its doc carries the full diagnosis and what it costs the ladder. In
//! short: a perturbed straight side is a cubic that is *nearly straight*, and
//! the flattening estimate sizes a curve by its control hull, which for that
//! shape is its whole length. The estimate is conservative — the direction a
//! black-window guard has to err in — but by 4.5× rather than 1.6×, so
//! [`crate::render::lod`] drops the hand earlier than the painted cost
//! requires. It is recorded rather than fixed here, because fixing it means
//! re-fitting a formula Phase 4 owns and every recorded estimate in the crate
//! is stated against.
//!
//! # Why the fill is not perturbed for a quad-shaped body
//!
//! A rectangle's *fill* stays a quad even in sketch mode — see
//! [`crate::render::scene`]. Only the outline is sketched. A wobbly stroke over
//! a crisp fill is what a marker on a whiteboard looks like anyway, and it
//! halves what sketch mode costs a node: one sketched stroke pair instead of a
//! sketched fill *and* a sketched stroke pair. Shapes with no quad form —
//! ellipses, diamonds — do get a perturbed fill, gently ([`fill`]).
//!
//! **This file names no UI framework.**

use crate::{
    geometry::Vec2,
    models::{ElementId, RenderQuality, SketchStyle},
    render::{
        plan::PathPaint,
        shapes::{Outline, SubpathCommand},
    },
};

/// The stable per-element seed §13 asks for: the document's hand, mixed with
/// the element's identity.
///
/// [`ElementId`] rather than a runtime index, so the wobble survives a save and
/// a reload — see the module doc. `part` separates the paths one element owns
/// (its fill from its stroke), so a shape's fill and its outline do not wobble
/// in lockstep, which looks mechanical.
pub fn element_seed(style: &SketchStyle, element: ElementId, part: u64) -> u64 {
    mix(
        mix(style.seed, element.raw().wrapping_add(0x9E37_79B9)),
        part,
    )
}

/// A seed for something that has no [`ElementId`] — a marker, an overlay, a
/// synthetic outline in a cost estimate.
pub fn derived_seed(seed: u64, part: u64) -> u64 {
    mix(seed, part)
}

/// **The generator.** One pass of the pen over `outline`.
///
/// `pass` is the stroke index, so two calls that differ only in it produce two
/// different squiggles over the same shape — which is what "multiple subtle
/// strokes" is. Everything else being equal, the same arguments always produce
/// the same commands, bit for bit.
///
/// The first pass of a `roughness == 0.0` style returns the outline unchanged,
/// which keeps "sketch with the hand turned off" free rather than merely cheap.
pub fn perturb(outline: &Outline, style: &SketchStyle, seed: u64, pass: u8) -> Outline {
    if style.roughness <= 0.0 || outline.is_empty() {
        return outline.clone();
    }

    let mut rng = SketchRng::new(mix(seed, pass as u64));
    // A cubic per line segment, so the command count grows but not the shape.
    let mut sketched = Outline::with_capacity(outline.commands().len() + 2);

    // `origin` walks the *canonical* outline and `current` the perturbed one:
    // the bow of a segment is computed from where the shape really goes, while
    // the pen carries its own accumulated error. Using the perturbed point for
    // both would let a long polyline drift arbitrarily far from its shape.
    let mut origin = Vec2::ZERO;
    let mut start = Vec2::ZERO;
    let mut started = false;

    for command in outline.commands() {
        match *command {
            SubpathCommand::MoveTo(to) => {
                sketched.move_to(jitter(to, style, &mut rng));
                origin = to;
                start = to;
                started = true;
            }
            SubpathCommand::LineTo(to) => {
                if !started {
                    sketched.move_to(jitter(to, style, &mut rng));
                    origin = to;
                    start = to;
                    started = true;
                    continue;
                }
                bow(&mut sketched, origin, to, style, &mut rng);
                origin = to;
            }
            SubpathCommand::CubicTo { c1, c2, to } => {
                // A curve already curves; it only needs its control points
                // nudged. Bowing one would fight the shape it is drawing.
                sketched.cubic_to(
                    jitter(c1, style, &mut rng),
                    jitter(c2, style, &mut rng),
                    jitter(to, style, &mut rng),
                );
                origin = to;
            }
            SubpathCommand::Close => {
                // **The imperfect corner.** A hand-drawn shape closes back on
                // its start with a bow like every other side, and then the
                // `close` joins the two jittered ends — which is exactly the
                // small overshoot a real pen leaves.
                if started && (start - origin).length() > f32::EPSILON {
                    bow(&mut sketched, origin, start, style, &mut rng);
                }
                sketched.close();
                origin = start;
            }
        }
    }

    sketched
}

/// Every stroke of one outline, in pass order.
///
/// A `Vec` rather than an iterator because each pass is an owned [`Outline`]
/// that goes straight into a [`PathPrimitive`](crate::render::plan::PathPrimitive),
/// and because the count is [`SketchStyle::strokes`] — two, not two thousand.
pub fn strokes(outline: &Outline, style: &SketchStyle, seed: u64) -> Vec<Outline> {
    (0..style.strokes())
        .map(|pass| perturb(outline, style, seed, pass))
        .collect()
}

/// A perturbed **fill**, for a shape with no quad form.
///
/// One pass, with the hand turned down: a fill's boundary is a large area of
/// colour rather than a line, so the same tremor that reads as pen on a stroke
/// reads as a mistake on a fill. Half the jitter and no bowing.
pub fn fill(outline: &Outline, style: &SketchStyle, seed: u64) -> Outline {
    let gentle = SketchStyle {
        jitter: style.jitter * FILL_JITTER_SCALE,
        bowing: 0.0,
        ..*style
    };
    perturb(outline, &gentle, seed, FILL_PASS)
}

/// How much of the stroke's tremor a fill boundary gets. See [`fill`].
pub const FILL_JITTER_SCALE: f32 = 0.5;

/// The pass index a fill is generated at, so it never coincides with a stroke's.
pub const FILL_PASS: u8 = 200;

/// **The vertex estimate for a sketched outline**, summed over its strokes.
///
/// Built by running the real generator rather than by a multiplier, for the
/// reason [`crate::render::lod::EdgeDetail::estimated_vertices`] gives: a
/// second formula is how an estimator and a painter drift apart, and the number
/// this returns is spent against a ceiling whose failure mode is a black
/// window.
pub fn estimated_vertices(
    outline: &Outline,
    style: &SketchStyle,
    seed: u64,
    paint: PathPaint,
    quality: RenderQuality,
) -> u32 {
    let quality = style.quality(quality);
    (0..style.strokes())
        .map(|pass| perturb(outline, style, seed, pass).estimated_vertices(paint, quality))
        .fold(0u32, |total, pass| total.saturating_add(pass))
}

/// One point, displaced. The tremor is in **screen pixels** and does not scale
/// with the shape — see [`SketchStyle`].
fn jitter(point: Vec2, style: &SketchStyle, rng: &mut SketchRng) -> Vec2 {
    let amount = style.jitter * style.roughness;
    Vec2::new(
        point.x + rng.signed() * amount,
        point.y + rng.signed() * amount,
    )
}

/// A straight segment as a bowed cubic with jittered ends.
///
/// The bow is perpendicular to the chord and proportional to its length — a
/// 40 px side bends less than a 400 px one, which is how a hand actually
/// behaves — and it is clamped, or a document-crossing edge would bow into a
/// semicircle.
fn bow(into: &mut Outline, from: Vec2, to: Vec2, style: &SketchStyle, rng: &mut SketchRng) {
    let delta = to - from;
    let length = delta.length();
    if !length.is_finite() || length <= f32::EPSILON {
        into.line_to(jitter(to, style, rng));
        return;
    }

    let normal = Vec2::new(-delta.y / length, delta.x / length);
    let amplitude =
        style.bowing * style.jitter * style.roughness * (length / BOW_REFERENCE).clamp(0.25, 3.0);
    let offset = normal * (rng.signed() * amplitude);

    let c1 = from + delta * (1.0 / 3.0) + offset;
    let c2 = from + delta * (2.0 / 3.0) + offset;
    into.cubic_to(
        jitter(c1, style, rng),
        jitter(c2, style, rng),
        jitter(to, style, rng),
    );
}

/// The segment length at which the bow is drawn at its nominal amplitude, in
/// screen pixels. Roughly a node's width, so a node's sides bow by about
/// `bowing × jitter`.
const BOW_REFERENCE: f32 = 160.0;

/// **A seeded, stateless, platform-independent generator.**
///
/// splitmix64, which is eight lines and has no dependency: `rand` would be a
/// new crate in `Cargo.lock` for a job whose whole requirement is *"the same
/// numbers every time"*, and a generator whose stream is defined by this file
/// cannot be changed by a version bump. Integer arithmetic throughout, so the
/// stream is identical on every platform — a document drawn on macOS wobbles
/// the same way on Windows.
#[derive(Debug, Clone)]
pub struct SketchRng(u64);

impl SketchRng {
    pub fn new(seed: u64) -> SketchRng {
        SketchRng(seed ^ GOLDEN)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(GOLDEN);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `-1.0..=1.0`.
    ///
    /// Built from 24 integer bits and an exact power-of-two divisor, so it is
    /// the same float everywhere — no `f64` rounding, no libm, no locale.
    pub fn signed(&mut self) -> f32 {
        let bits = (self.next_u64() >> 40) as f32; // 0 .. 2^24 - 1
        bits / 8_388_608.0 - 1.0
    }
}

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

fn mix(a: u64, b: u64) -> u64 {
    let mut z = a ^ b.wrapping_mul(GOLDEN);
    z = (z ^ (z >> 33)).wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    z = (z ^ (z >> 33)).wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    z ^ (z >> 33)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{geometry::Rect, models::ElementId};
    use crate::{
        models::Color,
        render::shapes::{ellipse, rectangle, rounded_rectangle},
    };

    fn style() -> SketchStyle {
        SketchStyle::DEFAULT
    }

    fn rect() -> Rect {
        Rect::new(Vec2::new(10.0, 20.0), Vec2::new(160.0, 64.0))
    }

    fn commands(outline: &Outline) -> Vec<SubpathCommand> {
        outline.commands().to_vec()
    }

    /// **§49's property, and the reason this module exists.**
    ///
    /// Same element, same seed, same geometry ⇒ the same generated geometry,
    /// compared as raw bits rather than approximately: `-0.0` and `0.0` are the
    /// same number and different bits, and a generator that produced one and
    /// then the other would be a cache key that missed forever.
    #[test]
    fn the_same_element_and_seed_and_geometry_generate_identical_bits() {
        let seed = element_seed(&style(), ElementId::new(7), 0);

        for pass in 0..4 {
            let first = perturb(&rectangle(rect()), &style(), seed, pass);
            let second = perturb(&rectangle(rect()), &style(), seed, pass);

            assert_eq!(
                bits(&first),
                bits(&second),
                "pass {pass} was not reproducible"
            );
        }
    }

    /// The other half of it: a *repaint* is not a fresh drawing. Ten
    /// generations, as ten frames of an unchanged element would be.
    #[test]
    fn a_hundred_repaints_of_one_element_never_differ() {
        let seed = element_seed(&style(), ElementId::new(3), 1);
        let first = bits(&perturb(&ellipse(rect()), &style(), seed, 0));

        for frame in 1..100 {
            assert_eq!(
                bits(&perturb(&ellipse(rect()), &style(), seed, 0)),
                first,
                "frame {frame} redrew the element differently"
            );
        }
    }

    #[test]
    fn two_elements_two_seeds_two_squiggles() {
        let a = element_seed(&style(), ElementId::new(1), 0);
        let b = element_seed(&style(), ElementId::new(2), 0);

        assert_ne!(a, b, "two elements must not share a seed");
        assert_ne!(
            bits(&perturb(&rectangle(rect()), &style(), a, 0)),
            bits(&perturb(&rectangle(rect()), &style(), b, 0)),
        );
    }

    /// A shape's fill and its outline must not wobble in lockstep — that reads
    /// as a printing offset rather than as a hand.
    #[test]
    fn one_element_separates_its_parts() {
        let style = style();
        assert_ne!(
            element_seed(&style, ElementId::new(9), 0),
            element_seed(&style, ElementId::new(9), 1),
        );
    }

    #[test]
    fn each_stroke_of_one_shape_is_a_different_squiggle() {
        let seed = element_seed(&style(), ElementId::new(4), 0);
        let passes = strokes(&rectangle(rect()), &style(), seed);

        assert_eq!(passes.len(), style().strokes() as usize);
        assert_ne!(bits(&passes[0]), bits(&passes[1]));
    }

    /// Changing the geometry changes the drawing — the other direction of the
    /// determinism property, and what makes the cache key's `version` field
    /// mean something.
    #[test]
    fn moving_the_element_redraws_it() {
        let seed = element_seed(&style(), ElementId::new(5), 0);
        let moved = Rect::new(rect().origin + Vec2::splat(40.0), rect().size);

        assert_ne!(
            bits(&perturb(&rectangle(rect()), &style(), seed, 0)),
            bits(&perturb(&rectangle(moved), &style(), seed, 0)),
        );
    }

    /// **The wobble stays near the shape.** A perturbation that could wander is
    /// a shape that leaves its own bounding box — which would escape the
    /// spatial index's cull rectangle and, at the pane's edge, be clipped away
    /// entirely.
    #[test]
    fn the_wobble_stays_within_a_bounded_margin() {
        let style = SketchStyle {
            roughness: 2.0,
            ..SketchStyle::DEFAULT
        };
        // Jitter on both control points and the endpoint, plus the bow, plus
        // the diagonal — generous, and still a small constant.
        let margin = style.jitter * style.roughness * 3.0 + style.bowing * style.jitter * 6.0;

        for element in 0..64u64 {
            let seed = element_seed(&style, ElementId::new(element), 0);
            for pass in 0..style.strokes() {
                let bounds = perturb(&rounded_rectangle(rect(), 8.0), &style, seed, pass)
                    .bounds()
                    .expect("a rectangle has bounds");

                assert!(
                    rect().inflate(margin).contains_rect(bounds),
                    "element {element} pass {pass} escaped by more than {margin} px: {bounds:?}"
                );
            }
        }
    }

    /// A hand that does not move is a clean drawing, and it must cost nothing
    /// extra to say so.
    #[test]
    fn a_roughness_of_zero_is_the_canonical_outline() {
        let flat = SketchStyle {
            roughness: 0.0,
            ..SketchStyle::DEFAULT
        };

        assert_eq!(
            bits(&perturb(&rectangle(rect()), &flat, 1, 0)),
            bits(&rectangle(rect())),
        );
    }

    #[test]
    fn an_empty_outline_stays_empty() {
        assert!(perturb(&Outline::new(), &style(), 1, 0).is_empty());
        assert!(
            strokes(&Outline::new(), &style(), 1)
                .iter()
                .all(|o| o.is_empty())
        );
    }

    /// A straight side becomes a curve — that *is* the effect, and it is also
    /// where the cost comes from. Stated as a test so a later "optimisation"
    /// that quietly emits lines again shows up as a failing expectation rather
    /// than as a canvas that stopped looking hand-drawn.
    #[test]
    fn a_straight_side_becomes_a_bowed_cubic() {
        let seed = element_seed(&style(), ElementId::new(6), 0);
        let sketched = perturb(&rectangle(rect()), &style(), seed, 0);

        let cubics = commands(&sketched)
            .iter()
            .filter(|c| matches!(c, SubpathCommand::CubicTo { .. }))
            .count();

        assert_eq!(cubics, 4, "each side of the rectangle bows");
        assert!(commands(&sketched).contains(&SubpathCommand::Close));
    }

    /// The number the budget is spent against, and the one Phase 6's tables
    /// report. A sketched rectangle is not a quad and cannot be one.
    #[test]
    fn a_sketched_rectangle_costs_far_more_than_a_clean_one() {
        let seed = element_seed(&style(), ElementId::new(8), 0);
        let paint = PathPaint::Stroke {
            color: Color::WHITE,
            width: 1.5,
        };
        let quality = RenderQuality::BALANCED;

        let clean = rectangle(rect()).estimated_vertices(paint, quality);
        let sketched = estimated_vertices(&rectangle(rect()), &style(), seed, paint, quality);

        assert!(
            sketched > clean * 4,
            "a sketched rectangle should be several times a clean one: {sketched} vs {clean}"
        );
        // And the tolerance factor has to be doing its job, or the number above
        // would be twice as large. See `SketchStyle::TOLERANCE_FACTOR`.
        let precise: u32 = (0..style().strokes())
            .map(|pass| {
                perturb(&rectangle(rect()), &style(), seed, pass).estimated_vertices(paint, quality)
            })
            .sum();
        assert!(
            sketched < precise,
            "the sketch tolerance must be looser than the document's: {sketched} vs {precise}"
        );
    }

    /// The generator's stream is part of the contract: it decides what every
    /// cached tessellation looks like, and a silent change to it would be a
    /// silent change to every saved document's appearance.
    #[test]
    fn the_generator_stream_is_pinned() {
        let mut rng = SketchRng::new(0);
        let first: Vec<u64> = (0..3).map(|_| rng.next_u64()).collect();

        assert_eq!(
            first,
            vec![
                7_960_286_522_194_355_700,
                487_617_019_471_545_679,
                17_909_611_376_780_542_444
            ],
        );

        let mut rng = SketchRng::new(1);
        for _ in 0..1_000 {
            let value = rng.signed();
            assert!((-1.0..=1.0).contains(&value), "{value} left the range");
        }
    }

    /// Compares two outlines as bits rather than as floats. See the first test.
    fn bits(outline: &Outline) -> Vec<u32> {
        fn push(out: &mut Vec<u32>, v: Vec2) {
            out.push(v.x.to_bits());
            out.push(v.y.to_bits());
        }

        let mut out = Vec::new();
        for command in outline.commands() {
            match *command {
                SubpathCommand::MoveTo(p) => {
                    out.push(0);
                    push(&mut out, p);
                }
                SubpathCommand::LineTo(p) => {
                    out.push(1);
                    push(&mut out, p);
                }
                SubpathCommand::CubicTo { c1, c2, to } => {
                    out.push(2);
                    push(&mut out, c1);
                    push(&mut out, c2);
                    push(&mut out, to);
                }
                SubpathCommand::Close => out.push(3),
            }
        }
        out
    }
}
