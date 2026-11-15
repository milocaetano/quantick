//! Cached tape placement and the tape memory under tick-style edits.
use super::*;
use quantick_engine::Side;
use quantick_orderflow::projection::PaneGeometry;

#[test]
fn zooming_the_tape_repositions_the_cached_frame_before_the_worker_answers() {
    let mut view = OrderflowView::new("WINV26");
    let before = view.config.clone();
    assert!(view.apply_preset("mini index regions", false));
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
                rect,
                &viewport,
                1,
                &frame,
                egui::Color32::BLACK,
                rect.width(),
                false,
                (90.0, 110.0),
                true,
            );
        });
        output
            .shapes
            .iter()
            .filter(|shape| matches!(shape.shape, egui::Shape::Circle(_) | egui::Shape::Mesh(_)))
            .count()
    };
    assert!(
        ink(&view) > 0,
        "an eight-second-old print is inside ten seconds"
    );
    view.set_live_lane_window(LaneWindow::Fixed { ms: 5_000 });
    assert_eq!(
        frame.live_edge.unwrap().window_ms,
        10_000,
        "the cached frame has not changed"
    );
    assert_eq!(
        ink(&view),
        0,
        "the current five-second window excludes that old print"
    );
}

#[test]
fn tick_style_edits_keep_retained_tape_membership_and_paint_unchanged() {
    let mut view = OrderflowView::new("BTCUSDT");
    let before = view.config.clone();
    assert!(view.apply_preset("mini index regions", false));
    view.commit_config_changes(before);
    view.set_live_lane_window(LaneWindow::Fixed { ms: 10_000 });
    let mut partial = None::<Bar>;
    for (agg_id, timestamp_ms, quantity, side) in [
        (1, 1_000, 10, Side::Buy),
        (2, 1_010, 20, Side::Sell),
        (3, 3_000, 5, Side::Buy),
    ] {
        let trade = Trade {
            agg_id,
            timestamp_ms,
            quantity: quantity.into(),
            price: 100.into(),
            side,
        };
        view.record_trade(&trade);
        if let Some(bar) = &mut partial {
            bar.extend(&trade);
        } else {
            partial = Some(Bar::opened_by(&trade));
        }
    }
    view.set_replay_clock_at(9_000, Some(3_000), None);
    let _ = view.project_visible(
        VisibleBarTimeline::new(1, 0, &[], partial.as_ref()),
        true,
        true,
        Some(10_000),
        (90.0, 110.0),
        Some(PaneGeometry {
            px_per_bar: 10.0,
            lane_width_px: 600.0,
            lane_window_ms: 10_000,
            height_px: 400.0,
            lane_bars: vec![(1_000, 3_000)],
        }),
    );
    view.flush_for_test();
    let frame = view.published.frame.clone().unwrap();
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
    let ink = |view: &OrderflowView| {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            view.draw_aggressions(
                &ctx.layer_painter(egui::LayerId::background()),
                rect,
                &viewport,
                1,
                &frame,
                egui::Color32::BLACK,
                rect.width(),
                false,
                (90.0, 110.0),
                true,
            );
        });
        format!("{:?}", output.shapes)
    };
    let expected_ink = ink(&view);
    assert_eq!(ink(&view), expected_ink, "the unchanged tape is warm");
    assert!(view.tape_dots.borrow().retained_group_count() > 0);
    let membership = format!("{:?}", view.tape_dots.borrow());
    let style = quantick_orderflow::TickBubbleStyle {
        zoom_regions: false,
        peak_spheres: None,
        viewport_scale: None,
        reference_mode: quantick_orderflow::TickReferenceMode::Fixed,
        reference_quantity: 30.0,
        max_radius: 25.0,
        label_min_radius: 8.0,
        opacity: 0.8,
    };
    let visible = quantick_orderflow::TickBubbleStyle {
        zoom_regions: false,
        peak_spheres: None,
        reference_mode: quantick_orderflow::TickReferenceMode::VisibleMax,
        ..style
    };
    for tick in [Some(style), Some(visible), Some(style), None] {
        view.edit_config(|config| config.volume_dots.tick_bubbles = tick);
        assert_eq!(
            format!("{:?}", view.tape_dots.borrow()),
            membership,
            "a left-chart edit cannot discard the right tape's members"
        );
        assert_eq!(ink(&view), expected_ink);
    }
}
