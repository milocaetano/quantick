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

#[cfg(test)]
mod display_scale_tests {
    use super::*;

    #[test]
    fn manual_candle_gain_preserves_facts_reference_and_both_side_ratios_in_every_mode() {
        let mut source = viewport_source();
        source.dots[0].mark.buy_quantity = source.dots[0].mark.quantity / Decimal::from(2);
        source.dots[0].mark.buy_share = 0.5;
        let original = source.clone();
        let camera = viewport_camera(64.0, (90.0, 110.0));
        let clip = [[0.0, 0.0], [300.0, 200.0]];
        let mut fixed = viewport_style();
        fixed.viewport_scale = None;
        fixed.reference_mode = quantick_orderflow::TickReferenceMode::Fixed;
        for tick in [None, Some(fixed), Some(viewport_style())] {
            let base = camera
                .viewport_frame(&source, tick, clip)
                .unwrap_or_else(|| source.clone());
            for gain in [0.25, 0.5, 2.0, 3.0] {
                let scaled_camera = camera.with_display_scale(gain);
                assert_ne!(scaled_camera.fingerprint(), camera.fingerprint());
                let scaled = scaled_camera.viewport_frame(&source, tick, clip).unwrap();
                assert_eq!(scaled.view.reference, base.view.reference);
                assert_eq!(scaled.effective_reference, base.effective_reference);
                assert_eq!(scaled.scale_basis, base.scale_basis);
                assert_eq!(
                    scaled.view.merge_support_radius,
                    base.view.merge_support_radius
                );
                assert_eq!(scaled.view.radius_limit, base.view.radius_limit * gain);
                assert_eq!(scaled.dots.len(), base.dots.len());
                for (before, after) in base.dots.iter().zip(&scaled.dots) {
                    let mut expected = before.clone();
                    expected.radius *= gain;
                    assert_eq!(after, &expected, "only drawn radius changes");
                    let (buy, sell) = before.side_radii();
                    let (scaled_buy, scaled_sell) = after.side_radii();
                    assert!((scaled_buy - buy * gain).abs() < 0.00001);
                    assert!((scaled_sell - sell * gain).abs() < 0.00001);
                }
                assert_eq!(
                    camera
                        .viewport_frame(&source, tick, clip)
                        .unwrap_or_else(|| source.clone()),
                    base
                );
            }
        }
        assert_eq!(source, original);
        assert!(
            camera
                .with_display_scale(1.0)
                .viewport_frame(&source, None, clip)
                .is_none()
        );
        assert_eq!(camera, camera.with_display_scale(f32::NAN));
    }

    #[test]
    fn manual_legacy_gain_keeps_peak_placement_and_source_centers() {
        let source = viewport_source();
        let camera = viewport_camera(64.0, (90.0, 110.0));
        let clip = [[0.0, 0.0], [300.0, 200.0]];
        let (base, identity) = camera.presentation(&source, None, clip);
        assert!(identity.is_none());
        for gain in [0.25, 3.0] {
            let (changed, displayed) = camera
                .with_display_scale(gain)
                .presentation(&source, None, clip);
            assert!(displayed.is_some());
            let mut expected = base.clone();
            for region in &mut expected.regions {
                region.source_disc.radius *= gain;
                region.disc.radius *= gain;
            }
            assert_eq!(changed, expected);
        }
    }
}

#[test]
fn a_sub_cent_btc_region_drawn_at_the_current_camera_never_reads_zero() {
    let trade = Trade {
        agg_id: 1,
        timestamp_ms: 1_000,
        price: 100.into(),
        quantity: Decimal::new(4, 3),
        side: Side::Sell,
    };
    let source = project_flow_tape(
        [FlowExecution {
            ordinal: 0,
            slot: 1,
            accepted_ordinal: 0,
            ticks_per_bar: 1.into(),
            trade: &trade,
            opening: false,
        }],
        1,
        1,
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
    );
    let original = source.clone();
    let style = viewport_style();
    let clip = [[0.0, 0.0], [300.0, 200.0]];
    for gain in [1.0, 2.0] {
        let camera = viewport_camera(64.0, (90.0, 110.0)).with_display_scale(gain);
        let (plan, displayed) = camera.presentation(&source, Some(style), clip);
        let displayed = displayed.expect("the visible-relative camera draws its own frame");
        let labels = plan.quantity_labels(&displayed, Some(style));
        assert_eq!(labels.len(), 1, "the lone region is a full, legible sphere");
        assert_eq!(labels[0].text, "0.004", "a positive quantity never reads 0");
        let region = &plan.regions[0];
        let (raw, drawn) = (&source.dots[0], &displayed.dots[region.dot_index]);
        assert_eq!(drawn.mark.quantity, Decimal::new(4, 3));
        assert_eq!(drawn.mark.quantity, raw.mark.quantity);
        assert_eq!(drawn.mark.side, raw.mark.side);
        assert_eq!(drawn.mark.agg_ids, raw.mark.agg_ids);
        assert_eq!(drawn.members, raw.members);
    }
    assert_eq!(
        source, original,
        "labelling never edits the published frame"
    );
}
