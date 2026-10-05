//! The words on a bracket leg's in-plot tag, in a resting order's grammar:
//! a pill at rest, the whole statement under the pointer.
//!
//! At rest the pill says what the leg is and what it is worth — `SL -77.29`
//! on a position — and nothing the chart already says elsewhere. The price
//! is the gutter chip's job; the ✕ waits for the pointer, because a stop
//! cleared by one stray press beside the axis is protection lost. The full
//! statement used to sit there all session, and with a position and two
//! bracketed orders up the tags buried the candles they sat on.
//!
//! A leg riding an unfilled order keeps two things even at rest, and both
//! are honesty rather than decoration. The order's id (`#1 SL -5`): two
//! resting orders' legs are otherwise identical and proximity lies — one
//! order's target can sit right beside the other's line. And its pill is a
//! *ghost*, outlined rather than filled, on a dashed line: `SL -5` in a solid
//! pill reads exactly like a live position's stop, and the eye lands on the
//! pill, not on a one-pixel dash. `on fill` says it in words once the tag
//! opens.

use quantick_engine::Side;
use quantick_sim::{BracketTarget, signed_points};
use rust_decimal::Decimal;

use crate::account::Leg;
use crate::format::{fmt_decimal, fmt_points, fmt_signed_points};

/// One protective leg, as its tag and line read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegPaint {
    /// Whose leg this is. Also the drag identity: a leg being moved and a
    /// leg being created differ only in whether it existed a frame ago.
    pub owner: BracketTarget,
    pub leg: Leg,
    /// Side of the trade the leg protects.
    pub side: Side,
    /// The price the leg is measured against: the position's average entry,
    /// or the order's own resting price.
    pub reference: Decimal,
    /// Size, so a level can be read as points rather than as a price.
    pub quantity: Decimal,
    /// This leg's level today, `None` while it does not exist yet.
    pub level: Option<Decimal>,
    /// The other leg's level, for the R:R read while dragging.
    pub other_level: Option<Decimal>,
    /// Whether the leg belongs to an order that has not filled: it is a
    /// promise, not a live exit, and paints dashed and ghosted to say so.
    pub pending: bool,
}

/// The words on a leg's tag. At rest: the order's id while it has not
/// filled, the leg, and its signed points (`#1 SL -5`, `TP +10`). Open:
/// the whole statement — the price, the unit, `on fill`, and the R:R
/// while `dragging`.
#[must_use]
pub fn leg_tag_text(paint: &LegPaint, shown: Decimal, open: bool, dragging: bool) -> String {
    let owner = match paint.owner {
        BracketTarget::Order(id) => format!("#{} ", id.0),
        BracketTarget::Position => String::new(),
    };
    let points = fmt_signed_points(signed_points(
        paint.side,
        paint.reference,
        shown,
        paint.quantity,
    ));
    if !open {
        return format!("{owner}{} {points}", paint.leg.word());
    }
    let mut text = format!(
        "{owner}{} {} {points} pts",
        paint.leg.word(),
        fmt_decimal(shown)
    );
    if paint.pending {
        text.push_str(" · on fill");
    }
    if dragging && let Some(ratio) = rr_ratio(paint, shown) {
        text.push_str(&format!(" · R:R {ratio}"));
    }
    text
}

/// Reward over risk at the dragged level, against the other leg — the read
/// that turns a drag into a decision. `None` until both legs are known or
/// while the risk is zero.
fn rr_ratio(paint: &LegPaint, dragged: Decimal) -> Option<String> {
    let other = paint.other_level?;
    let entry = paint.reference;
    let (stop, target) = match paint.leg {
        Leg::StopLoss => (dragged, other),
        Leg::TakeProfit => (other, dragged),
    };
    let risk = entry.saturating_sub(stop).abs();
    let reward = target.saturating_sub(entry).abs();
    (risk > Decimal::ZERO).then(|| fmt_points(reward / risk))
}
