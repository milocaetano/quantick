//! The order-flow engine's hot paths, counted and held to budgets.
//!
//! | Path | Unit | What runs |
//! | --- | --- | --- |
//! | `orderflow.trade.book` | print | `BookEngine::record_trade` |
//! | `orderflow.depth.book` | depth update | `BookEngine::handle_depth_event_at`, interleaved with the prints that advance the clock and close bars |
//! | `orderflow.frame.book` | frame | `BookEngine::project_at` over the newest 120 bars, heatmap and bubbles on; every counted frame a rebuild, one projection interval after the last |
//! | `orderflow.trade.flow.regions` / `.spheres` | print | one live FLOW frame of five prints: `FlowSession::project`, its capture and an inline worker's append and publication, divided by the prints; regional FLOW and typed-reference tick spheres |
//!
//! The prints are the recorded WIN slice under `tests/fixtures`, replayed in
//! laps; the depth is a twenty-level book a side whose updates move one bid
//! and one ask level, at the live envelope's 1,000 updates to 300 prints. The
//! work is counted with [`work_meter`] on the thread doing it, after a
//! session of history and a warm-up, so a cold buffer is not charged to the
//! window. Counts do not move with the host's load, so the budgets hold in the
//! ordinary `cargo test`; a path that starts cloning history, or rebuilding a
//! buffer per print or per update, crosses them. A slowdown that allocates
//! nothing is invisible here.
//!
//! `app`'s `session_length_tests` hold the same book paths through the app's
//! test binary, against session length; these hold them where they live,
//! without building the app.
//!
//! The budgets are declared first, each with the calibration it was set
//! from, and never read back from a measurement.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quantick_engine::work_meter::{self, Counting, Tally};
use quantick_engine::{Bar, BarBuilder, Side, TickBarBuilder, Trade};
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot, DepthEvent};
use quantick_orderflow::HeatmapConfig;
use quantick_orderflow::engine::{BookEngine, ProjectionRequest};
use quantick_orderflow::projection::PriceWindow;
use quantick_orderflow::projection::flow_tape::{
    FlowChunk, FlowReference, FlowRequest, FlowRunner, FlowSession, FlowTapeFrame, FlowTapeView,
    FlowWorkerCache, OwnedFlowExecution,
};
use rust_decimal::Decimal;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

// ---------------------------------------------------------------------------
// Budgets: heap work per unit. Declared at no more than twice the
// calibration, rounded down to a readable number.
// ---------------------------------------------------------------------------

const KIB: f64 = 1_024.0;
const MIB: f64 = 1_048_576.0;

/// Per-unit ceilings of one path.
#[derive(Clone, Copy, Debug)]
struct Budget {
    path: &'static str,
    unit: &'static str,
    /// Allocations per unit.
    allocs: f64,
    /// Bytes allocated per unit.
    bytes: f64,
    /// Bytes reallocation may copy per unit.
    copy_bytes: f64,
}

