//! Hot-path work against session length — SE6's regression check.
//!
//! One pane's live paths, each run after a *short* and a *long* session of
//! dense prints, then driven through the same window of live work; the work
//! of that window is counted per unit and held to budgets declared below,
//! before any measurement:
//!
//! | Path | Unit | Thread | What runs |
//! | --- | --- | --- | --- |
//! | `trade.chart` | print | UI | `ChartState::ingest_live` (tick:50, footprint on) and one lane command per 5 prints |
//! | `trade.book` | print | book worker | `BookEngine::record_trade` |
//! | `depth.book` | depth update | book worker | `BookEngine::handle_depth_event_at` |
//! | `frame.book` | frame | book worker | `BookEngine::project_at` over the newest 120 bars, heatmap and bubbles on |
//! | `frame.app.tick50` / `frame.app.time1d` | frame of 5 prints | UI | the whole application frame (`run_frame`): feed drain, ingest, worker sends and reads, layout and paint, with the lane and CVD on |
//! | `frame.worker.tick50` / `frame.worker.time1d` | frame of 5 prints | indicator worker | the same frames on the worker: the batch, the lane walk, the deltas |
//!
//! `time:1d` keeps the whole session inside one forming bar, so its lane walk
//! is where an O(forming prints) fold would show; `tick:50` is the trader's
//! footprint chart.
//!
//! **What is counted.** Heap work on the thread doing it ([`crate::work_meter`]:
//! allocations, bytes allocated, bytes a reallocation may copy), prints folded
//! by the forming-run walk, prints copied to the worker by the lane transport,
//! and CPU nanoseconds per unit, the median lap — reported, never asserted in
//! the fast variant. A loop
//! that walks history without allocating is invisible to the counts; the long
//! variant's time ratio is what catches it.
//!
//! **Two variants.** [`hot_path_work_is_independent_of_session_length`] runs
//! in the ordinary `cargo test` at 1 and 10 minutes of the sustained rate,
//! counts only. The long variant, `#[ignore]`d, holds 1 minute against the
//! envelope's edge (`RETAINED_TRADES_PER_PANE` prints) and also bounds the
//! time ratio:
//!
//! ```sh
//! env -u QUANTICK_BUBBLES cargo test --release -p quantick-app \
//!     session_length_tests::long -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Its output is committed as `docs/quality/session-length/long.txt`.

use super::*;
use crate::indicator_worker::{IndicatorCommand, IndicatorSource, LaneTransport};
use crate::live_envelope::{DEPTH_UPDATES_PER_S, RETAINED_TRADES_PER_PANE, SUSTAINED_TRADES_PER_S};
use crate::state::ChartState;
use crate::work_meter::{self, Tally};
use quantick_engine::Trade;
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
use quantick_orderflow::HeatmapConfig;
use quantick_orderflow::engine::{BookEngine, ProjectionRequest};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Budgets. Declared before the measurement code, and never read back from it.
// ---------------------------------------------------------------------------

/// Prints of history in the short session: one minute at the envelope's
/// sustained rate.
const SHORT_SESSION: usize = 60 * SUSTAINED_TRADES_PER_S as usize;
/// The fast variant's long session: ten minutes, 10x the short one — the
/// smallest ratio the rubric's "two lengths" reading accepts.
const FAST_LONG_SESSION: usize = 10 * SHORT_SESSION;
/// The long variant's long session: the envelope's edge, 220x the short one.
const LONG_SESSION: usize = RETAINED_TRADES_PER_PANE;

/// How much more work per unit the long session may cost than the short one:
/// a tenth, plus the path's absolute slack. Work that grows with history
/// grows by the session ratio — tenfold in the fast variant — so a tenth
/// separates the two with room for rounding and the odd timer-driven line.
const GROWTH_TOLERANCE: f64 = 0.10;
/// The long variant's bound on CPU time per unit, long over short: time
/// moves with caches and load where counts do not, so it gets half again.
const TIME_GROWTH_TOLERANCE: f64 = 0.50;

/// Prints per live frame: the sustained rate at 60 frames a second.
const PRINTS_PER_FRAME: usize = 5;
/// A measured window: `frames` live frames, and the depth updates that
/// arrive over the same seconds at the envelope's depth rate.
#[derive(Clone, Copy, Debug)]
struct Window {
    frames: usize,
}

impl Window {
    fn prints(self) -> usize {
        self.frames * PRINTS_PER_FRAME
    }

    fn depth_updates(self) -> usize {
        self.frames * DEPTH_UPDATES_PER_S as usize / 60
    }
}

