use super::*;

fn canvas() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 900.0))
}

/// The context column stacks its panes top to bottom, and spends its
/// height exactly once — the vertical twin of the row's own rule.
#[test]
fn a_column_stacks_top_to_bottom_and_spends_its_height_once() {
    let area = canvas();
    let areas = split_column(area, &[PaneWidth::Auto, PaneWidth::Auto]);
    assert_eq!(areas.panes.len(), 2);
    assert_eq!(areas.dividers.len(), 1);
    assert!(
        areas.panes[0].bottom() <= areas.panes[1].top(),
        "the first pane is the upper one"
    );
    let spent: f32 = areas.panes.iter().map(egui::Rect::height).sum::<f32>()
        + areas.dividers.iter().map(egui::Rect::height).sum::<f32>();
    assert!((spent - area.height()).abs() < 1e-3, "spent {spent}");
    assert_eq!(areas.panes[0].top(), area.top());
    assert_eq!(areas.panes[1].bottom(), area.bottom());
}

/// A column keeps the full width: stacking splits one axis only, exactly
/// as the row keeps the full height.
#[test]
fn stacking_never_narrows_a_pane() {
    let area = canvas();
    for count in 1..=MAX_CONTEXT_PANES {
        let areas = split_column(area, &vec![PaneWidth::Auto; count]);
        for pane in &areas.panes {
            assert_eq!(pane.left(), area.left());
            assert_eq!(pane.right(), area.right());
        }
    }
}

/// The two axes are one function, so a divider dragged vertically behaves
/// exactly as one dragged horizontally. Asserted by transposing a canvas
/// and comparing the split it produces.
#[test]
fn a_column_splits_where_a_row_of_the_same_shares_would() {
    let widths = [PaneWidth::Manual(0.35), PaneWidth::Auto];
    let across = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
    let down = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(600.0, 1000.0));

    let row = split_row(across, &widths);
    let column = split_column(down, &widths);
    assert!(
        (row.panes[0].width() - column.panes[0].height()).abs() < 1e-3,
        "the same shares must carve the same extent on either axis: \
             row gave {} and column gave {}",
        row.panes[0].width(),
        column.panes[0].height()
    );
    assert!((row.dividers[0].width() - column.dividers[0].height()).abs() < 1e-3);
}

#[test]
fn a_collapsed_pane_in_a_column_is_the_same_rail_a_row_gives() {
    let area = canvas();
    let areas = split_column(
        area,
        &[PaneWidth::Collapsed { restore: 0.5 }, PaneWidth::Auto],
    );
    assert!(
        (areas.panes[0].height() - COLLAPSED_PANE_WIDTH_PX).abs() < 1e-3,
        "a stacked rail came out {} px",
        areas.panes[0].height()
    );
}
