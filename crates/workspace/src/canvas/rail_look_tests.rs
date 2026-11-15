//! A rail's marks and words: the band, an edge on each long side the
//! window's chrome does not already line, a chevron toward the side the pane
//! opens to and a tick per pane behind it, all inside the rail.

use super::*;

const CANVAS: CanvasRect = CanvasRect::new(44.0, 80.0, 1320.0, 722.0);
/// A rail standing between panes: neither long side on the canvas's edge.
const INNER: CanvasRect = CanvasRect::new(600.0, 80.0, COLLAPSED_PANE_WIDTH_PX, 722.0);

fn look(flat: bool, forward: bool, ticks: usize, on_chrome: [bool; 2]) -> RailLook {
    RailLook {
        flat,
        opens_forward: forward,
        ticks,
        on_chrome,
    }
}

fn inked(marks: &[(CanvasRect, RailInk)], ink: RailInk) -> Vec<CanvasRect> {
    (marks.iter())
        .filter(|(_, worn)| *worn == ink)
        .map(|(at, _)| *at)
        .collect()
}

fn inside(rect: CanvasRect, rail: CanvasRect) -> bool {
    rect.x >= rail.x
        && rect.y >= rail.y
        && rect.right() <= rail.right()
        && rect.bottom() <= rail.bottom()
}

/// Between two panes: the band, both edges, a chevron of five rows pointing
/// right centred on the rail's length, and one tick after it.
#[test]
fn a_rail_between_panes_paints_its_band_both_edges_a_chevron_and_a_tick() {
    let marks = rail_marks(INNER, look(false, true, 1, [false, false]));
    assert_eq!(marks[0], (INNER, RailInk::Band));
    assert_eq!(
        inked(&marks, RailInk::Edge),
        [
            CanvasRect::new(600.0, 80.0, 1.0, 722.0),
            CanvasRect::new(607.0, 80.0, 1.0, 722.0)
        ]
    );
    let grip = inked(&marks, RailInk::Grip);
    let (chevron, ticks) = grip.split_at(RAIL_CHEVRON_ROWS);
    let start = 80.0 + ((722.0 - RAIL_CHEVRON_ROWS as f32) / 2.0).floor();
    let across: Vec<f32> = chevron.iter().map(|row| row.x - 600.0).collect();
    assert_eq!(across, [2.0, 3.0, 4.0, 3.0, 2.0], "it points right");
    for (row, at) in chevron.iter().enumerate() {
        assert_eq!((at.y, at.w, at.h), (start + row as f32, 2.0, 1.0));
    }
    let first = start + RAIL_CHEVRON_ROWS as f32 + RAIL_TICKS_AFTER_PX;
    assert_eq!(ticks, [CanvasRect::new(603.0, first, 2.0, 2.0)]);
}

/// A rail opening left points left; one folded flat points up or down and
/// lays its ticks along its length.
#[test]
fn the_chevron_points_the_way_the_pane_opens() {
    let left = rail_marks(INNER, look(false, false, 0, [false, false]));
    let across: Vec<f32> = (inked(&left, RailInk::Grip).iter())
        .map(|row| row.x - 600.0)
        .collect();
    assert_eq!(across, [4.0, 3.0, 2.0, 3.0, 4.0]);

    let flat = CanvasRect::new(44.0, 300.0, 1320.0, COLLAPSED_PANE_WIDTH_PX);
    let marks = rail_marks(flat, look(true, true, 4, [false, false]));
    let grip = inked(&marks, RailInk::Grip);
    let down: Vec<f32> = grip[..RAIL_CHEVRON_ROWS]
        .iter()
        .map(|row| row.y - 300.0)
        .collect();
    assert_eq!(down, [2.0, 3.0, 4.0, 3.0, 2.0]);
    assert!(
        grip[..RAIL_CHEVRON_ROWS]
            .iter()
            .all(|row| row.w == 1.0 && row.h == 2.0)
    );
    let ticks = &grip[RAIL_CHEVRON_ROWS..];
    assert_eq!(ticks.len(), 4, "one per chart of the grid");
    for pair in ticks.windows(2) {
        assert_eq!(pair[1].x - pair[0].x, RAIL_TICK_PX + RAIL_TICK_GAP_PX);
        assert_eq!(pair[0].y, 303.0);
    }
}

