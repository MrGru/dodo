//! §39's performance instrumentation: named probes, off by default, cheap
//! enough to leave in.
//!
//! §39 asks for "feature-gated or debug instrumentation" over a named list of
//! timings, and for it to be easy to compare before and after an optimisation.
//! This is that list as an enum, an array of counters indexed by it, and a flag.
//!
//! # Why a value on the view rather than a feature or a global
//!
//! A cargo feature would mean the benchmark harness and the shipping build are
//! different programs, and the number you optimise against is then not the
//! number you ship. A global would mean two canvases in one process share a
//! total. So [`Instruments`] is a plain field: the view builds one from
//! `DODO_FLOW_INSTRUMENT`, the benchmark harness builds one that is always on,
//! and a test builds one and asserts on it.
//!
//! # What "cheap enough to leave in" costs
//!
//! When it is off, [`Instruments::start`] is one bool test and
//! [`Instruments::record`] is one more; no clock is read. When it is on, each
//! probe is two `Instant::now()` calls — about 40 ns on the M1 — against
//! visibility queries measured in microseconds and paints in milliseconds.
//!
//! The awkward part of the API is deliberate. A scope guard holding
//! `&mut Instruments` could not coexist with the `&mut self` the instrumented
//! code needs, so [`Timer`] is a `Copy` value that borrows nothing:
//!
//! ```
//! # use dodo_flow::instrument::{Instruments, Probe};
//! # struct View { instruments: Instruments }
//! # impl View {
//! #     fn work(&mut self) {}
//! fn frame(&mut self) {
//!     let timer = self.instruments.start();
//!     self.work();
//!     self.instruments.record(Probe::CanvasPaint, timer);
//! }
//! # }
//! ```
//!
//! **This file names no UI framework.**

use std::{fmt, time::Instant};

/// The timings §39 names, plus the two counters that are not timings.
///
/// The list is §39's verbatim where it applies to this phase. `NodeBuild` and
/// `TextLayout` exist with nothing recording them yet — Phase 5 owns rich node
/// elements and text — because the point of a named list is that a later phase
/// finds its name already there rather than inventing a second vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Probe {
    /// [`SpatialIndex::query_visible`](crate::spatial::SpatialIndex::query_visible).
    VisibilityQuery,
    /// Turning the visible set into primitives — [`crate::render::scene`].
    RenderExtract,
    /// Building rich GPUI elements for visible nodes. Phase 5's.
    NodeBuild,
    /// [`GraphWorld::rebuild_dirty_geometry`](crate::runtime::GraphWorld::rebuild_dirty_geometry).
    EdgeRoute,
    /// A cached tessellation was reused. **A count, not a timing** — see
    /// [`Instruments::count`].
    GeometryCacheHit,
    /// A tessellation had to be built. A count.
    GeometryCacheMiss,
    /// `PaintPlan::paint_into`, which is where tessellation actually happens.
    CanvasPaint,
    /// [`SpatialIndex::sync`](crate::spatial::SpatialIndex::sync).
    SpatialUpdate,
    /// A pointer press resolved, or a rubber band committed.
    HitTest,
    /// Shaping a line of text. Phase 5's.
    TextLayout,
}

impl Probe {
    /// Every probe, in report order.
    pub const ALL: [Probe; 10] = [
        Probe::VisibilityQuery,
        Probe::RenderExtract,
        Probe::NodeBuild,
        Probe::EdgeRoute,
        Probe::GeometryCacheHit,
        Probe::GeometryCacheMiss,
        Probe::CanvasPaint,
        Probe::SpatialUpdate,
        Probe::HitTest,
        Probe::TextLayout,
    ];

    /// The name §39 writes it under, so a log line and the requirements
    /// document use the same word.
    pub fn name(self) -> &'static str {
        match self {
            Probe::VisibilityQuery => "visibility_query",
            Probe::RenderExtract => "render_extract",
            Probe::NodeBuild => "node_build",
            Probe::EdgeRoute => "edge_route",
            Probe::GeometryCacheHit => "geometry_cache_hit",
            Probe::GeometryCacheMiss => "geometry_cache_miss",
            Probe::CanvasPaint => "canvas_paint",
            Probe::SpatialUpdate => "spatial_update",
            Probe::HitTest => "hit_test",
            Probe::TextLayout => "text_layout",
        }
    }

    fn index(self) -> usize {
        Probe::ALL
            .iter()
            .position(|probe| *probe == self)
            .unwrap_or(0)
    }
}

/// One probe's accumulated total.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sample {
    pub calls: u64,
    pub nanos: u64,
}

impl Sample {
    pub fn is_empty(&self) -> bool {
        self.calls == 0
    }

    /// Mean cost per call, in microseconds. Zero for a probe that only counts.
    pub fn micros_per_call(&self) -> f64 {
        if self.calls == 0 {
            return 0.0;
        }
        self.nanos as f64 / 1_000.0 / self.calls as f64
    }

    pub fn total_millis(&self) -> f64 {
        self.nanos as f64 / 1_000_000.0
    }
}

/// A started measurement, or nothing at all when instrumentation is off.
///
/// `Copy` and borrowing nothing — see the module doc for why that shape is the
/// whole design.
#[derive(Debug, Clone, Copy)]
pub struct Timer(Option<Instant>);

impl Timer {
    /// A timer that will record nothing, for a caller with no instruments.
    pub const OFF: Timer = Timer(None);
}

