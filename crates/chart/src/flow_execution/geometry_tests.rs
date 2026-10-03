use super::*;
use quantick_engine::{Side, Trade};
use quantick_orderflow::projection::flow_tape::{
    FlowExecution, FlowReference, FlowTapeView, project_flow_tape,
};

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
