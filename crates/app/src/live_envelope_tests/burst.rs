//! The envelope's rates, driven through the real workers.
//!
//! [`Rig`] is one chart pane reduced to its live path: a [`ChartState`], the
//! lane cursor, the production [`IndicatorWorker`] hosting `native.cvd`, and
//! the production [`BookWorker`] recording aggressions and depth — fed per
//! frame exactly as `pane/series.rs` and `orderflow_view.rs` feed them, with
//! the per-frame reads that are the parked queue's retry points.
//!
//! "Every trade arrived" is checked three independent ways: the indicator's
//! committed rows equal the closed bars and its last row equals the
//! cumulative delta computed here from the prints; its preview equals the
//! delta of the whole tape; and the book retained one aggression per print
//! and applied every depth update sent.

use crate::indicator_worker::{
    IndicatorCommand, IndicatorEvent, IndicatorSource, IndicatorWorker, LaneTransport, SlotId,
};
use crate::live_envelope::*;
use crate::orderflow_worker::{BookCommand, BookWorker};
use crate::state::{BarSpec, ChartState};
use crate::worker_progress::{Counts, Phase, WorkerProgress, tests::Gate};
use quantick_engine::{Side, Trade};
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot, DepthEvent};
use quantick_orderflow::HeatmapConfig;
use quantick_orderflow::engine::ProjectionRequest;
use rust_decimal::Decimal;
use std::time::{Duration, Instant};

const SYMBOL: &str = "WINV26";
const GENERATION: u64 = 10;
const EPOCH_MS: i64 = 1_700_000_000_000;
pub(super) const FRAMES_PER_S: u64 = 60;
const FRAME: Duration = Duration::from_micros(16_667);
/// Seconds the sustained rate is held for.
pub(super) const SUSTAINED_SECONDS: u64 = 5;
/// Seconds the burst rate is held for — "a few", per [`BURST_TRADES_PER_S`].
pub(super) const BURST_SECONDS: u64 = 3;
/// Seconds of burst-rate frames the stalled workers are sent.
pub(super) const ABOVE_SECONDS: u64 = 40;
/// Lane rungs the rig asks for: a lane on screen, so forming runs travel.
const RUNGS: usize = 16;

fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(Decimal::from(price), Decimal::from(quantity)).expect("a valid level")
}

/// A deterministic tape: integer quantities, so every cumulative delta is
/// exact in `f64`, and a buy/sell mix that never nets to zero by accident.
#[derive(Default)]
pub(super) struct Tape {
    pub prints: Vec<Trade>,
    /// Exchange milliseconds, advanced by the rate each print is played at.
    clock_ms: f64,
}

impl Tape {
    fn next(&mut self, rate: u64) -> Trade {
        let id = self.prints.len() as u64 + 1;
        self.clock_ms += 1_000.0 / rate as f64;
        let trade = Trade {
            agg_id: id,
            timestamp_ms: EPOCH_MS + self.clock_ms as i64,
            price: Decimal::from(100 + (id % 7) as i64),
            quantity: Decimal::from(1 + (id % 3) as i64),
            side: if id % 5 < 3 { Side::Buy } else { Side::Sell },
        };
        self.prints.push(trade.clone());
        trade
    }

    fn delta(prints: &[Trade]) -> f64 {
        prints
            .iter()
            .map(|t| {
                let q: f64 = t.quantity.try_into().expect("small quantity");
                if t.side == Side::Buy { q } else { -q }
            })
            .sum()
    }
}

/// The deepest each queue got, sampled once per frame.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Depths {
    pub indicator_queued: u64,
    pub book_queued: u64,
    pub indicator_parked: u64,
    pub book_parked: u64,
}

pub(super) struct Rig {
    pub state: ChartState,
    lane: LaneTransport,
    pub indicators: IndicatorWorker,
    pub book: BookWorker,
    next_update_id: u64,
    rows: usize,
    last_row: Option<f64>,
    preview: Option<f64>,
    pub depths: Depths,
    /// Frames that found the previous frame not yet admitted and waited for
    /// it: how often the workers fell behind the frame cadence.
    pub late_frames: usize,
}

