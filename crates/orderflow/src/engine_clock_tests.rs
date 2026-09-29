//! Projection time is an explicit input only for the independent tape mode.
use super::*;
use quantick_engine::{Side, Trade};

fn print() -> Trade {
    Trade {
        agg_id: 1,
        timestamp_ms: 1_000,
        price: Decimal::from(100),
        quantity: Decimal::from(10),
        side: Side::Buy,
    }
}

fn request(now_ms: Option<i64>) -> ProjectionRequest {
    ProjectionRequest {
        timeline_revision: 0,
        first_bar_index: 0,
        closed: Vec::new(),
        partial: Some(Bar::opened_by(&print())),
        lane: true,
        on_newest_bar: true,
        lane_reference_ms: Some(10_000),
        lane_now_ms: now_ms,
        price_range: (90.0, 110.0),
        dot_zoom: None,
    }
}

fn engine(tape_only: bool) -> BookEngine {
    let mut engine = BookEngine::new("WINV26");
    let config = HeatmapConfig {
        show_aggressions: true,
        live_lane: crate::LiveLaneStyle {
            enabled: true,
            show_aggressions: true,
            tape_only,
            ..Default::default()
        },
        ..Default::default()
    };
    engine.apply_visual_config(config);
    engine.record_trade(&print());
    engine
}

#[test]
fn tape_only_projection_uses_the_callers_now_between_market_events() {
    let engine = engine(true);
    assert_eq!(
        engine.live_edge(&request(Some(1_016))).unwrap().now_ms,
        1_016
    );
    assert_eq!(
        engine.live_edge(&request(Some(2_200))).unwrap().now_ms,
        2_200
    );
    assert_eq!(engine.live_edge(&request(Some(900))).unwrap().now_ms, 1_000);
}

#[test]
fn ordinary_btc_style_lane_keeps_its_event_anchored_clock() {
    let engine = engine(false);
    assert_eq!(
        engine.live_edge(&request(Some(2_200))).unwrap().now_ms,
        1_000
    );
    assert_eq!(engine.live_edge(&request(None)).unwrap().now_ms, 1_000);
}

#[test]
fn a_published_frame_carries_the_clock_its_positions_were_projected_against() {
    let mut engine = engine(true);
    let input = request(Some(2_000));
    let frame = engine
        .project_at(&input, std::time::Instant::now())
        .expect("the chart's forming bar and live lane project");
    let edge = frame.live_edge.expect("a tape frame declares its mapping");
    assert_eq!(edge.now_ms, 2_000);
    assert_eq!(edge.window_ms, engine.config.lane_window_ms(10_000));
    let later = engine
        .project_at(&request(Some(2_016)), std::time::Instant::now())
        .expect("a later clock still projects the same tape");
    assert_eq!(later.live_edge.unwrap().now_ms, 2_016);
    assert_eq!(
        frame.live_edge.unwrap().now_ms,
        2_000,
        "old frames remain coherent"
    );
}

fn rolling_window_frame(tape_only: bool, partial_open_ms: i64) -> Arc<VisibleOrderflow> {
    let mut engine = BookEngine::new("TEST");
    engine.apply_visual_config(HeatmapConfig {
        show_aggressions: true,
        live_lane: crate::LiveLaneStyle {
            enabled: true,
            show_depth: false,
            show_aggressions: true,
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
    for (agg_id, timestamp_ms, price, quantity) in [
        (1, 1_000, 90, 1),
        (2, 5_000, 110, 2),
        (3, 25_000, 100, 3),
        (4, 30_000, 101, 4),
    ] {
        engine.record_trade(&Trade {
            agg_id,
            timestamp_ms,
            price: Decimal::from(price),
            quantity: Decimal::from(quantity),
            side: Side::Buy,
        });
    }
    let mut partial = Bar::opened_by(&Trade {
        agg_id: 3,
        timestamp_ms: partial_open_ms,
        price: Decimal::from(100),
        quantity: Decimal::from(3),
        side: Side::Buy,
    });
    partial.close_time = 30_000;
    engine
        .project_at(
            &ProjectionRequest {
                partial: Some(partial),
                lane_reference_ms: Some(6_000),
                lane_now_ms: Some(30_000),
                // Older tape prices must also reach auto-fit from outside the
                // currently painted price window.
                price_range: (99.0, 102.0),
                dot_zoom: Some(DotZoom {
                    native_tape: tape_only,
                    tape_window_ms: 100,
                    tape_level_ticks: 1,
                    candle_level_ticks: 1,
                    lane_bars: vec![(partial_open_ms, 30_000)],
                }),
                ..request(Some(30_000))
            },
            std::time::Instant::now(),
        )
        .expect("the forming candle and thirty-second tape project")
}

#[test]
fn tape_only_keeps_the_full_window_before_the_forming_candle() {
    for partial_open_ms in [24_000, 29_000] {
        let frame = rolling_window_frame(true, partial_open_ms);
        let tape: Vec<_> = frame
            .projection
            .aggressions
            .iter()
            .filter(|mark| mark.live)
            .collect();
        assert_eq!(
            tape.iter().map(|mark| mark.quantity).sum::<Decimal>(),
            Decimal::from(10),
            "a bar close must not remove the earlier part of the thirty-second tape"
        );
        assert_eq!(
            crate::projection::tape_price_range(&frame.projection.aggressions),
            Some((90.0, 110.0)),
            "auto-fit must see all tape prices, including those before the visible candle"
        );
    }
}

#[test]
fn ordinary_lane_keeps_its_existing_visible_bar_coverage() {
    let frame = rolling_window_frame(false, 24_000);
    let tape: Vec<_> = frame
        .projection
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .collect();
    assert_eq!(
        tape.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        Decimal::from(7),
        "the full-window override belongs only to the separate tape mode"
    );
    assert_eq!(
        crate::projection::tape_price_range(&frame.projection.aggressions),
        Some((100.0, 101.0))
    );
}
