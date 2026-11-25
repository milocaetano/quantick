use super::*;
use crate::ChartPoint;
use crate::chart::PriceScale;
use crate::tools::position_payload::{DEFAULT_RATIO, PositionPayload, set_plan_ratio};
use crate::tools::position_text::plan_plate;

/// The plans these geometry tests measure were drawn at 1 : 2.
fn two_to_one() -> PositionPayload {
    PositionPayload {
        ratio: 2.0,
        ..PositionPayload::default()
    }
}

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
    let payload = two_to_one();
    let ctxt = context(&payload, &anchors, &scale);
    let target_y = scale.y(120.0);
    assert_eq!(
        pointer_hint(&points, pos2(200.0, target_y), &ctxt, Side::Long),
        Some(PointerHint::ResizeSide {
            handle: PROJECTED_HANDLE
        })
    );
    let dragged = drag_payload(
        &points,
        PROJECTED_HANDLE,
        pos2(200.0, scale.y(125.0)),
        &ctxt,
        Side::Long,
    )
    .expect("the projected leg takes the drag");
    let dragged = dragged.as_any().downcast_ref::<PositionPayload>().unwrap();
    assert!((dragged.projected_offset.unwrap() - 25.0).abs() < 1e-9);
    // Stop 90, entry 100, target pinned at 125: the plan now reads 1 : 2.5.
    assert!(
        (dragged.ratio - 2.5).abs() < 1e-9,
        "the ratio follows the pin"
    );
    assert!(crate::position_preset_of(dragged.ratio).is_none());
    // Pulled across the entry, the target stays where it was.
    let crossed = drag_payload(
        &points,
        PROJECTED_HANDLE,
        pos2(200.0, scale.y(95.0)),
        &ctxt,
        Side::Long,
    )
    .unwrap();
    let crossed = crossed.as_any().downcast_ref::<PositionPayload>().unwrap();
    assert_eq!(crossed.projected_offset, Some(20.0));
}

#[test]
fn a_line_grab_moves_only_that_lines_price() {
    let points = [pos2(100.0, 200.0), pos2(300.0, 300.0)];
    let moved = drag_line(&points, MEASURED_LINE_HANDLE, pos2(180.0, 320.0)).unwrap();
    assert_eq!(moved.as_slice(), &[pos2(100.0, 200.0), pos2(300.0, 320.0)]);
    let moved = drag_line(&points, ENTRY_LINE_HANDLE, pos2(180.0, 190.0)).unwrap();
    assert_eq!(moved.as_slice(), &[pos2(100.0, 190.0), pos2(300.0, 300.0)]);
    assert_eq!(drag_line(&points, 0, pos2(0.0, 0.0)), None);
}

#[test]
fn a_selected_position_names_its_legs_and_its_ratio_risk_first() {
    let scale = scale();
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 90.0)];
    let points = screen(&scale, &anchors);
    let payload = two_to_one();
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
            "Target 120 · +20 pts",
            "Stop 90 · -10 pts",
            "Long · risk 1 : reward 2 · entry 100"
        ],
    );
}

/// A long plan entered at 100 with its stop measured at 95: target 110.
fn placed_plan() -> crate::Drawings<crate::DrawingTool> {
    let tool = crate::DrawingTool::by_id("long-position").unwrap();
    let mut drawings = crate::Drawings::default();
    let placement = crate::Placement {
        tool,
        points: vec![ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 95.0)],
        band: crate::DrawingBand::Price,
        opening: crate::NewDrawing::explicit(DrawingStyle::default(), Box::new(two_to_one())),
        author: None,
        name: None,
        text: None,
    };
    crate::update(&mut drawings, crate::DrawingIntent::Place(placement));
    drawings
}

fn stored_legs(drawings: &crate::Drawings<crate::DrawingTool>) -> (f64, f64, f64) {
    let drawing = &drawings.items()[0];
    let plan = position_plan(drawing.tool, &drawing.points, drawing.payload.as_ref()).unwrap();
    (plan.entry, plan.stop, plan.target)
}

