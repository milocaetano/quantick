//! Interpretation of displayed depth and coverage, independent of a painter.

use super::{BEFORE_CAPTURE, BOOK_PENDING};
use crate::config::painting::GAP_EDGE_EPSILON;

/// Only a current, exactly confirmed row can preview the book in chart forward
/// space. It is a snapshot preview, with no historical-time claim. A gapped
/// book supplies no eligible cells; `live` is the frame's verdict on whether
/// its book is still current at the pane's clock
/// ([`VisibleOrderflow::current_book_live`](crate::engine::VisibleOrderflow::current_book_live)).
pub fn current_book_preview(cell: &super::HeatmapCell, live: bool) -> bool {
    live && cell.current_book && !cell.time_averaged
}

/// Whether a pane previews its current book in the strip between the last
/// candle and the tape: its lane asks for it
/// ([`LiveLaneStyle::forward_book_preview`](crate::config::LiveLaneStyle::forward_book_preview))
/// and the frame holds its book current (`live`). The `live` argument
/// [`current_book_preview`] and [`depth_caption`] take.
#[must_use]
pub fn forward_book_preview(lane: &crate::config::LiveLaneStyle, live: bool) -> bool {
    lane.forward_book_preview && live
}

pub fn depth_caption(cells: &[super::HeatmapCell], preview_visible: bool) -> Option<&'static str> {
    let averaged = cells.iter().any(|cell| cell.time_averaged);
    let preview = preview_visible && cells.iter().any(|cell| current_book_preview(cell, true));
    match (averaged, preview) {
        (true, true) => Some("Time-averaged depth · current book preview in forward space"),
        (true, false) => Some("Time-averaged depth"),
        (false, true) => Some("Current book preview in forward space"),
        (false, false) => None,
    }
}

/// Closed-bar membership of a normalized depth cell, excluding the live lane.
pub fn depth_cell_slots(x0: f64, x1: f64, regions: usize, has_lane: bool) -> (usize, usize, bool) {
    let regions = regions.max(1);
    let bars = regions.saturating_sub(usize::from(has_lane));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let first = (x0.clamp(0.0, 1.0) * regions as f64).floor() as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let after = (x1.clamp(0.0, 1.0) * regions as f64).ceil() as usize;
    let touches_lane = has_lane && after > bars;
    if first >= bars {
        (bars, bars, touches_lane)
    } else {
        (first, after.max(first + 1).min(bars), touches_lane)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GapMarks {
    pub fill: bool,
    pub left_boundary: bool,
    pub right_boundary: bool,
}

/// A leading absence needs its capture boundary, while interior gaps need
/// both boundaries and a fill. Viewport edges are not coverage transitions.
pub fn gap_marks_bounds(
    left: f32,
    right: f32,
    chart_left: f32,
    chart_right: f32,
    leading: bool,
) -> GapMarks {
    let on_chart_edge = |x: f32| {
        (x - chart_left).abs() < GAP_EDGE_EPSILON || (x - chart_right).abs() < GAP_EDGE_EPSILON
    };
    GapMarks {
        fill: !leading,
        left_boundary: !leading && !on_chart_edge(left),
        right_boundary: !on_chart_edge(right),
    }
}

pub fn gap_label(reason: &str) -> &'static str {
    match reason {
        BEFORE_CAPTURE => "L2 unavailable before capture",
        BOOK_PENDING => "L2 loading for this stretch",
        "capture_disabled" => "L2 capture disabled",
        "sequence_gap" => "L2 sequence gap · resynchronizing",
        _ => "L2 continuity unavailable",
    }
}

#[cfg(test)]
mod caption_tests {
    use super::depth_caption;
    use crate::projection::HeatmapCell;

    /// Review round 1 #7: an averaged lane draws no reduction holes and no
    /// gaps around consumption bubbles (the approved look, as at 72a5a747);
    /// its caption says so rather than hiding them silently.
    #[test]
    fn an_averaged_lane_says_its_reduction_holes_are_not_drawn() {
        let averaged = HeatmapCell {
            generation: 1,
            side: crate::RestingSide::Bid,
            price_bucket: rust_decimal::Decimal::from(99),
            quantity: rust_decimal::Decimal::from(5),
            x0: 0.5,
            x1: 0.9,
            y0: 0.2,
            y1: 0.3,
            intensity: 0.8,
            alpha: 0.8,
            carried: false,
            time_averaged: true,
            current_book: false,
        };
        let caption = depth_caption(std::slice::from_ref(&averaged), false).unwrap();
        assert!(caption.contains("reduction holes not drawn"), "{caption}");
        let exact = HeatmapCell {
            time_averaged: false,
            ..averaged
        };
        assert_eq!(depth_caption(&[exact], false), None);
    }
}