/// The fast variant's window: five seconds.
const FAST_WINDOW: Window = Window { frames: 300 };
/// The long variant's window: thirty seconds, so its stopwatch reads more
/// than noise.
const LONG_WINDOW: Window = Window { frames: 1_800 };
/// Frames run before the window, so one-off setup work (the lane's cold
/// seed, the first layout) is not charged to it.
const WARMUP_FRAMES: usize = 30;
/// Lane rungs the ingest path asks for, as a lane on screen does.
const LANE_RUNGS: usize = 16;

/// A path's budget: ceilings per unit at *any* session length, and the slack
/// its growth check allows on top of [`GROWTH_TOLERANCE`].
#[derive(Clone, Copy, Debug)]
struct Budget {
    path: &'static str,
    /// Allocations per unit.
    allocs: f64,
    /// Bytes allocated per unit.
    bytes: f64,
    /// Bytes reallocation may copy per unit.
    copy_bytes: f64,
    /// Prints the forming-run walk folds per unit.
    folds: f64,
    /// Prints copied to the worker by the lane transport per unit.
    entries: f64,
    /// Absolute allocations per unit the growth check tolerates.
    slack_allocs: f64,
    /// Absolute bytes per unit the growth check tolerates (allocated and
    /// copied alike).
    slack_bytes: f64,
}

/// The ingest path per print. A print lands in the tape, the builder and the
/// footprint ladder: no allocation of its own beyond the amortised growth of
/// the retained vectors and a new ladder every 50th print; the lane command
/// every fifth print carries exactly the new prints.
const TRADE_CHART: Budget = Budget {
    path: "trade.chart",
    allocs: 2.0,
    bytes: 256.0,
    copy_bytes: 512.0,
    folds: 0.0,
    entries: 1.0,
    slack_allocs: 0.05,
    slack_bytes: 16.0,
};
/// The book's print path: one aggression into capped history.
const TRADE_BOOK: Budget = Budget {
    path: "trade.book",
    allocs: 4.0,
    bytes: 1_024.0,
    copy_bytes: 1_024.0,
    folds: 0.0,
    entries: 0.0,
    slack_allocs: 0.05,
    slack_bytes: 32.0,
};
/// The book's depth path: one delta applied and its runs recorded.
///
/// First declared at 8 allocations and 2 KiB per update — a guess below the
/// path's existing cost, which the calibration run at `00846de3` measured at
/// 19.1 allocations and 6.7 KiB per update, flat across both sessions.
/// Re-declared at twice the calibration; the growth tolerance, the claim
/// this check exists for, is unchanged from the first declaration.
const DEPTH_BOOK: Budget = Budget {
    path: "depth.book",
    allocs: 40.0,
    bytes: 16_384.0,
    copy_bytes: 16_384.0,
    folds: 0.0,
    entries: 0.0,
    slack_allocs: 0.05,
    slack_bytes: 32.0,
};
/// One heatmap projection of the visible window: sized by what is visible
/// (120 bars, the price window), not by what is retained.
///
/// First declared at 2,000 allocations and 4 MiB; the calibration run at
/// `00846de3` measured 2,068 and 4.4 MiB, flat across both sessions.
/// Re-declared at twice the calibration, as `DEPTH_BOOK` was.
const FRAME_BOOK: Budget = Budget {
    path: "frame.book",
    allocs: 4_000.0,
    bytes: 8.0 * 1_048_576.0,
    copy_bytes: 8.0 * 1_048_576.0,
    folds: 0.0,
    entries: 0.0,
    slack_allocs: 20.0,
    slack_bytes: 16_384.0,
};
/// One application frame on the UI thread: egui's layout and paint, the
/// visible bars, five prints in, worker deltas out. Sized by the window, not
/// by the tape.
const FRAME_APP: Budget = Budget {
    path: "frame.app",
    allocs: 20_000.0,
    bytes: 16.0 * 1_048_576.0,
    copy_bytes: 16.0 * 1_048_576.0,
    folds: 0.0,
    entries: PRINTS_PER_FRAME as f64,
    slack_allocs: 50.0,
    slack_bytes: 65_536.0,
};
/// One frame's batch on the indicator worker: the lane walk and the deltas.
/// The walk is bounded by `quantick_engine::forming_run`: at most `CHECKPOINT_SPACING - 1`
/// folds per rung over `MAX_LANE_RUNGS` rungs, plus one per print appended.
const FRAME_WORKER: Budget = Budget {
    path: "frame.worker",
    allocs: 2_000.0,
    bytes: 1_048_576.0,
    copy_bytes: 1_048_576.0,
    folds: (crate::indicator_worker::MAX_LANE_RUNGS
        * (quantick_engine::forming_run::CHECKPOINT_SPACING - 1)
        + PRINTS_PER_FRAME) as f64,
    entries: 0.0,
    slack_allocs: 20.0,
    slack_bytes: 8_192.0,
};

