//! The context stack's boundary: floors, folds and reopening.

use super::*;
use crate::canvas::CanvasRect;

/// The footer a context chart carries under its plot.
const FOOTER_PX: f32 = 28.0;

fn stack(count: usize) -> ContextStack {
    let mut stack = ContextStack::default();
    stack.heights = vec![PaneWidth::Auto; count];
    stack
}

fn carve(column: CanvasRect, heights: &[PaneWidth]) -> Split {
    split(column.y, column.h, heights)
}

fn length((from, to): split::Span) -> f32 {
    to - from
}

#[test]
fn resizing_one_pair_keeps_its_neighbor_and_short_columns_stay_put() {
    for height in [1500.0, 150.0] {
        let column = CanvasRect::new(10.0, 80.0, 600.0, height);
        let mut stack = stack(3);
        let before = carve(column, &stack.heights);
        let bottom = column.bottom();
        let result = stack.resize(0, bottom, column, before.dividers(), FOOTER_PX);
        let result = result.unwrap();
        let after = carve(column, &stack.heights);
        let (was, now) = (before.dividers[1], after.dividers[1]);
        assert!((split::centre(was) - split::centre(now)).abs() < 0.001);
        if height < 300.0 {
            assert!(!result.changed);
            assert_eq!(before.dividers(), after.dividers());
        } else {
            assert!(result.changed);
            assert!(stack.heights[1].is_collapsed());
            assert!(length(after.panes[1]) <= COLLAPSED_PANE_WIDTH_PX + 1.0);
        }
    }
}

/// A drag that stays clear of both folds trades height between the pair in
/// the direction it moves: up shrinks the upper chart and grows the lower,
/// down gives it back.
#[test]
fn a_boundary_drag_trades_height_in_the_direction_it_moves() {
    let column = CanvasRect::new(0.0, 40.0, 600.0, 900.0);
    let mut stack = stack(2);
    let lengths = |stack: &ContextStack| {
        let split = carve(column, &stack.heights);
        [length(split.panes[0]), length(split.panes[1])]
    };
    let equal = lengths(&stack);
    for (step, upward) in [(-60.0, true), (120.0, false)] {
        let before = lengths(&stack);
        let dividers = carve(column, &stack.heights).dividers().to_vec();
        let wanted = split::centre(dividers[0]) + step;
        let moved = stack.resize(0, wanted, column, &dividers, FOOTER_PX);
        assert!(moved.unwrap().changed);
        let after = lengths(&stack);
        assert_eq!(
            (after[0] < before[0], after[1] > before[1]),
            (upward, upward),
            "{step} px: {before:?} -> {after:?}"
        );
    }
    assert!(lengths(&stack)[0] > equal[0], "the net drag was downward");
}

/// Fold either chart against its edge and reopen it, over and over; a short
/// column reopens to past the fold threshold, a tall one to the full floor.
#[test]
fn either_context_pane_can_collapse_and_reopen_repeatedly() {
    for (height, reopened) in [(900.0, MIN_PANE_WIDTH_PX), (440.0, COLLAPSE_AT_PX)] {
        let column = CanvasRect::new(0.0, 40.0, 600.0, height);
        let mut stack = stack(2);
        for slot in [0, 1, 1, 0] {
            let before = carve(column, &stack.heights);
            let edge = if slot == 0 { column.y } else { column.bottom() };
            let folded = stack.resize(0, edge, column, before.dividers(), FOOTER_PX);
            assert!(folded.unwrap().changed);
            assert!(stack.heights[slot].is_collapsed());
            let folded = carve(column, &stack.heights);
            assert!((length(folded.panes[slot]) - COLLAPSED_PANE_WIDTH_PX).abs() < 1.0);
            let middle = column.y + column.h / 2.0;
            let opened = stack.resize(0, middle, column, folded.dividers(), FOOTER_PX);
            assert!(opened.unwrap().changed);
            assert!(stack.heights.iter().all(|height| !height.is_collapsed()));
            let opened = carve(column, &stack.heights);
            assert!(
                opened
                    .panes()
                    .iter()
                    .all(|pane| length(*pane) >= reopened - 0.001)
            );
        }
    }
}

#[test]
fn shares_folds_and_the_named_pair_round_trip() {
    let mut stack = stack(0);
    stack.restore(&[0.4, 0.6], &[true, false]);
    assert_eq!(stack.height_shares(), vec![0.4, 0.6]);
    assert_eq!(stack.collapsed_slots(), vec![true, false]);
    stack.expand();
    assert_eq!(stack.collapsed_slots(), vec![false, false]);
    stack.restore(&[0.3, f32::NAN], &[true, true]);
    assert_eq!(
        stack.heights,
        vec![PaneWidth::Collapsed { restore: 0.3 }, PaneWidth::Auto]
    );
    stack.fold_one(false);
    assert_eq!(stack.collapsed_slots(), vec![false, true]);
    assert_eq!(stack.height_shares(), vec![0.3, 0.5]);
    let pair = ResizeContextPair {
        upper_pane_id: 7,
        lower_pane_id: 9,
        wanted_y: 0.0,
    };
    assert_eq!(stack.pair_index(&pair), Err(ContextResizeError::NotDrawn));
    let column = CanvasRect::new(0.0, 0.0, 10.0, 10.0);
    stack.frame = Some(StackFrame {
        column,
        pane_ids: vec![5, 7, 9],
    });
    assert_eq!(stack.pair_index(&pair), Ok(1));
    let reversed = ResizeContextPair {
        upper_pane_id: 9,
        lower_pane_id: 7,
        wanted_y: 0.0,
    };
    assert_eq!(
        stack.pair_index(&reversed),
        Err(ContextResizeError::NotAdjacent)
    );
}

/// The hand on a divider counts from the press: grabbed off its centre, the
/// first move asks for the divider's place plus that move, never the
/// pointer's own place; let go back where it was pressed, the heights are
/// as they were, automatic ones too.
#[test]
fn a_divider_hand_counts_from_the_press_and_a_return_leaves_the_heights() {
    use crate::canvas::Hand;
    let mut stack = stack(2);
    let frame = |pressed, held, released, delta| Hand::of([pressed, held, released], delta);
    assert_eq!(
        stack.hand(0, frame(true, true, false, 0.0), 400.0),
        None,
        "a press asks nothing"
    );
    assert_eq!(
        stack.hand(0, frame(false, true, false, 10.0), 400.0),
        Some(410.0)
    );
    assert_eq!(
        stack.hand(1, frame(false, false, false, 0.0), 700.0),
        None,
        "another divider idles"
    );
    stack.heights[0] = PaneWidth::Manual(0.6);
    assert_eq!(
        stack.hand(0, frame(false, true, false, -10.0), 410.0),
        Some(400.0)
    );
    assert_eq!(stack.hand(0, frame(false, false, true, 0.0), 400.0), None);
    assert_eq!(
        stack.heights,
        vec![PaneWidth::Auto; 2],
        "back where it started"
    );
    stack.hand(0, frame(true, true, false, 0.0), 400.0);
    stack.hand(0, frame(false, true, false, 30.0), 400.0);
    stack.heights[0] = PaneWidth::Manual(0.6);
    stack.hand(0, frame(false, false, true, 0.0), 430.0);
    assert_eq!(
        stack.heights[0],
        PaneWidth::Manual(0.6),
        "a drag that ends elsewhere keeps its heights"
    );
}