/// A long side on the canvas's own edge shares the chrome's line there:
/// its edge is left out, the other kept. Upright at either window edge and
/// flat under the toolbar.
#[test]
fn a_side_on_the_canvas_edge_leaves_its_line_to_the_chrome() {
    let held = 1;
    let left = CanvasRect::new(44.0, 80.0, COLLAPSED_PANE_WIDTH_PX, 722.0);
    let at_left = RailLook::on(left, CANVAS, (false, true), held);
    assert_eq!(at_left.on_chrome, [true, false]);
    let edges = inked(&rail_marks(left, at_left), RailInk::Edge);
    assert_eq!(edges, [CanvasRect::new(51.0, 80.0, 1.0, 722.0)]);

    let right = CanvasRect::new(1356.0, 80.0, COLLAPSED_PANE_WIDTH_PX, 722.0);
    let at_right = RailLook::on(right, CANVAS, (false, false), held);
    assert_eq!(at_right.on_chrome, [false, true]);
    let edges = inked(&rail_marks(right, at_right), RailInk::Edge);
    assert_eq!(edges, [CanvasRect::new(1356.0, 80.0, 1.0, 722.0)]);

    let top = CanvasRect::new(44.0, 80.0, 1320.0, COLLAPSED_PANE_WIDTH_PX);
    let flat = RailLook::on(top, CANVAS, (true, true), held);
    assert_eq!(flat.on_chrome, [true, false]);
    let edges = inked(&rail_marks(top, flat), RailInk::Edge);
    assert_eq!(edges, [CanvasRect::new(44.0, 87.0, 1320.0, 1.0)]);

    let between = RailLook::on(INNER, CANVAS, (false, true), held);
    assert_eq!(between.on_chrome, [false, false]);
}

/// Ticks stop at the cap; a rail too short for the chevron and its ticks
/// drops the ticks, then keeps the chevron rows that fit; nothing ever lands
/// outside the rail.
#[test]
fn a_short_rail_drops_its_ticks_then_clamps_its_chevron() {
    let many = rail_marks(INNER, look(false, true, 12, [false, false]));
    assert_eq!(
        inked(&many, RailInk::Grip).len(),
        RAIL_CHEVRON_ROWS + RAIL_MAX_TICKS
    );

    let short = CanvasRect::new(0.0, 0.0, COLLAPSED_PANE_WIDTH_PX, 12.0);
    let marks = rail_marks(short, look(false, true, 4, [false, false]));
    assert_eq!(
        inked(&marks, RailInk::Grip).len(),
        RAIL_CHEVRON_ROWS,
        "no ticks"
    );

    let tiny = CanvasRect::new(0.0, 0.0, COLLAPSED_PANE_WIDTH_PX, 3.0);
    let marks = rail_marks(tiny, look(false, true, 4, [false, false]));
    assert_eq!(inked(&marks, RailInk::Grip).len(), 3, "the rows that fit");
    for rail in [INNER, short, tiny] {
        for ticks in [0, 1, 4, 12] {
            for flat in [false, true] {
                let rail = if flat {
                    CanvasRect::new(rail.x, rail.y, rail.h, rail.w)
                } else {
                    rail
                };
                for (at, _) in rail_marks(rail, look(flat, ticks % 2 == 0, ticks, [false; 2])) {
                    assert!(inside(at, rail), "{at:?} outside {rail:?}");
                }
            }
        }
    }
}

/// The words a rail says on hover and in the scene: a lone pane by its
/// name, otherwise by count, then how to open it.
#[test]
fn a_rail_says_what_it_holds_and_how_to_open_it() {
    let chart = RailHolds::of([PaneKind::Time]);
    assert_eq!(
        chart.words(Some("Time · 1m · WINQ26")),
        "Time · 1m · WINQ26 · drag to open"
    );
    assert_eq!(chart.words(None), "1 chart · drag to open");
    let flow = RailHolds::of([PaneKind::Flow]);
    assert_eq!(flow.words(None), "Order flow · drag to open");
    let grid = RailHolds::of([PaneKind::Time; 4]);
    assert_eq!(
        (grid.panes(), grid.words(Some("ignored"))),
        (4, "4 charts · drag to open".to_owned())
    );
    let both = RailHolds::of([PaneKind::Time, PaneKind::Flow]);
    assert_eq!(both.words(None), "2 panes · drag to open");
}

/// The grid's context side folded behind one rail: it holds four charts,
/// points right, leaves its left line to the window's chrome, and the seam
/// beside it is no line of its own.
#[test]
fn the_grid_folded_behind_one_rail_holds_four_charts_and_its_seam_is_no_line() {
    let mut state = CanvasState::from_preset(preset("grid4+flow").unwrap());
    assert!(state.fold_flow_side(false, true));
    let rails = tree_rails(&state, CANVAS, CANVAS_DIVIDER_PX);
    assert_eq!(rails.len(), 1);
    assert_eq!(rails[0].holds, RailHolds::of([PaneKind::Time; 4]));
    assert_eq!(state.side_rail_holds(false), rails[0].holds);
    assert_eq!(state.side_rail_holds(true), RailHolds::of([PaneKind::Flow]));
    assert_eq!(rails[0].look(CANVAS), look(false, true, 4, [true, false]));
    let seams = tree_seams(&state, CANVAS, CANVAS_DIVIDER_PX);
    let beside = seams
        .iter()
        .find(|seam| seam.rail.is_some())
        .expect("the rail's seam");
    assert!(!beside.is_line());
    let open = CanvasState::from_preset(preset("grid4+flow").unwrap());
    assert!(
        tree_seams(&open, CANVAS, CANVAS_DIVIDER_PX)
            .iter()
            .all(TreeSeam::is_line)
    );
}