// ---------------------------------------------------------------------------
// Measurement.
// ---------------------------------------------------------------------------

/// Work per unit over one measured window.
#[derive(Clone, Copy, Debug, Default)]
struct PerUnit {
    units: u64,
    allocs: f64,
    bytes: f64,
    copy_bytes: f64,
    /// The largest single reallocation copy inside the window, in bytes: a
    /// stall, not a rate.
    largest_copy: u64,
    folds: f64,
    entries: f64,
    ns: f64,
}

impl PerUnit {
    fn over(units: usize, heap: Tally, folds: u64, entries: u64, ns: f64) -> Self {
        let n = units as f64;
        Self {
            units: units as u64,
            allocs: heap.allocs as f64 / n,
            bytes: heap.alloc_bytes as f64 / n,
            copy_bytes: heap.realloc_copy_bytes as f64 / n,
            largest_copy: heap.largest_realloc_copy,
            folds: folds as f64 / n,
            entries: entries as f64 / n,
            ns,
        }
    }
}

/// The median lap, per unit, in nanoseconds: robust to the interrupt or the
/// descheduled thread that lands in one lap of a short window, where a total
/// would carry it whole.
fn median_ns(mut laps: Vec<Duration>, units_per_lap: usize) -> f64 {
    if laps.is_empty() {
        return 0.0;
    }
    laps.sort_unstable();
    laps[laps.len() / 2].as_nanos() as f64 / units_per_lap as f64
}

/// One path's two measurements.
#[derive(Clone, Copy, Debug)]
struct Pair {
    budget: Budget,
    short: PerUnit,
    long: PerUnit,
    short_session: usize,
    long_session: usize,
}

/// Every way `pair` breaks its budget; empty when it holds. Counts only —
/// the time ratio is [`time_violations`].
fn violations(pair: &Pair) -> Vec<String> {
    let b = pair.budget;
    let mut found = Vec::new();
    let metrics = [
        (
            "allocs",
            b.allocs,
            b.slack_allocs,
            pair.short.allocs,
            pair.long.allocs,
        ),
        (
            "bytes",
            b.bytes,
            b.slack_bytes,
            pair.short.bytes,
            pair.long.bytes,
        ),
        (
            "copy_bytes",
            b.copy_bytes,
            b.slack_bytes,
            pair.short.copy_bytes,
            pair.long.copy_bytes,
        ),
        ("folds", b.folds, 0.0, pair.short.folds, pair.long.folds),
        (
            "entries",
            b.entries,
            0.0,
            pair.short.entries,
            pair.long.entries,
        ),
    ];
    for (name, ceiling, slack, short, long) in metrics {
        for (length, value) in [(pair.short_session, short), (pair.long_session, long)] {
            if value > ceiling {
                found.push(format!(
                    "{}: {name} {value:.3}/unit at {length} prints exceeds the ceiling {ceiling}",
                    b.path
                ));
            }
        }
        let allowed = short * (1.0 + GROWTH_TOLERANCE) + slack;
        if long > allowed {
            found.push(format!(
                "{}: {name} grew with the session: {short:.3}/unit at {} prints, {long:.3} at {} \
                 (allowed {allowed:.3})",
                b.path, pair.short_session, pair.long_session
            ));
        }
    }
    found
}

/// The long variant's time bound.
fn time_violations(pair: &Pair) -> Vec<String> {
    let allowed = pair.short.ns * (1.0 + TIME_GROWTH_TOLERANCE);
    if pair.long.ns > allowed {
        vec![format!(
            "{}: CPU time grew with the session: {:.0} ns/unit at {} prints, {:.0} at {} (allowed {allowed:.0})",
            pair.budget.path, pair.short.ns, pair.short_session, pair.long.ns, pair.long_session
        )]
    } else {
        Vec::new()
    }
}

