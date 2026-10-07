//! The book beside a tape panned into the past: the depth the history kept
//! for that stretch of market time, placed on the tape's own clock, and an
//! honest gap where none was recorded.
use super::*;
use crate::history::RestingSide;
use crate::projection::{BEFORE_CAPTURE, BOOK_PENDING, PastTape, PlacedPastHeat};
use quantick_engine::{Side, Trade};
use quantick_orderbook::{BookCoverage, BookDelta, BookSnapshot};

const WINDOW_MS: i64 = 30_000;
const DOT_MS: i64 = 100;
const BOOK_STEP_MS: i64 = 1_000;
const GENERATION: u64 = 1;
const EPSILON: f64 = 1e-9;

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

/// The book's clock: the next update id and the next tick.
struct BookClock {
    update_id: u64,
    book_ms: i64,
}

/// Record `trades`, the book's clock ticking every second between them.
fn record(engine: &mut BookEngine, clock: &mut BookClock, trades: &[Trade]) {
    for trade in trades {
        while clock.book_ms <= trade.timestamp_ms {
            update(engine, clock, Vec::new());
        }
        engine.record_trade(trade);
    }
}

/// One book tick, changing `bids`.
fn update(engine: &mut BookEngine, clock: &mut BookClock, bids: Vec<BookLevel>) {
    engine.handle_depth_event(DepthEvent::Update {
        symbol: "WINV26".to_owned(),
        generation: GENERATION,
        event_time_ms: clock.book_ms,
        delta: BookDelta::new(clock.update_id, clock.update_id, bids, Vec::new()),
    });
    clock.update_id += 1;
    clock.book_ms += BOOK_STEP_MS;
}

/// A recorded session: a bid wall at 95 and an ask wall at 108 standing the
/// whole time, the book's clock ticking every second between the prints.
fn engine(trades: &[Trade], retention_ms: i64) -> BookEngine {
    session(trades, retention_ms).0
}

fn session(trades: &[Trade], retention_ms: i64) -> (BookEngine, BookClock) {
    session_with(trades, config(retention_ms))
}

fn session_with(trades: &[Trade], config: HeatmapConfig) -> (BookEngine, BookClock) {
    let mut engine = BookEngine::new("WINV26");
    engine.set_enabled(true, GENERATION);
    engine.apply_visual_config(config);
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
    let mut clock = BookClock {
        update_id: 11,
        book_ms: BOOK_STEP_MS,
    };
    record(&mut engine, &mut clock, trades);
    (engine, clock)
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

/// The bands of one side.
fn side(heat: &HeatmapProjection, side: RestingSide) -> Vec<&crate::projection::HeatmapCell> {
    heat.cells.iter().filter(|cell| cell.side == side).collect()
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
        heat.cells
            .iter()
            .all(|cell| cell.x0 >= lane - EPSILON && cell.x1 <= 1.0 + EPSILON),
        "every band sits on the tape, none on the candles"
    );
    for wall in [RestingSide::Bid, RestingSide::Ask] {
        let cells = side(&heat, wall);
        assert!(!cells.is_empty(), "the {wall:?} wall is drawn");
        let from = cells
            .iter()
            .map(|cell| cell.x0)
            .fold(f64::INFINITY, f64::min);
        let to = cells
            .iter()
            .map(|cell| cell.x1)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (from - lane).abs() < EPSILON && (to - 1.0).abs() < EPSILON,
            "the {wall:?} wall stood the whole visible window: {from}..{to}"
        );
        assert!(cells.iter().all(|cell| cell.intensity > 0.0));
    }
    assert!(heat.gaps.is_empty(), "the book was recorded the whole time");
}

