use super::*;
use quantick_engine::{Side, Trade};
use quantick_orderflow::projection::flow_tape::{
    FlowExecution, FlowReference, FlowTapeView, project_flow_tape,
};
use rust_decimal::Decimal;

fn dot() -> FlowTapeDot {
    let trade = Trade {
        agg_id: 1,
        timestamp_ms: 1000,
        price: 105.into(),
        quantity: 1.into(),
        side: Side::Buy,
    };
    let frame = project_flow_tape(
        [FlowExecution {
            ordinal: 0,
            slot: 2,
            accepted_ordinal: 0,
            ticks_per_bar: 2.into(),
            trade: &trade,
            opening: false,
        }],
        1,
        1,
        1,
        FlowTapeView {
            first_slot: 2,
            end_slot: 3,
            clip_left: 2.into(),
            clip_right: 3.into(),
            width_px: 100.0,
            height_px: 200.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::VisibleRegions,
            radius_limit: 12.0,
            merge_support_radius: 6.0,
            exclude_opening: false,
        },
    );
    frame.dots.into_iter().next().unwrap()
}

fn geometry(viewport: Viewport, range: (f64, f64), inverted: bool) -> FlowExecutionGeometry {
    FlowExecutionGeometry::new(viewport, 10, 7, range, [20.0, 20.0, 500.0, 220.0], inverted)
        .unwrap()
}

#[test]
fn price_axis_orientation_and_manual_range_preserve_the_fractional_source_position() {
    let dot = dot();
    assert_eq!(dot.candle_position, Decimal::new(225, 2));
    let ordinary = geometry(Viewport::new(), (90.0, 110.0), false);
    let inverted = geometry(Viewport::new(), (90.0, 110.0), true);
    let manual = geometry(Viewport::new(), (90.0, 130.0), false);
    assert_eq!(ordinary.point(&dot), Some((494.0, 70.0)));
    assert_eq!(inverted.point(&dot), Some((494.0, 170.0)));
    assert_eq!(manual.point(&dot), Some((494.0, 145.0)));
    assert_ne!(ordinary.fingerprint(), inverted.fingerprint());
    assert_ne!(ordinary.fingerprint(), manual.fingerprint());
}

#[test]
fn viewport_pan_zoom_prefix_and_live_growth_invalidate_the_exact_transform_key() {
    let dot = dot();
    let original = geometry(Viewport::new(), (90.0, 110.0), false);
    let mut viewport = Viewport::new();
    viewport.pan_pixels(16.0, 10);
    let panned = geometry(viewport, (90.0, 110.0), false);
    assert_eq!(panned.point(&dot), Some((510.0, 70.0)));
    viewport.zoom(2.0);
    let zoomed = geometry(viewport, (90.0, 110.0), false);
    assert_eq!(zoomed.point(&dot), Some((520.0, 70.0)));
    let no_prefix = FlowExecutionGeometry::new(
        Viewport::new(),
        10,
        0,
        (90.0, 110.0),
        original.history,
        false,
    )
    .unwrap();
    assert_eq!(no_prefix.point(&dot), Some((438.0, 70.0)));
    let grown = FlowExecutionGeometry::new(
        Viewport::new(),
        11,
        7,
        (90.0, 110.0),
        original.history,
        false,
    )
    .unwrap();
    assert_eq!(grown.point(&dot), Some((486.0, 70.0)));
    for changed in [panned, zoomed, no_prefix, grown] {
        assert_ne!(changed, original);
        assert_ne!(changed.fingerprint(), original.fingerprint());
    }
}

#[test]
fn rect_move_and_resize_change_the_key_while_invalid_price_ranges_are_rejected() {
    let dot = dot();
    let original = geometry(Viewport::new(), (90.0, 110.0), false);
    for index in 0..4 {
        let mut bounds = original.history;
        bounds[index] += 10.0;
        let changed =
            FlowExecutionGeometry::new(Viewport::new(), 10, 7, (90.0, 110.0), bounds, false)
                .unwrap();
        assert_ne!(changed.fingerprint(), original.fingerprint());
        assert_eq!(
            changed.point(&dot).unwrap().0,
            if index == 2 { 504.0 } else { 494.0 }
        );
    }
    for range in [
        (110.0, 90.0),
        (100.0, 100.0),
        (f64::NAN, 110.0),
        (90.0, f64::INFINITY),
    ] {
        assert!(
            FlowExecutionGeometry::new(Viewport::new(), 10, 7, range, original.history, false)
                .is_none()
        );
    }
    assert_eq!(original, geometry(Viewport::new(), (90.0, 110.0), false));
}

fn viewport_style() -> quantick_orderflow::TickBubbleStyle {
    quantick_orderflow::TickBubbleStyle {
        reference_quantity: 900.0,
        reference_mode: quantick_orderflow::TickReferenceMode::VisibleMax,
        viewport_scale: Some(quantick_orderflow::ViewportBubbleScale {
            x_units_per_pixel: 1.0 / 22.0,
            price_units_per_pixel: 1.0,
        }),
        max_radius: 32.0,
        label_min_radius: 8.0,
        opacity: 0.9,
    }
}

