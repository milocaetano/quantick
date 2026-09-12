//! What paper trading draws on the chart itself.
//!
//! The position line, the resting orders, their bracket legs and the gutter
//! chips that price them - plus [`PaintCtx`], the one place that turns a
//! price into a `y` and a rectangle into a hit test. Every geometry helper a
//! press has to agree with lives here beside the paint that produced it: the
//! ✕ is pressable exactly while it is painted because both ask this module
//! the same question.

use eframe::egui;
use quantick_engine::Side;
use quantick_sim::{Bracket, BracketTarget, OrderRole, signed_points};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::cmd::{CmdPreview, cmd_preview_layout};
use super::leg_tag::LegPaint;
use super::paint_ctx::{PaintCtx, bracket_handle_rect, clamp_tag_center, handles_visible};
use super::{
    CMD_LINE_MAX_DASHES, LINE_HOVER_WIDTH_PX, LINE_WIDTH_PX, Leg, ORDER_DASH_PX, ORDER_GAP_PX,
    POSITION_LINE_WIDTH_PX, PaperDrag, PaperTrading, TAG_HOVER_SLACK_PX, TagKey, kind_short,
    kind_word, leg_color, side_word, side_word_upper,
};
use crate::chart::PriceScale;
use crate::paper_chrome::{fmt_decimal, fmt_signed_points, points_color, position_word};
use crate::theme;