fn table(pairs: &[Pair]) -> String {
    let mut out = String::from(
        "| path | session prints | units | allocs/unit | bytes/unit | copy bytes/unit | largest copy | folds/unit | lane entries/unit | median ns/unit |\n\
         | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for pair in pairs {
        for (session, m) in [
            (pair.short_session, pair.short),
            (pair.long_session, pair.long),
        ] {
            out.push_str(&format!(
                "| {} | {session} | {} | {:.3} | {:.1} | {:.1} | {} | {:.2} | {:.2} | {:.0} |\n",
                pair.budget.path,
                m.units,
                m.allocs,
                m.bytes,
                m.copy_bytes,
                m.largest_copy,
                m.folds,
                m.entries,
                m.ns
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The dense tape.
// ---------------------------------------------------------------------------

/// Midnight UTC, so a `time:1d` bar holds every print of either session.
const EPOCH_MS: i64 = 1_720_051_200_000;
const SYMBOL: &str = "TESTUSDT";
const GENERATION: u64 = 10;

/// Print `i` of a tape at the sustained rate: a price that walks a bounded
/// band both ways, integer quantities, a buy/sell mix that never nets out.
fn print(i: u64) -> Trade {
    Trade {
        agg_id: i + 1,
        timestamp_ms: EPOCH_MS + (i * 1_000 / SUSTAINED_TRADES_PER_S) as i64,
        price: Decimal::from(176_000 + ((i * 7_919) % 41) as i64 * 5 - 100),
        quantity: Decimal::from(1 + (i % 5) as i64),
        side: if i % 7 < 4 {
            quantick_engine::Side::Buy
        } else {
            quantick_engine::Side::Sell
        },
    }
}

fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(Decimal::from(price), Decimal::from(quantity)).expect("a valid level")
}

/// Depth update `id`: one bid and one ask level move, twenty levels a side.
fn depth(id: u64, at_ms: i64) -> DepthEvent {
    let rung = (id % 20) as i64 * 5;
    DepthEvent::Update {
        symbol: SYMBOL.into(),
        generation: GENERATION,
        event_time_ms: at_ms,
        delta: BookDelta::new(
            id,
            id,
            vec![level(175_795 - rung, 1 + (id % 9) as i64)],
            vec![level(176_205 + rung, 1 + (id % 7) as i64)],
        ),
    }
}

// ---------------------------------------------------------------------------
// The paths.
// ---------------------------------------------------------------------------

/// Rounds a window is split into. The short and the long session take them in
/// turn — short, long, long, short, … — so load on a shared host lands on both
/// sessions alike instead of on whichever ran second.
const ROUNDS: usize = 6;

/// Run `units` of work on the short (`rigs[0]`) and the long (`rigs[1]`)
/// session in alternating rounds.
fn alternate<R>(rigs: &mut [R; 2], units: usize, mut run: impl FnMut(&mut R, usize)) {
    let per_round = units / ROUNDS;
    for round in 0..ROUNDS {
        let order = if round % 2 == 0 { [0, 1] } else { [1, 0] };
        for index in order {
            run(&mut rigs[index], per_round);
        }
    }
}

/// Work gathered over one path's measured laps.
#[derive(Default)]
struct Acc {
    heap: Tally,
    laps: Vec<Duration>,
    entries: u64,
}

impl Acc {
    /// Run `work` as one lap: its heap work (largest copy within the lap
    /// included) and its time are added to the path's.
    fn lap<T>(&mut self, work: impl FnOnce() -> T) -> T {
        work_meter::reset_largest();
        let before = work_meter::tally();
        let started = Instant::now();
        let out = work();
        self.laps.push(started.elapsed());
        self.heap = add(self.heap, work_meter::tally().since(before));
        out
    }

    fn per_unit(self, units_per_lap: usize, folds: u64) -> PerUnit {
        let units = self.laps.len() * units_per_lap;
        PerUnit::over(
            units,
            self.heap,
            folds,
            self.entries,
            median_ns(self.laps, units_per_lap),
        )
    }
}

fn add(sum: Tally, more: Tally) -> Tally {
    Tally {
        allocs: sum.allocs + more.allocs,
        alloc_bytes: sum.alloc_bytes + more.alloc_bytes,
        reallocs: sum.reallocs + more.reallocs,
        realloc_copy_bytes: sum.realloc_copy_bytes + more.realloc_copy_bytes,
        largest_realloc_copy: sum.largest_realloc_copy.max(more.largest_realloc_copy),
    }
}

/// How a pane's tape reached its session length.
#[derive(Clone, Copy, Debug)]
enum Load {
    /// Loaded at once: a recovered session or a replay's history.
    Backfill,
    /// Built print by print over the session.
    Live,
}

/// What building the session's tape cost, print by print: the D3 stall.
#[derive(Clone, Copy, Debug, Default)]
struct Growth {
    /// Bytes reallocation may copy, over the whole session, per print.
    copy_per_print: f64,
    /// The largest single reallocation copy while it was built.
    largest_copy: u64,
}

/// One pane's ingest path: the chart state and the lane cursor, as the pane
/// feeds them per frame.
struct ChartRig {
    state: ChartState,
    lane: LaneTransport,
    next: u64,
    growth: Growth,
    acc: Acc,
}

impl ChartRig {
    /// A tick:50 chart with the footprint on, holding a tape of `session`
    /// prints loaded as `load` says.
    fn new(session: usize, load: Load) -> Self {
        let mut rig = Self {
            state: ChartState::new(BarSpec::Tick(50)),
            lane: LaneTransport::default(),
            next: 0,
            growth: Growth::default(),
            acc: Acc::default(),
        };
        rig.state.set_footprint_enabled(true);
        rig.lane.set_rungs(LANE_RUNGS);
        match load {
            Load::Backfill => {
                let tape: Vec<Trade> = (0..session as u64).map(print).collect();
                rig.state.ingest_backfill(&tape);
                rig.next = session as u64;
            }
            Load::Live => {
                work_meter::reset_largest();
                let before = work_meter::tally();
                while (rig.next as usize) < session {
                    let prints = PRINTS_PER_FRAME.min(session - rig.next as usize);
                    rig.send(prints);
                }
                let built = work_meter::tally().since(before);
                rig.growth = Growth {
                    copy_per_print: built.realloc_copy_bytes as f64 / session as f64,
                    largest_copy: built.largest_realloc_copy,
                };
            }
        }
        rig
    }

    /// One frame's prints, then the frame's lane command; the prints copied.
    fn send(&mut self, prints: usize) -> u64 {
        for _ in 0..prints {
            let before = self.state.bars().len();
            self.state.ingest_live(&print(self.next));
            self.next += 1;
            if self.state.bars().len() > before {
                self.lane.reset();
            }
        }
        match self
            .lane
            .command(self.state.partial().cloned(), self.state.trades())
        {
            IndicatorCommand::PartialUpdated { run, .. } => run.len() as u64,
            _ => unreachable!("the lane transport only builds forming-bar updates"),
        }
    }

    /// `frames` measured frames; `inject` runs after each, outside the lap's
    /// clock but inside its heap count — where the checker plants a cost.
    fn frames(&mut self, frames: usize, inject: &dyn Fn(&ChartState)) {
        for _ in 0..frames {
            let mut acc = std::mem::take(&mut self.acc);
            let entries = acc.lap(|| {
                let copied = self.send(PRINTS_PER_FRAME);
                inject(&self.state);
                copied
            });
            acc.entries += entries;
            self.acc = acc;
        }
    }
}

/// `trade.chart.*` at both sessions, loaded as `load` says: a window of
/// frames of prints with one lane command each.
fn trade_chart(
    sessions: [usize; 2],
    load: Load,
    window: Window,
    inject: &dyn Fn(&ChartState),
) -> ([PerUnit; 2], [Growth; 2]) {
    let mut rigs = sessions.map(|session| ChartRig::new(session, load));
    alternate(&mut rigs, window.frames, |rig, frames| {
        rig.frames(frames, inject);
    });
    let growth = [rigs[0].growth, rigs[1].growth];
    (
        rigs.map(|rig| rig.acc.per_unit(PRINTS_PER_FRAME, 0)),
        growth,
    )
}

/// The book after a session of `session` prints and the depth that comes
/// with them at the envelope's ratio, plus the chart that bars them.
struct Book {
    engine: BookEngine,
    chart: ChartState,
    next_print: u64,
    next_depth: u64,
    clock_ms: i64,
    /// Frames projected so far: the test's clock for the projection cache.
    frames: u64,
    base: Instant,
    trade: Acc,
    depth: Acc,
    frame: Acc,
}

impl Book {
    fn after(session: usize) -> Self {
        let mut engine = BookEngine::new(SYMBOL.to_owned());
        engine.apply_visual_config(HeatmapConfig {
            enabled: true,
            show_aggressions: true,
            ..HeatmapConfig::default()
        });
        engine.set_enabled(true, GENERATION);
        let bids = (0..20).map(|n| level(175_795 - n * 5, 5)).collect();
        let asks = (0..20).map(|n| level(176_205 + n * 5, 6)).collect();
        engine.handle_depth_event_at(
            DepthEvent::Snapshot {
                symbol: SYMBOL.into(),
                generation: GENERATION,
                observed_at_ms: EPOCH_MS,
                effective_at_ms: EPOCH_MS,
                price_step: Some(Decimal::from(5)),
                snapshot: BookSnapshot::new(
                    GENERATION,
                    bids,
                    asks,
                    BookCoverage::Limited {
                        levels_per_side: 20,
                    },
                ),
            },
            EPOCH_MS,
        );
        let mut book = Self {
            engine,
            chart: ChartState::new(BarSpec::Tick(50)),
            next_print: 0,
            next_depth: GENERATION + 1,
            clock_ms: EPOCH_MS,
            frames: 0,
            base: Instant::now(),
            trade: Acc::default(),
            depth: Acc::default(),
            frame: Acc::default(),
        };
        for _ in 0..session {
            book.print();
        }
        book
    }

    /// One print, and the depth updates due before the next one.
    fn print(&mut self) {
        let trade = print(self.next_print);
        self.next_print += 1;
        self.clock_ms = trade.timestamp_ms;
        self.engine.record_trade(&trade);
        self.chart.ingest_live(&trade);
        let due = (self.next_print * DEPTH_UPDATES_PER_S / SUSTAINED_TRADES_PER_S)
            .saturating_sub(self.next_depth - GENERATION - 1);
        for _ in 0..due {
            self.depth();
        }
    }

    fn depth(&mut self) {
        let id = self.next_depth;
        self.next_depth += 1;
        self.engine
            .handle_depth_event_at(depth(id, self.clock_ms), self.clock_ms);
    }

    /// The layout a pane showing the newest 120 bars asks for.
    fn request(&self) -> ProjectionRequest {
        let bars = self.chart.bars();
        let first = bars.len().saturating_sub(120);
        ProjectionRequest {
            timeline_revision: self.chart.timeline_revision(),
            first_bar_index: first,
            closed: bars[first..].to_vec(),
            partial: self.chart.partial().cloned(),
            lane: false,
            on_newest_bar: true,
            lane_reference_ms: None,
            price_range: (175_700.0, 176_300.0),
        }
    }

    /// `prints` measured prints on the book's own print path; the chart
    /// that bars them is fed outside the lap.
    fn trades(&mut self, prints: usize) {
        for _ in 0..prints {
            let trade = print(self.next_print);
            self.next_print += 1;
            self.clock_ms = trade.timestamp_ms;
            let engine = &mut self.engine;
            self.trade.lap(|| engine.record_trade(&trade));
            self.chart.ingest_live(&trade);
        }
    }

    /// `updates` measured depth updates.
    fn depths(&mut self, updates: usize) {
        for _ in 0..updates {
            let mut acc = std::mem::take(&mut self.depth);
            acc.lap(|| self.depth());
            self.depth = acc;
        }
    }

    /// `frames` measured frames: five prints each (outside the lap), then
    /// the projection the worker builds for the pane, on the test's clock.
    fn project(&mut self, frames: usize) {
        for _ in 0..frames {
            for _ in 0..PRINTS_PER_FRAME {
                self.print();
            }
            let now = self.base + Duration::from_micros(16_667 * self.frames);
            self.frames += 1;
            let mut acc = std::mem::take(&mut self.frame);
            acc.lap(|| {
                let request = self.request();
                std::hint::black_box(self.engine.project_at(&request, now));
            });
            self.frame = acc;
        }
    }
}

/// `trade.book`, `depth.book` and `frame.book` at both sessions.
fn book_paths(sessions: [usize; 2], window: Window) -> [[PerUnit; 2]; 3] {
    let mut books = sessions.map(Book::after);
    alternate(&mut books, window.prints(), Book::trades);
    alternate(&mut books, window.depth_updates(), Book::depths);
    alternate(&mut books, window.frames, Book::project);
    let [short, long] = books;
    [
        [short.trade.per_unit(1, 0), long.trade.per_unit(1, 0)],
        [short.depth.per_unit(1, 0), long.depth.per_unit(1, 0)],
        [short.frame.per_unit(1, 0), long.frame.per_unit(1, 0)],
    ]
}

/// The whole application after a session of `session` prints on `spec`,
/// with the lane and CVD on, warmed up and ready to measure.
struct AppRig {
    ctx: egui::Context,
    app: QuantickApp,
    events: mpsc::Sender<FeedEvent>,
    next: u64,
    worker_before: crate::indicator_worker::LaneProbe,
    traffic_before: usize,
    acc: Acc,
}

impl AppRig {
    fn new(spec: &BarSpec, session: usize) -> Self {
        let (mut app, events, _commands, _book) = test_app();
        app.active_tab_mut().flow_pane.spec.set(spec.clone());
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();
        assert_eq!(app.active_tab().flow_pane.state.spec(), spec);
        assert!(
            app.active_tab_mut()
                .tape_mut()
                .apply_preset("dense tape btc"),
            "the dense preset puts a lane on screen"
        );
        app.active_tab_mut()
            .flow_pane
            .add_indicator(IndicatorSource::Native {
                id: "native.cvd".to_owned(),
                values: Vec::new(),
            });
        events
            .try_send(FeedEvent::Backfilled(
                (0..session as u64).map(print).collect(),
            ))
            .unwrap();
        app.active_tab_mut().drain_feed();
        // Read again after the warm-up below; these only fill the fields.
        let worker = &app.active_tab().flow_pane.indicator_worker;
        let worker_before = worker.lane_probe_for_test();
        let traffic_before = worker.lane_traffic_for_test();
        let mut rig = Self {
            ctx: egui::Context::default(),
            app,
            events,
            next: session as u64,
            worker_before,
            traffic_before,
            acc: Acc::default(),
        };
        for _ in 0..WARMUP_FRAMES {
            rig.frame();
            rig.app.active_tab_mut().flow_pane.indicator_worker.flush();
        }
        assert!(
            rig.app
                .active_tab()
                .flow_pane
                .frame
                .lane_divider_x
                .is_some(),
            "the fixture draws a lane"
        );
        let worker = &rig.app.active_tab().flow_pane.indicator_worker;
        rig.worker_before = worker.lane_probe_for_test();
        rig.traffic_before = worker.lane_traffic_for_test();
        rig
    }

    /// One frame of five prints. Each frame's batch is taken by the worker
    /// before the next frame runs, so every frame drains exactly the previous
    /// one's deltas whatever the host's load.
    fn frame(&mut self) {
        self.events
            .try_send(FeedEvent::LiveBatch(
                (self.next..self.next + PRINTS_PER_FRAME as u64)
                    .map(print)
                    .collect(),
            ))
            .unwrap();
        self.next += PRINTS_PER_FRAME as u64;
        run_frame(&mut self.app, &self.ctx);
    }

    /// `frames` measured frames: the application's frame is the lap; the
    /// worker's settle is not.
    fn frames(&mut self, frames: usize) {
        for _ in 0..frames {
            let mut acc = std::mem::take(&mut self.acc);
            acc.lap(|| self.frame());
            self.acc = acc;
            self.app.active_tab_mut().flow_pane.indicator_worker.flush();
        }
    }

    /// The UI thread's work and the indicator worker's, per frame.
    fn finish(self) -> [PerUnit; 2] {
        let worker = &self.app.active_tab().flow_pane.indicator_worker;
        let after = worker.lane_probe_for_test();
        let traffic = (worker.lane_traffic_for_test() - self.traffic_before) as u64;
        // Counted on the worker thread across runs, so tick:50's cut at every
        // close does not restart it.
        let folds = after.folds.saturating_sub(self.worker_before.folds);
        let frames = self.acc.laps.len();
        let heap = after.heap.since(self.worker_before.heap);
        let mut ui = self.acc;
        ui.entries = traffic;
        [
            ui.per_unit(1, 0),
            PerUnit::over(frames, heap, folds, 0, 0.0),
        ]
    }
}

/// `frame.app.*` and `frame.worker.*` at both sessions on `spec`.
fn app_frames(spec: &BarSpec, sessions: [usize; 2], window: Window) -> [[PerUnit; 2]; 2] {
    let mut rigs = sessions.map(|session| AppRig::new(spec, session));
    alternate(&mut rigs, window.frames, AppRig::frames);
    let [short, long] = rigs.map(AppRig::finish);
    [[short[0], long[0]], [short[1], long[1]]]
}

// ---------------------------------------------------------------------------
// The variants.
// ---------------------------------------------------------------------------

/// Every path at the short session and at `long_session`, plus the tape's
/// growth while each live session was built.
fn measure(long_session: usize, window: Window) -> (Vec<Pair>, [Growth; 2]) {
    let sessions = [SHORT_SESSION, long_session];
    let pair = |budget: Budget, [short, long]: [PerUnit; 2]| Pair {
        budget,
        short,
        long,
        short_session: SHORT_SESSION,
        long_session,
    };
    let named = |budget: Budget, path: &'static str| Budget { path, ..budget };
    let none = |_: &ChartState| {};
    let mut pairs = Vec::new();
    let (backfilled, _) = trade_chart(sessions, Load::Backfill, window, &none);
    pairs.push(pair(
        named(TRADE_CHART, "trade.chart.backfilled"),
        backfilled,
    ));
    let (live, growth) = trade_chart(sessions, Load::Live, window, &none);
    pairs.push(pair(named(TRADE_CHART, "trade.chart.live"), live));

    let [trade, depth, frame] = book_paths(sessions, window);
    pairs.push(pair(TRADE_BOOK, trade));
    pairs.push(pair(DEPTH_BOOK, depth));
    pairs.push(pair(FRAME_BOOK, frame));

    for (spec, app_path, worker_path) in [
        (BarSpec::Tick(50), "frame.app.tick50", "frame.worker.tick50"),
        (
            BarSpec::Time(86_400_000),
            "frame.app.time1d",
            "frame.worker.time1d",
        ),
    ] {
        let [ui, worker] = app_frames(&spec, sessions, window);
        pairs.push(pair(named(FRAME_APP, app_path), ui));
        pairs.push(pair(named(FRAME_WORKER, worker_path), worker));
    }
    (pairs, growth)
}

fn growth_table(long_session: usize, growth: [Growth; 2]) -> String {
    let mut out = String::from(
        "| tape built live to | copy bytes/print over the session | largest single copy |\n\
         | ---: | ---: | ---: |\n",
    );
    for (session, g) in [(SHORT_SESSION, growth[0]), (long_session, growth[1])] {
        out.push_str(&format!(
            "| {session} | {:.1} | {} |\n",
            g.copy_per_print, g.largest_copy
        ));
    }
    out
}

/// The fast variant: 1 against 10 minutes of the sustained rate, counts only.
#[test]
fn hot_path_work_is_independent_of_session_length() {
    let (pairs, growth) = measure(FAST_LONG_SESSION, FAST_WINDOW);
    println!("{}", table(&pairs));
    println!("{}", growth_table(FAST_LONG_SESSION, growth));
    let broken: Vec<String> = pairs.iter().flat_map(violations).collect();
    assert!(broken.is_empty(), "{}", broken.join("\n"));
}

/// The long variant: 1 minute against the envelope's edge, counts and time.
#[test]
#[ignore = "long variant: minutes and gigabytes; run with --ignored, see the module docs"]
fn long() {
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|_| "unknown".to_owned());
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .map(|out| !out.stdout.is_empty())
        .unwrap_or(true);
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_owned());
    println!("# session-length long variant");
    println!(
        "sha: {sha}{}",
        if dirty {
            " (tracked files modified)"
        } else {
            ""
        }
    );
    println!("host: {host}");
    println!(
        "command: env -u QUANTICK_BUBBLES cargo test --release -p quantick-app \
         session_length_tests::long -- --ignored --nocapture --test-threads=1"
    );
    println!(
        "sessions: {SHORT_SESSION} and {LONG_SESSION} prints at {SUSTAINED_TRADES_PER_S}/s; \
         window {} frames x {PRINTS_PER_FRAME} prints, {} depth updates; \
         tolerance counts +{:.0}%, time +{:.0}%",
        LONG_WINDOW.frames,
        LONG_WINDOW.depth_updates(),
        GROWTH_TOLERANCE * 100.0,
        TIME_GROWTH_TOLERANCE * 100.0
    );
    let started = Instant::now();
    let (pairs, growth) = measure(LONG_SESSION, LONG_WINDOW);
    println!();
    println!("{}", table(&pairs));
    println!("{}", growth_table(LONG_SESSION, growth));
    let broken: Vec<String> = pairs
        .iter()
        .flat_map(|pair| {
            let mut all = violations(pair);
            // Worker-thread paths carry no stopwatch of their own.
            if pair.short.ns > 0.0 {
                all.extend(time_violations(pair));
            }
            all
        })
        .collect();
    println!(
        "verdict: {} ({:.0} s)",
        if broken.is_empty() {
            "within budget".to_owned()
        } else {
            format!("OVER BUDGET\n{}", broken.join("\n"))
        },
        started.elapsed().as_secs_f64()
    );
    assert!(broken.is_empty(), "{}", broken.join("\n"));
}

/// The checker catches what it exists to catch: a per-frame clone of the
/// tape — the shape of an O(history) regression — breaks the ingest path's
/// budget at the fast variant's lengths.
#[test]
fn the_check_fails_a_path_whose_work_grows_with_the_session() {
    let clone_the_tape = |state: &ChartState| {
        std::hint::black_box(state.trades().to_vec());
    };
    let ([short, long], _) = trade_chart(
        [SHORT_SESSION, FAST_LONG_SESSION],
        Load::Backfill,
        FAST_WINDOW,
        &clone_the_tape,
    );
    let planted = Pair {
        budget: TRADE_CHART,
        short,
        long,
        short_session: SHORT_SESSION,
        long_session: FAST_LONG_SESSION,
    };
    let found = violations(&planted);
    assert!(
        found
            .iter()
            .any(|line| line.contains("bytes grew with the session")),
        "a planted O(history) cost must break the growth budget: {found:?}"
    );

    // Pure arithmetic: tenfold growth fails, flat work passes.
    let flat = PerUnit {
        units: 1,
        allocs: 1.0,
        bytes: 100.0,
        ..PerUnit::default()
    };
    let grown = PerUnit {
        allocs: 10.0,
        bytes: 1_000.0,
        ..flat
    };
    let pair = |long| Pair {
        budget: Budget {
            allocs: 100.0,
            bytes: 10_000.0,
            ..TRADE_CHART
        },
        short: flat,
        long,
        short_session: SHORT_SESSION,
        long_session: FAST_LONG_SESSION,
    };
    assert!(violations(&pair(flat)).is_empty());
    assert_eq!(violations(&pair(grown)).len(), 2);
}
