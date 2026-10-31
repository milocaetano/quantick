//! One drawn FLOW frame's headless work, counted and held to budgets.
//!
//! | Path | Unit | What runs |
//! | --- | --- | --- |
//! | `chart.frame.flow_session` | frame | `flow_execution::project_styled_flow_executions`: the session's request, admission and capture, the worker's own work subtracted |
//! | `chart.frame.flow_plan` | frame | `FlowPresentation::styled` over the published frame and `place_flow_chrome` for the caption |
//!
//! The scene is the one `app`'s ignored live FLOW bench
//! (`orderflow_render/tests/flow_live_bench.rs`) paints: the recorded busy
//! Binance spot minute (`crates/stores/tests/fixtures/btc/spot-busy.csv`)
//! replayed as a seeded random walk, a tick:50 chart in the "Binance BTC Spot
//! spheres" look, the pane's worker run inline. Counted, not timed: work is
//! read with [`work_meter`] on the UI thread, so the budgets hold in the
//! ordinary `cargo test` whatever the host's load. What the bench also sees,
//! egui's paint and tessellation and the GPU upload, is `app`'s and not
//! counted here; the worker's work is `orderflow`'s `trade.flow` budget.
//!
//! The budgets are declared first, each with the calibration it was set
//! from, and never read back from a measurement.

use std::cell::{Cell, RefCell};
use std::str::FromStr as _;
use std::sync::Arc;

use quantick_chart::flow_execution::{
    FlowChartView, FlowExecutionGeometry, FlowPresentation, place_flow_chrome,
    project_styled_flow_executions,
};
use quantick_chart::state::ChartState;
use quantick_chart::viewport::Viewport;
use quantick_engine::work_meter::{self, Counting, Tally};
use quantick_engine::{BarSpec, Side, Trade};
use quantick_orderflow::projection::flow_tape::{
    FlowChunk, FlowRequest, FlowRunner, FlowSession, FlowTapeFrame, FlowWorkerCache,
};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

// ---------------------------------------------------------------------------
// Budgets: heap work per frame. Declared at no more than twice the
// calibration, rounded down to a readable number.
// ---------------------------------------------------------------------------

const KIB: f64 = 1_024.0;

/// Per-frame ceilings of one path.
#[derive(Clone, Copy, Debug)]
struct Budget {
    path: &'static str,
    /// Allocations per frame.
    allocs: f64,
    /// Bytes allocated per frame.
    bytes: f64,
    /// Bytes reallocation may copy per frame.
    copy_bytes: f64,
}

/// The UI thread's FLOW capture, about 0.8 live prints a frame. Calibrated
/// at `d5b0f95f`: 4.2 allocations, 95.5 bytes and 66.7 copy bytes per frame.
const FRAME_SESSION: Budget = Budget {
    path: "chart.frame.flow_session",
    allocs: 8.0,
    bytes: 160.0,
    copy_bytes: 128.0,
};
/// The sphere plan and the caption's place. Calibrated at `d5b0f95f`: 3.0
/// allocations, 33,367 bytes and 48,960 copy bytes per frame (816 regions).
const FRAME_PLAN: Budget = Budget {
    path: "chart.frame.flow_plan",
    allocs: 6.0,
    bytes: 64.0 * KIB,
    copy_bytes: 90.0 * KIB,
};

// ---------------------------------------------------------------------------
// The scene.
// ---------------------------------------------------------------------------

const BUSY: &str = include_str!("../../stores/tests/fixtures/btc/spot-busy.csv");
/// History before the live frames: about twenty minutes of this tape.
const HISTORY_PRINTS: usize = 60_000;
const HISTORY_PACE: f64 = 0.6;
/// The live tape the bench's perf session saw: about 39 prints a second.
const LIVE_PACE: f64 = 0.155;
const FRAME_MS: f64 = 1000.0 / 60.0;
/// Frames run before counting (the visible window's cold fill), and counted.
const WARMUP_FRAMES: usize = 20;
const MEASURED_FRAMES: usize = 600;
const TICKS_PER_BAR: u64 = 50;
const PX_PER_BAR: f32 = 8.0;
/// The bench's candle pane, in logical points: left, top, right, bottom.
const HISTORY: [f32; 4] = [40.0, 120.0, 970.0, 1300.0];
/// The caption panel `place_flow_chrome` places.
const CAPTION: [[f32; 2]; 2] = [[48.0, 148.0], [368.0, 192.0]];
/// The "Binance BTC Spot spheres" look.
const TICK: quantick_orderflow::TickBubbleStyle = quantick_orderflow::TickBubbleStyle {
    reference_quantity: 1.0,
    max_radius: 32.0,
    label_min_radius: 8.0,
    opacity: 0.9,
    reference_mode: quantick_orderflow::TickReferenceMode::Fixed,
    viewport_scale: None,
};