/// The tape's clock moving one second moves every band one second's share
/// of the lane to the left: the bands ride with the prints, never with the
/// frame.
#[test]
fn past_bands_follow_the_tape_clock() {
    let trades = tape();
    let end_ms = latest(&trades) - 4 * WINDOW_MS;
    // A level that stood five seconds in the middle of the held window.
    let split = trades
        .iter()
        .position(|trade| trade.timestamp_ms > end_ms - WINDOW_MS / 2)
        .unwrap();
    let (head, tail) = trades.split_at(split);
    let (mut engine, mut clock) = session(head, crate::config::DEFAULT_RETENTION_MS);
    let level =
        |quantity: i64| vec![BookLevel::new(Decimal::from(97), Decimal::from(quantity)).unwrap()];
    update(&mut engine, &mut clock, level(20));
    record(&mut engine, &mut clock, &tail[..30]);
    update(&mut engine, &mut clock, level(0));
    record(&mut engine, &mut clock, &tail[30..]);

    let (past, slots) = panned(&mut engine, &trades, end_ms);
    let (lane, width) = (lane_from(slots), 1.0 / slots as f64);
    let first = past.heat.placed(end_ms, past.window_ms, slots);
    assert_eq!(
        first,
        past.heat.placed(end_ms, past.window_ms, slots),
        "placing is a pure function of the clock"
    );
    let shifted = past.heat.placed(end_ms + 1_000, past.window_ms, slots);
    let shift = 1_000.0 / past.window_ms as f64 * width;

    let brief = |heat: &HeatmapProjection| -> Vec<(f64, f64)> {
        heat.cells
            .iter()
            .filter(|cell| cell.quantity == Decimal::from(20))
            .map(|cell| (cell.x0, cell.x1))
            .collect()
    };
    let (before, after) = (brief(&first), brief(&shifted));
    assert_eq!(before.len(), 1, "the brief level is one band: {before:?}");
    assert!(
        before[0].0 > lane + shift && before[0].1 < 1.0,
        "inside the window"
    );
    assert_eq!(after.len(), 1);
    assert!(
        (after[0].0 - (before[0].0 - shift)).abs() < EPSILON
            && (after[0].1 - (before[0].1 - shift)).abs() < EPSILON,
        "one second of the clock moves the band one second's share of the lane:          {before:?} -> {after:?}, by {shift}"
    );
    assert!(
        shifted
            .cells
            .iter()
            .all(|cell| cell.x0 >= lane - EPSILON && cell.x1 <= 1.0 + EPSILON),
        "a moved clock still clips to the tape"
    );
}

/// The past is coloured against its own book and held: the live book
/// growing a wall far bigger than anything in the held window neither dims
/// the past bands nor makes the engine read them again.
#[test]
fn a_held_past_keeps_its_own_scale_and_reads_its_book_once() {
    let trades = tape();
    let (head, tail) = trades.split_at(1_200);
    let (mut engine, mut clock) = session(head, crate::config::DEFAULT_RETENTION_MS);
    let end_ms = latest(head) - 3 * WINDOW_MS;
    let (before, slots) = panned(&mut engine, head, end_ms);
    assert_eq!(
        before.heat.liquidity_reference,
        Decimal::from(50),
        "the strongest wall the held window saw"
    );

    record(&mut engine, &mut clock, &tail[..300]);
    update(
        &mut engine,
        &mut clock,
        vec![BookLevel::new(Decimal::from(95), Decimal::from(5_000)).unwrap()],
    );
    record(&mut engine, &mut clock, &tail[300..]);
    let (after, _) = panned(&mut engine, head, end_ms);

    assert!(
        Arc::ptr_eq(&before.heat, &after.heat),
        "nothing the held window was read from changed, so nothing is read again"
    );
    let heat = after.heat.placed(end_ms, after.window_ms, slots);
    let bid = side(&heat, RestingSide::Bid);
    assert!(!bid.is_empty());
    assert!(
        bid.iter().all(|cell| (cell.intensity - 1.0).abs() < 1e-6),
        "the held wall still reads full against its own book"
    );

    // Its own inputs moving does read it again.
    engine.set_tape_end(Some(end_ms));
    let mut zoomed = request(head);
    zoomed.price_range = (92.0, 112.0);
    engine
        .project_at(&zoomed, Instant::now())
        .expect("projects");
    let rescaled = engine.published().past_tape.expect("still held");
    assert!(!Arc::ptr_eq(&before.heat, &rescaled.heat));
}

/// A drag outruns the blocks the engine published: the stretch of the window
/// they do not reach is labelled as not read yet, never left as an empty
/// book. With no blocks at all the whole held window is.
#[test]
fn a_drag_past_the_published_book_is_labelled_pending() {
    let trades = tape();
    let mut engine = engine(&trades, crate::config::DEFAULT_RETENTION_MS);
    let end_ms = latest(&trades) - 4 * WINDOW_MS;
    let (past, slots) = panned(&mut engine, &trades, end_ms);
    let lane = lane_from(slots);
    let pending = |heat: &HeatmapProjection| -> Vec<(f64, f64)> {
        heat.gaps
            .iter()
            .filter(|gap| gap.reason == BOOK_PENDING)
            .map(|gap| (gap.x0, gap.x1))
            .collect()
    };
    assert!(
        pending(&past.heat.placed(end_ms, past.window_ms, slots)).is_empty(),
        "the published blocks cover the window they were read for"
    );

    let ahead = past.heat.until_ms + WINDOW_MS / 2;
    let outrun = pending(&past.heat.placed(ahead, past.window_ms, slots));
    let start = ahead - past.window_ms;
    let covered_to =
        lane + (past.heat.until_ms - start) as f64 / past.window_ms as f64 / slots as f64;
    assert_eq!(outrun.len(), 1, "{outrun:?}");
    assert!((outrun[0].0 - covered_to).abs() < EPSILON && (outrun[0].1 - 1.0).abs() < EPSILON);

    let mut placed = PlacedPastHeat::default();
    let unread = placed.place(
        None,
        (end_ms, past.window_ms, slots),
        past.heat.effective_grouping,
    );
    assert_eq!(pending(&unread), vec![(lane, 1.0)]);
    assert!(unread.cells.is_empty());
}