fn viewport_source() -> FlowTapeFrame {
    let trades = [(1, 100, 1), (2, 102, 4), (3, 105, 16)].map(|(id, price, quantity)| Trade {
        agg_id: id,
        timestamp_ms: id as i64 * 1_000,
        price: price.into(),
        quantity: quantity.into(),
        side: if id == 2 { Side::Sell } else { Side::Buy },
    });
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal,
                accepted_ordinal: 0,
                ticks_per_bar: 1.into(),
                trade,
                opening: false,
            }),
        1,
        3,
        3,
        FlowTapeView {
            first_slot: 0,
            end_slot: 3,
            clip_left: 0.into(),
            clip_right: 3.into(),
            width_px: 300.0,
            height_px: 200.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::VisibleRegionsAtLeast(900.into()),
            radius_limit: 32.0,
            merge_support_radius: 6.0,
            exclude_opening: false,
        },
    )
}

fn viewport_camera(px_per_bar: f32, range: (f64, f64)) -> FlowExecutionGeometry {
    let mut viewport = Viewport::new();
    viewport.set_px_per_bar(px_per_bar);
    FlowExecutionGeometry::new(viewport, 3, 0, range, [0.0, 0.0, 300.0, 200.0], false).unwrap()
}

#[test]
fn viewport_spheres_use_current_camera_and_exact_visible_volume_without_regrouping() {
    let source = viewport_source();
    let original = source.clone();
    let style = viewport_style();
    let clip = [[0.0, 0.0], [300.0, 200.0]];
    let a = viewport_camera(64.0, (90.0, 110.0));
    let b = viewport_camera(32.0, (80.0, 120.0));
    let first = a.viewport_frame(&source, Some(style), clip).unwrap();
    let compressed = b.viewport_frame(&source, Some(style), clip).unwrap();
    let again = a.viewport_frame(&source, Some(style), clip).unwrap();
    assert_eq!(
        first, again,
        "A-B-A has no remembered calibration or worker dependency"
    );
    assert_eq!(
        source, original,
        "the published factual frame remains untouched"
    );
    assert_eq!(
        first.effective_reference,
        Some(16.into()),
        "no configured floor"
    );
    assert!(compressed.view.radius_limit < first.view.radius_limit);
    for ((raw, a), b) in source.dots.iter().zip(&first.dots).zip(&compressed.dots) {
        let mut resized = raw.clone();
        resized.radius = a.radius;
        resized.mark.size = a.mark.size;
        assert_eq!(
            &resized, a,
            "all membership, side and center facts stay exact"
        );
        assert!(
            (a.radius / b.radius - first.view.radius_limit / compressed.view.radius_limit).abs()
                < 0.00001
        );
    }
    assert_eq!(first.dots[1].radius / first.dots[0].radius, 2.0);
    assert_eq!(first.dots[2].radius / first.dots[0].radius, 4.0);
    assert_eq!(
        first,
        a.viewport_frame(&viewport_source(), Some(style), clip)
            .unwrap(),
        "replay rebuild is deterministic"
    );
    for reference_mode in [
        quantick_orderflow::TickReferenceMode::Fixed,
        quantick_orderflow::TickReferenceMode::VisibleMax,
    ] {
        let legacy = quantick_orderflow::TickBubbleStyle {
            viewport_scale: None,
            reference_mode,
            ..style
        };
        assert!(
            a.viewport_frame(&source, Some(legacy), clip).is_none(),
            "WIN and old policies keep the published radii"
        );
    }
}

#[test]
fn current_clip_excludes_whales_before_scaling_and_quiet_views_use_the_full_range() {
    let source = viewport_source();
    let camera = viewport_camera(64.0, (90.0, 110.0));
    let style = viewport_style();
    let clip = [[0.0, 0.0], [260.0, 200.0]];
    let displayed = camera.viewport_frame(&source, Some(style), clip).unwrap();
    assert_eq!(displayed.dots.len(), 2);
    assert_eq!(displayed.loaded_executions, 3);
    assert_eq!(displayed.offscreen_executions, 1);
    assert_eq!(displayed.off_axis_executions, 0);
    assert_eq!(
        displayed
            .dots
            .iter()
            .map(|dot| dot.mark.trade_count)
            .sum::<usize>(),
        2
    );
    assert_eq!(displayed.effective_reference, Some(4.into()));
    assert_eq!(displayed.dots[1].radius, displayed.view.radius_limit);
    let off_axis = viewport_camera(64.0, (99.0, 103.0))
        .viewport_frame(&source, Some(style), [[0.0, 0.0], [300.0, 200.0]])
        .unwrap();
    assert_eq!(off_axis.effective_reference, Some(4.into()));
    assert_eq!(off_axis.dots.len(), 2);
    assert_eq!(off_axis.loaded_executions, 3);
    assert_eq!(off_axis.offscreen_executions, 0);
    assert_eq!(off_axis.off_axis_executions, 1);
    assert_eq!(
        off_axis
            .dots
            .iter()
            .map(|dot| dot.mark.trade_count)
            .sum::<usize>(),
        2
    );
    let empty = camera
        .viewport_frame(&source, Some(style), [[0.0, 0.0], [10.0, 200.0]])
        .unwrap();
    assert!(empty.dots.is_empty());
    assert_eq!(empty.effective_reference, None);
}
