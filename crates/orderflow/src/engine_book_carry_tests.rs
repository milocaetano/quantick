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
