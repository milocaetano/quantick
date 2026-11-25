//! Where a selected drawing's context bar and its popovers go: screen
//! rectangles in, a position out. Pure geometry, so the host only converts
//! its own rectangle type at the edge and a test needs no window. Sizes are
//! [`Px`] used as a width and a height.

use crate::{Px, PxRect};

/// Gap between the object's bounding box and the bar.
pub const OBJECT_GAP_PX: f32 = 6.0;
/// Gap between the bar and a popover hanging off one of its slots.
pub const POPOVER_GAP_PX: f32 = 6.0;

/// Clamp a window of `size` at `position` into `chart`, top-left biased when
/// the window is larger than the pane.
#[must_use]
pub fn clamp_into(position: Px, size: Px, chart: PxRect) -> Px {
    let max_x = (chart.right() - size.x).max(chart.left());
    let max_y = (chart.bottom() - size.y).max(chart.top());
    Px::new(
        position.x.clamp(chart.left(), max_x),
        position.y.clamp(chart.top(), max_y),
    )
}

/// Where the bar opens for an object whose screen bounding box is `bbox`.
///
/// Above the object and aligned to the *left* of its box — not centred.
/// Centring looks tidier and is wrong: the bar's width changes with the
/// object's capabilities, so a centred bar puts the colour swatch in a
/// different place for every tool. Left-aligned, slot one is always in the
/// same place relative to the thing it edits.
///
/// `right_limit` is where the live lane starts, when the pane has one. The
/// bar never covers the live region: that is where the price the trader is
/// reading is being formed.
#[must_use]
pub fn place(chart: PxRect, right_limit: f32, bbox: PxRect, size: Px) -> Px {
    let above = bbox.top() - OBJECT_GAP_PX - size.y;
    let below = bbox.bottom() + OBJECT_GAP_PX;
    if above < chart.top() && below + size.y > chart.bottom() {
        // Taller than the pane leaves no room above or below — a volume
        // profile spanning the price axis, a channel across the whole view.
        // The top edge of a full-height object *is* the object, and the bar
        // must never sit on the figure it edits.
        //
        // Beside it instead, on whichever side has room. Vertically it keeps
        // to the object's top, so the controls stay where the eye left them.
        let gap = OBJECT_GAP_PX;
        let y = (bbox.top()).clamp(chart.top(), (chart.bottom() - size.y).max(chart.top()));
        let right_of = bbox.right() + gap;
        let left_of = bbox.left() - gap - size.x;
        if right_of + size.x <= right_limit.min(chart.right()) {
            return Px::new(right_of, y);
        }
        if left_of >= chart.left() {
            return Px::new(left_of, y);
        }
        // The object leaves no strip on either side either. Nothing can be
        // placed clear of it; the top edge is the least-bad answer, and this
        // is now the only case that reaches it.
        return Px::new(
            bbox.left()
                .clamp(chart.left(), (chart.right() - size.x).max(chart.left())),
            chart.top() + gap,
        );
    }
    let y = if above >= chart.top() { above } else { below };
    // Prefer to stay clear of the live lane; if the bar is wider than the
    // history area itself, the chart's own edge wins over the preference.
    let right = right_limit.min(chart.right()).max(chart.left() + size.x);
    let max_x = (right - size.x).max(chart.left());
    let x = bbox.left().clamp(chart.left(), max_x);
    let max_y = (chart.bottom() - size.y).max(chart.top());
    Px::new(x, y.clamp(chart.top(), max_y))
}

/// The rectangle the context bar may occupy: the pane, with the live lane
/// taken off its right edge.
///
/// The lane is where the price the trader is reading is being formed, and
/// losing sight of it is the trader's first veto — [`place`] has kept clear
/// of it since it was written. A *parked* bar is placed by a different rule
/// and has to be held to the same one, so both go through this rectangle
/// rather than through the pane's own.
///
/// Never narrower than the bar itself: a lane wider than the history area
/// would otherwise leave nothing to place in, and a pane's own edge is a
/// better answer than an empty rectangle.
#[must_use]
pub fn bounds(chart: PxRect, right_limit: f32, size: Px) -> PxRect {
    let right = right_limit
        .min(chart.right())
        .max(chart.left() + size.x)
        .min(chart.right());
    PxRect::from_min_max(chart.min, Px::new(right, chart.bottom()))
}

