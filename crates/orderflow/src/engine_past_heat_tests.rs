//! The book beside a tape panned into the past: the depth the history kept
//! for that stretch of market time, placed on the tape's own clock, and an
//! honest gap where none was recorded.
use super::*;
use crate::history::RestingSide;
use crate::projection::{BEFORE_CAPTURE, PastTape};
use quantick_engine::{Side, Trade};
use quantick_orderbook::{BookCoverage, BookDelta, BookSnapshot};

const WINDOW_MS: i64 = 30_000;
const DOT_MS: i64 = 100;
const BOOK_STEP_MS: i64 = 1_000;
const GENERATION: u64 = 1;

/// A busy tape: one print every 170 ms for five minutes, walking a few
/// ticks around 100.
fn tape() -> Vec<Trade> {
    (0..1_800_u64)
        .map(|index| {
            let step = i64::try_from(index).unwrap();
            Trade {
                agg_id: index + 1,
                timestamp_ms: 1_000 + step * 170,
                price: Decimal::from(100 + (step * 7 % 11) - (step / 97 % 5)),
                quantity: Decimal::from(1 + step % 4),
                side: if step % 3 == 0 { Side::Sell } else { Side::Buy },
            }
        })
        .collect()
}

fn latest(trades: &[Trade]) -> i64 {
    trades.last().unwrap().timestamp_ms
}

