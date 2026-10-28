//! The words on a bracket leg's in-plot tag, in a resting order's grammar:
//! a pill at rest, the whole statement under the pointer.
//!
//! The pill says what the leg is and what it is worth — `SL -77.29` on a
//! position — and never its price, open or at rest: the price is the gutter
//! chip's job, right beside it, and a pill repeating it says the one thing
//! the trader can already read. The worth is in the figure the P&L switch
//! leads with, one figure, and a click on the pill flips money and points.
//! The ✕ waits for the pointer, because a stop cleared by one stray press
//! beside the axis is protection lost.
//!
//! The aim's legs, before the click, carry both figures instead
//! ([`aim_leg_text`]): that is the moment a trader asks whether the setup is
//! worth taking, in money and in points at once.
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
use quantick_trading::{BracketTarget, signed_points};
use rust_decimal::Decimal;

use crate::account::Leg;
use crate::format::fmt_points;
use crate::pnl::SymbolPnl;

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
/// filled, the leg, and what the leg is worth in the figure the ticket
/// leads with — `pnl`'s compact form, the HUD's and the rungs' own
/// (`#1 SL -5 pts × 1`, `TP +2.00 BRL`). Open: `on fill` after it, and the
/// R:R while `dragging`.
#[must_use]
pub fn leg_tag_text(
    paint: &LegPaint,
    shown: Decimal,
    open: bool,
    dragging: bool,
    pnl: SymbolPnl<'_>,
) -> String {
    let owner = match paint.owner {
        BracketTarget::Order(id) => format!("#{} ", id.0),
        BracketTarget::Position => String::new(),
    };
    let at = signed_points(paint.side, paint.reference, shown, paint.quantity);
    let figure = pnl.reader.compact(pnl.symbol, at, paint.quantity);
    let mut text = format!("{owner}{} {figure}", paint.leg.word());
    if !open {
        return text;
    }
    if paint.pending {
        text.push_str(" · on fill");
    }
    if dragging && let Some(ratio) = rr_ratio(paint, shown) {
        text.push_str(&format!(" · R:R {ratio}"));
    }
    text
}

/// One leg the aim would carry, as its chip reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AimLeg {
    pub leg: Leg,
    /// Side of the order being aimed.
    pub side: Side,
    /// The aimed price: where the order would rest.
    pub entry: Decimal,
    /// The leg's own level.
    pub level: Decimal,
    /// The size the click would send; see [`super::CmdPreview::quantity`].
    pub quantity: Option<Decimal>,
}

/// The words on one leg the aim carries, before the click: what it is worth
/// if the order fills and the leg is hit, money and points both, the leading
/// figure first - `TP +92.00 BRL · +460 pts × 1`. `slice` names a ladder
/// rung's part of the order (`all` for the rest of it); `even` closes with
/// the ruler's `1:1`. With no size to read, only the word stands: a worth
/// for a size nobody can send is a number the trader would act on.
#[must_use]
pub fn aim_leg_text(aim: &AimLeg, slice: Option<&str>, even: bool, pnl: SymbolPnl<'_>) -> String {
    let mut text = aim.leg.word().to_owned();
    if let Some(quantity) = aim.quantity {
        let at = signed_points(aim.side, aim.entry, aim.level, quantity);
        text.push(' ');
        text.push_str(&pnl.reader.trade(pnl.symbol, at, quantity));
    }
    if let Some(slice) = slice {
        text.push_str(&format!(" · {slice}"));
    }
    if even {
        text.push_str(" · 1:1");
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