/// The painter and the cursor ask every UI frame; the same book on the same
/// clock is placed once.
#[test]
fn the_same_book_on_the_same_clock_is_placed_once() {
    let trades = tape();
    let mut engine = engine(&trades, crate::config::DEFAULT_RETENTION_MS);
    let end_ms = latest(&trades) - 4 * WINDOW_MS;
    let (past, slots) = panned(&mut engine, &trades, end_ms);
    let grouping = past.heat.effective_grouping;
    let mut placed = PlacedPastHeat::default();
    let clock = (end_ms, past.window_ms, slots);
    let first = placed.place(Some(&past.heat), clock, grouping);
    assert!(Arc::ptr_eq(
        &first,
        &placed.place(Some(&past.heat), clock, grouping)
    ));
    assert_eq!(*first, past.heat.placed(end_ms, past.window_ms, slots));

    let moved = placed.place(
        Some(&past.heat),
        (end_ms + 1, past.window_ms, slots),
        grouping,
    );
    assert!(!Arc::ptr_eq(&first, &moved), "a moved clock places again");
    let republished = Arc::new((*past.heat).clone());
    let again = placed.place(
        Some(&republished),
        (end_ms + 1, past.window_ms, slots),
        grouping,
    );
    assert!(!Arc::ptr_eq(&moved, &again), "a new book places again");
}

/// The tape held at the live edge draws the same bands the live lane does:
/// one rule cuts both, so the two cannot drift apart.
#[test]
fn a_tape_held_at_the_edge_draws_the_live_lanes_bands() {
    let trades = tape();
    let mut engine = engine(&trades, crate::config::DEFAULT_RETENTION_MS);
    engine.set_tape_end(None);
    let live = engine
        .project_at(&request(&trades), Instant::now())
        .expect("the tape projects");
    let lane = lane_from(live.slot_count);
    let rows = |cells: &mut dyn Iterator<Item = &crate::projection::HeatmapCell>| {
        let mut rows: Vec<_> = cells
            .map(|cell| {
                (
                    cell.side,
                    cell.price_bucket,
                    cell.y0.to_bits(),
                    cell.y1.to_bits(),
                )
            })
            .collect();
        rows.sort();
        rows.dedup();
        rows
    };
    let live_rows = rows(
        &mut live
            .projection
            .cells
            .iter()
            .filter(|cell| cell.x0 >= lane - EPSILON),
    );
    let end_ms = latest(&trades);
    let (past, slots) = panned(&mut engine, &trades, end_ms);
    let held = past.heat.placed(end_ms, past.window_ms, slots);
    assert!(!live_rows.is_empty());
    assert_eq!(rows(&mut held.cells.iter()), live_rows);
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
    assert!((gap.x0 - lane).abs() < EPSILON, "from where the tape opens");
    assert!(
        (gap.x1 - middle).abs() < 0.03,
        "to where the retained book begins: {}",
        gap.x1
    );
    assert!(
        heat.cells.iter().all(|cell| cell.x0 >= gap.x1 - EPSILON),
        "and no band is invented before it"
    );
    assert!(!heat.cells.is_empty(), "the retained half is drawn");
}

/// Trader 2026-10-07: the tape alone with volume dots off, held in the past,
/// read "L2 loading" forever: the past was projected only for volume dots.
/// The held book belongs to the tape, dots or not, and the tape still draws
/// no past marks without them.
#[test]
fn a_held_tape_without_volume_dots_still_draws_its_book() {
    let trades = tape();
    let mut tape_only = config(crate::config::DEFAULT_RETENTION_MS);
    tape_only.live_lane.tape_only = true;
    tape_only.volume_dots.enabled = false;
    assert!(tape_only.native_tape());
    let (mut engine, _) = session_with(&trades, tape_only);
    let end_ms = latest(&trades) - 4 * WINDOW_MS;
    let (past, slots) = panned(&mut engine, &trades, end_ms);
    assert!(
        past.projection.aggressions.is_empty(),
        "no volume dots, no past marks"
    );
    let heat = PlacedPastHeat::default().place(
        Some(&past.heat),
        (end_ms, past.window_ms, slots),
        past.heat.effective_grouping,
    );
    assert!(
        heat.gaps.iter().all(|gap| gap.reason != BOOK_PENDING),
        "the book was read, not left pending: {:?}",
        heat.gaps
    );
    for wall in [RestingSide::Bid, RestingSide::Ask] {
        assert!(!side(&heat, wall).is_empty(), "the {wall:?} wall is drawn");
    }
}
