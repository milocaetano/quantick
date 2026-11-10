//! The context stack's boundary: floors, folds and reopening.

use super::*;
use crate::canvas::CanvasRect;

/// The footer a context chart carries under its plot.
const FOOTER_PX: f32 = 28.0;

fn stack(count: usize) -> ContextStack {
    ContextStack {
        heights: vec![PaneWidth::Auto; count],
        frame: None,
    }
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