/// The trader's WIN layout: the map off on the candles, on on the tape.
fn config(retention_ms: i64) -> HeatmapConfig {
    HeatmapConfig {
        show_depth: false,
        show_aggressions: true,
        retention_ms,
        live_lane: crate::LiveLaneStyle {
            enabled: true,
            show_depth: true,
            show_aggressions: true,
            native_tape: true,
            window: crate::LaneWindow::Fixed { ms: WINDOW_MS },
            ..Default::default()
        },
        volume_dots: crate::config::VolumeDotStyle {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A recorded session: a bid wall at 95 and an ask wall at 108 standing the
/// whole time, the book's clock ticking every second between the prints.
fn engine(trades: &[Trade], retention_ms: i64) -> BookEngine {
    let mut engine = BookEngine::new("WINV26");
    engine.set_enabled(true, GENERATION);
    engine.apply_visual_config(config(retention_ms));
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
    let (mut update_id, mut book_ms) = (11, BOOK_STEP_MS);
    for trade in trades {
        while book_ms <= trade.timestamp_ms {
            engine.handle_depth_event(DepthEvent::Update {
                symbol: "WINV26".to_owned(),
                generation: GENERATION,
                event_time_ms: book_ms,
                delta: BookDelta::new(update_id, update_id, Vec::new(), Vec::new()),
            });
            update_id += 1;
            book_ms += BOOK_STEP_MS;
        }
        engine.record_trade(trade);
    }
    engine
}

fn request(trades: &[Trade]) -> ProjectionRequest {
    let now = latest(trades);
    let mut bar = Bar::opened_by(&trades[0]);
    bar.close_time = now;
    ProjectionRequest {
        timeline_revision: 0,
        first_bar_index: 0,
        closed: Vec::new(),
        partial: Some(bar),
        lane: true,
        on_newest_bar: true,
        lane_reference_ms: Some(6_000),
        lane_now_ms: Some(now),
        price_range: (90.0, 115.0),
        dot_zoom: Some(DotZoom {
            native_tape: true,
            tape_window_ms: DOT_MS,
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            lane_bars: vec![(1_000, now)],
        }),
    }
}

/// Pan the tape to `end_ms`, and return the past tape and the frame's slots.
fn panned(engine: &mut BookEngine, trades: &[Trade], end_ms: i64) -> (Arc<PastTape>, usize) {
    engine.set_tape_end(Some(end_ms));
    let frame = engine
        .project_at(&request(trades), Instant::now())
        .expect("the tape projects");
    let past = engine
        .published()
        .past_tape
        .expect("a past window was asked for");
    (past, frame.slot_count)
}

/// Where the lane opens on a frame of `slots` regions.
fn lane_from(slots: usize) -> f64 {
    (slots as f64 - 1.0) / slots as f64
}

/// Trader 2026-10-07, WIN: dragging the tape into the past took the heatmap
/// bands off it and left the bubbles alone on black. The history still held
/// the book for that stretch; the past tape just never drew it.
#[test]
fn a_tape_panned_into_the_past_keeps_the_book_it_stood_beside() {
    let trades = tape();
    let mut engine = engine(&trades, crate::config::DEFAULT_RETENTION_MS);
    let end_ms = latest(&trades) - 4 * WINDOW_MS - 1_234;
    let (past, slots) = panned(&mut engine, &trades, end_ms);
    let heat = past.heat.placed(end_ms, past.window_ms, slots);
    let lane = lane_from(slots);

    assert!(
        !heat.cells.is_empty(),
        "the recorded book is drawn beside the past tape"
    );
    assert!(
        heat.cells
            .iter()
            .all(|cell| cell.x0 >= lane - 1e-9 && cell.x1 <= 1.0 + 1e-9),
        "every band sits on the tape, none on the candles"
    );
    for side in [RestingSide::Bid, RestingSide::Ask] {
        let cells: Vec<_> = heat.cells.iter().filter(|cell| cell.side == side).collect();
        let from = cells.iter().map(|cell| cell.x0).fold(f64::INFINITY, f64::min);
        let to = cells.iter().map(|cell| cell.x1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (from - lane).abs() < 1e-9 && (to - 1.0).abs() < 1e-9,
            "the {side:?} wall stood the whole visible window: {from}..{to}"
        );
        assert!(cells.iter().all(|cell| cell.intensity > 0.0));
    }
    assert!(heat.gaps.is_empty(), "the book was recorded the whole time");
}

/// The same pan a few seconds later shows the same book at the same place on
/// the tape's own clock: bands move with the prints, never with the frame.
#[test]
fn past_bands_follow_the_tape_clock() {
    let trades = tape();
    let mut engine = engine(&trades, crate::config::DEFAULT_RETENTION_MS);
    let end_ms = latest(&trades) - 4 * WINDOW_MS;
    let (past, slots) = panned(&mut engine, &trades, end_ms);
    let first = past.heat.placed(end_ms, past.window_ms, slots);
    let again = past.heat.placed(end_ms, past.window_ms, slots);
    assert_eq!(first, again, "placing is a pure function of the clock");
    let shifted = past.heat.placed(end_ms + 1_000, past.window_ms, slots);
    assert!(
        shifted
            .cells
            .iter()
            .all(|cell| cell.x0 >= lane_from(slots) - 1e-9 && cell.x1 <= 1.0 + 1e-9),
        "a moved clock still clips to the tape"
    );
}

/// Before the retained book there is nothing to draw, and the tape says so
/// instead of reading as an empty book.
#[test]
fn a_past_tape_before_the_retained_book_says_so() {
    let trades = tape();
    let retention_ms = 3 * WINDOW_MS;
    let mut engine = engine(&trades, retention_ms);
    // Half the window before the retained book, half after it.
    let retained_from = engine
        .history
        .retention_start_ms()
        .expect("the book was recorded");
    let end_ms = retained_from + WINDOW_MS / 2;
    let (past, slots) = panned(&mut engine, &trades, end_ms);
    let heat = past.heat.placed(end_ms, past.window_ms, slots);
    let lane = lane_from(slots);
    let middle = f64::midpoint(lane, 1.0);

    let gap = heat
        .gaps
        .iter()
        .find(|gap| gap.reason == BEFORE_CAPTURE)
        .expect("the unrecorded stretch is labelled");
    assert!((gap.x0 - lane).abs() < 1e-9, "from where the tape opens");
    assert!(
        (gap.x1 - middle).abs() < 0.03,
        "to where the retained book begins: {}",
        gap.x1
    );
    assert!(
        heat.cells.iter().all(|cell| cell.x0 >= gap.x1 - 1e-9),
        "and no band is invented before it"
    );
    assert!(!heat.cells.is_empty(), "the retained half is drawn");
}