/// One aggression into capped history. Calibrated at `3f9380bd`: 0.033
/// allocations, 9.1 bytes and 218.5 copy bytes per print (98 allocations and
/// two buffer doublings over the 3,000 prints).
const TRADE_BOOK: Budget = Budget {
    path: "orderflow.trade.book",
    unit: "print",
    allocs: 0.06,
    bytes: 16.0,
    copy_bytes: 400.0,
};
/// One delta applied and its runs recorded, interleaved with the prints that
/// move the clock. Calibrated at `53266e52`: 16.11 allocations, 6,648.6 bytes
/// and 0 copy bytes per update over 6,000 updates. The copy bytes measured
/// zero, so they get a small absolute allowance of 64 bytes per update
/// (384 KiB over the window): one buffer starting to grow per update crosses
/// it.
const DEPTH_BOOK: Budget = Budget {
    path: "orderflow.depth.book",
    unit: "depth update",
    allocs: 32.0,
    bytes: 12_288.0,
    copy_bytes: 64.0,
};
/// One heatmap projection of the visible window, rebuilt: the counted frames
/// are a projection interval apart, so the cache serves none. Calibrated at
/// `53266e52`: 3,642.7 allocations, 5,728,389 bytes (5.46 MiB) and 2,327,929
/// copy bytes (2.22 MiB) per rebuild. Bytes are declared at 1.46x, not 2x: a
/// copy of the liquidity history per projection adds 4.45 MB a rebuild here
/// (10,179,414 bytes, 1.78x), and the budget must catch it.
const FRAME_BOOK: Budget = Budget {
    path: "orderflow.frame.book",
    unit: "frame",
    allocs: 7_000.0,
    bytes: 8.0 * MIB,
    copy_bytes: 4.25 * MIB,
};
/// A live regional FLOW frame, per print. Calibrated at `3f9380bd`: 1,161.1
/// allocations, 813,776 bytes and 991,557 copy bytes per print — about 4 MB
/// allocated per five-print frame over a 6,000-print window: an untyped
/// reference is projected whole every frame, the worker's memo is for typed
/// ones. Recorded as today's cost, not judged here.
const TRADE_FLOW_REGIONS: Budget = Budget {
    path: "orderflow.trade.flow.regions",
    unit: "print",
    allocs: 2_000.0,
    bytes: 1.5 * MIB,
    copy_bytes: 1.75 * MIB,
};
/// A live tick-sphere FLOW frame, per print. Calibrated at `d91a4c29`: 265.7
/// allocations, 102,727 bytes and 159,018 copy bytes per print.
const TRADE_FLOW_SPHERES: Budget = Budget {
    path: "orderflow.trade.flow.spheres",
    unit: "print",
    allocs: 500.0,
    bytes: 192.0 * KIB,
    copy_bytes: 300.0 * KIB,
};

// ---------------------------------------------------------------------------
// Sizes.
// ---------------------------------------------------------------------------

/// Prints of history before anything is counted.
const SESSION_PRINTS: usize = 6_000;
/// Prints counted on the book's print path.
const MEASURED_PRINTS: usize = 3_000;
/// Depth updates counted on the book's depth path, as the prints between
/// them bring them due: about 1,800 prints.
const MEASURED_DEPTH_UPDATES: usize = 6_000;
/// Projections run before the counted ones, and the projections counted.
const WARMUP_FRAMES: usize = 10;
const MEASURED_FRAMES: usize = 30;
/// The clock between two projections: past orderflow's 220 ms
/// `PROJECTION_INTERVAL` (crate-private), so the cache never serves a counted
/// frame and each one is a rebuild.
const PROJECTION_STEP: Duration = Duration::from_millis(250);
/// Prints between two projections: the envelope's 300 a second over
/// [`PROJECTION_STEP`].
const PRINTS_PER_PROJECTION: usize = 75;
/// Prints per live frame: the envelope's 300 prints a second at 60 frames.
const PRINTS_PER_FRAME: usize = 5;
/// Depth updates per print, as the envelope's 1,000 to 300.
const DEPTH_UPDATES_PER_S: usize = 1_000;
const TRADES_PER_S: usize = 300;
/// The chart both projections draw against: tick:50, 120 bars on screen.
const TICKS_PER_BAR: usize = 50;
const VISIBLE_BARS: usize = 120;
/// FLOW: history, warm-up frames (the cold fill of the visible window) and
/// counted frames.
const FLOW_HISTORY_PRINTS: usize = 12_000;
const FLOW_WARMUP_FRAMES: usize = 20;
const FLOW_MEASURED_FRAMES: usize = 60;
/// The chart's regional FLOW request sizes (`quantick_chart`'s
/// `flow_execution` constants), in logical pixels.
const FLOW_RADIUS_LIMIT_PX: f32 = 12.0;
const FLOW_MERGE_SUPPORT_RADIUS_PX: f32 = 6.0;
const PX_PER_BAR: f32 = 8.0;
const PANE_HEIGHT_PX: f32 = 1_180.0;

