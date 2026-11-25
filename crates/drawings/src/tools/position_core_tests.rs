use super::*;
use crate::ChartPoint;
use crate::chart::PriceScale;

fn plan(side: Side, entry: f64, edge: f64, ratio: f64) -> Legs {
    legs(side, entry, edge, ratio, None).expect("a measured plan")
}

/// The whole placement rule: the drag's start is the entry, the measured
/// zone is the target on the profit side and the stop on the loss side, and
/// the other leg is the ratio's projection.
#[test]
fn the_four_drag_directions_place_the_legs_the_rule_names() {
    let cases = [
        // (side, entry, edge, measured, stop, target)
        (Side::Long, 100.0, 90.0, Measured::Stop, 90.0, 120.0),
        (Side::Long, 100.0, 120.0, Measured::Target, 90.0, 120.0),
        (Side::Short, 100.0, 110.0, Measured::Stop, 110.0, 80.0),
        (Side::Short, 100.0, 80.0, Measured::Target, 110.0, 80.0),
    ];
    for (side, entry, edge, measured, stop, target) in cases {
        let legs = plan(side, entry, edge, 2.0);
        assert_eq!(
            legs,
            Legs {
                entry,
                stop,
                target,
                measured
            },
            "{side:?} measured from {entry} to {edge}"
        );
        assert!((legs.ratio() - 2.0).abs() < 1e-12);
    }
}

#[test]
fn a_one_to_one_plan_mirrors_the_measured_zone() {
    let legs = plan(Side::Long, 100.0, 95.0, 1.0);
    assert_eq!((legs.stop, legs.target), (95.0, 105.0));
}

#[test]
fn a_flat_or_non_finite_measurement_is_no_plan() {
    assert_eq!(legs(Side::Long, 100.0, 100.0, 2.0, None), None);
    assert_eq!(legs(Side::Short, 100.0, f64::NAN, 2.0, None), None);
    assert_eq!(legs(Side::Long, 100.0, 90.0, 0.0, None), None);
}

#[test]
fn a_pinned_leg_holds_its_price_while_on_its_own_side() {
    let legs = legs(Side::Long, 100.0, 90.0, 2.0, Some(124.0)).unwrap();
    assert_eq!((legs.stop, legs.target), (90.0, 124.0));
    assert!((legs.ratio() - 2.4).abs() < 1e-12);
    // A pin the entry has since crossed is no target: the ratio takes over.
    let crossed = super::legs(Side::Long, 130.0, 120.0, 2.0, Some(124.0)).unwrap();
    assert_eq!(crossed.target, 150.0);
}

#[test]
fn the_tag_steps_through_the_presets_and_wraps() {
    assert_eq!(next_ratio(1.0), 1.5);
    assert_eq!(next_ratio(2.0), 3.0);
    assert_eq!(next_ratio(2.4), 3.0);
    assert_eq!(next_ratio(3.0), 1.0);
    assert_eq!(ratio_label(2.0), "2:1");
    assert_eq!(ratio_label(1.5), "1.5:1");
    assert_eq!(ratio_label(2.375), "2.38:1");
}

#[test]
fn a_preset_carries_the_ratio_and_a_saved_plan_its_pin() {
    let saved = PositionPayload {
        ratio: 3.0,
        projected: Some(131.5),
    };
    let mut preset = PositionPayload::default();
    assert!(preset.import_preset(&saved.export_preset().unwrap()));
    assert_eq!(
        preset,
        PositionPayload {
            ratio: 3.0,
            projected: None
        }
    );
    let mut restored = PositionPayload::default();
    assert!(restored.import_saved(&saved.export_saved().unwrap()));
    assert_eq!(restored, saved);
}

#[test]
fn an_unreadable_ratio_is_refused_on_import() {
    let mut payload = PositionPayload::default();
    for ratio in [0.0, -1.0, MAX_RATIO + 1.0] {
        let value = toml::Value::try_from(PresetData { version: 1, ratio }).unwrap();
        assert!(!payload.import_preset(&value), "{ratio}");
    }
    assert_eq!(payload, PositionPayload::default());
}

fn context<'a>(
    payload: &'a PositionPayload,
    anchors: &'a [ChartPoint],
    scale: &'a PriceScale,
) -> DrawContext<'a> {
    DrawContext {
        appearance: quantick_appearance::ResolvedTheme::bundled_dark(),
        payload,
        anchors,
        scale,
        px_per_bar: 10.0,
        unit: super::super::ValueUnit::Price,
        primary_band: true,
        style: DrawingStyle::default(),
        selected: true,
        halo: false,
        content_editing: false,
        text: &crate::UNMEASURED,
    }
}

