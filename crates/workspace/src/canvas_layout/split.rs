//! Carving one axis of the canvas into panes and the dividers between them.
//!
//! The divider sits **on** the split rather than beside it, so a pane spans
//! from one divider's inner edge to the next and the row spends the canvas
//! exactly once. A split runs along one axis only, and a row and a column are
//! this one function, because two would drift and a trader would find a
//! divider that behaved one way horizontally and another way vertically.
//!
//! Per-frame and allocation-free: the spans are fixed arrays sized for
//! [`MAX_CANVAS_PANES`], because this runs for every tab on every frame.

use super::{CANVAS_DIVIDER_PX, MAX_CANVAS_PANES, PaneWidth, resolve_shares};

/// Where one pane or divider runs along the split axis: from, to.
pub type Span = (f32, f32);

/// One axis, carved: `count` pane spans in order and one fewer divider
/// spans, `dividers[i]` between `panes[i]` and `panes[i + 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Split {
    pub count: usize,
    pub panes: [Span; MAX_CANVAS_PANES],
    pub dividers: [Span; MAX_CANVAS_PANES],
}

impl Split {
    /// The panes, in order.
    #[must_use]
    pub fn panes(&self) -> &[Span] {
        &self.panes[..self.count]
    }

    /// The dividers, one per seam.
    #[must_use]
    pub fn dividers(&self) -> &[Span] {
        &self.dividers[..self.count.saturating_sub(1)]
    }
}

/// Where a span's middle is.
#[must_use]
pub fn centre((from, to): Span) -> f32 {
    (from + to) / 2.0
}

/// Carve the `extent` from `start` into one pane per entry of `widths`, at
/// most [`MAX_CANVAS_PANES`]; a pane is never negative.
#[must_use]
pub fn split(start: f32, extent: f32, widths: &[PaneWidth]) -> Split {
    let count = widths.len().min(MAX_CANVAS_PANES);
    let end = start + extent;
    let mut out = Split {
        count,
        panes: [(start, start); MAX_CANVAS_PANES],
        dividers: [(start, start); MAX_CANVAS_PANES],
    };
    if count < 2 {
        out.panes[0] = (start, end);
        return out;
    }
    let mut shares = [0.0_f32; MAX_CANVAS_PANES];
    resolve_shares(&widths[..count], extent, &mut shares[..count]);
    let half = CANVAS_DIVIDER_PX / 2.0;
    let span = |from: f32, to: f32| (from, to.max(from));
    // Boundaries are cumulative shares of the *whole* extent, so the divider
    // centres land where the trader dragged them, and the panes fall out of
    // the boundaries rather than the other way round: no sliver is left.
    let (mut head, mut cumulative) = (start, 0.0_f32);
    for (index, share) in shares[..count - 1].iter().enumerate() {
        cumulative += share;
        let boundary = (start + extent * cumulative).clamp(start, end);
        out.panes[index] = span(head, boundary - half);
        out.dividers[index] = span(boundary - half, boundary + half);
        head = boundary + half;
    }
    out.panes[count - 1] = span(head.min(end), end);
    out
}