const SYMBOL: &str = "WINV26";
const GENERATION: u64 = 10;
/// The recorded slice's price band, padded by two steps, as the axis.
const PRICE_RANGE: (f64, f64) = (187_435.0, 187_545.0);

// ---------------------------------------------------------------------------
// The tape and the book.
// ---------------------------------------------------------------------------

/// The recorded WIN prints, parsed as `projection`'s recorded-stability
/// tests parse them.
fn recording() -> Vec<Trade> {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/win_2026_09_22_historical_tape.csv"
    ))
    .lines()
    .filter(|line| line.starts_with("2026-09-22,"))
    .map(|line| {
        let fields: Vec<_> = line.split(',').collect();
        let time: Vec<i64> = fields[1]
            .split([':', '.'])
            .map(|part| part.parse().expect("recorded civil-time component"))
            .collect();
        Trade {
            agg_id: 0,
            timestamp_ms: 1_790_046_000_000
                + ((time[0] * 60 + time[1]) * 60 + time[2]) * 1_000
                + time[3],
            price: fields[2].parse().expect("recorded price"),
            quantity: fields[5].parse().expect("recorded volume"),
            side: match fields[6] {
                "B" => Side::Buy,
                "S" => Side::Sell,
                _ => panic!("recorded venue side"),
            },
        }
    })
    .collect()
}

/// The recording replayed in laps, each lap starting 100 ms after the last
/// one ended, so prints stay in time order and ids keep rising.
struct Tape {
    recording: Vec<Trade>,
    lap_ms: i64,
    next: usize,
}

impl Tape {
    fn new() -> Self {
        let recording = recording();
        let lap_ms = recording.last().expect("prints").timestamp_ms
            - recording.first().expect("prints").timestamp_ms
            + 100;
        Self {
            recording,
            lap_ms,
            next: 0,
        }
    }

    fn next(&mut self) -> Trade {
        let laps = self.next / self.recording.len();
        let mut trade = self.recording[self.next % self.recording.len()].clone();
        trade.agg_id = self.next as u64 + 1;
        trade.timestamp_ms += laps as i64 * self.lap_ms;
        self.next += 1;
        trade
    }
}

fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(Decimal::from(price), Decimal::from(quantity)).expect("a valid level")
}

/// Depth update `id`: one bid and one ask level move, twenty levels a side,
/// the book just outside the recorded print band.
fn depth(id: u64, at_ms: i64) -> DepthEvent {
    let rung = (id % 20) as i64 * 5;
    DepthEvent::Update {
        symbol: SYMBOL.into(),
        generation: GENERATION,
        event_time_ms: at_ms,
        delta: BookDelta::new(
            id,
            id,
            vec![level(187_440 - rung, 1 + (id % 9) as i64)],
            vec![level(187_540 + rung, 1 + (id % 7) as i64)],
        ),
    }
}

fn add(a: Tally, b: Tally) -> Tally {
    Tally {
        allocs: a.allocs + b.allocs,
        alloc_bytes: a.alloc_bytes + b.alloc_bytes,
        reallocs: a.reallocs + b.reallocs,
        realloc_copy_bytes: a.realloc_copy_bytes + b.realloc_copy_bytes,
        largest_realloc_copy: a.largest_realloc_copy.max(b.largest_realloc_copy),
    }
}

/// The heap work `run` does on this thread.
fn counted<T>(run: impl FnOnce() -> T) -> (T, Tally) {
    let before = work_meter::tally();
    let out = run();
    (out, work_meter::tally().since(before))
}

/// Hold `work`, done over `units` units, to `budget`.
fn holds_to_budget(budget: Budget, work: Tally, units: usize) {
    let per = |total: u64| total as f64 / units as f64;
    let measured = (
        per(work.allocs),
        per(work.alloc_bytes),
        per(work.realloc_copy_bytes),
    );
    println!(
        "{}: {:.3} allocs, {:.1} bytes, {:.1} copy bytes per {} over {units}",
        budget.path, measured.0, measured.1, measured.2, budget.unit
    );
    assert!(
        measured.0 <= budget.allocs
            && measured.1 <= budget.bytes
            && measured.2 <= budget.copy_bytes,
        "{} over budget per {}: {:.3} allocs (budget {}), {:.1} bytes (budget {}), \
         {:.1} copy bytes (budget {}); totals over {units}: {work:?}",
        budget.path,
        budget.unit,
        measured.0,
        budget.allocs,
        measured.1,
        budget.bytes,
        measured.2,
        budget.copy_bytes,
    );
}

