//! The in-plot tag on a bracket leg's line, in a resting order's grammar:
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
//!
//! Open is one per-frame value, [`OpenTag`], computed with the order tags in
//! `fill_open_tags` and read by the paint here and by `control_at`'s press:
//! the ✕ is pressable exactly while it is painted.

use eframe::egui;
use quantick_engine::Side;
use quantick_sim::{BracketTarget, signed_points};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::{LINE_WIDTH_PX, Leg, OpenTag, PaintCtx, PaperDrag, PaperTrading, TagKey};
use super::{leg_color, tag_row_hit};
use crate::chart::PriceScale;
use crate::paper_chrome::{fmt_decimal, fmt_points, fmt_signed_points};

/// One protective leg's paint inputs (see `draw_bracket_leg`).
pub(super) struct LegPaint {
    /// Whose leg this is. Also the drag identity: a leg being moved and a
    /// leg being created differ only in whether it existed a frame ago.
    pub(super) owner: BracketTarget,
    pub(super) leg: Leg,
    /// Side of the trade the leg protects.
    pub(super) side: Side,
    /// The price the leg is measured against: the position's average entry,
    /// or the order's own resting price.
    pub(super) reference: Decimal,
    /// Size, so a level can be read as points rather than as a price.
    pub(super) quantity: Decimal,
    /// This leg's level today, `None` while it does not exist yet.
    pub(super) level: Option<Decimal>,
    /// The other leg's level, for the R:R read while dragging.
    pub(super) other_level: Option<Decimal>,
    /// Whether the leg belongs to an order that has not filled: it is a
    /// promise, not a live exit, and paints dashed and ghosted to say so.
    pub(super) pending: bool,
}

impl PaperTrading {
    /// The leg tags this frame opens, appended to the order tags'.
    ///
    /// A leg opens when the pointer is on its row — the rule an order's tag
    /// follows (`tag_row_hit`) — or when the capture hook forces every tag
    /// open. A leg in the hand is left out: its drag paints the open form
    /// itself, and a moving leg offers no ✕. A ladder's rungs keep their own
    /// tags (`draw_ladder_rungs`).
    pub(super) fn fill_open_legs(
        &self,
        open: &mut Vec<OpenTag>,
        pointer: Option<egui::Pos2>,
        chart: egui::Rect,
        scale: &PriceScale,
    ) {
        for owner in self.account.bracket_owners() {
            let Some((_, _, bracket, _)) = self.account.bracket_owner(owner) else {
                continue;
            };
            if bracket.is_laddered() {
                continue;
            }
            for leg in [Leg::StopLoss, Leg::TakeProfit] {
                let Some(level) = leg.level(bracket) else {
                    continue;
                };
                if self.drag == (PaperDrag::Leg { owner, leg }) {
                    continue;
                }
                let y = scale.y(level.to_f64().unwrap_or_default());
                if self.order_hover_force || pointer.is_some_and(|at| tag_row_hit(at, y, chart)) {
                    open.push(OpenTag {
                        key: TagKey::Leg(owner, leg),
                        cancel: true,
                    });
                }
            }
        }
    }

    /// One protective leg: its resting line and tag, the drag that reprices
    /// it, or — while a create-drag runs and the leg does not exist yet —
    /// the dashed preview of where release would put it. The tag gains the
    /// live R:R read once both legs are known, which is what turns the drag
    /// into a decision.
    pub(super) fn draw_bracket_leg(&self, ctx: &PaintCtx<'_>, paint: &LegPaint) {
        let identity = (paint.owner, paint.leg);
        let amending =
            matches!(self.drag, PaperDrag::Leg { owner, leg } if (owner, leg) == identity);
        let creating = matches!(self.drag, PaperDrag::CreateLeg { owner, leg } if (owner, leg) == identity)
            && paint.level.is_none();
        let resting = paint.level.map(|level| level.to_f64().unwrap_or_default());
        let price = if amending || creating {
            self.drag_price.or(resting)
        } else {
            resting
        };
        let Some(price) = price else { return };
        let y = ctx.scale.y(price);
        if !ctx.in_range(y) {
            return;
        }
        let dragging = amending || creating;
        let hovered = ctx.hovers_line(y);
        let shown = if dragging {
            self.account.snap(price)
        } else {
            paint.level.unwrap_or_else(|| self.account.snap(price))
        };
        let color = leg_color(paint.leg);
        // Dashed while it is being created (it is not placed yet) and while
        // it rides an unfilled order (it is a promise that arms on the
        // fill). Solid only once it is a live exit on an open position.
        ctx.level_line(
            y,
            color,
            creating || paint.pending,
            LINE_WIDTH_PX,
            hovered,
            dragging,
        );
        ctx.gutter_chip(y, color, &fmt_decimal(shown));
        let open = self.open_tag(TagKey::Leg(paint.owner, paint.leg));
        let text = leg_tag_text(paint, shown, dragging || open.is_some(), dragging);
        ctx.chip_tag(
            y,
            color,
            &text,
            open.is_some_and(|tag| tag.cancel),
            paint.pending,
        );
    }
}

/// The words on a leg's tag. At rest: the order's id while it has not
/// filled, the leg, and its signed points (`#1 SL -5`, `TP +10`). Open:
/// the whole statement — the price, the unit, `on fill`, and the R:R
/// while `dragging`.
pub(super) fn leg_tag_text(paint: &LegPaint, shown: Decimal, open: bool, dragging: bool) -> String {
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
