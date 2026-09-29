//! The native tape beside the candles is built exactly as the tape-only pane
//! built it: processing follows the native switch, never the pane's width.
use super::*;
use quantick_engine::{Side, Trade};

fn print(agg_id: u64, timestamp_ms: i64, price: i64, quantity: i64, side: Side) -> Trade {
    Trade {
        agg_id,
        timestamp_ms,
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
        side,
    }
}

/// `native_tape` and `tape_only` set as asked, everything else the WIN tape.
fn engine(native_tape: bool, tape_only: bool) -> BookEngine {
    let mut engine = BookEngine::new("WINV26");
    engine.apply_visual_config(HeatmapConfig {
        show_aggressions: true,
        live_lane: crate::LiveLaneStyle {
            enabled: true,
            show_depth: false,
            show_aggressions: true,
            native_tape,
            tape_only,
            window: crate::LaneWindow::Fixed { ms: 30_000 },
            ..Default::default()
        },
        volume_dots: crate::config::VolumeDotStyle {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    });
    for (agg_id, timestamp_ms, price, quantity, side) in [
        (1, 1_000, 90, 1, Side::Buy),
        (2, 5_000, 110, 2, Side::Sell),
        (3, 25_000, 100, 3, Side::Buy),
        (4, 25_040, 100, 5, Side::Sell),
        (5, 30_000, 101, 4, Side::Buy),
    ] {
        engine.record_trade(&print(agg_id, timestamp_ms, price, quantity, side));
    }
    engine
}

/// The visible candles are `closed` and, when on screen, the forming bar.
fn request(closed: Vec<Bar>, partial: Option<Bar>, now_ms: i64) -> ProjectionRequest {
    let on_newest_bar = partial.is_some();
    ProjectionRequest {
        timeline_revision: 0,
        first_bar_index: 0,
        closed,
        partial,
        lane: true,
        on_newest_bar,
        lane_reference_ms: Some(6_000),
        lane_now_ms: Some(now_ms),
        price_range: (85.0, 115.0),
        dot_zoom: Some(DotZoom {
            native_tape: true,
            tape_window_ms: 100,
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            lane_bars: vec![(24_000, 30_000)],
        }),
    }
}

fn forming() -> Bar {
    let mut partial = Bar::opened_by(&print(3, 24_000, 100, 3, Side::Buy));
    partial.close_time = 30_000;
    partial
}

fn closed() -> Vec<Bar> {
    let mut first = Bar::opened_by(&print(1, 1_000, 90, 1, Side::Buy));
    first.close_time = 4_999;
    let mut second = Bar::opened_by(&print(2, 5_000, 110, 2, Side::Sell));
    second.close_time = 23_999;
    vec![first, second]
}

fn live(frame: &VisibleOrderflow) -> Vec<AggressionPrimitive> {
    frame
        .projection
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .cloned()
        .collect()
}

#[test]
fn the_native_tape_beside_the_candles_projects_what_tape_only_projected() {
    let now = std::time::Instant::now();
    let mut tape_only = engine(false, true);
    let mut beside = engine(true, false);
    for clock in [30_000, 30_016, 42_000] {
        let input = request(closed(), Some(forming()), clock);
        let expected = tape_only.project_at(&input, now).expect("the tape projects");
        let actual = beside.project_at(&input, now).expect("the tape projects");
        assert_eq!(
            actual.live_edge, expected.live_edge,
            "the native tape runs on the supplied clock"
        );
        assert_eq!(actual.live_edge.map(|edge| edge.now_ms), Some(clock));
        assert_eq!(*actual.projection, *expected.projection, "at {clock}");
        assert_eq!(actual.volume_dots, expected.volume_dots);
        assert_eq!(actual.slot_count, expected.slot_count);
    }
    assert!(
        beside
            .project_at(&request(closed(), Some(forming()), 30_000), now)
            .unwrap()
            .projection
            .aggressions
            .iter()
            .all(|mark| mark.live),
        "the native tape keys no candle-slot mark"
    );
}

/// Panning the candles into history changes which bars are on screen, and
/// nothing on the tape: its prints, their memberships, their execution
/// coordinates and the prices the axis fits to.
#[test]
fn panning_the_candles_leaves_the_native_tape_unchanged() {
    let now = std::time::Instant::now();
    let mut following = engine(true, false);
    let mut panned = engine(true, false);
    let at_live = following
        .project_at(&request(closed(), Some(forming()), 30_000), now)
        .expect("the tape projects");
    let in_history = panned
        .project_at(&request(closed()[..1].to_vec(), None, 30_000), now)
        .expect("the tape projects while the candles look back");
    let tape = live(&at_live);
    assert!(!tape.is_empty());
    let panned_tape = live(&in_history);
    assert_eq!(
        panned_tape.len(),
        tape.len(),
        "no print leaves or joins the tape"
    );
    for (panned, following) in panned_tape.iter().zip(&tape) {
        assert_eq!(panned.agg_ids, following.agg_ids);
        assert_eq!(panned.quantity, following.quantity);
        assert_eq!(panned.buy_quantity, following.buy_quantity);
        assert_eq!(panned.price, following.price);
        assert_eq!(panned.timestamp_quantity, following.timestamp_quantity);
    }
    assert_eq!(
        crate::projection::tape_price_range(&in_history.projection.aggressions),
        crate::projection::tape_price_range(&at_live.projection.aggressions),
        "the shared axis keeps fitting the same tape"
    );
    assert_eq!(
        in_history.projection.tape_facts, at_live.projection.tape_facts,
        "the tape's facts do not depend on the candles on screen"
    );
}