/// The book's heap work for one print and the depth updates that follow it.
struct PrintWork {
    /// `record_trade`.
    record: Tally,
    /// Every `handle_depth_event_at` the print brought due.
    depth: Tally,
    depth_updates: usize,
}

/// The book after a session of prints and the depth that comes with them,
/// plus the tick:50 bars a chart would cut from the same prints.
struct Book {
    engine: BookEngine,
    tape: Tape,
    bars: TickBarBuilder,
    closed: Vec<Bar>,
    prints: usize,
    next_depth: u64,
    clock_ms: i64,
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
        let tape = Tape::new();
        let start_ms = tape.recording[0].timestamp_ms;
        let bids = (0..20).map(|n| level(187_440 - n * 5, 5)).collect();
        let asks = (0..20).map(|n| level(187_540 + n * 5, 6)).collect();
        engine.handle_depth_event_at(
            DepthEvent::Snapshot {
                symbol: SYMBOL.into(),
                generation: GENERATION,
                observed_at_ms: start_ms,
                effective_at_ms: start_ms,
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
            start_ms,
        );
        let mut book = Self {
            engine,
            tape,
            bars: TickBarBuilder::new(TICKS_PER_BAR as u64),
            closed: Vec::new(),
            prints: 0,
            next_depth: GENERATION + 1,
            clock_ms: start_ms,
        };
        for _ in 0..session {
            book.print();
        }
        book
    }

    /// One print and the depth updates due before the next, at the print's
    /// time; returns the work of each path apart.
    fn print(&mut self) -> PrintWork {
        let trade = self.tape.next();
        self.prints += 1;
        self.clock_ms = trade.timestamp_ms;
        let engine = &mut self.engine;
        let ((), record) = counted(|| engine.record_trade(&trade));
        self.bars.push_into(&trade, &mut self.closed);
        let due = (self.prints * DEPTH_UPDATES_PER_S / TRADES_PER_S)
            .saturating_sub((self.next_depth - GENERATION - 1) as usize);
        let mut depth = Tally::default();
        for _ in 0..due {
            depth = add(depth, self.depth());
        }
        PrintWork {
            record,
            depth,
            depth_updates: due,
        }
    }

    /// One depth update; returns its work.
    fn depth(&mut self) -> Tally {
        let event = depth(self.next_depth, self.clock_ms);
        self.next_depth += 1;
        let engine = &mut self.engine;
        let clock_ms = self.clock_ms;
        counted(|| engine.handle_depth_event_at(event, clock_ms)).1
    }

    /// The layout a pane showing the newest 120 bars asks for.
    fn request(&self) -> ProjectionRequest {
        let first = self.closed.len().saturating_sub(VISIBLE_BARS);
        ProjectionRequest {
            timeline_revision: self.prints as u64,
            first_bar_index: first,
            closed: self.closed[first..].to_vec(),
            partial: self.bars.partial().cloned(),
            lane: false,
            on_newest_bar: true,
            lane_reference_ms: None,
            lane_now_ms: None,
            price_range: PRICE_RANGE,
            dot_zoom: None,
        }
    }
}

#[test]
fn book_print_path_holds_to_budget() {
    let mut book = Book::after(SESSION_PRINTS);
    let mut work = Tally::default();
    for _ in 0..MEASURED_PRINTS {
        work = add(work, book.print().record);
    }
    holds_to_budget(TRADE_BOOK, work, MEASURED_PRINTS);
}

