//! Shared drawing visibility and rectangle-domain facts for alert consumers.
use crate::{
    Drawing, DrawingBand, DrawingId, DrawingTool, Drawings, RECTANGLE_TOOL_ID, RectanglePayload,
};

pub fn region_pause<T>(drawing: &Drawing<T>, all_hidden: bool) -> Option<&'static str> {
    if drawing.foreign_market {
        return Some("region on another market — paused");
    }
    if drawing.off_series {
        return Some("region off its series — paused");
    }
    if drawing.hidden || all_hidden {
        return Some("region hidden — paused");
    }
    None
}

/// Native coordinates and prices, before a consumer converts into its kernel.
#[must_use]
pub fn region_at(
    drawings: &Drawings<DrawingTool>,
    id: DrawingId,
    slot: usize,
) -> Option<(f64, f64, bool)> {
    let drawing = drawings.items().get(drawings.index_of(id)?)?;
    if region_pause(drawing, drawings.all_hidden()).is_some() {
        return None;
    }
    let [a, b] = drawing.points.as_slice() else {
        return None;
    };
    let (left, right) = drawing
        .payload
        .as_any()
        .downcast_ref::<RectanglePayload>()
        .map_or((false, false), |payload| {
            (payload.extend_left, payload.extend_right)
        });
    #[allow(clippy::cast_precision_loss)]
    let slot = slot as f32;
    Some((
        a.price,
        b.price,
        (left || slot >= a.bar.min(b.bar)) && (right || slot <= a.bar.max(b.bar)),
    ))
}

#[must_use]
pub fn region_can_fire<T>(drawing: &Drawing<T>, closed_slots: usize) -> bool {
    let extend_right = drawing
        .payload
        .as_any()
        .downcast_ref::<RectanglePayload>()
        .is_some_and(|payload| payload.extend_right);
    if extend_right {
        return true;
    }
    let [a, b] = drawing.points.as_slice() else {
        return false;
    };
    #[allow(clippy::cast_precision_loss)]
    let next_slot = closed_slots as f32;
    a.bar.max(b.bar) >= next_slot
}

/// Whether `id` can carry a strategy on a pane that has closed
/// `closed_slots` bars, in the words the trader reads when it cannot.
///
/// Re-validates everything the menu's gate promised: this is also the seam
/// a programmatic caller comes through, and it must not be able to arm what
/// the menu would refuse — the wrong shape, another band, a drawing with no
/// footing here, or one nobody can see.
pub fn check_arm(
    drawings: &Drawings<DrawingTool>,
    id: DrawingId,
    closed_slots: usize,
) -> Result<(), String> {
    let Some(index) = drawings.index_of(id) else {
        return Err("the drawing is gone".to_owned());
    };
    let target = &drawings.items()[index];
    if target.tool.id() != RECTANGLE_TOOL_ID
        || target.band != DrawingBand::Price
        || target.points.len() != 2
    {
        return Err("only price-band rectangles carry strategies".to_owned());
    }
    if target.foreign_market || target.off_series {
        return Err(
            "this drawing belongs to another market or lost its series — redraw the \
             region here first"
                .to_owned(),
        );
    }
    if target.hidden || drawings.all_hidden() {
        return Err("unhide the drawing first — an armed region stays visible".to_owned());
    }
    // A region whose drawn span can no longer cover a future bar can never
    // fire: the badge would show "armed" over a bot that is structurally
    // done. One predicate, shared with re-arm and the badge, refuses it with
    // the fix in hand.
    if !region_can_fire(target, closed_slots) {
        return Err(
            "the region ends before the next bar, so nothing can ever fire — \
             stretch it past the right edge, or turn on \"extend right\" in its \
             Region settings"
                .to_owned(),
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "alert_geometry_tests.rs"]
mod tests;