/// Price 100 at y 200, 90 at y 300: ten pixels per point.
fn scale() -> PriceScale {
    PriceScale::from_range(0.0, 200.0, 0.0, 2000.0)
}

fn screen(scale: &PriceScale, anchors: &[ChartPoint]) -> Vec<Px> {
    anchors
        .iter()
        .map(|anchor| pos2(anchor.bar * 10.0, scale.y(anchor.price)))
        .collect()
}

#[test]
fn dragging_the_projected_leg_pins_it_and_leaves_the_rest() {
    let scale = scale();
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 90.0)];
    let points = screen(&scale, &anchors);
    let payload = PositionPayload::default();
    let ctxt = context(&payload, &anchors, &scale);
    let target_y = scale.y(120.0);
    assert_eq!(
        pointer_hint(&points, pos2(200.0, target_y), &ctxt, Side::Long),
        Some(PointerHint::ResizeSide {
            handle: PROJECTED_HANDLE
        })
    );
    let dragged = drag_projected(
        &points,
        PROJECTED_HANDLE,
        pos2(200.0, scale.y(125.0)),
        &ctxt,
        Side::Long,
    )
    .expect("the projected leg takes the drag");
    let dragged = dragged.as_any().downcast_ref::<PositionPayload>().unwrap();
    assert!((dragged.projected.unwrap() - 125.0).abs() < 1e-9);
    assert_eq!(dragged.ratio, DEFAULT_RATIO, "the chosen ratio is kept");
    // Pulled across the entry, the target stays where it was.
    let crossed = drag_projected(
        &points,
        PROJECTED_HANDLE,
        pos2(200.0, scale.y(95.0)),
        &ctxt,
        Side::Long,
    )
    .unwrap();
    let crossed = crossed.as_any().downcast_ref::<PositionPayload>().unwrap();
    assert_eq!(crossed.projected, Some(120.0));
}

#[test]
fn a_line_grab_moves_only_that_lines_price() {
    let points = [pos2(100.0, 200.0), pos2(300.0, 300.0)];
    let moved = drag_line(
        &points,
        MEASURED_LINE_HANDLE,
        pos2(180.0, 320.0),
        Constrain::Free,
    )
    .unwrap();
    assert_eq!(moved.as_slice(), &[pos2(100.0, 200.0), pos2(300.0, 320.0)]);
    let moved = drag_line(
        &points,
        ENTRY_LINE_HANDLE,
        pos2(180.0, 190.0),
        Constrain::Free,
    )
    .unwrap();
    assert_eq!(moved.as_slice(), &[pos2(100.0, 190.0), pos2(300.0, 300.0)]);
    assert_eq!(drag_line(&points, 0, pos2(0.0, 0.0), Constrain::Free), None);
}

#[test]
fn a_click_on_the_plan_tag_steps_the_ratio_and_releases_the_pin() {
    let points = [pos2(100.0, 200.0), pos2(300.0, 300.0)];
    let mut payload = PositionPayload {
        ratio: 2.0,
        projected: Some(124.0),
    };
    assert!(
        !click(&points, pos2(250.0, 260.0), &mut payload),
        "inside a zone, off the tag"
    );
    assert!(click(&points, pos2(130.0, 203.0), &mut payload));
    assert_eq!(
        payload,
        PositionPayload {
            ratio: 3.0,
            projected: None
        }
    );
}

#[test]
fn a_position_paints_both_zones_and_says_it_is_a_plan() {
    let scale = scale();
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 90.0)];
    let points = screen(&scale, &anchors);
    let payload = PositionPayload::default();
    let ctxt = context(&payload, &anchors, &scale);
    let mut scene = MarkScene::default();
    let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 2000.0));
    paint(
        &mut scene,
        chart,
        DrawingStyle::default(),
        &points,
        &ctxt,
        Side::Long,
    );
    let texts: Vec<String> = scene
        .shapes
        .iter()
        .filter_map(|shape| match shape {
            crate::MarkShape::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        [
            "TP 120 · +20 pts",
            "SL 90 · -10 pts",
            "Plan long · 100 · 2:1"
        ],
    );
}