/// The depth updates the envelope sends between prints, each at the time of
/// the print before it, so the clock moves and bars close as live.
#[test]
fn book_depth_path_holds_to_budget() {
    let mut book = Book::after(SESSION_PRINTS);
    let mut work = Tally::default();
    let mut updates = 0;
    while updates < MEASURED_DEPTH_UPDATES {
        let print = book.print();
        work = add(work, print.depth);
        updates += print.depth_updates;
    }
    holds_to_budget(DEPTH_BOOK, work, updates);
}

/// One rebuild a frame: the test's clock moves [`PROJECTION_STEP`] between
/// projections, so none is served from the cache.
#[test]
fn book_projection_holds_to_budget() {
    let mut book = Book::after(SESSION_PRINTS);
    let base = Instant::now();
    let mut work = Tally::default();
    let mut projected = 0;
    let mut builds_before = 0;
    for frame in 0..WARMUP_FRAMES + MEASURED_FRAMES {
        for _ in 0..PRINTS_PER_PROJECTION {
            book.print();
        }
        let request = book.request();
        let now = base + PROJECTION_STEP * frame as u32;
        if frame == WARMUP_FRAMES {
            builds_before = book.engine.health().projection_builds;
        }
        let engine = &mut book.engine;
        let (visible, spent) = counted(|| engine.project_at(&request, now));
        if frame >= WARMUP_FRAMES {
            work = add(work, spent);
            projected += usize::from(visible.is_some());
        }
    }
    assert_eq!(projected, MEASURED_FRAMES, "every counted frame projects");
    assert_eq!(
        book.engine.health().projection_builds - builds_before,
        MEASURED_FRAMES as u64,
        "every counted frame rebuilds the projection"
    );
    holds_to_budget(FRAME_BOOK, work, MEASURED_FRAMES);
}

// ---------------------------------------------------------------------------
// FLOW.
// ---------------------------------------------------------------------------

/// The pane's FLOW worker run inline when the session collects its
/// publication, as `app`'s live FLOW bench runs it: on the app it is a
/// thread, here its work lands on the counted one.
#[derive(Default)]
struct InlineWorker {
    cache: RefCell<FlowWorkerCache>,
    request: RefCell<Option<FlowRequest>>,
    queue: RefCell<Vec<FlowChunk>>,
}

impl FlowRunner for InlineWorker {
    fn request(&mut self, request: &FlowRequest, _: bool) -> bool {
        *self.request.get_mut() = Some(request.clone());
        false
    }
    fn submit(&self, chunk: FlowChunk) -> Result<(), Box<FlowChunk>> {
        self.queue.borrow_mut().push(chunk);
        Ok(())
    }
    fn finished(&self) -> Option<Arc<FlowTapeFrame>> {
        let request = self.request.borrow().clone()?;
        let mut cache = self.cache.borrow_mut();
        cache.select_request(&request);
        for chunk in self.queue.borrow_mut().drain(..) {
            cache.append(&chunk);
        }
        cache.project_if_due(&request, true)
    }
}

