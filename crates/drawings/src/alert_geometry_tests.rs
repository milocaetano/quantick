//! The arm gate: which drawings may carry a strategy, and the sentence a
//! refusal reads as.

use super::*;
use crate::{ChartPoint, DRAWING_TOOLS};

fn tool(id: &str) -> DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .expect("the tool is registered")
}

/// A two-anchor drawing of `tool_id` over bars 0.5 … 1.5, and its id.
fn placed(tool_id: &str) -> (Drawings<DrawingTool>, DrawingId) {
    let mut drawings = Drawings::default();
    let tool = tool(tool_id);
    drawings.place(tool, ChartPoint::at_time(0.5, 40.0, Some(1_000)));
    drawings.place(tool, ChartPoint::at_time(1.5, 60.0, Some(2_000)));
    let id = drawings.items()[0].id;
    (drawings, id)
}

#[test]
fn a_price_rectangle_that_covers_the_next_bar_arms() {
    let (drawings, id) = placed(RECTANGLE_TOOL_ID);
    assert_eq!(check_arm(&drawings, id, 1), Ok(()));
}

#[test]
fn a_missing_drawing_and_another_shape_are_refused_in_words() {
    let (drawings, id) = placed(RECTANGLE_TOOL_ID);
    let gone = DrawingId(id.0 + 1);
    assert_eq!(
        check_arm(&drawings, gone, 1).err().as_deref(),
        Some("the drawing is gone")
    );
    let (lines, line) = placed("trend-line");
    assert_eq!(
        check_arm(&lines, line, 1).err().as_deref(),
        Some("only price-band rectangles carry strategies")
    );
}

#[test]
fn a_hidden_or_ended_region_is_refused_with_the_fix() {
    let (mut drawings, id) = placed(RECTANGLE_TOOL_ID);
    let ended = check_arm(&drawings, id, 5).expect_err("the span ends at bar 1.5");
    assert!(ended.starts_with("the region ends before the next bar"));
    drawings.set_hidden_at(0, true);
    assert_eq!(
        check_arm(&drawings, id, 1).err().as_deref(),
        Some("unhide the drawing first \u{2014} an armed region stays visible")
    );
}