thread_local! {
    /// The inline worker's heap work, subtracted from the session's.
    static WORKER: Cell<Tally> = const { Cell::new(Tally {
        allocs: 0,
        alloc_bytes: 0,
        reallocs: 0,
        realloc_copy_bytes: 0,
        largest_realloc_copy: 0,
    }) };
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

/// The pane's worker, run when the session collects its publication; its
/// work is tallied apart, since on the app it runs on its own thread.
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
        let before = work_meter::tally();
        let frame = (|| {
            let request = self.request.borrow().clone()?;
            let mut cache = self.cache.borrow_mut();
            cache.select_request(&request);
            for chunk in self.queue.borrow_mut().drain(..) {
                cache.append(&chunk);
            }
            cache.project_if_due(&request, true)
        })();
        let spent = work_meter::tally().since(before);
        WORKER.with(|worker| worker.set(add(worker.get(), spent)));
        frame
    }
}

/// One recorded print as a step from the one before it.
struct Step {
    dt_ms: i64,
    dp: Decimal,
    quantity: Decimal,
    side: Side,
}

fn steps(csv: &str) -> Vec<Step> {
    let rows: Vec<(i64, Decimal, Decimal, Side)> = csv
        .lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<_> = line.split(',').collect();
            (
                fields[1].parse().expect("recorded time"),
                Decimal::from_str(fields[2]).expect("recorded price"),
                Decimal::from_str(fields[3]).expect("recorded quantity"),
                if fields[4] == "buy" {
                    Side::Buy
                } else {
                    Side::Sell
                },
            )
        })
        .collect();
    rows.windows(2)
        .map(|pair| Step {
            dt_ms: pair[1].0 - pair[0].0,
            dp: pair[1].1 - pair[0].1,
            quantity: pair[1].2,
            side: pair[1].3,
        })
        .collect()
}

/// The busy minute replayed in a seeded order of upward and downward laps,
/// as the bench's tape replays it: the price walks like BTC's and the
/// quantities keep their recorded tail.
struct BtcTape {
    minute: Vec<Step>,
    seed: u64,
    step: usize,
    downward: bool,
    time_ms: f64,
    price: Decimal,
    next_id: u64,
}

impl BtcTape {
    fn new() -> Self {
        Self {
            minute: steps(BUSY),
            seed: 0x9E37_79B9_7F4A_7C15,
            step: 0,
            downward: false,
            time_ms: 1_790_000_000_000.0,
            price: Decimal::from(80_000),
            next_id: 1,
        }
    }

    fn next(&mut self, pace: f64) -> Trade {
        if self.step == self.minute.len() {
            self.seed = self
                .seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.downward = (self.seed >> 62) & 1 == 1;
            self.step = 0;
        }
        let step = &self.minute[self.step];
        self.step += 1;
        self.time_ms += step.dt_ms as f64 * pace;
        self.price += if self.downward { -step.dp } else { step.dp };
        self.next_id += 1;
        Trade {
            agg_id: self.next_id,
            timestamp_ms: self.time_ms as i64,
            price: self.price,
            quantity: step.quantity,
            side: step.side,
        }
    }
}

/// The visible bars' price range, held while they stay inside it.
fn fit(state: &ChartState, slots: std::ops::Range<usize>, held: Option<(f64, f64)>) -> (f64, f64) {
    let bars = state.bars().iter().chain(state.partial());
    let (low, high) =
        bars.skip(slots.start)
            .take(slots.len())
            .fold((f64::MAX, f64::MIN), |(low, high), bar| {
                (
                    low.min(bar.low.to_f64().expect("a finite low")),
                    high.max(bar.high.to_f64().expect("a finite high")),
                )
            });
    match held {
        Some(range) if range.0 <= low && high <= range.1 => range,
        _ => {
            let pad = (high - low).max(1.0) * 0.04;
            (low - pad, high + pad)
        }
    }
}

/// Hold `work`, done over `frames` frames, to `budget`.
fn holds_to_budget(budget: Budget, work: Tally, frames: usize) {
    let per = |total: u64| total as f64 / frames as f64;
    let measured = (
        per(work.allocs),
        per(work.alloc_bytes),
        per(work.realloc_copy_bytes),
    );
    println!(
        "{}: {:.1} allocs, {:.0} bytes, {:.0} copy bytes per frame over {frames}",
        budget.path, measured.0, measured.1, measured.2
    );
    assert!(
        measured.0 <= budget.allocs
            && measured.1 <= budget.bytes
            && measured.2 <= budget.copy_bytes,
        "{} over budget per frame: {:.1} allocs (budget {}), {:.0} bytes (budget {}), \
         {:.0} copy bytes (budget {}); totals over {frames} frames: {work:?}",
        budget.path,
        measured.0,
        budget.allocs,
        measured.1,
        budget.bytes,
        measured.2,
        budget.copy_bytes,
    );
}

