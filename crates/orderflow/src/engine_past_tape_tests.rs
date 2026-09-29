//! The native tape panned into the past: its own pipeline cut to a bounded
//! stretch of retained prints, frozen on the window's own grid, and never a
//! change to the live frame beside it.
use super::*;
use crate::projection::{
    AggressionPrimitive, DotSizing, PastTape, PastTapeMemory, TapeDotGeometry, TapeDotView,
    past_block_ms,
};
use quantick_engine::{Side, Trade};

const WINDOW_MS: i64 = 30_000;
const DOT_MS: i64 = 100;

fn print(agg_id: u64, timestamp_ms: i64, price: i64, quantity: i64, side: Side) -> Trade {
    Trade {
        agg_id,
        timestamp_ms,
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
        side,
    }
}

/// A deterministic, busy WIN-like tape: one print every 170 ms for five
/// minutes, prices walking a few ticks, sizes and sides varied.
fn tape() -> Vec<Trade> {
    (0..1_800_u64)
        .map(|index| {
            let step = i64::try_from(index).unwrap();
            let price = 100 + (step * 7 % 11) - (step / 97 % 5);
            let side = if step % 3 == 0 { Side::Sell } else { Side::Buy };
            print(index + 1, 1_000 + step * 170, price, 1 + step % 4, side)
        })
        .collect()
}