/// Every anchor move passes through the store, whichever surface made it:
/// a handle, a nudge or a linked pane's edit.
#[test]
fn moving_one_line_leaves_the_other_two_and_moving_the_plan_moves_all_three() {
    let mut drawings = placed_plan();
    assert_eq!(stored_legs(&drawings), (100.0, 95.0, 110.0));
    // The entry alone moves to 102: the target stays at 110, not 116.
    assert!(drawings.move_anchor(0, 0, ChartPoint::at(10.0, 102.0)));
    assert_eq!(stored_legs(&drawings), (102.0, 95.0, 110.0));
    // The stop alone moves to 96.
    assert!(drawings.set_points(
        0,
        &[ChartPoint::at(10.0, 102.0), ChartPoint::at(30.0, 96.0)]
    ));
    assert_eq!(stored_legs(&drawings), (102.0, 96.0, 110.0));
    // The whole plan moves up 5: every leg goes with it.
    drawings.select(Some(0));
    drawings.translate_selected(0.0, 5.0);
    assert_eq!(stored_legs(&drawings), (107.0, 101.0, 115.0));
}

#[test]
fn an_unpinned_plan_moved_whole_keeps_following_its_ratio() {
    let mut drawings = placed_plan();
    drawings.select(Some(0));
    drawings.translate_selected(0.0, 5.0);
    assert_eq!(stored_legs(&drawings), (105.0, 100.0, 115.0));
    let payload = drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<PositionPayload>()
        .unwrap();
    assert_eq!(payload.projected_offset, None, "a rigid move pins nothing");
}

#[test]
fn a_flat_plan_is_refused_before_it_is_placed() {
    let flat = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 100.0)];
    let measured = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 95.0)];
    assert!(LONG.placement_refusal(&flat).is_some());
    assert_eq!(SHORT.placement_refusal(&measured), None);
}

#[test]
fn the_content_bounds_reserve_the_projected_zone_on_both_sides() {
    let anchors = PxRect::from_min_max(pos2(100.0, 200.0), pos2(300.0, 250.0));
    let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 1000.0));
    let bounds = LONG.content_bounds(&two_to_one(), anchors, chart, crate::TextSpacing::default());
    // The zones reach 100 to 350; the plates hang 18 px beyond each.
    assert_eq!((bounds.min.y, bounds.max.y), (82.0, 368.0));
}

#[test]
fn a_plan_names_its_legs_for_a_readback() {
    let tool = crate::DrawingTool::by_id("short-position").unwrap();
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 80.0)];
    let plan = position_plan(tool, &anchors, &two_to_one()).unwrap();
    assert_eq!(
        (plan.stop, plan.target, plan.measured),
        (110.0, 80.0, Measured::Target)
    );
    let rectangle = crate::DrawingTool::by_id("rectangle").unwrap();
    assert_eq!(position_plan(rectangle, &anchors, &two_to_one()), None);
}

#[test]
fn a_target_projected_off_the_chart_keeps_its_chip_on_it() {
    let scale = scale();
    // Entry 100 at y 1000, stop 60 measured: target 180 at y 200, above a
    // chart that starts at y 400.
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 60.0)];
    let points = screen(&scale, &anchors);
    let payload = two_to_one();
    let ctxt = context(&payload, &anchors, &scale);
    let mut scene = MarkScene::default();
    let chart = PxRect::from_min_max(pos2(0.0, 400.0), pos2(1000.0, 2000.0));
    paint(
        &mut scene,
        chart,
        DrawingStyle::default(),
        &points,
        &ctxt,
        Side::Long,
    );
    let target_at = scene
        .shapes
        .iter()
        .find_map(|shape| match shape {
            crate::MarkShape::Text { text, at, .. } if text.starts_with("Target") => Some(*at),
            _ => None,
        })
        .unwrap();
    assert!(target_at.y >= 400.0, "{target_at:?}");
}

#[test]
fn a_plan_entirely_off_the_chart_paints_no_chip() {
    let scale = scale();
    // Entry 100 at y 1000 and every leg below a chart that ends at y 500.
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 95.0)];
    let points = screen(&scale, &anchors);
    let payload = two_to_one();
    let ctxt = context(&payload, &anchors, &scale);
    let mut scene = MarkScene::default();
    let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 500.0));
    paint(
        &mut scene,
        chart,
        DrawingStyle::default(),
        &points,
        &ctxt,
        Side::Long,
    );
    assert!(!scene.shapes.iter().any(|shape| matches!(
        shape,
        crate::MarkShape::Text { text, .. } if text.starts_with("Target") || text.starts_with("Stop")
    )));
}

#[test]
fn a_pinned_leg_reserves_its_own_reach_and_the_painted_width() {
    let anchors = PxRect::from_min_max(pos2(100.0, 200.0), pos2(100.0, 250.0));
    let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 1000.0));
    let payload = PositionPayload {
        ratio: 2.0,
        projected_offset: Some(30.0),
        ..PositionPayload::default()
    };
    let bounds = LONG.content_bounds(
        &payload,
        anchors,
        chart,
        crate::TextSpacing::new(10.0, 10.0),
    );
    assert_eq!((bounds.min.y, bounds.max.y), (-118.0, 568.0));
    assert_eq!(bounds.max.x, 100.0 + MIN_WIDTH_BARS * 10.0);
}

