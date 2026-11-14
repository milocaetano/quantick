use super::*;

#[test]
fn full_action_footprint_keeps_the_inspector_title_reachable() {
    let size = egui::vec2(364.0, 61.0);
    let wanted = egui::pos2(47.0, 268.0);
    for bottom in [400.0, 800.0] {
        let chart = egui::Rect::from_min_max(egui::pos2(47.0, 88.0), egui::pos2(900.0, bottom));
        let legend = egui::Rect::from_min_max(chart.min, egui::pos2(600.0, 150.0));
        let inspector = egui::Rect::from_min_max(
            egui::pos2(288.0, 294.0),
            egui::pos2(620.0, bottom.min(535.0)),
        );
        let object = egui::Rect::from_min_max(egui::pos2(47.0, 250.0), egui::pos2(900.0, 251.0));
        let placed = egui::Rect::from_min_size(
            avoid_panels(
                wanted,
                size,
                chart,
                [Some(legend), Some(inspector)],
                Some(object),
            ),
            size,
        );
        assert!(chart.contains_rect(placed));
        assert!(!placed.intersects(legend));
        assert!(!placed.intersects(inspector));
        assert!(!placed.intersects(object));
    }
}

#[test]
fn crowded_panel_fallback_keeps_the_action_footprint_in_the_viewport() {
    let chart = egui::Rect::from_min_size(egui::pos2(47.0, 88.0), egui::vec2(400.0, 100.0));
    let size = egui::vec2(364.0, 61.0);
    let parked = egui::pos2(900.0, 900.0);
    let placed = avoid_panels(parked, size, chart, [Some(chart), None], None);
    assert!(chart.contains_rect(egui::Rect::from_min_size(placed, size)));
    assert_eq!(placed, clamp_into_chart(parked, size, chart));
}

#[test]
fn automatic_and_parked_bars_avoid_the_measured_legend_and_lane() {
    for width in [650.0, 1000.0, 1500.0] {
        let chart = egui::Rect::from_min_size(egui::pos2(60.0, 88.0), egui::vec2(width, 700.0));
        let size = egui::vec2(460.0, 40.0);
        let lane = chart.right() - 100.0;
        let reachable = bounds(chart, lane, size);
        let legend =
            egui::Rect::from_min_size(chart.min + egui::vec2(6.0, 42.0), egui::vec2(500.0, 65.0));
        let bbox = chart.shrink(10.0);
        for wanted in [
            drawings::context_bar::place(chart, lane, bbox, size),
            legend.min,
            egui::pos2(4000.0, legend.top()),
        ] {
            let placed = egui::Rect::from_min_size(
                avoid_panels(wanted, size, reachable, [Some(legend), None], None),
                size,
            );
            assert!(!placed.intersects(legend), "{placed:?} / {legend:?}");
            assert!(reachable.contains_rect(placed));
            assert!(placed.right() <= lane);
        }
    }
}

/// The live lane is off limits to the bar however it got where it is.
///
/// `context_bar::place` has kept clear of that strip since it was written
/// — it is where the price the trader is reading is being formed, and
/// losing sight of it is the trader's first veto. A parked bar is placed
/// by a different rule, so the bound both rules clamp into is what keeps
/// them honest: without it the repair would happily pin a parked bar at
/// the pane's own right edge, on top of the forming column, for every
/// object of the session.
#[test]
fn the_context_bar_bound_takes_the_live_lane_off_the_pane() {
    let chart = egui::Rect::from_min_max(egui::pos2(60.0, 88.0), egui::pos2(1284.0, 832.0));
    let size = egui::vec2(292.0, 40.0);
    let divider = 900.0;

    let reachable = bounds(chart, divider, size);
    assert_eq!(
        reachable.right(),
        divider,
        "the lane is not the bar's to use"
    );
    assert_eq!(
        clamp_into_chart(egui::pos2(3000.0, 400.0), size, reachable).x,
        divider - size.x,
        "a bar parked out past the lane is repaired to its inner edge"
    );

    // No lane on this pane: the chart's own edge is the answer, exactly as
    // the automatic rule takes `chart.right()` when the divider is None.
    assert_eq!(bounds(chart, chart.right(), size).right(), chart.right());

    // A lane wider than the history area leaves nothing to place in. The
    // pane's edge beats an empty rectangle — the same call `place` makes.
    let all_lane = bounds(chart, chart.left() + 10.0, size);
    assert!(
        all_lane.width() >= size.x.min(chart.width()),
        "a bar still has somewhere to go: {all_lane:?}"
    );
    assert!(chart.contains_rect(all_lane), "and it is inside the pane");
}