fn engine(trades: &[Trade]) -> BookEngine {
    let mut engine = BookEngine::new("WINV26");
    engine.apply_visual_config(HeatmapConfig {
        show_aggressions: true,
        live_lane: crate::LiveLaneStyle {
            enabled: true,
            show_depth: false,
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
    });
    for trade in trades {
        engine.record_trade(trade);
    }
    engine
}

fn latest(trades: &[Trade]) -> i64 {
    trades.last().unwrap().timestamp_ms
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

fn past_tape(trades: &[Trade], end_ms: i64) -> Arc<PastTape> {
    let mut engine = engine(trades);
    engine.set_tape_end(Some(end_ms));
    engine
        .project_at(&request(trades), Instant::now())
        .expect("the tape projects");
    engine
        .published()
        .past_tape
        .expect("a past window was asked for")
}

fn sizing() -> DotSizing {
    DotSizing {
        native_tape: true,
        tape_column_px: 1.0,
        candle_column_px: 8.0,
        px_per_price: 20.0,
        typed_full: None,
    }
}

fn view(end_ms: i64, tape: &PastTape) -> TapeDotView {
    TapeDotView {
        now_ms: end_ms,
        window_ms: tape.window_ms,
        dot_window_ms: DOT_MS,
        evicted_through_ms: None,
        prices: crate::projection::PriceWindow::new(Decimal::from(90), Decimal::from(115)).unwrap(),
        geometry: TapeDotGeometry {
            left_x: 0.0,
            right_x: 1.0,
            width_px: 300.0,
            height_px: 500.0,
        },
    }
}

fn frame(memory: &mut PastTapeMemory, tape: &PastTape, end_ms: i64) -> Vec<AggressionPrimitive> {
    let style = crate::config::theme::OrderflowRenderStyle::from_config(
        &HeatmapConfig::default(),
        [0, 0, 0, 255],
    );
    memory
        .project(
            &tape.projection.aggressions,
            tape,
            view(end_ms, tape),
            sizing(),
            &style.bubbles,
            &style.live_lane,
            &[],
        )
        .marks
}

/// The facts a mark stands for, without its screen position or size.
fn membership(mark: &AggressionPrimitive) -> (i64, i64, Decimal, Decimal, usize) {
    (
        mark.first_timestamp_ms,
        mark.last_timestamp_ms,
        mark.price,
        mark.quantity,
        mark.trade_count,
    )
}

#[test]
fn asking_for_a_past_tape_never_changes_the_live_frame() {
    let trades = tape();
    let now = Instant::now();
    let mut live = engine(&trades);
    let mut panned = engine(&trades);
    panned.set_tape_end(Some(latest(&trades) - 90_000));
    let input = request(&trades);
    let expected = live.project_at(&input, now).unwrap();
    let actual = panned.project_at(&input, now).unwrap();
    assert_eq!(*actual.projection, *expected.projection);
    assert_eq!(actual.live_edge, expected.live_edge);
    assert_eq!(actual.volume_dots, expected.volume_dots);
    assert!(
        live.published().past_tape.is_none(),
        "live asks for no past"
    );
    let past = panned.published().past_tape.expect("the past tape");
    assert_eq!(past.end_ms, latest(&trades) - 90_000);
    panned.set_tape_end(None);
    panned.project_at(&input, now).unwrap();
    assert!(
        panned.published().past_tape.is_none(),
        "back at live the past tape is gone"
    );
}

/// Every print of the blocks the window touches, and nothing else: the
/// block-aligned stretch around the window, with exact quantities.
#[test]
fn a_past_tape_holds_exactly_the_prints_of_its_blocks() {
    let trades = tape();
    let end = latest(&trades) - 120_000;
    let past = past_tape(&trades, end);
    let block = past_block_ms(WINDOW_MS, DOT_MS);
    assert_eq!(past.block_ms, block);
    assert_eq!(past.window_ms, WINDOW_MS);
    assert_eq!(past.from_ms, (end - WINDOW_MS).div_euclid(block) * block);
    assert_eq!(past.until_ms, (end.div_euclid(block) + 1) * block);
    let expected: Decimal = trades
        .iter()
        .filter(|trade| (past.from_ms..past.until_ms).contains(&trade.timestamp_ms))
        .map(|trade| trade.quantity)
        .sum();
    let marks = &past.projection.aggressions;
    assert!(marks.iter().all(|mark| mark.live));
    assert_eq!(
        marks.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        expected
    );
    assert!(
        marks
            .iter()
            .all(|mark| mark.first_timestamp_ms >= past.from_ms
                && mark.last_timestamp_ms < past.until_ms)
    );
    assert_eq!(past.retained_from_ms, Some(1_000));
    assert_eq!(
        past.settled_through_ms,
        latest(&trades) - WINDOW_MS,
        "a block settles once a window has passed since its end"
    );
}

#[test]
fn the_same_data_and_window_give_the_same_past_marks() {
    let trades = tape();
    let end = latest(&trades) - 100_000;
    let (first, second) = (past_tape(&trades, end), past_tape(&trades, end));
    assert_eq!(first.projection, second.projection);
    let a = frame(&mut PastTapeMemory::default(), &first, end);
    let b = frame(&mut PastTapeMemory::default(), &second, end);
    assert!(!a.is_empty());
    assert_eq!(a, b);
}

/// Panning moves marks and never regroups them: a mark visible at both ends
/// holds the same prints, shifted by exactly the pan — within a block and
/// across one — and a memory that panned there agrees with one that started
/// there.
#[test]
fn panning_the_past_moves_marks_without_regrouping_them() {
    let trades = tape();
    // The window starts 2 s into a block: the first pan stays inside it,
    // the second crosses into the block before.
    let block = past_block_ms(WINDOW_MS, DOT_MS);
    let start = 6 * block + 2_000;
    assert!(start < latest(&trades) - 2 * WINDOW_MS);
    for pan in [1_300, 20_000] {
        let end = start - pan;
        let (here, there) = (past_tape(&trades, start), past_tape(&trades, end));
        let mut panned = PastTapeMemory::default();
        let before = frame(&mut panned, &here, start);
        let after = frame(&mut panned, &there, end);
        let fresh = frame(&mut PastTapeMemory::default(), &there, end);
        assert_eq!(after, fresh, "frozen past: order of visits is irrelevant");
        let shift = pan as f64 / WINDOW_MS as f64;
        let mut shared = 0;
        for mark in &before {
            if let Some(moved) = after
                .iter()
                .find(|other| membership(other) == membership(mark))
            {
                shared += 1;
                assert!(
                    (moved.x - (mark.x + shift)).abs() < 1e-9,
                    "{} vs {} + {shift}",
                    moved.x,
                    mark.x
                );
            }
        }
        let overlap = before.iter().filter(|mark| mark.x + shift <= 1.0).count();
        assert_eq!(shared, overlap, "every overlapping mark kept its prints");
        assert!(shared > 10, "the fixture overlaps: {shared}");
    }
}

/// The past is bounded by the window: a stretch an hour back walks only its
/// own prints (plus a window of late-delivery grace), never the hour after it.
#[test]
fn the_past_walk_reads_its_window_and_not_the_history_after_it() {
    let mut history = LiquidityHistory::new(HeatmapConfig::default());
    for trade in tape() {
        history.record_aggression(&trade);
    }
    // A print delivered late: executed at 20_050, arriving after 60_000.
    history.record_aggression(&print(9_999, 20_050, 101, 3, Side::Buy));
    let from = 20_000;
    let reach = 30_000;
    let walked: Vec<_> = history
        .aggressions_between(from, Some(reach))
        .map(|trade| trade.timestamp_ms)
        .collect();
    assert!(walked.iter().all(|time| *time < reach + 170));
    assert!(walked.len() < 70, "{} prints walked", walked.len());
    assert!(
        !walked.contains(&20_050),
        "delivered after the tape passed `reach`"
    );
    assert!(
        history
            .aggressions_between(from, None)
            .any(|trade| trade.timestamp_ms == 20_050),
        "without a reach the walk runs to the newest delivery"
    );
}

#[test]
fn retention_is_reported_where_the_first_complete_print_is() {
    let mut config = HeatmapConfig::default();
    config.max_aggressions = 100;
    let mut history = LiquidityHistory::new(config);
    assert_eq!(history.tape_retained_from_ms(), None);
    for trade in tape().into_iter().take(300) {
        history.record_aggression(&trade);
    }
    let evicted = history
        .evicted_through_ms()
        .expect("the cap evicted prints");
    assert_eq!(history.tape_retained_from_ms(), Some(evicted + 1));
    let fresh = {
        let mut history = LiquidityHistory::new(HeatmapConfig::default());
        history.record_aggression(&tape()[3]);
        history.tape_retained_from_ms()
    };
    assert_eq!(fresh, Some(tape()[3].timestamp_ms));
}
