//! A rail's marks, light and words, as the trader picked them ("Option 2 ·
//! Band"): the band, the edges the window's chrome does not already line, a
//! 5 x 7 chevron centred on the band's middle column toward the side the pane
//! opens to, and a dash per pane behind it; all inside the rail, however
//! narrow. The neighbouring pane comes up to the rail, no gutter between.

use super::*;

const CANVAS: CanvasRect = CanvasRect::new(44.0, 80.0, 1320.0, 722.0);
/// A rail standing between panes: neither long side on the canvas's edge.
const INNER: CanvasRect = CanvasRect::new(600.0, 80.0, COLLAPSED_PANE_WIDTH_PX, 722.0);
const LEFT: CanvasRect = CanvasRect::new(44.0, 80.0, COLLAPSED_PANE_WIDTH_PX, 722.0);

fn look(flat: bool, forward: bool, dashes: usize, on_chrome: [bool; 2]) -> RailLook {
    RailLook {
        flat,
        opens_forward: forward,
        dashes,
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

/// The chevron's rows across the rail, less the rail's own start.
fn across(grip: &[CanvasRect], flat: bool, from: f32) -> Vec<f32> {
    let rows = &grip[..RAIL_CHEVRON_ROWS];
    rows.iter()
        .map(|row| if flat { row.y } else { row.x } - from)
        .collect()
}

/// At the left window edge: the band, one edge on the inner side, a chevron
/// of seven two-pixel rows pointing right whose 5 px box is centred on the
/// band's middle column (the rail less its edge), and one dash, 3 px across
/// and 1 px along, 4 px past the chevron's last row.
#[test]
fn a_rail_at_the_window_edge_paints_its_band_inner_edge_chevron_and_dash() {
    let marks = rail_marks(LEFT, look(false, true, 1, [true, false]));
    assert_eq!(marks[0], (LEFT, RailInk::Band));
    assert_eq!(
        inked(&marks, RailInk::Edge),
        [CanvasRect::new(51.0, 80.0, 1.0, 722.0)]
    );
    let grip = inked(&marks, RailInk::Grip);
    assert_eq!(grip.len(), RAIL_CHEVRON_ROWS + 1);
    // The band less its edge is x 44..=50, middle column 47: box 45..=49.
    assert_eq!(
        across(&grip, false, 44.0),
        [1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0]
    );
    let start = 80.0 + ((722.0 - RAIL_CHEVRON_ROWS as f32) / 2.0).floor();
    for (row, at) in grip[..RAIL_CHEVRON_ROWS].iter().enumerate() {
        assert_eq!((at.y, at.w, at.h), (start + row as f32, 2.0, 1.0));
    }
    let last = start + RAIL_CHEVRON_ROWS as f32 - 1.0;
    assert_eq!(
        grip[RAIL_CHEVRON_ROWS],
        CanvasRect::new(46.0, last + 4.0, 3.0, 1.0)
    );
}

/// At the right window edge it points left, its box centred on the band
/// less the inner edge on its left; folded flat under the toolbar it points
/// down, the same glyph turned, with dashes 1 px along and 3 px across at a
/// 3 px pitch, one per chart of the grid.
#[test]
fn the_chevron_points_the_way_the_pane_opens_and_turns_with_the_rail() {
    let right = CanvasRect::new(1356.0, 80.0, COLLAPSED_PANE_WIDTH_PX, 722.0);
    let marks = rail_marks(right, look(false, false, 0, [false, true]));
    assert_eq!(
        inked(&marks, RailInk::Edge),
        [CanvasRect::new(1356.0, 80.0, 1.0, 722.0)]
    );
    // The band less its edge is x 1357..=1363, middle 1360: box 1358..=1362.
    let grip = inked(&marks, RailInk::Grip);
    assert_eq!(
        across(&grip, false, 1356.0),
        [5.0, 4.0, 3.0, 2.0, 3.0, 4.0, 5.0]
    );

    let top = CanvasRect::new(44.0, 80.0, 1320.0, COLLAPSED_PANE_WIDTH_PX);
    let marks = rail_marks(top, look(true, true, 4, [true, false]));
    assert_eq!(
        inked(&marks, RailInk::Edge),
        [CanvasRect::new(44.0, 87.0, 1320.0, 1.0)]
    );
    let grip = inked(&marks, RailInk::Grip);
    assert_eq!(
        across(&grip, true, 80.0),
        [1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0]
    );
    assert!(
        grip[..RAIL_CHEVRON_ROWS]
            .iter()
            .all(|row| row.w == 1.0 && row.h == 2.0)
    );
    let dashes = &grip[RAIL_CHEVRON_ROWS..];
    assert_eq!(dashes.len(), 4, "one per chart of the grid");
    assert_eq!(RAIL_DASH_PITCH_PX, 4.0, "a 1 px dash and a 3 px gap");
    for pair in dashes.windows(2) {
        assert_eq!(pair[1].x - pair[0].x, RAIL_DASH_PITCH_PX);
    }
    assert!(
        dashes
            .iter()
            .all(|dash| (dash.y, dash.w, dash.h) == (82.0, 1.0, 3.0))
    );
}

/// A long side on the canvas's own edge shares the chrome's line there; a
/// rail between two panes keeps both edges.
#[test]
fn a_side_on_the_canvas_edge_leaves_its_line_to_the_chrome() {
    assert_eq!(
        RailLook::on(LEFT, CANVAS, (false, true), 1).on_chrome,
        [true, false]
    );
    let right = CanvasRect::new(1356.0, 80.0, COLLAPSED_PANE_WIDTH_PX, 722.0);
    assert_eq!(
        RailLook::on(right, CANVAS, (false, false), 1).on_chrome,
        [false, true]
    );
    let top = CanvasRect::new(44.0, 80.0, 1320.0, COLLAPSED_PANE_WIDTH_PX);
    assert_eq!(
        RailLook::on(top, CANVAS, (true, true), 1).on_chrome,
        [true, false]
    );
    let between = RailLook::on(INNER, CANVAS, (false, true), 1);
    assert_eq!(between.on_chrome, [false, false]);
    let edges = inked(&rail_marks(INNER, between), RailInk::Edge);
    assert_eq!(
        edges,
        [
            CanvasRect::new(600.0, 80.0, 1.0, 722.0),
            CanvasRect::new(607.0, 80.0, 1.0, 722.0)
        ]
    );
}

/// Dashes stop at the cap; a rail too short for the chevron and its dashes
/// drops the dashes, then keeps the chevron rows that fit; nothing ever
/// lands outside the rail, however narrow.
#[test]
fn a_short_or_narrow_rail_keeps_every_mark_inside() {
    let many = rail_marks(INNER, look(false, true, 12, [false, false]));
    let grip = inked(&many, RailInk::Grip);
    assert_eq!(grip.len(), RAIL_CHEVRON_ROWS + RAIL_MAX_DASHES);

    let short = CanvasRect::new(0.0, 0.0, COLLAPSED_PANE_WIDTH_PX, 14.0);
    let marks = rail_marks(short, look(false, true, 4, [false, false]));
    assert_eq!(
        inked(&marks, RailInk::Grip).len(),
        RAIL_CHEVRON_ROWS,
        "no dashes"
    );

    let tiny = CanvasRect::new(0.0, 0.0, COLLAPSED_PANE_WIDTH_PX, 3.0);
    let marks = rail_marks(tiny, look(false, true, 4, [false, false]));
    assert_eq!(inked(&marks, RailInk::Grip).len(), 3, "the rows that fit");
    for across in [0.5, 1.0, 1.5, 2.0, 3.0, 5.0, 8.0] {
        for along in [3.0, 14.0, 722.0] {
            for dashes in [0, 1, 4, 12] {
                for flat in [false, true] {
                    let rail = if flat {
                        CanvasRect::new(10.0, 20.0, along, across)
                    } else {
                        CanvasRect::new(10.0, 20.0, across, along)
                    };
                    let chrome = [dashes % 2 == 0, dashes % 3 == 0];
                    let look = look(flat, dashes % 2 == 0, dashes, chrome);
                    for (at, _) in rail_marks(rail, look) {
                        assert!(inside(at, rail), "{at:?} outside {rail:?}");
                    }
                }
            }
        }
    }
}

/// One rule for every painter: a rail lights while the pointer is on its
/// hit area, stays lit while a drag it began is held, and stays at rest
/// while a drag that began anywhere else passes over it.
#[test]
fn a_rail_lights_on_hover_and_its_own_drag_never_on_another() {
    assert!(rail_lit(true, false, false), "hovered");
    assert!(!rail_lit(false, false, false), "away");
    assert!(rail_lit(true, true, true), "its own drag, on it");
    assert!(rail_lit(false, true, true), "its own drag, wandered off");
    assert!(!rail_lit(true, false, true), "another drag passing over it");
    assert!(!rail_lit(false, false, true), "another drag elsewhere");
}

/// The words a rail says on hover and in the scene: a lone pane by its
/// name, otherwise by count, then how to open it.
#[test]
fn a_rail_says_what_it_holds_and_how_to_open_it() {
    let chart = RailHolds::of([PaneKind::Time]);
    assert_eq!(
        chart.words(|| Some("Time · 1m · WINQ26".to_owned())),
        "Time · 1m · WINQ26 · drag to open"
    );
    assert_eq!(chart.words(|| None), "1 chart · drag to open");
    let flow = RailHolds::of([PaneKind::Flow]);
    assert_eq!(flow.words(|| None), "Order flow · drag to open");
    let grid = RailHolds::of([PaneKind::Time; 4]);
    assert_eq!(
        (
            grid.panes(),
            grid.words(|| panic!("a grid is no lone pane"))
        ),
        (4, "4 charts · drag to open".to_owned())
    );
    let both = RailHolds::of([PaneKind::Time, PaneKind::Flow]);
    assert_eq!(both.words(|| None), "2 panes · drag to open");
}

/// The grid's context side folded behind one rail: it holds four charts,
/// points right, leaves its left line to the window's chrome, the seam
/// beside it is no line, and the flow pane comes up to the rail's edge.
#[test]
fn the_grid_folded_behind_one_rail_holds_four_charts_and_its_neighbour_meets_it() {
    let mut state = CanvasState::from_preset(preset("grid4+flow").unwrap());
    assert!(state.fold_flow_side(false, true));
    let rails = tree_rails(&state, CANVAS, CANVAS_DIVIDER_PX);
    assert_eq!(rails.len(), 1);
    assert_eq!(rails[0].holds, RailHolds::of([PaneKind::Time; 4]));
    assert_eq!(state.side_rail_holds(false), rails[0].holds);
    assert_eq!(state.side_rail_holds(true), RailHolds::of([PaneKind::Flow]));
    assert_eq!(rails[0].look(CANVAS), look(false, true, 4, [true, false]));
    let seams = tree_seams(&state, CANVAS, CANVAS_DIVIDER_PX);
    let beside = (seams.iter())
        .find(|seam| seam.rail.is_some())
        .expect("the rail's seam");
    assert!(!beside.is_line());
    let open = CanvasState::from_preset(preset("grid4+flow").unwrap());
    assert!((tree_seams(&open, CANVAS, CANVAS_DIVIDER_PX).iter()).all(TreeSeam::is_line));

    let mut seats = Seats::default();
    let bands = tree_bands(&state, &mut seats, CANVAS, CANVAS_DIVIDER_PX);
    let flow = (bands.iter())
        .find(|band| band.seat.is_none())
        .expect("the flow band");
    assert_eq!(flow.rect.x, rails[0].rect.right(), "no gutter");
    assert_eq!(flow.rect.right(), CANVAS.right());
}

/// Flat under the toolbar: the chart below comes up to the rail's edge.
#[test]
fn a_flat_rails_neighbour_meets_it() {
    let mut state = CanvasState::from_preset(preset("flow-over-time").unwrap());
    assert!(state.fold_flow_side(true, true));
    let rails = tree_rails(&state, CANVAS, CANVAS_DIVIDER_PX);
    let mut seats = Seats::default();
    let bands = tree_bands(&state, &mut seats, CANVAS, CANVAS_DIVIDER_PX);
    let chart = (bands.iter()).find(|band| !band.railed).expect("the chart");
    assert_eq!(chart.rect.y, rails[0].rect.bottom(), "no gutter");
    assert_eq!(chart.rect.bottom(), CANVAS.bottom());
}

/// The charts beside the flow pane in its split are what its context rail
/// holds: a chart docked beyond that split is not behind the rail.
#[test]
fn the_context_rail_holds_only_the_charts_in_the_flow_panes_split() {
    let mut state = CanvasState::from_preset(preset("time+flow").unwrap());
    let beyond = state.add(PaneKind::Time, CANVAS).unwrap();
    assert!(
        state
            .dock(beyond, DockTarget::Canvas(Edge::Bottom))
            .unwrap()
    );
    assert_eq!(
        state.side_rail_holds(false),
        RailHolds::of([PaneKind::Time])
    );
    assert!(state.fold_flow_side(false, true));
    assert_eq!(
        state.side_rail_holds(false),
        RailHolds::of([PaneKind::Time])
    );
}

/// Three charts side by side, the middle one folded: both seams beside it
/// name its rail, so a grab on either side lights it.
#[test]
fn a_rail_between_two_seams_is_named_by_both() {
    let mut state = CanvasState::from_preset(preset("charts-3").unwrap());
    let seam = SeamAddress {
        path: Vec::new(),
        index: 0,
    };
    let mut held = None;
    for _ in 0..16 {
        state
            .step_seam_drag(&mut held, (&seam, CANVAS.w / 16.0, false), CANVAS)
            .unwrap();
    }
    let rails = tree_rails(&state, CANVAS, CANVAS_DIVIDER_PX);
    assert_eq!(rails.len(), 1);
    let seams = tree_seams(&state, CANVAS, CANVAS_DIVIDER_PX);
    let beside: Vec<_> = seams.iter().filter_map(|seam| seam.rail).collect();
    assert_eq!(beside.len(), 2, "a seam on each side");
    assert!(beside.iter().all(|rail| rail.slot == rails[0].slot));
}
