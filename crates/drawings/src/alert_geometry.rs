//! Shared drawing visibility and rectangle-domain facts for alert consumers.
use crate::{Drawing, DrawingId, DrawingTool, Drawings, RectanglePayload};

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
