use super::*;
use quantick_engine::{BarSpec, Side, Trade};
#[test]
fn source_reset_immediately_clears_readback_without_a_valid_chart_layout() {
    let mut owner = OrderflowView::new("A");
    owner.config.live_lane.native_tape = true;
    owner.config.live_lane.tape_only = false;
    owner.config.volume_dots.enabled = true;
    owner.config.show_aggressions = true;
    owner.set_ignore_flow_opening(true);
    let mut state = ChartState::new(BarSpec::Tick(2));
    state.ingest_live(&Trade {
        agg_id: 1,
        timestamp_ms: 1000,
        price: 100.into(),
        quantity: 1.into(),
        side: Side::Buy,
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while owner.flow_execution_frame().is_none() {
        owner.project_flow_executions(
            &state,
            0..1,
            egui::vec2(100.0, 200.0),
            (90.0, 110.0),
            (0.0, 1.0),
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    owner.project_flow_executions(
        &state,
        0..0,
        egui::vec2(100.0, 200.0),
        (90.0, 110.0),
        (0.0, 1.0),
    );
    assert!(owner.flow_execution_frame().is_none());
    assert_eq!(owner.flow_execution_progress().requested_executions, 0);
    while owner.flow_execution_frame().is_none() {
        owner.project_flow_executions(
            &state,
            0..1,
            egui::vec2(100.0, 200.0),
            (90.0, 110.0),
            (0.0, 1.0),
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    owner.reset_for_symbol("B");
    assert!(owner.flow_execution_frame().is_none());
    assert_eq!(owner.flow_execution_progress().requested_executions, 0);
    assert!(!owner.flow_execution_progress().pending);
    assert!(owner.ignore_flow_opening());
    owner.project_flow_executions(
        &state,
        0..1,
        egui::vec2(100.0, 200.0),
        (90.0, 110.0),
        (0.0, 1.0),
    );
    assert!(owner.flow_execution_progress().pending);
    owner.reset_flow_executions();
    assert!(owner.flow_execution_frame().is_none());
    assert_eq!(owner.flow_execution_progress().submitted_executions, 0);
}

#[test]
fn viewport_readback_uses_last_painted_frame_and_rejects_a_changed_look_or_source() {
    use std::sync::Arc;
    let mut owner = OrderflowView::new("BTCUSDT");
    owner.config.live_lane.native_tape = true;
    owner.config.volume_dots.enabled = true;
    owner.config.show_aggressions = true;
    owner.config.volume_dots.tick_bubbles = Some(quantick_orderflow::TickBubbleStyle {
        viewport_scale: Some(quantick_orderflow::ViewportBubbleScale {
            x_units_per_pixel: 1.0 / 22.0,
            price_units_per_pixel: 1.0,
        }),
        reference_mode: quantick_orderflow::TickReferenceMode::Fixed,
        reference_quantity: 100.0,
        max_radius: 32.0,
        label_min_radius: 8.0,
        opacity: 0.9,
    });
    let mut state = ChartState::new(BarSpec::Tick(1));
    state.ingest_live(&Trade {
        agg_id: 1,
        timestamp_ms: 1_000,
        price: 100.into(),
        quantity: 4.into(),
        side: Side::Buy,
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while owner.flow_execution_frame().is_none() {
        owner.project_flow_executions(
            &state,
            0..1,
            egui::vec2(100.0, 200.0),
            (90.0, 110.0),
            (0.0, 1.0),
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let source = Arc::clone(owner.flow_execution_handle().unwrap());
    assert!(
        owner.flow_execution_snapshot_frame().is_none(),
        "unpainted camera has no claimed actual radius"
    );
    let painted = Arc::new(source.viewport_relative(5.0, |_| true));
    owner.note_flow_paint(&source, Some(&painted));
    assert!(Arc::ptr_eq(
        &owner.flow_execution_snapshot_frame().unwrap(),
        &painted
    ));
    owner
        .config
        .volume_dots
        .tick_bubbles
        .as_mut()
        .unwrap()
        .max_radius = 48.0;
    assert!(
        owner.flow_execution_snapshot_frame().is_none(),
        "old look cannot masquerade as new"
    );
    owner
        .config
        .volume_dots
        .tick_bubbles
        .as_mut()
        .unwrap()
        .viewport_scale = None;
    assert!(
        Arc::ptr_eq(&owner.flow_execution_snapshot_frame().unwrap(), &source),
        "legacy resumes published radii"
    );
    owner.reset_flow_executions();
    assert!(owner.flow_execution_snapshot_frame().is_none());
}