#[test]
fn a_rigid_move_whose_shifts_round_apart_still_carries_the_pin() {
    let payload = PositionPayload {
        ratio: 2.0,
        projected_offset: Some(-4.0),
        ..PositionPayload::default()
    };
    let before = [ChartPoint::at(10.0, 2.675), ChartPoint::at(30.0, 1.005)];
    let after = before.map(|point| ChartPoint::at(point.bar, point.price + 0.01));
    assert_ne!(
        after[0].price - before[0].price,
        after[1].price - before[1].price
    );
    assert!(reanchored(&payload, &before, &after, Side::Short).is_none());
}

fn painted(payload: &PositionPayload, selected: bool) -> MarkScene {
    let scale = scale();
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 90.0)];
    let points = screen(&scale, &anchors);
    let mut ctxt = context(payload, &anchors, &scale);
    ctxt.selected = selected;
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
    scene
}

#[test]
fn an_unselected_position_is_two_zones_in_its_own_colours_and_no_words() {
    let payload = PositionPayload::default();
    let scene = painted(&payload, false);
    assert!(
        !scene
            .shapes
            .iter()
            .any(|shape| matches!(shape, crate::MarkShape::Text { .. })),
        "no plate while unselected"
    );
    let fills: Vec<_> = scene
        .shapes
        .iter()
        .filter_map(|shape| match shape {
            crate::MarkShape::Rect { fill, .. } => Some(*fill),
            _ => None,
        })
        .collect();
    let zone = |[r, g, b]: [u8; 3]| crate::rgba_unmultiplied(r, g, b, payload.zone_alpha());
    assert_eq!(
        fills,
        [zone(payload.stop_rgb), zone(payload.target_rgb)],
        "the zones take the drawing's colours, never the theme's buy and sell ink"
    );
    assert_eq!(payload.zone_alpha(), 25, "10 % opacity");
    let edges = scene
        .shapes
        .iter()
        .filter(|shape| matches!(shape, crate::MarkShape::Line { .. }))
        .count();
    assert_eq!(edges, 2, "the two thin zone edges and no entry line");
}

#[test]
fn a_selected_position_inks_its_plates_in_its_own_colours() {
    let payload = PositionPayload {
        target_rgb: [1, 2, 3],
        stop_rgb: [4, 5, 6],
        ..two_to_one()
    };
    let plates: Vec<_> = painted(&payload, true)
        .shapes
        .iter()
        .filter_map(|shape| match shape {
            crate::MarkShape::Rect {
                fill, rounding_px, ..
            } if *rounding_px > 0.0 => Some(*fill),
            _ => None,
        })
        .collect();
    assert_eq!(plates[0], crate::rgba_unmultiplied(1, 2, 3, u8::MAX));
    assert_eq!(plates[1], crate::rgba_unmultiplied(4, 5, 6, u8::MAX));
}

#[test]
fn a_fresh_position_opens_at_one_to_one_in_the_factory_colours() {
    let payload = LONG.default_payload();
    let payload = payload.as_any().downcast_ref::<PositionPayload>().unwrap();
    assert_eq!(payload.ratio, 1.0);
    assert_eq!(DEFAULT_RATIO, 1.0);
    assert_eq!(payload.target_rgb, [0x2E, 0xA0, 0x64]);
    assert_eq!(payload.stop_rgb, [0xD6, 0x30, 0x31]);
    assert_eq!(payload.zone_opacity_pct, 10);
    assert!(crate::position_preset_of(payload.ratio).is_some());
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 95.0)];
    let plan = plan_of(Side::Long, &anchors, payload).unwrap();
    assert_eq!((plan.stop, plan.target), (95.0, 105.0));
}

/// A preset moves the target and nothing else, whichever leg was measured
/// and whether a drag had made the plan custom.
#[test]
fn a_preset_moves_only_the_target() {
    let cases = [
        // A custom stop-measured long, a target-measured long, and a
        // target-measured short whose stop a drag pinned.
        (Side::Long, 90.0, Some(13.0)),
        (Side::Long, 120.0, None),
        (Side::Short, 80.0, Some(7.0)),
    ];
    for (side, edge, pin) in cases {
        let mut anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, edge)];
        let mut payload = PositionPayload {
            projected_offset: pin,
            ..two_to_one()
        };
        let before = plan_of(side, &anchors, &payload).unwrap();
        set_plan_ratio(side, &mut anchors, &mut payload, 3.0).unwrap();
        let after = plan_of(side, &anchors, &payload).unwrap();
        assert_eq!(after.entry, before.entry, "{side:?} {edge}: entry");
        assert_eq!(after.stop, before.stop, "{side:?} {edge}: stop");
        assert!(
            (after.ratio() - 3.0).abs() < 1e-12,
            "{side:?} {edge}: target"
        );
        assert!(crate::position_preset_of(payload.ratio).is_some());
        assert_eq!(anchors[0], ChartPoint::at(10.0, 100.0));
    }
}