/// §39's probes and their totals.
#[derive(Debug, Clone, Default)]
pub struct Instruments {
    enabled: bool,
    samples: [Sample; Probe::ALL.len()],
}

impl Instruments {
    /// Instrumentation off. The shipping default.
    pub fn off() -> Instruments {
        Instruments::default()
    }

    pub fn on() -> Instruments {
        Instruments {
            enabled: true,
            ..Instruments::default()
        }
    }

    /// On when `DODO_FLOW_INSTRUMENT` is set, off otherwise.
    ///
    /// Read once, at construction, rather than per probe: the environment does
    /// not change while a canvas is open, and this is the frame path.
    pub fn from_env() -> Instruments {
        if std::env::var_os("DODO_FLOW_INSTRUMENT").is_some() {
            Instruments::on()
        } else {
            Instruments::off()
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Starts a measurement. **One bool test when off**, and no clock read.
    pub fn start(&self) -> Timer {
        if self.enabled {
            Timer(Some(Instant::now()))
        } else {
            Timer::OFF
        }
    }

    /// Ends a measurement started by [`start`](Instruments::start).
    pub fn record(&mut self, probe: Probe, timer: Timer) {
        let Some(started) = timer.0 else {
            return;
        };
        let sample = &mut self.samples[probe.index()];
        sample.calls += 1;
        sample.nanos = sample
            .nanos
            .saturating_add(started.elapsed().as_nanos() as u64);
    }

    /// Adds to a probe that counts rather than times — the two geometry-cache
    /// probes, which are hits and misses rather than durations.
    pub fn count(&mut self, probe: Probe, times: u64) {
        if !self.enabled {
            return;
        }
        self.samples[probe.index()].calls += times;
    }

    pub fn sample(&self, probe: Probe) -> Sample {
        self.samples[probe.index()]
    }

    /// Clears every total, keeping the enabled flag — a benchmark measures one
    /// scenario at a time.
    pub fn reset(&mut self) {
        self.samples = [Sample::default(); Probe::ALL.len()];
    }

    /// A table of every probe that recorded anything.
    pub fn report(&self) -> Report<'_> {
        Report(self)
    }
}

/// [`Instruments::report`]'s output, formatted on demand so building it costs
/// nothing until it is printed.
pub struct Report<'a>(&'a Instruments);

impl fmt::Display for Report<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.0.enabled {
            return writeln!(f, "  (instrumentation off — set DODO_FLOW_INSTRUMENT=1)");
        }

        for probe in Probe::ALL {
            let sample = self.0.sample(probe);
            if sample.is_empty() {
                continue;
            }
            writeln!(
                f,
                "  {:<20} {:>9} calls   {:>9.3} ms   {:>8.2} µs/call",
                probe.name(),
                sample.calls,
                sample.total_millis(),
                sample.micros_per_call()
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_probe_has_a_distinct_name_and_a_stable_index() {
        let mut names: Vec<&str> = Probe::ALL.iter().map(|probe| probe.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two probes share a name");

        for (expected, probe) in Probe::ALL.iter().enumerate() {
            assert_eq!(probe.index(), expected);
        }
    }

    #[test]
    fn instrumentation_that_is_off_records_nothing() {
        let mut instruments = Instruments::off();

        let timer = instruments.start();
        instruments.record(Probe::CanvasPaint, timer);
        instruments.count(Probe::GeometryCacheHit, 5);

        assert!(instruments.sample(Probe::CanvasPaint).is_empty());
        assert!(instruments.sample(Probe::GeometryCacheHit).is_empty());
        assert_eq!(
            instruments.report().to_string(),
            "  (instrumentation off — set DODO_FLOW_INSTRUMENT=1)\n"
        );
    }

    #[test]
    fn a_recorded_probe_accumulates_calls_and_time() {
        let mut instruments = Instruments::on();

        for _ in 0..3 {
            let timer = instruments.start();
            std::hint::black_box((0..1_000).sum::<u64>());
            instruments.record(Probe::VisibilityQuery, timer);
        }

        let sample = instruments.sample(Probe::VisibilityQuery);
        assert_eq!(sample.calls, 3);
        assert!(sample.nanos > 0);
        assert!(sample.micros_per_call() > 0.0);
        assert!(
            instruments
                .report()
                .to_string()
                .contains("visibility_query")
        );
    }

    #[test]
    fn a_counting_probe_has_calls_and_no_time() {
        let mut instruments = Instruments::on();
        instruments.count(Probe::GeometryCacheMiss, 7);
        instruments.count(Probe::GeometryCacheMiss, 3);

        let sample = instruments.sample(Probe::GeometryCacheMiss);
        assert_eq!(sample.calls, 10);
        assert_eq!(sample.nanos, 0);
        assert_eq!(sample.micros_per_call(), 0.0);
    }

    #[test]
    fn reset_clears_totals_but_not_the_switch() {
        let mut instruments = Instruments::on();
        instruments.count(Probe::HitTest, 1);

        instruments.reset();
        assert!(instruments.sample(Probe::HitTest).is_empty());
        assert!(instruments.is_enabled());
    }

    /// A timer taken while off and recorded after switching on must not invent
    /// a duration out of the switch.
    #[test]
    fn a_timer_from_an_off_instrument_stays_off() {
        let mut instruments = Instruments::off();
        let timer = instruments.start();

        instruments.set_enabled(true);
        instruments.record(Probe::SpatialUpdate, timer);

        assert!(instruments.sample(Probe::SpatialUpdate).is_empty());
    }
}
