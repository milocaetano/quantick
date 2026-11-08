use super::*;

const SPACING: PanelSpacing = PanelSpacing {
    neighbourhood: NEIGHBOURHOOD_PX,
    gap: 12.0,
    min_width: 300.0,
};
const W: f32 = 320.0;
const H: f32 = 280.0;

fn area(left: f32, top: f32, right: f32, bottom: f32) -> Area {
    Area {
        left,
        top,
        right,
        bottom,
    }
}

fn centred(x: f32, y: f32, side: f32) -> Area {
    area(
        x - side / 2.0,
        y - side / 2.0,
        x + side / 2.0,
        y + side / 2.0,
    )
}

fn rect(placed: PanelPlacement, height: f32) -> Area {
    Area::from_min_size(
        placed.left,
        placed.top,
        placed.max_width.unwrap_or(W),
        height,
    )
}

/// The trader's ask (2026-11-08): drawing settings open next to the drawing,
/// not in a far corner — but never on it, nor on the price action right
/// beside it.
#[test]
fn a_small_object_gets_the_panel_right_beside_it_clear_of_its_neighbourhood() {
    let chart = area(0.0, 0.0, 1_400.0, 800.0);
    let object = centred(500.0, 300.0, 40.0);
    let placed = place_beside(chart, object, W, H, SPACING);
    assert_eq!(
        placed,
        PanelPlacement {
            left: object.right + NEIGHBOURHOOD_PX,
            top: object.top,
            max_width: None,
        },
        "right beside it, top-aligned, on the side with more room"
    );
    let panel = rect(placed, H);
    assert!(chart.contains(panel));
    assert_eq!(panel.overlap(object.expand(NEIGHBOURHOOD_PX)), 0.0);
    assert_eq!(placed, place_beside(chart, object, W, H, SPACING));
}

/// A drawing near the right edge gets its panel on its left — still beside
/// it, not in a corner.
#[test]
fn the_side_with_more_room_wins() {
    let chart = area(0.0, 0.0, 1_400.0, 800.0);
    let object = centred(1_100.0, 400.0, 40.0);
    let placed = place_beside(chart, object, W, H, SPACING);
    assert_eq!(placed.left, object.left - NEIGHBOURHOOD_PX - W);
    assert_eq!(placed.top, object.top);
}

/// Equal room either side: the tie always goes right, or the panel would
/// appear somewhere new every time.
#[test]
fn a_tie_goes_to_the_right() {
    let chart = area(0.0, 0.0, 1_400.0, 800.0);
    let object = centred(700.0, 400.0, 40.0);
    assert!(place_beside(chart, object, W, H, SPACING).left > object.right);
}

/// A horizontal line across the chart leaves no room on either side, so the
/// panel goes below it, clear of it.
#[test]
fn a_drawing_too_wide_to_sit_beside_gets_the_panel_below() {
    let chart = area(0.0, 0.0, 1_400.0, 800.0);
    let object = area(0.0, 200.0, 1_400.0, 204.0);
    let panel = rect(place_beside(chart, object, W, H, SPACING), H);
    assert!(panel.top >= object.bottom + NEIGHBOURHOOD_PX, "{panel:?}");
    assert!(chart.contains(panel));
}

/// Near the bottom, a side placement slides up to stay on the chart instead
/// of losing its last rows below it.
#[test]
fn a_side_placement_near_the_bottom_slides_up_onto_the_chart() {
    let chart = area(60.0, 88.0, 1_284.0, 832.0);
    let object = centred(500.0, 780.0, 40.0);
    let panel = rect(place_beside(chart, object, 360.0, H, SPACING), H);
    assert!(chart.contains(Area {
        right: panel.left + 360.0,
        ..panel
    }));
    assert!(panel.left >= object.right, "still beside it: {panel:?}");
}

/// Hugging the right edge, the right side clamps back onto the object; the
/// left side clears, so it wins.
#[test]
fn an_object_at_the_right_edge_never_gets_the_clamped_right_side() {
    let chart = area(0.0, 0.0, 1_000.0, 600.0);
    let object = area(900.0, 200.0, 1_000.0, 300.0);
    let panel = rect(place_beside(chart, object, W, H, SPACING), H);
    assert_eq!(panel.overlap(object), 0.0);
    assert!(panel.right <= object.left, "{panel:?}");
}

/// No side fits beside a tall, wide-ish object, but a corner is clear: the
/// farthest clear one wins.
#[test]
fn with_no_side_free_the_farthest_clear_corner_wins() {
    let chart = area(0.0, 0.0, 1_000.0, 600.0);
    // Too wide for a panel either side and too tall for one above or below,
    // yet between the corners' columns, so all four stay clear.
    let object = area(340.0, 150.0, 660.0, 450.0);
    let placed = place_beside(chart, object, W, H, SPACING);
    assert_eq!(placed.max_width, None);
    assert_eq!(
        (placed.left, placed.top),
        (12.0, 12.0),
        "the top-left corner; an exact tie takes the first"
    );
}

/// A profile down the middle fouls every corner; the panel goes into the
/// gutter beside it, narrowed, rather than onto the figure.
#[test]
fn a_drawing_too_big_for_any_corner_sends_the_panel_to_a_gutter() {
    let chart = area(0.0, 0.0, 1_200.0, 700.0);
    let object = area(320.0, 0.0, 880.0, 700.0);
    let placed = place_beside(chart, object, W, H, SPACING);
    let panel = rect(placed, H);
    assert_eq!(panel.overlap(object), 0.0, "{panel:?}");
    assert!(chart.contains(panel));
    assert!(
        placed
            .max_width
            .is_some_and(|width| width >= SPACING.min_width)
    );
}

/// When nothing clears, the chosen spot covers the object least of all the
/// candidates.
#[test]
fn with_nothing_clear_the_least_overlap_wins() {
    let chart = area(0.0, 0.0, 1_000.0, 600.0);
    let object = area(50.0, 30.0, 950.0, 570.0);
    let placed = place_beside(chart, object, W, H, SPACING);
    let chosen = rect(placed, H).overlap(object);
    for (left, top) in [(12.0, 12.0), (668.0, 12.0), (12.0, 308.0), (668.0, 308.0)] {
        let other = Area::from_min_size(left, top, W, H).overlap(object);
        assert!(chosen <= other + 0.01, "{chosen} vs {other}");
    }
    assert!(chart.contains(rect(placed, H)));
}

#[test]
fn clamp_into_moves_the_least_distance_inside() {
    let chart = area(0.0, 0.0, 1_000.0, 600.0);
    assert_eq!(clamp_into(900.0, -5.0, 200.0, 100.0, chart), (800.0, 0.0));
    assert_eq!(clamp_into(10.0, 20.0, 200.0, 100.0, chart), (10.0, 20.0));
    assert_eq!(
        clamp_into(10.0, 20.0, 2_000.0, 100.0, chart),
        (0.0, 20.0),
        "too wide: pinned to the left edge"
    );
}
