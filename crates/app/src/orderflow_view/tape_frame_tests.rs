//! The current tape window applies even while the worker's frame is cached.
use super::*;
use quantick_engine::Side;
use quantick_orderflow::projection::PaneGeometry;

#[test]
fn zooming_the_tape_repositions_the_cached_frame_before_the_worker_answers() {
    let mut view = OrderflowView::new("WINV26");
    let before = view.config.clone();
    assert!(view.apply_preset("mini index regions"));
    view.commit_config_changes(before);
    view.set_live_lane_window(LaneWindow::Fixed { ms: 10_000 });
    let mut trade = Trade {
        agg_id: 1,
        timestamp_ms: 0,
        price: Decimal::from(100),
        quantity: Decimal::from(10),
        side: Side::Buy,
    };
    view.record_trade(&trade);
    let mut partial = Bar::opened_by(&trade);
    trade.agg_id = 2;
    trade.timestamp_ms = 1_000;
    view.record_trade(&trade);
    partial.extend(&trade);
    view.set_replay_clock_at(9_000, Some(1_000), None);
    let _ = view.project_visible(
        VisibleBarTimeline::new(1, 0, &[], Some(&partial)),
        true,
        true,
        Some(10_000),
        (90.0, 110.0),
        Some(PaneGeometry {
            px_per_bar: 10.0,
            lane_width_px: 600.0,
            lane_window_ms: 10_000,
            height_px: 400.0,
            lane_bars: vec![(0, 1_000)],
        }),
    );
    view.flush_for_test();
    let frame = view.published.frame.clone().expect("the tape projected");
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
    let ink = |view: &OrderflowView| {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            view.draw_aggressions(
                &ctx.layer_painter(egui::LayerId::background()),
                rect, &viewport, 1, &frame, egui::Color32::BLACK,
                rect.width(), false, (90.0, 110.0),
            );
        });
        output.shapes.iter().filter(|shape| {
            matches!(shape.shape, egui::Shape::Circle(_) | egui::Shape::Mesh(_))
        }).count()
    };
    assert!(ink(&view) > 0, "an eight-second-old print is inside ten seconds");
    view.set_live_lane_window(LaneWindow::Fixed { ms: 5_000 });
    assert_eq!(frame.live_edge.unwrap().window_ms, 10_000, "the cached frame has not changed");
    assert_eq!(ink(&view), 0, "the current five-second window excludes that old print");
}