#[test]
fn a_drawn_flow_frame_holds_to_budget() {
    let mut tape = BtcTape::new();
    let history: Vec<Trade> = (0..HISTORY_PRINTS)
        .map(|_| tape.next(HISTORY_PACE))
        .collect();
    let mut state = ChartState::new(BarSpec::Tick(TICKS_PER_BAR));
    state.ingest_backfill(&history);
    drop(history);
    let mut session = FlowSession::<InlineWorker>::default();
    let mut viewport = Viewport::new();
    viewport.set_px_per_bar(PX_PER_BAR);
    let width = HISTORY[2] - HISTORY[0];
    let height = HISTORY[3] - HISTORY[1];
    let mut range = None;
    let mut clock_ms = tape.time_ms;
    let mut pending = Some(tape.next(LIVE_PACE));
    let (mut session_work, mut plan_work) = (Tally::default(), Tally::default());
    let mut worker_work = Tally::default();
    let (mut regions, mut prints) = (0, 0);
    for frame_index in 0..WARMUP_FRAMES + MEASURED_FRAMES {
        let measured = frame_index >= WARMUP_FRAMES;
        // The prints whose exchange time this frame has reached.
        clock_ms += if measured { FRAME_MS } else { 0.0 };
        while measured
            && pending
                .as_ref()
                .is_some_and(|print| print.timestamp_ms as f64 <= clock_ms)
        {
            state.ingest_live(&pending.take().expect("a pending print"));
            pending = Some(tape.next(LIVE_PACE));
            prints += 1;
        }
        let total = state.bars().len() + usize::from(state.partial().is_some());
        let visible = (width / PX_PER_BAR).ceil() as usize + 1;
        let slots = total.saturating_sub(visible)..total;
        let fitted = fit(&state, slots.clone(), range);
        range = Some(fitted);
        let right = f64::from(viewport.right_edge_bar(total)) + 1.0;
        let left = right - f64::from(width / PX_PER_BAR);

        let worker_before = WORKER.with(Cell::get);
        let before = work_meter::tally();
        project_styled_flow_executions(
            true,
            &state,
            &mut session,
            FlowChartView {
                slots: slots.clone(),
                pixels: (slots.len() as f32 * PX_PER_BAR, height),
                range: fitted,
                clip: (left, right),
                tick_bubbles: Some(TICK),
            },
        );
        let spent = work_meter::tally().since(before);
        let worker = WORKER.with(Cell::get).since(worker_before);
        let Some(frame) = session.frame_handle().cloned() else {
            assert!(!measured, "every counted frame has a FLOW frame to draw");
            continue;
        };
        let geometry = FlowExecutionGeometry::new(viewport, total, 0, fitted, HISTORY, false)
            .expect("a valid geometry");

        let before = work_meter::tally();
        let plan = FlowPresentation::styled(
            &frame,
            [[HISTORY[0], HISTORY[1]], [HISTORY[2], HISTORY[3]]],
            Some(TICK),
            |dot| geometry.point(dot).map(|(x, y)| [x, y]),
        );
        std::hint::black_box(place_flow_chrome(
            [[HISTORY[0], HISTORY[1]], [HISTORY[2], HISTORY[3]]],
            CAPTION,
            &plan.regions,
        ));
        let planned = work_meter::tally().since(before);
        if measured {
            session_work = add(
                session_work,
                Tally {
                    allocs: spent.allocs - worker.allocs,
                    alloc_bytes: spent.alloc_bytes - worker.alloc_bytes,
                    reallocs: spent.reallocs - worker.reallocs,
                    realloc_copy_bytes: spent.realloc_copy_bytes - worker.realloc_copy_bytes,
                    largest_realloc_copy: 0,
                },
            );
            plan_work = add(plan_work, planned);
            worker_work = add(worker_work, worker);
            regions = plan.regions.len();
        }
        drop(plan);
    }
    println!(
        "scene: {MEASURED_FRAMES} frames, {prints} live prints, {regions} sphere regions; \
         the inline worker's own work over them, not budgeted here: {worker_work:?}"
    );
    assert!(prints > 0, "the counted frames carry live prints");
    assert!(regions > 0, "the counted frames draw spheres");
    holds_to_budget(FRAME_SESSION, session_work, MEASURED_FRAMES);
    holds_to_budget(FRAME_PLAN, plan_work, MEASURED_FRAMES);
}