impl Rig {
    pub(super) fn new(spec: BarSpec, indicators: WorkerProgress, book: WorkerProgress) -> Self {
        let rig = Self {
            state: ChartState::new(spec),
            lane: LaneTransport::default(),
            indicators: IndicatorWorker::spawn_with_progress(indicators),
            book: BookWorker::spawn_with_progress(SYMBOL, book),
            next_update_id: GENERATION + 1,
            rows: 0,
            last_row: None,
            preview: None,
            depths: Depths::default(),
            late_frames: 0,
        };
        rig.indicators.send(IndicatorCommand::Add {
            slot: SlotId(1),
            source: IndicatorSource::Native {
                id: "native.cvd".to_owned(),
                values: Vec::new(),
            },
        });
        rig.book.send(BookCommand::ApplyVisualConfig(HeatmapConfig {
            show_aggressions: true,
            ..HeatmapConfig::default()
        }));
        rig.book.send(BookCommand::SetEnabled {
            enabled: true,
            generation_floor: GENERATION,
        });
        rig.book.send(BookCommand::Depth {
            event: DepthEvent::Snapshot {
                symbol: SYMBOL.into(),
                generation: GENERATION,
                observed_at_ms: EPOCH_MS,
                effective_at_ms: EPOCH_MS,
                price_step: Some(Decimal::ONE),
                snapshot: BookSnapshot::new(
                    GENERATION,
                    vec![level(99, 5)],
                    vec![level(101, 6)],
                    BookCoverage::Limited { levels_per_side: 1 },
                ),
            },
            received_at_ms: EPOCH_MS,
        });
        rig
    }

    /// One UI frame: `trades` new prints at `rate`, `depth` book updates, the
    /// forming-bar update and a layout request — then the frame's reads.
    pub(super) fn frame(&mut self, tape: &mut Tape, trades: usize, rate: u64, depth: usize) {
        self.lane.set_rungs(RUNGS);
        for _ in 0..trades {
            let trade = tape.next(rate);
            self.book.send(BookCommand::Trade(trade.clone()));
            let before = self.state.bars().len();
            self.state.ingest_live(&trade);
            if self.state.bars().len() > before {
                self.lane.reset();
                let closed = self.state.bars().last().cloned().expect("a bar closed");
                self.indicators.send(IndicatorCommand::BarClosed(closed));
            }
        }
        for _ in 0..depth {
            let id = self.next_update_id;
            self.next_update_id += 1;
            let at = EPOCH_MS + tape.clock_ms as i64;
            self.book.send(BookCommand::Depth {
                event: DepthEvent::Update {
                    symbol: SYMBOL.into(),
                    generation: GENERATION,
                    event_time_ms: at,
                    delta: BookDelta::new(
                        id,
                        id,
                        vec![level(99, 5 + (id % 4) as i64)],
                        vec![level(101, 6 + (id % 3) as i64)],
                    ),
                },
                received_at_ms: at,
            });
        }
        let partial = self
            .lane
            .command(self.state.partial().cloned(), self.state.trades());
        self.indicators.send(partial);
        self.book.send(BookCommand::Project(ProjectionRequest {
            timeline_revision: 1,
            first_bar_index: 0,
            closed: Vec::new(),
            partial: None,
            lane: false,
            on_newest_bar: true,
            lane_reference_ms: None,
            price_range: (90.0, 110.0),
        }));
        self.read();
    }

    /// The per-frame reads a pane makes: indicator deltas, the book mailbox.
    /// Both are also where a parked queue is offered to its channel again.
    pub(super) fn read(&mut self) {
        for event in self.indicators.drain_events() {
            match event {
                IndicatorEvent::Rebuilt { rows, columns, .. } => {
                    self.rows = rows;
                    self.last_row = columns.first().and_then(|c| c.last().copied());
                }
                IndicatorEvent::Appended { row, .. } => {
                    self.rows += 1;
                    self.last_row = row.first().copied();
                }
                IndicatorEvent::Preview { frame, .. } => {
                    self.preview = frame.and_then(|f| f.values.first().copied());
                }
                _ => {}
            }
        }
        let _ = self.book.published_base_grouping();
        let (indicator, book) = (self.indicator_counts(), self.book_counts());
        let d = &mut self.depths;
        d.indicator_queued = d.indicator_queued.max(indicator.queued);
        d.book_queued = d.book_queued.max(book.queued);
        d.indicator_parked = d.indicator_parked.max(indicator.parked);
        d.book_parked = d.book_parked.max(book.parked);
    }

    pub(super) fn indicator_counts(&self) -> Counts {
        self.indicators.progress().counts
    }

    pub(super) fn book_counts(&self) -> Counts {
        self.book.progress().counts
    }

    /// Wait, bounded, until both workers have taken every command sent so
    /// far into a batch, counting the frame as late if it had to.
    ///
    /// The envelope's per-frame claim — a worker drains one frame before the
    /// next arrives — is a statement about wall time, measured by the harness
    /// and the dense replay rather than asserted here: a descheduled test
    /// thread would otherwise fail the queue for the machine's sake.
    fn await_admitted(&mut self) {
        let admitted = |rig: &Self| {
            let (indicator, book) = (rig.indicator_counts(), rig.book_counts());
            indicator.queued + indicator.parked + book.queued + book.parked == 0
        };
        if admitted(self) {
            return;
        }
        self.late_frames += 1;
        let deadline = Instant::now() + Duration::from_secs(30);
        while !admitted(self) {
            assert!(Instant::now() < deadline, "the workers never caught up");
            std::thread::sleep(Duration::from_micros(200));
            self.read();
        }
    }