#[test]
fn a_ratio_outside_the_readable_range_or_a_flat_plan_changes_nothing() {
    let mut anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 90.0)];
    let mut payload = two_to_one();
    for ratio in [0.0, MAX_RATIO + 1.0, f64::NAN] {
        assert!(set_plan_ratio(Side::Long, &mut anchors, &mut payload, ratio).is_err());
    }
    let mut flat = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 100.0)];
    assert!(set_plan_ratio(Side::Long, &mut flat, &mut payload, 2.0).is_err());
    assert_eq!(payload, two_to_one());
    assert_eq!(anchors[1].price, 90.0);
}

#[test]
fn the_width_handles_sit_at_the_entry_lines_ends() {
    let scale = scale();
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 90.0)];
    let points = screen(&scale, &anchors);
    let payload = two_to_one();
    let ctxt = context(&payload, &anchors, &scale);
    let handles = handles(&points, &ctxt, Side::Long).unwrap();
    let entry_y = scale.y(100.0);
    assert_eq!(
        handles.as_slice(),
        &[pos2(100.0, entry_y), pos2(300.0, entry_y)]
    );
}

/// An edge drag resizes the columns and leaves every price where it was;
/// the other edge holds, and the width never drops under the floor.
#[test]
fn an_edge_drag_changes_the_width_and_no_leg_price() {
    let scale = scale();
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 90.0)];
    let points = screen(&scale, &anchors);
    let payload = two_to_one();
    let ctxt = context(&payload, &anchors, &scale);
    let before = plan_of(Side::Long, &anchors, &payload).unwrap();
    let wider = drag_payload(
        &points,
        RIGHT_EDGE_HANDLE,
        pos2(500.0, 0.0),
        &ctxt,
        Side::Long,
    )
    .unwrap();
    let wider = wider.as_any().downcast_ref::<PositionPayload>().unwrap();
    // The anchors are 20 bars apart: one span unit. 400 px is two units.
    assert_eq!(wider.span, Some([0.0, 2.0]));
    assert_eq!(wider.width_bars(&anchors), 40.0);
    assert_eq!(plan_of(Side::Long, &anchors, wider), Some(before));
    let ctxt = context(wider, &anchors, &scale);
    let narrowest = drag_payload(
        &points,
        LEFT_EDGE_HANDLE,
        pos2(450.0, 900.0),
        &ctxt,
        Side::Long,
    )
    .unwrap();
    let narrowest = narrowest
        .as_any()
        .downcast_ref::<PositionPayload>()
        .unwrap();
    // The 14-bar floor is 0.7 of a 20-bar unit.
    assert_eq!(narrowest.span, Some([1.3, 2.0]));
    assert_eq!(plan_of(Side::Long, &anchors, narrowest), Some(before));
}

#[test]
fn a_dragged_measured_edge_makes_the_plan_custom_at_the_ratio_it_shows() {
    let mut drawings = placed_plan();
    // Stop 95 to 96 with the target held at 110: risk 4, reward 10.
    assert!(drawings.set_points(
        0,
        &[ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 96.0)]
    ));
    let payload = drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<PositionPayload>()
        .unwrap();
    assert!((payload.ratio - 2.5).abs() < 1e-9);
    assert!(crate::position_preset_of(payload.ratio).is_none());
}

#[test]
fn the_plan_plate_names_the_side_and_the_ratio_risk_first() {
    assert_eq!(
        plan_plate(Side::Short, 1.5, 165_461.58),
        "Short · risk 1 : reward 1.5 · entry 165461.58"
    );
}

