//! Projection time is an explicit input only for the independent tape mode.
use super::*;
use quantick_engine::{Side, Trade};

fn request(now_ms: Option<i64>) -> ProjectionRequest {
    ProjectionRequest {
        timeline_revision: 0,
        first_bar_index: 0,
        closed: Vec::new(),
        partial: None,
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
    let mut config = HeatmapConfig::default();
    config.show_aggressions = true;
    config.live_lane.enabled = true;
    config.live_lane.show_aggressions = true;
    config.live_lane.tape_only = tape_only;
    engine.apply_visual_config(config);
    engine.record_trade(&Trade {
        agg_id: 1,
        timestamp_ms: 1_000,
        price: Decimal::from(100),
        quantity: Decimal::from(10),
        side: Side::Buy,
    });
    engine
}

#[test]
fn tape_only_projection_uses_the_callers_now_between_market_events() {
    let engine = engine(true);
    assert_eq!(engine.live_edge(&request(Some(1_016))).unwrap().now_ms, 1_016);
    assert_eq!(engine.live_edge(&request(Some(2_200))).unwrap().now_ms, 2_200);
    assert_eq!(engine.live_edge(&request(Some(900))).unwrap().now_ms, 1_000);
}

#[test]
fn ordinary_btc_style_lane_keeps_its_event_anchored_clock() {
    let engine = engine(false);
    assert_eq!(engine.live_edge(&request(Some(2_200))).unwrap().now_ms, 1_000);
    assert_eq!(engine.live_edge(&request(None)).unwrap().now_ms, 1_000);
}

#[test]
fn a_published_frame_carries_the_clock_its_positions_were_projected_against() {
    let mut engine = engine(true);
    let input = request(Some(2_000));
    let frame = engine
        .project_at(&input, std::time::Instant::now())
        .expect("the tape itself supplies a projection region");
    let edge = frame.live_edge.expect("a tape frame declares its mapping");
    assert_eq!(edge.now_ms, 2_000);
    assert_eq!(edge.window_ms, engine.config.lane_window_ms(10_000));
    let later = engine
        .project_at(&request(Some(2_016)), std::time::Instant::now())
        .expect("a later clock still projects the same tape");
    assert_eq!(later.live_edge.unwrap().now_ms, 2_016);
    assert_eq!(frame.live_edge.unwrap().now_ms, 2_000, "old frames remain coherent");
}