    /// Frames with nothing new until nothing is parked, then both barriers.
    pub(super) fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.indicator_counts().parked + self.book_counts().parked > 0 {
            assert!(Instant::now() < deadline, "parked commands never drained");
            std::thread::sleep(Duration::from_millis(1));
            self.read();
        }
        self.indicators.flush();
        self.book.flush();
        self.read();
    }

    /// Every print reached the bars, the indicator and the book; every
    /// accepted command was retired and none failed.
    pub(super) fn assert_nothing_lost(&self, tape: &Tape, depth_sent: u64) {
        let closed: u64 = self.state.bars().iter().map(|b| b.trade_count).sum();
        let forming = self.state.partial().map_or(0, |b| b.trade_count);
        assert_eq!(
            closed + forming,
            tape.prints.len() as u64,
            "every print is in a bar"
        );
        assert_eq!(
            self.rows,
            self.state.bars().len(),
            "one indicator row per closed bar"
        );
        let closed_prints = &tape.prints[..closed as usize];
        assert_eq!(
            self.last_row,
            Some(Tape::delta(closed_prints)),
            "closed-bar CVD"
        );
        if forming > 0 {
            assert_eq!(self.preview, Some(Tape::delta(&tape.prints)), "forming CVD");
        }
        let book = self.book.published().health;
        assert_eq!(
            book.aggression_count,
            tape.prints.len(),
            "one aggression per print"
        );
        assert_eq!(book.depth_updates, depth_sent, "every depth update applied");
        for counts in [self.indicator_counts(), self.book_counts()] {
            assert_eq!(counts.failed_sends, 0);
            assert_eq!(counts.parked, 0);
            assert_eq!(counts.queued, 0);
            assert_eq!(
                counts.accepted, counts.retired,
                "every accepted command retired"
            );
        }
        // Last: the inspection is itself a command, accepted before retired.
        if forming > 0 {
            let (run, _capacity) = self.indicators.retained_lane_for_test();
            assert_eq!(run as u64, forming, "the lane run holds the forming prints");
        }
    }
}

