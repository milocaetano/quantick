use super::*;

fn canvas() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0))
}

/// The row spends the canvas exactly once, whatever it holds. A sliver of
/// unpainted canvas between two panes reads as a rendering fault.
fn assert_spends_the_canvas(area: egui::Rect, areas: &RowAreas) {
    let spent: f32 = areas.panes.iter().map(|pane| pane.width()).sum::<f32>()
        + areas
            .dividers
            .iter()
            .map(|divider| divider.width())
            .sum::<f32>();
    assert!(
        (spent - area.width()).abs() < 1e-3,
        "the row spent {spent} of a {} canvas",
        area.width()
    );
    assert_eq!(areas.panes.first().map(egui::Rect::left), Some(area.left()));
    assert_eq!(
        areas.panes.last().map(egui::Rect::right),
        Some(area.right())
    );
    for pane in &areas.panes {
        assert_eq!(pane.top(), area.top(), "the split is vertical only");
        assert_eq!(pane.bottom(), area.bottom());
    }
}

#[test]
fn a_lone_pane_takes_the_whole_canvas_and_needs_no_divider() {
    let area = canvas();
    let areas = split_row(area, &[PaneWidth::Auto]);
    assert_eq!(areas.panes.as_slice(), &[area]);
    assert!(areas.dividers.is_empty(), "one pane has no seam");
}

#[test]
fn there_is_always_one_fewer_divider_than_pane() {
    for count in 1..=MAX_CANVAS_PANES {
        let widths = vec![PaneWidth::Auto; count];
        let areas = split_row(canvas(), &widths);
        assert_eq!(areas.panes.len(), count);
        assert_eq!(
            areas.dividers.len(),
            count - 1,
            "{count} panes must have {} seams",
            count - 1
        );
    }
}

#[test]
fn every_pane_count_spends_the_canvas_exactly_once() {
    for count in 1..=MAX_CANVAS_PANES {
        let widths = vec![PaneWidth::Auto; count];
        let area = canvas();
        assert_spends_the_canvas(area, &split_row(area, &widths));
    }
}

/// The two-pane row must land where the old fixed splitter did, or every
/// saved workspace would open on a canvas that shifted under it.
#[test]
fn a_two_pane_row_puts_the_divider_on_the_share_it_was_given() {
    let area = canvas();
    for asked in [0.25_f32, 0.35, 0.5, 0.65, 0.75] {
        let areas = split_row(area, &[PaneWidth::Manual(asked), PaneWidth::Auto]);
        let split = (areas.dividers[0].center().x - area.left()) / area.width();
        assert!(
            (split - asked).abs() < 1e-3,
            "asked for {asked}, divider landed at {split}"
        );
        assert_eq!(areas.dividers[0].width(), CANVAS_DIVIDER_PX);
        assert_eq!(areas.panes[0].right(), areas.dividers[0].left());
        assert_eq!(areas.dividers[0].right(), areas.panes[1].left());
        assert_spends_the_canvas(area, &areas);
    }
}

/// The shape the old model could not express at all: two context panes
/// beside the flow pane.
#[test]
fn a_three_pane_row_carves_two_seams_in_order() {
    let area = canvas();
    let areas = split_row(
        area,
        &[
            PaneWidth::Manual(0.175),
            PaneWidth::Manual(0.175),
            PaneWidth::Auto,
        ],
    );
    assert_eq!(areas.panes.len(), 3);
    assert_eq!(areas.dividers.len(), 2);
    assert!(areas.panes[0].right() <= areas.panes[1].left());
    assert!(areas.panes[1].right() <= areas.panes[2].left());
    assert!(
        areas.panes[2].width() > areas.panes[0].width() + areas.panes[1].width(),
        "the flow pane keeps the majority of the canvas"
    );
    assert_spends_the_canvas(area, &areas);
}

#[test]
fn a_collapsed_pane_takes_its_rail_and_no_more() {
    let area = canvas();
    let areas = split_row(
        area,
        &[PaneWidth::Collapsed { restore: 0.35 }, PaneWidth::Auto],
    );
    assert!(
        (areas.panes[0].width() - COLLAPSED_PANE_WIDTH_PX).abs() < 1.0,
        "a collapsed pane is a rail, got {} px",
        areas.panes[0].width()
    );
    assert!(
        areas.panes[0].width() > 0.0,
        "a collapsed pane is never zero — there would be nothing left to click"
    );
    assert_spends_the_canvas(area, &areas);
}

#[test]
fn every_pane_collapsed_still_spends_the_canvas() {
    let area = canvas();
    let widths = vec![PaneWidth::Collapsed { restore: 0.25 }; 3];
    let areas = split_row(area, &widths);
    assert_eq!(areas.panes.len(), 3);
    assert_spends_the_canvas(area, &areas);
}

/// A window dragged narrower than the panes' floors is a real state, and
/// the row must degrade rather than overflow: the panes get thin, the
/// canvas is still spent once, and nothing is painted outside it.
#[test]
fn a_canvas_too_narrow_for_its_panes_degrades_instead_of_overflowing() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(60.0, 600.0));
    let areas = split_row(area, &[PaneWidth::Auto, PaneWidth::Auto, PaneWidth::Auto]);
    assert_eq!(areas.panes.len(), 3);
    for pane in &areas.panes {
        assert!(pane.left() >= area.left() - 1e-3);
        assert!(pane.right() <= area.right() + 1e-3);
        assert!(pane.width() >= 0.0, "a pane never has negative width");
    }
    assert_spends_the_canvas(area, &areas);
}

#[test]
fn a_zero_width_canvas_is_survived_rather_than_panicked_on() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(0.0, 600.0));
    let areas = split_row(area, &[PaneWidth::Auto, PaneWidth::Auto]);
    assert_eq!(areas.panes.len(), 2);
    for pane in &areas.panes {
        assert!(pane.width() >= 0.0);
    }
}

#[test]
fn an_empty_row_draws_nothing() {
    let areas = split_row(canvas(), &[]);
    assert!(areas.panes.is_empty());
    assert!(areas.dividers.is_empty());
}
