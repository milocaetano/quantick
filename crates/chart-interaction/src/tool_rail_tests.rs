// The rail geometry's tests, moved out of the app's toolrail module with the
// code they cover. The ones that need the shipped drawing registry stay there.

use super::*;

#[test]
fn rail_thickness_is_margin_button_margin() {
    assert_eq!(
        TOOLBOX_THICKNESS_PX,
        2.0 * TOOLBOX_MARGIN_PX + BUTTON_HIT_PX
    );
}

#[test]
fn nearest_picks_each_edge_from_its_midpoint() {
    let screen = Area::from_min_size(0.0, 0.0, 800.0, 600.0);
    assert_eq!(
        ToolboxDock::nearest((10.0, 300.0), screen),
        ToolboxDock::Left
    );
    assert_eq!(
        ToolboxDock::nearest((400.0, 10.0), screen),
        ToolboxDock::Top
    );
    assert_eq!(
        ToolboxDock::nearest((400.0, 590.0), screen),
        ToolboxDock::Bottom
    );
}

/// The right border is not on offer. A drop anywhere in the right half —
/// including the far edge, where the old rail would have docked — lands
/// on one of the three edges that exist, and never on the price side of
/// the window.
#[test]
fn nothing_dropped_on_the_right_edge_docks_there() {
    let screen = Area::from_min_size(0.0, 0.0, 800.0, 600.0);
    for point in [
        (799.0, 300.0),
        (790.0, 100.0),
        (760.0, 500.0),
        (799.0, 599.0),
    ] {
        let dock = ToolboxDock::nearest(point, screen);
        assert!(
            matches!(
                dock,
                ToolboxDock::Left | ToolboxDock::Top | ToolboxDock::Bottom
            ),
            "a drop at {point:?} resolved to {dock:?}"
        );
    }
}

#[test]
fn nearest_normalises_so_a_wide_screen_does_not_bias_top_bottom() {
    let screen = Area::from_min_size(0.0, 0.0, 1920.0, 600.0);
    // Raw pixel distances tie at 300 vs 300; normalised 0.156 vs 0.5.
    assert_eq!(
        ToolboxDock::nearest((300.0, 300.0), screen),
        ToolboxDock::Left
    );
}

/// The offset can never leave the content: no blank band at either end,
/// which is the state a trader reads as "the toolbar is lost".
#[test]
fn the_band_offset_never_leaves_the_content() {
    let viewport = run_length(BAND_MIN_VISIBLE_ITEMS);
    assert_eq!(
        band_max_offset(viewport, BAND_MIN_VISIBLE_ITEMS),
        0.0,
        "content that fits cannot scroll"
    );
    assert_eq!(
        band_max_offset(viewport, 2),
        0.0,
        "less content than viewport still cannot scroll"
    );
    let max = band_max_offset(viewport, BAND_MIN_VISIBLE_ITEMS + 3);
    assert!(max > 0.0, "surplus content scrolls");
    assert_eq!(
        max,
        run_length(BAND_MIN_VISIBLE_ITEMS + 3) - viewport,
        "the last button lands flush with the band's far edge"
    );
}

/// The band never shows half a button. The viewport is whole slots, so
/// the travel divides into whole slots too and every position a chevron
/// can reach lands on a button boundary — no star badge floating beside
/// a sliced icon, which is what the first capture of the spill state
/// showed.
#[test]
fn the_band_never_shows_a_sliced_button() {
    let slot = BUTTON_HIT_PX + TOOLBOX_ITEM_GAP_PX;
    for extent in [489.0_f32, 500.0, 517.3, 560.0, 599.9, 632.9] {
        for pins in 0..8 {
            let anchored = anchored_favorites(extent, pins);
            let viewport = band_viewport(extent, anchored);
            let items = band_visible_items(viewport);
            assert_eq!(
                viewport,
                run_length(items),
                "viewport is whole buttons at {extent} px with {pins} pins"
            );
            let max = band_max_offset(viewport, items + 5);
            let steps = max / slot;
            assert!(
                (steps - steps.round()).abs() < 1e-3,
                "travel divides into whole buttons at {extent} px with {pins} pins"
            );
        }
    }
}

/// A chevron click leaves one button of overlap, so the trader keeps a
/// landmark across the jump instead of a fresh screen of icons.
#[test]
fn a_chevron_step_keeps_one_button_of_overlap() {
    let slot = BUTTON_HIT_PX + TOOLBOX_ITEM_GAP_PX;
    let viewport = run_length(5);
    assert_eq!(band_visible_items(viewport), 5);
    assert_eq!(band_scroll_step(viewport), 4.0 * slot);
    // Even a band down to its last button must still move on a click.
    assert!(band_scroll_step(BUTTON_HIT_PX) >= slot);
    assert!(band_scroll_step(0.0) >= slot);
}

/// Pins are anchored while the band keeps its floor, and spill into the
/// band after that — a star can cost a pin its anchor, never its
/// existence.
#[test]
fn favorites_spill_into_the_band_instead_of_evicting_the_toolbar() {
    let extent = scroll_length(0);
    assert_eq!(anchored_favorites(extent, 0), 0);
    assert_eq!(
        anchored_favorites(extent, 4),
        0,
        "the band's floor outranks the anchor"
    );
    let roomy = scroll_length(3);
    assert_eq!(anchored_favorites(roomy, 3), 3, "all three fit anchored");
    assert_eq!(
        anchored_favorites(roomy, 6),
        3,
        "the surplus spills rather than shrinking the band past its floor"
    );
}