impl PaperTrading {
    /// Paint the simulated lines and their in-plot tags: pending orders
    /// (dashed, accent), then the position's entry / stop-loss / take-profit
    /// (solid, semantic colors). Gutter chips keep the last-price chip's
    /// geometry and carry *the price and nothing else*, so prices never
    /// disagree about their pixel; the words and the ✕s live in tags
    /// right-anchored inside the plot. Every interactive rect painted here
    /// comes from the same pure geometry the press-time hit-test computes
    /// (`close_button_rect`, `bracket_handle_rect`) — nothing is cached
    /// across frames, because a live chart autoscales between paint and
    /// press. `pointer` is `Some` only on the pane that owns paper input,
    /// so hover affordances paint nowhere else.
    #[expect(
        clippy::too_many_arguments,
        reason = "the pane hands over its frame geometry; bundling it here would rename, not simplify"
    )]
    pub fn draw_layer(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        tag_right: f32,
        axis_x: f32,
        scale: &PriceScale,
        reserved_chip_y: Option<f32>,
        pointer: Option<egui::Pos2>,
    ) {
        let ctx = PaintCtx {
            painter,
            chart_rect,
            tag_right,
            axis_x,
            scale,
            reserved_chip_y,
            pointer,
        };

        for order in self.account.venue.working_orders() {
            let Some(level) = order.price else { continue };
            let dragged = self.drag == PaperDrag::Order(order.id);
            let price = if dragged {
                self.drag_price
                    .unwrap_or_else(|| level.to_f64().unwrap_or_default())
            } else {
                level.to_f64().unwrap_or_default()
            };
            let y = ctx.scale.y(price);
            // The order's own line may be scrolled off while a leg of its
            // bracket is still on screen, so the legs are painted before
            // this gate rather than after it. `control_at` offers a leg's
            // cross whenever that leg is visible; skipping the paint here
            // left a cross that cleared a take profit on what looked like
            // empty chart — the inversion of "an invisible control is not a
            // control", and the more dangerous half of it.
            let entry_visible = ctx.in_range(y);
            if !entry_visible {
                self.draw_bracket_of(&ctx, BracketTarget::Order(order.id), true, false);
                continue;
            }
            // At rest the tag is a pill; it opens under the pointer. Read,
            // never recomputed — this frame's input already decided it,
            // from the pointer and the rect the *press* will use.
            let open = self.open_tag(TagKey::Order(order.id));
            let expanded = open.is_some();
            // The line's own emphasis keeps its own 10 px band: that band
            // is `line_at`'s, so a line that lights up is a line the press
            // can actually grab. The tag opens over a wider row and near a
            // chart edge over a different one, which is why the two
            // questions stayed separate.
            let hovered = ctx.hovers_line(y) || self.hovered_order == Some(order.id);
            let shown = if dragged {
                self.account.snap(price)
            } else {
                level
            };
            // A protective leg is a working order like any other, but it
            // is not an entry and must never read as one: it takes its
            // role's colour and says what it does rather than which way it
            // trades. Four accent-dashed `SELL LMT 1` rows over a long the
            // trader is already holding is the misreading this surface
            // cannot afford.
            let color = order_line_color(order.role);
            ctx.level_line(y, color, true, LINE_WIDTH_PX, hovered, dragged);
            ctx.gutter_chip(y, color, &fmt_decimal(shown));
            // Resting, the pill states what the gutter chip cannot — which
            // side and what kind of order waits there. The price is the
            // chip's job, and the id only matters once you mean to act on
            // it, so both wait for the open form.
            let name = order_line_name(order);
            let mut text = if expanded {
                format!(
                    "#{} {} {} @ {}",
                    order.id.0,
                    name,
                    fmt_decimal(order.quantity),
                    fmt_decimal(shown),
                )
            } else {
                format!("{name} {}", fmt_decimal(order.quantity))
            };
            // An order that can remove itself says so: without this, a
            // retest limit vanishing at its target reads as a glitch.
            if expanded && let Some(cancel) = order.cancel_at {
                text.push_str(&format!(" · cancels @ {}", fmt_decimal(cancel)));
            }
            // The ✕ is painted exactly when the press-side offers it —
            // one value, read twice, never two formulas.
            ctx.chip_tag(y, color, &text, open.is_some_and(|tag| tag.cancel), false);

            // The order's own protective legs, and the handles for the ones
            // it does not have yet. Dashed, because they are a promise: they
            // arm on the fill and not before. They paint here, beside the
            // order, rather than after every order — an order's stop belongs
            // in that order's z-order, not above a later order's line.
            //
            // Revealed by the same two things that open the tag: the line
            // under the pointer, or the order's row hovered in the dock. An
            // always-visible pair of buttons beside every resting order would
            // bury the candles they sit on.
            self.draw_bracket_of(
                &ctx,
                BracketTarget::Order(order.id),
                true,
                hovered || expanded,
            );
        }

        // Whether the pointer is on the position's entry line or its tag —
        // written inside the block below, read by the bracket paint after
        // it, because the tag's rect is only known once it is drawn.
        let mut position_reveal = false;
        if let Some(position) = self.account.venue.position().cloned() {
            let color = theme::side_color(position.side);
            let entry_y = ctx.scale.y(position.avg_price.to_f64().unwrap_or_default());
            if ctx.in_range(entry_y) {
                ctx.level_line(entry_y, color, false, POSITION_LINE_WIDTH_PX, false, false);
                ctx.gutter_chip(entry_y, color, &fmt_decimal(position.avg_price));
                let side_text = format!(
                    "SIM {} {}",
                    position_word(position.side),
                    fmt_decimal(position.quantity),
                );
                let points = self
                    .account
                    .venue
                    .mark_price()
                    .map(|mark| position.open_points(mark))
                    .map(|open| {
                        (
                            format!("{} pts", fmt_signed_points(open)),
                            points_color(open),
                        )
                    });
                let line_hovered = ctx.hovers_line(entry_y);
                let tag_rect = ctx.position_tag(entry_y, color, &side_text, points);
                // The handles are revealed by the entry line or its tag —
                // the affordance behind "drag from the position line to
                // create".
                let over_tag = ctx
                    .pointer
                    .is_some_and(|pointer| tag_rect.expand(TAG_HOVER_SLACK_PX).contains(pointer));
                position_reveal = line_hovered || over_tag;
            }

            // Live legs on an open position: solid, because they are exits
            // that can fire on the next print.
            self.draw_bracket_of(&ctx, BracketTarget::Position, false, position_reveal);
        }

        if let Some(armed) = self.account.armed {
            let hint = format!(
                "click a price to place your {} {} - Esc cancels",
                side_word(armed.side),
                kind_word(armed.kind),
            );
            // Bottom-left: the top-left corner belongs to the position HUD,
            // and arming an entry with a position open is a normal flow.
            painter.text(
                chart_rect.left_bottom() + egui::vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                hint,
                egui::FontId::proportional(12.0),
                theme::ACCENT,
            );
        }

        // On top of every order line: the cmd preview is the thing being
        // aimed right now.
        self.draw_cmd_preview(&ctx);
    }

    /// The protection the aim is carrying, drawn beside it.
    ///
    /// Under the ruler both levels sit the same distance from the pointer,
    /// so the pair *is* the 1:1 read, and one chip states that distance in
    /// points and in ticks - a trader deciding whether a setup is worth
    /// taking is asking how far, not where. Under a strategy the whole
    /// ladder is drawn instead, every rung of it, before the click.
    pub(super) fn draw_aim_bracket(&self, ctx: &PaintCtx<'_>, preview: &CmdPreview) {
        if preview.bracket.is_empty() {
            return;
        }
        let laddered = preview.bracket.is_laddered();
        // The same arithmetic the levels came from. Measuring the chip
        // against the tick while the lines were walked in points printed
        // `0.02 pts` beside a line twenty points away - a number a trader
        // would have sized a position from.
        let distance = self
            .ruler_step()
            .saturating_mul(Decimal::from(preview.ruler_ticks));
        for part in preview.bracket.parts() {
            for (level, word, color) in [
                (part.stop_loss, "SL", theme::SELL),
                (part.take_profit, "TP", theme::BUY),
            ] {
                let Some(level) = level else { continue };
                let y = ctx.scale.y(level.to_f64().unwrap_or_default());
                if !ctx.in_range(y) {
                    continue;
                }
                ctx.level_line(y, color, true, LINE_WIDTH_PX, false, false);
                ctx.gutter_chip(y, color, &fmt_decimal(level));
                let text = if laddered {
                    // A ladder labels each rung with the slice it closes:
                    // "TP" on three lines at three prices says nothing about
                    // which part each one belongs to.
                    format!(
                        "{word} {} · {}",
                        fmt_decimal(level),
                        part.quantity.map_or_else(|| "all".to_owned(), fmt_decimal),
                    )
                } else if preview.ruler_ticks > 0 {
                    // No tick count: it was only ever meaningful while one
                    // notch *was* one tick, and a notch is now worth what
                    // the instrument's step says.
                    format!(
                        "{word} {} · {} pts · 1:1",
                        fmt_decimal(level),
                        fmt_decimal(distance),
                    )
                } else {
                    format!("{word} {}", fmt_decimal(level))
                };
                ctx.chip_tag(y, color, &text, false, false);
            }
        }
    }

    /// Every rung of a laddered bracket, labelled with the slice it closes.
    ///
    /// Dashed like any protection that has not armed yet, and carrying the
    /// quantity because "TP" on three lines at three prices says nothing
    /// about which part each one belongs to. No handles and no `×`: a rung
    /// is the strategy's, not the pointer's.
    fn draw_ladder_rungs(
        &self,
        ctx: &PaintCtx<'_>,
        owner: BracketTarget,
        side: Side,
        reference: Decimal,
        bracket: Bracket,
    ) {
        let order = match owner {
            BracketTarget::Order(id) => Some(id),
            BracketTarget::Position => None,
        };
        for (index, part) in bracket.parts().enumerate() {
            for (level, leg) in [
                (part.stop_loss, Leg::StopLoss),
                (part.take_profit, Leg::TakeProfit),
            ] {
                let Some(level) = level else { continue };
                let color = leg_color(leg);
                // The rung being hauled follows the pointer, exactly as a
                // whole bracket's leg does: what the trader sees moving is
                // what the release will submit.
                let dragging = order.is_some_and(|id| {
                    self.drag
                        == PaperDrag::Rung {
                            order: id,
                            index,
                            leg,
                        }
                });
                let shown = if dragging {
                    self.drag_price
                        .map_or(level, |price| self.account.snap(price))
                } else {
                    level
                };
                let y = ctx.scale.y(shown.to_f64().unwrap_or_default());
                if !ctx.in_range(y) {
                    continue;
                }
                let hovered = ctx.hovers_line(y);
                ctx.level_line(y, color, true, LINE_WIDTH_PX, hovered, dragging);
                ctx.gutter_chip(y, color, &fmt_decimal(shown));
                let share = part.quantity.unwrap_or(Decimal::ONE);
                let points = signed_points(side, reference, shown, share);
                // The cross only where a press would take it: a rung of a
                // resting entry. A position's rungs are working orders by
                // then and carry their own.
                ctx.chip_tag(
                    y,
                    color,
                    &format!(
                        "{} {} · {} · {} pts",
                        leg.word(),
                        fmt_decimal(shown),
                        fmt_decimal(share),
                        fmt_signed_points(points),
                    ),
                    order.is_some() && !dragging,
                    false,
                );
            }
        }
    }

    /// Both legs of one bracket owner, plus the labelled handles for the
    /// legs it does not have yet.
    ///
    /// One function for the position and for every working order: the
    /// grammar a trader learns on a position is the grammar that then works
    /// on an order, because it is the same code. `reveal` is whether the
    /// owner's own line or tag is under the pointer — the handles appear
    /// with it, since an always-visible pair of buttons beside every
    /// resting order would bury the candles they sit on.
    fn draw_bracket_of(
        &self,
        ctx: &PaintCtx<'_>,
        owner: BracketTarget,
        pending: bool,
        reveal: bool,
    ) {
        let Some((side, reference, bracket, quantity)) = self.account.bracket_owner(owner) else {
            return;
        };
        // A ladder's rungs are several prices and none of them is amendable
        // by a drag: the numbers belong to the strategy that shaped them.
        // They are drawn — an order the trader protected must never look
        // naked — and then this function stops, so no handle is offered that
        // would replace the whole ladder with one level.
        if bracket.is_laddered() {
            self.draw_ladder_rungs(ctx, owner, side, reference, bracket);
            return;
        }
        for leg in [Leg::StopLoss, Leg::TakeProfit] {
            self.draw_bracket_leg(
                ctx,
                &LegPaint {
                    owner,
                    leg,
                    side,
                    reference,
                    quantity,
                    level: leg.level(bracket),
                    other_level: leg.other(bracket),
                    pending,
                },
            );
        }
        if self.drag != PaperDrag::None {
            return;
        }
        let reference_y = ctx.scale.y(reference.to_f64().unwrap_or_default());
        if !ctx.in_range(reference_y) {
            return;
        }
        let center = clamp_tag_center(reference_y, ctx.chart_rect.top(), ctx.chart_rect.bottom());
        // Each handle sits on its leg's *price* side of the reference,
        // mapped through the chart's orientation: upside down the TP handle
        // keeps pointing at take-profit prices instead of trading places
        // with the stop's — the same price-not-pixels rule
        // `decide_pending_leg` follows.
        let flip = ctx.scale.is_inverted();
        let missing = [Leg::StopLoss, Leg::TakeProfit]
            .into_iter()
            .filter(|leg| leg.level(bracket).is_none());
        let over_handle = ctx.pointer.is_some_and(|pointer| {
            missing.clone().any(|leg| {
                bracket_handle_rect(ctx.tag_right, center, leg.sits_above_entry(side) != flip)
                    .contains(pointer)
            })
        });
        if !handles_visible(ctx.pointer, reveal, over_handle) {
            return;
        }
        for leg in missing {
            let rect =
                bracket_handle_rect(ctx.tag_right, center, leg.sits_above_entry(side) != flip);
            ctx.bracket_handle(rect, leg.word(), leg_color(leg));
        }
    }

    /// The cmd-trading preview: a dashed line at the pointer's price
    /// running out to the right edge, the label riding beside the cursor,
    /// and the exact price on the gutter — the trader reads what this
    /// click will place *before* it commits, which is the safety the
    /// right-click menu cannot offer. The line is what ties the label at
    /// the pointer to the price on the axis, so it spans the whole way
    /// rather than hugging the edge.
    pub(super) fn draw_cmd_preview(&self, ctx: &PaintCtx<'_>) {
        let Some(preview) = self.cmd_preview else {
            return;
        };
        // Only the pane that owns paper input paints the aim — except in
        // a harness run, whose panes never own a pointer at all.
        if ctx.pointer.is_none() && self.cmd_preview_force.is_none() {
            return;
        }
        if !ctx.in_range(preview.pointer.y) {
            return;
        }
        let color = theme::side_color(preview.side);
        // Lay out against the band the input hit-tests (its right edge is
        // the lane divider when the live tape lane is up), never the full
        // chart rect — a label painted right of the divider would be a
        // click target the press could not find (the overlay-controls
        // rule).
        let band = egui::Rect::from_min_max(
            ctx.chart_rect.min,
            egui::pos2(
                ctx.tag_right.min(ctx.chart_rect.right()),
                ctx.chart_rect.max.y,
            ),
        );
        // The aim belongs to the band it was aimed in. Both panes of a
        // split draw this layer from one simulator, and the label now
        // rides an x — so laying a flow-pane pointer out against the time
        // pane's band would paint a label off the end of it. Live, the
        // other pane holds no pointer and never reaches here; under the
        // capture hook it does, which is how this was seen at all.
        if preview.pointer.x < band.left() || preview.pointer.x > band.right() {
            return;
        }
        let (start, end, label) = cmd_preview_layout(band, ctx.axis_x, preview.pointer);
        // The gesture in progress reads a step above a resting order's
        // line: this is the one thing on the chart the next click acts on.
        // The dash period stretches on a very wide plot rather than paying
        // for a segment every 8 px across it (see `CMD_LINE_MAX_DASHES`).
        let period = ((end.x - start.x) / CMD_LINE_MAX_DASHES).max(ORDER_DASH_PX + ORDER_GAP_PX);
        let dash = period * ORDER_DASH_PX / (ORDER_DASH_PX + ORDER_GAP_PX);
        ctx.painter.extend(egui::Shape::dashed_line(
            &[start, end],
            egui::Stroke::new(LINE_HOVER_WIDTH_PX, color),
            dash,
            period - dash,
        ));
        ctx.gutter_chip(preview.pointer.y, color, &fmt_decimal(preview.price));
        // Full fill, always: while it paints, the click is already live.
        // There is no resting state to distinguish — releasing the
        // modifier is what makes the aim go away.
        ctx.painter
            .rect_filled(label, egui::Rounding::same(3.0), color);
        let quantity = self
            .quantity_preview()
            .map_or_else(|| "?".to_owned(), fmt_decimal);
        let text = format!(
            "{} {} {}",
            side_word_upper(preview.side),
            kind_word(preview.kind),
            quantity,
        );
        let galley =
            ctx.painter
                .layout_no_wrap(text, egui::FontId::monospace(11.0), theme::CHIP_INK);
        ctx.painter.galley(
            egui::pos2(
                label.center().x - galley.size().x / 2.0,
                label.center().y - galley.size().y / 2.0,
            ),
            galley,
            theme::CHIP_INK,
        );
        self.draw_aim_bracket(ctx, &preview);
        // The wheel is an invisible affordance, and with no strategy armed
        // it is the only path to a stop. One faint line under the aim's own
        // label says so - at the pointer, at the moment it matters - and it
        // erases itself the first time the wheel is rolled, so a trader who
        // knows never reads it twice.
        if preview.bracket.is_empty() && !self.ruler_rolled {
            let (_, _, label) = cmd_preview_layout(
                egui::Rect::from_min_max(
                    ctx.chart_rect.min,
                    egui::pos2(
                        ctx.tag_right.min(ctx.chart_rect.right()),
                        ctx.chart_rect.max.y,
                    ),
                ),
                ctx.axis_x,
                preview.pointer,
            );
            ctx.painter.text(
                egui::pos2(label.center().x, label.max.y + 3.0),
                egui::Align2::CENTER_TOP,
                "roll the wheel for a stop and target",
                egui::FontId::proportional(10.0),
                theme::TEXT_SUPPORT,
            );
        }
    }
}

/// A working order's line colour: accent for an entry waiting to trade, the
/// leg's own side colour for protection guarding a position.
fn order_line_color(role: OrderRole) -> egui::Color32 {
    match role {
        OrderRole::Entry => theme::ACCENT,
        OrderRole::StopLoss => theme::SELL,
        OrderRole::TakeProfit => theme::BUY,
    }
}

/// What a working order calls itself in its tag. An entry names its side and
/// kind, because those are what the accent line cannot say; a protective leg
/// names its job, because "SELL LMT" over a long is a lie about what it is
/// waiting to do.
fn order_line_name(order: &quantick_sim::Order) -> String {
    match order.role {
        OrderRole::Entry => format!("{} {}", side_word_upper(order.side), kind_short(order.kind)),
        OrderRole::StopLoss => "SL".to_owned(),
        OrderRole::TakeProfit => "TP".to_owned(),
    }
}