/// Play `rate` prints/s and `depth_rate` updates/s for `seconds`, one frame
/// every 16.7 ms of wall time — a UI's cadence, not a flood — each frame sent
/// only once the previous one was admitted ([`Rig::late_frames`] counts the
/// waits). Returns the depth updates sent.
pub(super) fn play(
    rig: &mut Rig,
    tape: &mut Tape,
    rate: u64,
    depth_rate: u64,
    seconds: u64,
) -> u64 {
    let frames = seconds * FRAMES_PER_S;
    let start = Instant::now();
    let mut sent_trades = 0;
    let mut sent_depth = 0;
    for frame in 1..=frames {
        let trades = (rate * frame / FRAMES_PER_S) - sent_trades;
        let depth = (depth_rate * frame / FRAMES_PER_S) - sent_depth;
        rig.await_admitted();
        rig.frame(tape, trades as usize, rate, depth as usize);
        sent_trades += trades;
        sent_depth += depth;
        let due = start + FRAME * u32::try_from(frame).expect("a short run");
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    sent_depth
}

#[test]
fn inside_the_envelope_every_print_arrives_and_no_queue_fills() {
    // tick:1 is the command-heaviest chart there is: every print closes a bar,
    // so every print is one indicator command as well as one book command.
    let mut rig = Rig::new(
        BarSpec::Tick(1),
        WorkerProgress::new(),
        WorkerProgress::new(),
    );
    let mut tape = Tape::default();
    let mut depth = play(
        &mut rig,
        &mut tape,
        SUSTAINED_TRADES_PER_S,
        DEPTH_UPDATES_PER_S,
        SUSTAINED_SECONDS,
    );
    depth += play(
        &mut rig,
        &mut tape,
        BURST_TRADES_PER_S,
        DEPTH_UPDATES_PER_S,
        BURST_SECONDS,
    );
    // The busiest frame the envelope allows, all at once, arriving at workers
    // that kept up with the burst before it — the envelope's claim is about a
    // frame, not about a frame stacked on a machine that fell behind, and
    // waiting here keeps a loaded CI runner from turning its own stall into a
    // failure of the queue.
    rig.settle();
    rig.frame(
        &mut tape,
        BURST_TRADES_PER_FRAME,
        BURST_TRADES_PER_S,
        BURST_DEPTH_UPDATES_PER_FRAME,
    );
    depth += BURST_DEPTH_UPDATES_PER_FRAME as u64;
    rig.settle();

    let expected = (SUSTAINED_TRADES_PER_S * SUSTAINED_SECONDS + BURST_TRADES_PER_S * BURST_SECONDS)
        as usize
        + BURST_TRADES_PER_FRAME;
    assert_eq!(tape.prints.len(), expected);
    // The keep-up half, coarsely: the workers drained a frame before the
    // next one arrived on most frames. Loose on purpose — a loaded runner
    // may be late now and then; a worker that cannot hold the envelope's
    // rate is late on nearly every frame and fails here.
    let frames = ((SUSTAINED_SECONDS + BURST_SECONDS) * FRAMES_PER_S) as usize;
    assert!(
        rig.late_frames * 2 < frames,
        "{} of {frames} frames found the previous one untaken",
        rig.late_frames
    );
    rig.assert_nothing_lost(&tape, depth);
    for counts in [rig.indicator_counts(), rig.book_counts()] {
        assert_eq!(counts.deferred, 0, "inside the envelope no queue fills");
        assert_eq!(counts.coalesced_parked, 0);
    }
    println!(
        "inside: prints={} depth_updates={depth} bars={} max_queued indicator={} book={} \
         (caps {INDICATOR_COMMAND_QUEUE} / {BOOK_COMMAND_QUEUE}) late_frames={} deferred=0 parked=0 lost=0",
        tape.prints.len(),
        rig.state.bars().len(),
        rig.depths.indicator_queued,
        rig.depths.book_queued,
        rig.late_frames,
    );
}

#[test]
fn above_the_envelope_a_stalled_worker_parks_and_folds_and_loses_nothing() {
    let (indicator_gate, book_gate) = (Gate::new(), Gate::new());
    let mut rig = Rig::new(
        BarSpec::Tick(50),
        WorkerProgress::with_clock(indicator_gate.clone()),
        WorkerProgress::with_clock(book_gate.clone()),
    );
    let mut tape = Tape::default();
    rig.settle();
    // Stall both workers inside the next batch they take: a worker far slower
    // than the tape is what "above the envelope" means to a queue.
    let indicator_hold = indicator_gate.hold(Phase::Applying);
    let book_hold = book_gate.hold(Phase::Applying);
    let per_frame = (BURST_TRADES_PER_S / FRAMES_PER_S) as usize;
    let per_frame_depth = (DEPTH_UPDATES_PER_S / FRAMES_PER_S) as usize;
    rig.frame(&mut tape, per_frame, BURST_TRADES_PER_S, per_frame_depth);
    indicator_hold.reached();
    book_hold.reached();
    let mut depth = per_frame_depth as u64;
    // Forty seconds of burst-rate frames against workers that take none of
    // them; every tenth frame prints nothing, the way a real tape pauses.
    // Forty, not more: past ~100,000 prints the heatmap's own retained-
    // aggression cap (`HeatmapConfig::max_aggressions`) starts evicting, and
    // this test counts aggressions to prove none were lost in the queue.
    for frame in 0..(ABOVE_SECONDS * FRAMES_PER_S) {
        let idle = frame % 10 == 0;
        let (trades, updates) = if idle {
            (0, 0)
        } else {
            (per_frame, per_frame_depth)
        };
        rig.frame(&mut tape, trades, BURST_TRADES_PER_S, updates);
        depth += updates as u64;
    }
    let (indicator, book) = (rig.indicator_counts(), rig.book_counts());
    for (name, counts, queue) in [
        ("indicator", indicator, INDICATOR_COMMAND_QUEUE),
        ("book", book, BOOK_COMMAND_QUEUE),
    ] {
        assert!(counts.parked > 0, "{name}: a full queue parks");
        assert!(counts.deferred > counts.parked, "{name}: and folds some");
        assert!(
            counts.coalesced_parked > 0,
            "{name}: superseded commands fold"
        );
        assert_eq!(
            counts.queued, queue as u64,
            "{name}: the channel is exactly full"
        );
        assert_eq!(counts.failed_sends, 0, "{name}: nothing was refused");
    }
    indicator_hold.release();
    book_hold.release();
    rig.settle();
    rig.assert_nothing_lost(&tape, depth);
    let (after_indicator, after_book) = (rig.indicator_counts(), rig.book_counts());
    assert_eq!(
        after_indicator.deferred, indicator.deferred,
        "counts are cumulative"
    );
    assert_eq!(after_book.deferred, book.deferred);
    println!(
        "above: prints={} depth_updates={depth} bars={} indicator parked_max={} deferred={} \
         coalesced={} | book parked_max={} deferred={} coalesced={} | lost=0",
        tape.prints.len(),
        rig.state.bars().len(),
        rig.depths.indicator_parked,
        indicator.deferred,
        indicator.coalesced_parked,
        rig.depths.book_parked,
        book.deferred,
        book.coalesced_parked,
    );
}
