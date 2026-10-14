//! The live book on a short tape, frame by frame: drawn from its last
//! confirmation out to the lane's live edge, the carried stretch labelled as
//! such, and never carried across a gap or past a book that stopped
//! confirming.
use super::*;
use crate::projection::{BOOK_CARRY_MAX_MS, HeatmapCell};
use quantick_engine::{Side, Trade};
use quantick_orderbook::{BookCoverage, BookDelta, BookSnapshot};
use std::time::Duration;

const WINDOW_MS: i64 = 300;
const GENERATION: u64 = 1;
const FRAME_MS: i64 = 16;
const CONFIRM_MS: i64 = 50;
const EPSILON: f64 = 1e-9;
/// The one print: the tape is quiet after it, as on the trader's frame.
const PRINT_MS: i64 = 1_000;

fn config() -> HeatmapConfig {
    HeatmapConfig {
        show_depth: false,
        show_aggressions: true,
        live_lane: crate::LiveLaneStyle {
            enabled: true,
            show_depth: true,
            show_aggressions: true,
            native_tape: true,
            window: crate::LaneWindow::Fixed { ms: WINDOW_MS },
            ..Default::default()
        },
        // The native tape is a tape of volume dots: only it runs on the
        // supplied clock.
        volume_dots: crate::config::VolumeDotStyle {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn print() -> Trade {
    Trade {
        agg_id: 1,
        timestamp_ms: PRINT_MS,
        price: Decimal::from(100),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// A bid wall at 95 and an ask wall at 108, then one print.
fn engine() -> BookEngine {
    let mut engine = BookEngine::new("WINV26");
    engine.set_enabled(true, GENERATION);
    engine.apply_visual_config(config());
    engine.handle_depth_event(DepthEvent::Snapshot {
        symbol: "WINV26".to_owned(),
        generation: GENERATION,
        observed_at_ms: 999,
        effective_at_ms: 999,
        price_step: Some(Decimal::ONE),
        snapshot: BookSnapshot::new(
            10,
            vec![BookLevel::new(Decimal::from(95), Decimal::from(50)).unwrap()],
            vec![BookLevel::new(Decimal::from(108), Decimal::from(40)).unwrap()],
            BookCoverage::Limited {
                levels_per_side: 10,
            },
        ),
    });
    engine.record_trade(&print());
    engine
}

/// A confirmation: the book unchanged, its clock moved to `book_ms`.
fn confirm(engine: &mut BookEngine, update_id: u64, book_ms: i64) {
    engine.handle_depth_event(DepthEvent::Update {
        symbol: "WINV26".to_owned(),
        generation: GENERATION,
        event_time_ms: book_ms,
        delta: BookDelta::new(update_id, update_id, Vec::new(), Vec::new()),
    });
}

fn request(now_ms: i64) -> ProjectionRequest {
    let mut bar = Bar::opened_by(&print());
    bar.close_time = PRINT_MS;
    ProjectionRequest {
        timeline_revision: 0,
        first_bar_index: 0,
        closed: Vec::new(),
        partial: Some(bar),
        lane: true,
        on_newest_bar: true,
        lane_reference_ms: Some(6_000),
        lane_now_ms: Some(now_ms),
        price_range: (90.0, 115.0),
        dot_zoom: Some(DotZoom {
            native_tape: true,
            tape_window_ms: 100,
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            lane_bars: vec![(PRINT_MS, now_ms)],
        }),
    }
}

/// One frame as the chart draws it: where the book ends, where the edge is,
/// and the bands past the last confirmation.
struct Frame {
    now_ms: i64,
    /// Where the latest confirmation sits on this frame's own lane.
    book_x: Option<f64>,
    book_ms: Option<i64>,
    heat_end_x: f64,
    edge_x: f64,
    carried: Vec<HeatmapCell>,
    observed: Vec<HeatmapCell>,
}

/// Run `frames` frames of a tape clock at 60 fps from `start_ms`, the book
/// confirming every 50 ms while `confirming(book_ms)` says it does, its
/// stamps `lag_ms` behind the tape clock.
fn run(
    engine: &mut BookEngine,
    start_ms: i64,
    frames: i64,
    lag_ms: i64,
    confirming: impl Fn(i64) -> bool,
) -> Vec<Frame> {
    let started = Instant::now();
    let mut update_id = engine
        .history
        .book()
        .last_update_id()
        .map_or(11, |id| id + 1);
    let mut next_confirm = engine.history.latest_book_ms().unwrap_or(PRINT_MS) + CONFIRM_MS;
    (0..frames)
        .map(|frame| {
            let now_ms = start_ms + frame * FRAME_MS;
            while next_confirm <= now_ms - lag_ms {
                if confirming(next_confirm) {
                    confirm(engine, update_id, next_confirm);
                    update_id += 1;
                }
                next_confirm += CONFIRM_MS;
            }
            let cache_now =
                started + Duration::from_millis(u64::try_from(frame * FRAME_MS).unwrap());
            let visible = engine
                .project_at(&request(now_ms), cache_now)
                .expect("the tape projects");
            let projection = &visible.projection;
            let edge_x = projection.live_now_x.expect("a lane has an edge");
            let lane_x = (visible.slot_count as f64 - 1.0) / visible.slot_count as f64;
            let book_ms = engine.history.latest_book_ms();
            let book_x = book_ms.map(|book_ms| {
                let unit = (book_ms - (now_ms - WINDOW_MS)) as f64 / WINDOW_MS as f64;
                lane_x + unit.clamp(0.0, 1.0) * (1.0 - lane_x)
            });
            let (carried, observed): (Vec<_>, Vec<_>) = projection
                .cells
                .iter()
                .cloned()
                .partition(|cell| cell.carried);
            Frame {
                now_ms,
                book_x,
                book_ms,
                heat_end_x: projection
                    .cells
                    .iter()
                    .map(|cell| cell.x1)
                    .fold(f64::NEG_INFINITY, f64::max),
                edge_x,
                carried,
                observed,
            }
        })
        .collect()
}

/// Trader 2026-10-09, WIN on a 282-408 ms tape: the heat ended 17-152 ms
/// short of the lane's edge, the stretch after it empty, though the book was
/// streaming and connected. The book's stamps run behind the tape clock by
/// how late the last print reached the bridge, and the settled half is
/// re-cut only once per projection interval. Neither is a missing book: the
/// last confirmed book is the book until the next one says otherwise.
#[test]
fn a_confirming_book_reaches_the_live_edge_in_every_frame() {
    let mut engine = engine();
    for lag_ms in [0, 17, 106, 152] {
        for (index, frame) in run(&mut engine, 2_000 + lag_ms * 100, 40, lag_ms, |_| true)
            .iter()
            .enumerate()
        {
            assert!(
                (frame.heat_end_x - frame.edge_x).abs() < EPSILON,
                "lag {lag_ms} ms, frame {index}: heat ends at {} short of the edge {}",
                frame.heat_end_x,
                frame.edge_x
            );
            let observed_end = frame
                .observed
                .iter()
                .map(|cell| cell.x1)
                .fold(f64::NEG_INFINITY, f64::max);
            let book_x = frame.book_x.expect("the book confirmed");
            assert!(
                (observed_end - book_x).abs() < EPSILON,
                "lag {lag_ms} ms, frame {index} at {}: observed depth ends at {observed_end},                  the latest confirmation is at {book_x} on this frame's clock",
                frame.now_ms
            );
        }
    }
}

/// The stretch after the last confirmation is labelled carried and drawn
/// dimmer than the same level observed, so it never passes for observed depth.
#[test]
fn the_carried_stretch_is_labelled_and_dimmer() {
    let mut engine = engine();
    let frames = run(&mut engine, 3_000, 30, 120, |_| true);
    let frame = frames.last().unwrap();
    assert!(
        !frame.carried.is_empty(),
        "the walls are carried to the edge"
    );
    for carried in &frame.carried {
        let observed = frame
            .observed
            .iter()
            .filter(|cell| cell.side == carried.side && cell.price_bucket == carried.price_bucket)
            .max_by(|a, b| a.x1.total_cmp(&b.x1))
            .expect("a carried band continues an observed one");
        assert!(
            (observed.x1 - carried.x0).abs() < EPSILON,
            "no seam between them"
        );
        assert!((carried.x1 - frame.edge_x).abs() < EPSILON);
        assert_eq!(carried.quantity, observed.quantity);
        assert!(
            carried.alpha < observed.alpha,
            "carried {} must read dimmer than observed {}",
            carried.alpha,
            observed.alpha
        );
    }
}

/// A book that stopped confirming is not carried: past the bound the heat
/// stops at the last confirmation and the empty stretch says so.
#[test]
fn a_book_that_stopped_confirming_is_not_carried() {
    let mut engine = engine();
    let stop_ms = 1_999;
    let frames = run(&mut engine, 2_000, 80, 0, |book_ms| book_ms <= stop_ms);
    let last = frames.last().unwrap();
    assert_eq!(last.book_ms, Some(stop_ms));
    assert!(
        frames
            .iter()
            .all(|frame| frame.book_ms.is_none_or(|book| book == stop_ms)),
        "the book never moved past its last confirmation"
    );
    let stale = frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| frame.now_ms - stop_ms > BOOK_CARRY_MAX_MS);
    let mut stale_frames = 0;
    for (index, frame) in stale {
        stale_frames += 1;
        assert!(frame.carried.is_empty(), "frame {index}: nothing carried");
        assert!(frame.heat_end_x < frame.edge_x - EPSILON, "frame {index}");
    }
    assert!(stale_frames > 0, "the run outlived the carry bound");
}

/// A disconnect opens a gap: nothing is carried across it.
#[test]
fn a_gap_is_never_carried_across() {
    let mut engine = engine();
    run(&mut engine, 2_000, 10, 0, |_| true);
    engine.handle_depth_event(DepthEvent::Status {
        symbol: "WINV26".to_owned(),
        generation: GENERATION,
        status: DepthStatus::Disconnected {
            error_class: "transport_closed",
        },
    });
    let frames = run(&mut engine, 2_200, 5, 0, |_| false);
    for (index, frame) in frames.iter().enumerate() {
        assert!(frame.carried.is_empty(), "frame {index}: nothing carried");
        assert!(frame.heat_end_x < frame.edge_x - EPSILON, "frame {index}");
    }
}

/// The live book's cost on one fixed scenario, for the performance gate: a
/// BTC-shaped book (80 levels a side on its 1 USD capture grid, 40 of them
/// changing every 100 ms, five minutes recorded), depth time-averaging on, a two-minute
/// lane, the book stamped 5 ms ahead of the trade-anchored clock, a request
/// and a pane frame every 16 ms for ten seconds. Prints the worker's Project
/// cost per request and the pane's per-frame book work. Ignored: a
/// measurement, not an assertion.
#[test]
#[ignore]
fn bench_the_live_book_on_the_worker_and_the_render_thread() {
    const SYMBOL: &str = "BTCUSDT";
    const LEVELS: i64 = 80;
    const CHANGES: i64 = 40;
    const BEAT_MS: i64 = 100;
    const PRINT_EVERY_MS: i64 = 500;
    const LANE_MS: i64 = 120_000;
    const BAR_MS: i64 = 15_000;
    const AHEAD_MS: i64 = 5;
    const RECORDED_MS: i64 = 300_000;
    const FRAMES: i64 = 600;
    let tick = Decimal::new(1, 1);
    let spacing = Decimal::ONE;
    let mid = Decimal::from(60_000);
    let mut config = HeatmapConfig {
        price_grouping: tick,
        show_depth: true,
        ..config()
    };
    config.live_lane.window = crate::LaneWindow::Fixed { ms: LANE_MS };
    config.live_lane.depth_time_averaging = true;
    let price = |bid: bool, index: i64| {
        if bid {
            mid - spacing * Decimal::from(index + 1)
        } else {
            mid + spacing * Decimal::from(index)
        }
    };
    // Every touch moves a level past the capture's churn band, so each beat
    // cuts a run per touched level, as a busy BTC book does.
    let quantity = |bid: bool, index: i64, high: bool| {
        Decimal::new(
            10 + i64::from(high) * 40 + (index * 13 + i64::from(bid) * 5) % 7,
            1,
        )
    };
    let mut engine = BookEngine::new(SYMBOL);
    engine.set_enabled(true, GENERATION);
    engine.apply_visual_config(config.clone());
    let side = |bid: bool, step: i64| -> Vec<BookLevel> {
        (0..LEVELS)
            .map(|index| BookLevel::new(price(bid, index), quantity(bid, index, step > 0)).unwrap())
            .collect()
    };
    engine.handle_depth_event(DepthEvent::Snapshot {
        symbol: SYMBOL.to_owned(),
        generation: GENERATION,
        observed_at_ms: 0,
        effective_at_ms: 0,
        price_step: Some(tick),
        snapshot: BookSnapshot::new(
            1,
            side(true, 0),
            side(false, 0),
            BookCoverage::Limited {
                levels_per_side: LEVELS as usize,
            },
        ),
    });
    let mut high = vec![false; 2 * LEVELS as usize];
    let mut beat = |engine: &mut BookEngine, step: i64| {
        let (mut bids, mut asks) = (Vec::new(), Vec::new());
        for change in 0..CHANGES {
            let (bid, index) = (change % 2 == 0, (step * 3 + change) % LEVELS);
            let toggle = &mut high[usize::try_from(index + i64::from(bid) * LEVELS).unwrap()];
            *toggle = !*toggle;
            let level = BookLevel::new(price(bid, index), quantity(bid, index, *toggle)).unwrap();
            if bid {
                bids.push(level)
            } else {
                asks.push(level)
            }
        }
        let id = u64::try_from(step + 1).unwrap();
        engine.handle_depth_event(DepthEvent::Update {
            symbol: SYMBOL.to_owned(),
            generation: GENERATION,
            event_time_ms: step * BEAT_MS,
            delta: BookDelta::new(id, id, bids, asks),
        });
        if (step * BEAT_MS) % PRINT_EVERY_MS == 0 {
            engine.record_trade(&Trade {
                agg_id: u64::try_from(step).unwrap(),
                timestamp_ms: step * BEAT_MS - AHEAD_MS,
                price: mid,
                quantity: Decimal::new(1, 2),
                side: if step % 2 == 0 { Side::Buy } else { Side::Sell },
            });
        }
    };
    let mut step = 1;
    while step * BEAT_MS <= RECORDED_MS {
        beat(&mut engine, step);
        step += 1;
    }
    let bar = |open_time: i64, close_time: i64| Bar {
        open_time,
        close_time,
        open: mid,
        high: mid + Decimal::ONE,
        low: mid - Decimal::ONE,
        close: mid,
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    };
    let closed: Vec<Bar> = (0..RECORDED_MS / BAR_MS)
        .map(|index| bar(index * BAR_MS, (index + 1) * BAR_MS - 1))
        .collect();
    let mut pane = crate::pane_tape::PaneTape::default();
    let started = Instant::now();
    let (mut worker, mut render) = (Duration::ZERO, Duration::ZERO);
    let mut live_ms = 0.0_f64;
    let (mut copies, mut cells, mut averaged) = (0, 0, 0);
    for frame in 0..FRAMES {
        let now_ms = RECORDED_MS + frame * FRAME_MS;
        while step * BEAT_MS <= now_ms + AHEAD_MS {
            beat(&mut engine, step);
            step += 1;
        }
        let request = ProjectionRequest {
            timeline_revision: 1,
            first_bar_index: 0,
            closed: closed.clone(),
            partial: Some(bar(RECORDED_MS, now_ms)),
            lane: true,
            on_newest_bar: true,
            lane_reference_ms: Some(BAR_MS),
            lane_now_ms: Some(now_ms),
            price_range: (59_900.0, 60_100.0),
            dot_zoom: Some(DotZoom {
                native_tape: true,
                tape_window_ms: 100,
                tape_level_ticks: 1,
                candle_level_ticks: 1,
                lane_bars: vec![(RECORDED_MS, now_ms)],
            }),
        };
        let cache_now = started + Duration::from_millis(u64::try_from(frame * FRAME_MS).unwrap());
        let began = Instant::now();
        let published = engine
            .project_at(&request, cache_now)
            .expect("the lane projects");
        worker += began.elapsed();
        live_ms += f64::from(engine.last_live_ms);
        let began = Instant::now();
        let shown = pane
            .complete_frame(&config, &request, Some(&published))
            .expect("the pane draws");
        render += began.elapsed();
        copies += usize::from(!Arc::ptr_eq(&shown, &published));
        cells = published.projection.cells.len();
        averaged = published
            .projection
            .cells
            .iter()
            .filter(|cell| cell.time_averaged)
            .count();
    }
    let frames = u32::try_from(FRAMES).unwrap();
    eprintln!(
        "BENCH live_book worker_ms_per_request={:.3} live_half_ms_per_request={:.3} \
         render_us_per_frame={:.2} cells={cells} averaged={averaged} copied_frames={copies} \
         runs={}",
        worker.as_secs_f64() * 1000.0 / f64::from(frames),
        live_ms / f64::from(frames),
        render.as_secs_f64() * 1_000_000.0 / f64::from(frames),
        engine.history.archived_run_count(),
    );
}
