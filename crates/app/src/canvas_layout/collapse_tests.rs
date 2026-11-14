use super::*;

/// The rail is near-zero without being zero, and that gap is the whole
/// design: a pane with no width has no handle, and a pane with no handle
/// cannot be brought back from the canvas it left.
#[test]
fn a_collapsed_pane_is_a_sliver_of_the_canvas_but_never_none_of_it() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1920.0, 1080.0));
    let areas = split_row(
        area,
        &[PaneWidth::Collapsed { restore: 0.35 }, PaneWidth::Auto],
    );
    let share = areas.panes[0].width() / area.width();
    assert!(
        share < 0.01,
        "a rail taking {share} of the canvas is not a rail"
    );
    assert!(
        areas.panes[0].width() > 0.0,
        "a rail with no width is the bug this exists to prevent"
    );
}

/// The hit area is bigger than the paint, and by enough to clear the floor
/// a pointer target is held to.
#[test]
fn the_rail_can_be_hit_even_though_it_is_thin() {
    // Read off a rail the splitter actually produced, rather than
    // comparing two literals: what matters is how far the hit area has to
    // reach past the paint that exists.
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1920.0, 1080.0));
    let painted = split_row(
        area,
        &[PaneWidth::Collapsed { restore: 0.35 }, PaneWidth::Auto],
    )
    .panes[0]
        .width();
    assert!(
        COLLAPSED_HIT_PX > painted,
        "the hit area must reach past the {painted}px it paints"
    );
}

/// The gesture that dismisses a pane has to be reachable before the pane's
/// own minimum stops the drag, or a trader could never reach it.
#[test]
fn the_collapse_threshold_is_inside_the_range_a_drag_can_reach() {
    // The threshold is in pixels and so is the floor, which is the whole
    // reason it stopped being a share of the canvas: as a share it sat
    // under the floor on a trading monitor and over it on a laptop, and
    // over it the floor never binds. Read at the widths a real window
    // takes, so the test says what it protects.
    for width in [1024.0_f32, 1280.0, 1600.0, 1920.0, 2560.0] {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(width, 900.0));
        let floor_share = MIN_PANE_WIDTH_PX / area.width();
        let threshold_share = COLLAPSE_AT_PX / area.width();
        assert!(
            threshold_share < floor_share,
            "on a {width}px canvas the pane is dismissed at {threshold_share}                  while its floor is {floor_share}, so every width between them                  is unreachable"
        );
    }
}