/// Repair automatic or parked placement around measured panels.
/// Bounded candidates keep the lane constraint; no paint shapes are scanned.
#[must_use]
pub fn avoid_panels(
    position: Px,
    size: Px,
    reachable: PxRect,
    panels: [Option<PxRect>; 2],
    object: Option<PxRect>,
) -> Px {
    let position = clamp_into(position, size, reachable);
    let gap = OBJECT_GAP_PX;
    let at = |point: Px| PxRect::from_min_size(point, size);
    if !panels
        .into_iter()
        .flatten()
        .any(|rect| at(position).intersects(rect.expand(gap)))
    {
        return position;
    }
    let around = |rect: PxRect| {
        [
            Px::new(position.x, rect.bottom()),
            Px::new(rect.right(), position.y),
            Px::new(position.x, rect.top() - size.y),
            Px::new(rect.left() - size.x, position.y),
        ]
    };
    let distance = |a: Px| (a.x - position.x).powi(2) + (a.y - position.y).powi(2);
    panels
        .into_iter()
        .flatten()
        .flat_map(|rect| around(rect.expand(gap)))
        .chain(object.into_iter().flat_map(|rect| around(rect.expand(gap))))
        .map(|candidate| clamp_into(candidate, size, reachable))
        .filter(|candidate| {
            panels
                .into_iter()
                .flatten()
                .all(|rect| !at(*candidate).intersects(rect))
        })
        .min_by(|a, b| {
            let covers_object = |point| object.is_some_and(|rect| at(point).intersects(rect));
            covers_object(*a)
                .cmp(&covers_object(*b))
                .then_with(|| distance(*a).total_cmp(&distance(*b)))
        })
        .unwrap_or(position)
}

/// Where a popover opens: hanging off the slot it belongs to — under it,
/// flipped above when there is no room, right-aligned on it rather than
/// pushed off it — and never outside `bounds`.
///
/// Downward is the default because that is where the eye goes from a control
/// it just pressed. It cannot be the only answer: the bar can sit one bar's
/// height off the bottom of the pane — automatic placement reaches that for
/// an object at the bottom, and a *parked* bar stays there for every object
/// after it — while the colour palette and the width rows are several times
/// that tall. A palette painted past the pane lands on the time axis, on the
/// neighbouring chart of a split, or off the window, and rows a trader cannot
/// reach read as rows that do not exist.
///
/// `size` is what the host measured last time this popover was open — zero
/// the first time, so that frame goes downward and the flip lands on the
/// next one. That is one frame of a surface the trader just clicked open,
/// and the alternative is measuring a panel before laying it out.
#[must_use]
pub fn popover_position(anchor: PxRect, size: Px, bounds: PxRect) -> Px {
    let below = Px::new(anchor.left(), anchor.bottom() + POPOVER_GAP_PX);
    let y = if below.y + size.y > bounds.bottom()
        && anchor.top() - POPOVER_GAP_PX - size.y >= bounds.top()
    {
        anchor.top() - POPOVER_GAP_PX - size.y
    } else if size.y <= bounds.height() {
        // Fits the pane, but on neither side of the slot: covering the bar
        // beats rows on the time axis or a neighbouring pane.
        below.y.min(bounds.bottom() - size.y)
    } else {
        below.y
    };
    // Horizontally it hangs from the slot's left edge, and when that would run
    // past the pane it right-aligns on the slot instead of being clamped to an
    // arbitrary column: a palette that no longer touches the control that
    // opened it reads as a panel that belongs to nothing. The clamp stays as
    // the last word, for a popover wider than the pane itself.
    let x = if below.x + size.x > bounds.right() {
        anchor.right() - size.x
    } else {
        below.x
    };
    let max_x = (bounds.right() - size.x).max(bounds.left());
    Px::new(x.clamp(bounds.left(), max_x), y)
}