/// The handles a selected position shows are squares — the entry line's two
/// width ends — and the halo traces its edges in the
/// selection's ink, never in a zone colour.
#[test]
fn a_selected_position_shows_square_handles_and_a_halo_in_selection_ink() {
    let scale = scale();
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 90.0)];
    let points = screen(&scale, &anchors);
    let payload = two_to_one();
    let ctxt = context(&payload, &anchors, &scale);
    let mut scene = MarkScene::default();
    let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 2000.0));
    let tool = crate::DrawingTool::by_id("long-position").unwrap();
    tool.paint(
        &mut scene,
        chart,
        DrawingStyle::default(),
        &points,
        &ctxt,
        true,
    );
    let ring = crate::SELECTED_ANCHOR_RING_WIDTH_PX;
    let squares = scene
        .shapes
        .iter()
        .filter(
            |shape| matches!(shape, crate::MarkShape::Rect { stroke, .. } if stroke.width == ring),
        )
        .count();
    assert_eq!(squares, 2);
    assert!(
        !scene
            .shapes
            .iter()
            .any(|shape| matches!(shape, crate::MarkShape::Circle { .. }))
    );
    let halo = ctxt.appearance.color(Token::DrawingSelectionHalo);
    let halo_edges = scene
        .shapes
        .iter()
        .filter(
            |shape| matches!(shape, crate::MarkShape::Line { stroke, .. } if stroke.color == halo),
        )
        .count();
    assert_eq!(halo_edges, 2);
}

/// The plan plate sits outside the box, so no width handle at the entry
/// line's ends covers its words: left of the box, or right of it when the
/// pane has no room on the left.
#[test]
fn the_plan_plate_never_sits_under_a_width_handle() {
    let payload = two_to_one();
    let scene = painted(&payload, true);
    let plan_at = |scene: &MarkScene| {
        scene
            .shapes
            .iter()
            .find_map(|shape| match shape {
                crate::MarkShape::Text { text, at, .. } if text.starts_with("Long") => Some(*at),
                _ => None,
            })
            .unwrap()
    };
    // The box spans x 100 to 300; its handles sit on those two columns.
    let x = plan_at(&scene).x;
    let handle = crate::SELECTED_ANCHOR_RADIUS_PX;
    assert!(
        x < 100.0 - handle || x > 300.0 + handle,
        "the plate's words start outside the box and its handles: {x}"
    );
}

/// A press on the entry anchor's corner grabs the width handle there: the
/// anchor has no dot of its own to take the press instead.
#[test]
fn a_press_on_the_entry_corner_grabs_the_width_handle() {
    let scale = scale();
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 90.0)];
    let points = screen(&scale, &anchors);
    let payload = two_to_one();
    let ctxt = context(&payload, &anchors, &scale);
    let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 2000.0));
    let tool = crate::DrawingTool::by_id("long-position").unwrap();
    assert_eq!(
        tool.hit_handle(chart, &points, points[0], 6.0, &ctxt),
        Some(LEFT_EDGE_HANDLE)
    );
    let right = pos2(300.0, scale.y(100.0));
    assert_eq!(
        tool.hit_handle(chart, &points, right, 6.0, &ctxt),
        Some(RIGHT_EDGE_HANDLE)
    );
}

/// A width stored at the floor reads back a hair under it in `f32`; the
/// saved drawing still loads with its look.
#[test]
fn a_position_saved_at_its_narrowest_still_loads() {
    let mut narrow = PositionPayload {
        target_rgb: [1, 2, 3],
        ..two_to_one()
    };
    narrow.span = Some([0.0, 0.7 - 1e-6]);
    let mut restored = PositionPayload::default();
    assert!(restored.import_saved(&narrow.export_saved().unwrap()));
    assert_eq!(restored, narrow);
}

/// A resized width is held in units of the anchors' span, so when the
/// anchors re-project onto a coarser timeframe the box keeps its length in
/// time instead of its count of bars.
#[test]
fn a_resized_width_keeps_its_time_across_a_timeframe_switch() {
    let payload = PositionPayload {
        span: Some([0.0, 3.0]),
        ..two_to_one()
    };
    // One-minute bars: anchors 20 bars apart at 10 px a bar.
    let minute = columns(pos2(100.0, 0.0), pos2(300.0, 0.0), &payload, 10.0);
    assert_eq!(minute, (100.0, 700.0));
    // The same anchors on five-minute bars sit 4 bars apart; the floor is
    // 14 bars, so three spans of 4 bars round up to it.
    let five = columns(pos2(100.0, 0.0), pos2(140.0, 0.0), &payload, 10.0);
    assert_eq!(five, (100.0, 240.0), "fourteen bars, the floor");
    let wide = PositionPayload {
        span: Some([0.0, 6.0]),
        ..two_to_one()
    };
    let five = columns(pos2(100.0, 0.0), pos2(140.0, 0.0), &wide, 10.0);
    assert_eq!(five, (100.0, 340.0), "six spans of four bars");
}