/// One FLOW frame over the newest [`VISIBLE_BARS`] tick:50 bars of `tape`,
/// built as `quantick_chart::flow_execution` builds it for a chart whose
/// every print is accepted into the bar its ordinal falls in: slot and
/// accepted ordinal within the slot, as `TickMembership::locate` answers.
fn flow_frame(session: &mut FlowSession<InlineWorker>, tape: &[Trade], look: FlowLook) {
    const EPOCH: u64 = 1;
    let total_slots = tape.len().div_ceil(TICKS_PER_BAR);
    let slots = total_slots.saturating_sub(VISIBLE_BARS)..total_slots;
    let span = |slots: Range<usize>| {
        (slots.start * TICKS_PER_BAR).min(tape.len())..(slots.end * TICKS_PER_BAR).min(tape.len())
    };
    let keep = session.keep_for(EPOCH, slots.clone(), total_slots, span);
    let request = FlowRequest {
        epoch: EPOCH,
        layout_revision: 0,
        source_count: tape.len(),
        requested: span(slots.clone()),
        keep,
        opening_windows: Vec::new(),
        opening_ordinals: Vec::new(),
        view: FlowTapeView {
            clip_left: Decimal::from(slots.start),
            clip_right: Decimal::from(slots.end),
            first_slot: slots.start,
            end_slot: slots.end,
            width_px: slots.len() as f32 * PX_PER_BAR,
            height_px: PANE_HEIGHT_PX,
            prices: PriceWindow::from_f64_range(PRICE_RANGE).expect("a valid axis"),
            reference: look.reference,
            radius_limit: look.radius_limit,
            merge_support_radius: FLOW_MERGE_SUPPORT_RADIUS_PX,
            exclude_opening: false,
        },
    };
    session.project(request, |range| FlowChunk {
        epoch: EPOCH,
        ticks_per_bar: Decimal::from(TICKS_PER_BAR),
        executions: tape[range.clone()]
            .iter()
            .enumerate()
            .map(|(offset, trade)| OwnedFlowExecution {
                ordinal: range.start + offset,
                slot: (range.start + offset) / TICKS_PER_BAR,
                accepted_ordinal: (range.start + offset) % TICKS_PER_BAR,
                trade: trade.clone(),
            })
            .collect(),
    });
}

/// How a pane sizes its FLOW spheres: what the request carries.
#[derive(Clone, Copy)]
struct FlowLook {
    reference: FlowReference,
    radius_limit: f32,
}

/// Regional FLOW: spheres against the visible regions' own maximum.
const REGIONS: FlowLook = FlowLook {
    reference: FlowReference::VisibleRegions,
    radius_limit: FLOW_RADIUS_LIMIT_PX,
};

/// Tick spheres against a typed reference, as the BTC spheres look sizes
/// them: the projection the worker keeps and refinishes per region window.
fn spheres() -> FlowLook {
    let style = quantick_orderflow::TickBubbleStyle {
        reference_quantity: 10.0,
        max_radius: 32.0,
        label_min_radius: 8.0,
        opacity: 0.9,
        reference_mode: quantick_orderflow::TickReferenceMode::Fixed,
        viewport_scale: None,
    };
    FlowLook {
        reference: FlowReference::from(style),
        radius_limit: style.bubbles().max_radius,
    }
}

#[test]
fn flow_regions_live_frames_hold_to_budget() {
    flow_live_frames_hold_to_budget(TRADE_FLOW_REGIONS, REGIONS);
}

#[test]
fn flow_spheres_live_frames_hold_to_budget() {
    flow_live_frames_hold_to_budget(TRADE_FLOW_SPHERES, spheres());
}

fn flow_live_frames_hold_to_budget(budget: Budget, look: FlowLook) {
    let mut source = Tape::new();
    let mut tape: Vec<Trade> = (0..FLOW_HISTORY_PRINTS).map(|_| source.next()).collect();
    let mut session = FlowSession::<InlineWorker>::default();
    let mut work = Tally::default();
    let mut publications = 0;
    let mut last: Option<Arc<FlowTapeFrame>> = None;
    for frame in 0..FLOW_WARMUP_FRAMES + FLOW_MEASURED_FRAMES {
        for _ in 0..PRINTS_PER_FRAME {
            tape.push(source.next());
        }
        let ((), spent) = counted(|| flow_frame(&mut session, &tape, look));
        if frame >= FLOW_WARMUP_FRAMES {
            work = add(work, spent);
            let current = session.frame_handle().cloned();
            if current
                .as_ref()
                .is_some_and(|now| last.as_ref().is_none_or(|last| !Arc::ptr_eq(last, now)))
            {
                publications += 1;
            }
            last = current;
        }
    }
    let frame = last.expect("a FLOW frame");
    assert!(!frame.dots.is_empty(), "the counted frames draw spheres");
    assert_eq!(
        publications, FLOW_MEASURED_FRAMES,
        "every counted frame publishes its new prints"
    );
    holds_to_budget(budget, work, FLOW_MEASURED_FRAMES * PRINTS_PER_FRAME);
}
