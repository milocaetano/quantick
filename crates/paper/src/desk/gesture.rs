//! The pointer on the chart: what it is over, and what a press does to it.
//!
//! One pass per frame decides the control under the pointer, the cursor it
//! deserves and the drag it may start or finish. It never draws — the
//! geometry it hit-tests is [`super::geometry`]'s, which the host's paint
//! reads too, so the control a press finds is the one the frame painted.
//! Everything here is a value: the host hands over the frame as
//! [`ChartFrame`] and the price axis as a [`PriceAxis`], and gets back the
//! [`ChartCommand`] to carry out.

use quantick_engine::Side;
use quantick_sim::{Bracket, BracketTarget, EntryKind, OrderId};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::cmd::{CmdPreview, CmdPreviewForce, HeldKeys};
use super::geometry::{
    Bounds, CREATE_DECIDE_THRESHOLD_PX, LINE_GRAB_RADIUS_PX, Point, PriceAxis, TAG_BUTTON_PX,
    TAG_GAP_PX, bracket_handle_rect, clamp_tag_center, close_button_rect, tag_row_hit,
};
use crate::PaperAccount;
use crate::account::Leg;

/// Which simulated line the pointer is dragging.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PaperDrag {
    #[default]
    None,
    /// Moving a protective leg that already exists.
    Leg { owner: BracketTarget, leg: Leg },
    /// Pulling a leg into existence, from its owner's line or its labelled
    /// handle; release submits it, exactly like repricing an existing one.
    CreateLeg { owner: BracketTarget, leg: Leg },
    /// Repricing a working order.
    Order(OrderId),
    /// The press landed on the position's entry line: an average entry is
    /// history, not an order, so the geometry stays put — but the gesture
    /// still belongs to the line (the chart must not pan under it). This is
    /// the state for a fully bracketed position, whose legs are their own
    /// handles.
    Blocked,
    /// The press landed on the position's entry line and at least one leg
    /// is missing: the first committed pull decides which leg the drag
    /// creates (profit side → take profit, losing side → stop loss). A
    /// working order needs no such state — its line already means
    /// "reprice", so its legs are born from their handles alone.
    CreatePending,
    /// Moving one rung of a resting entry's ladder.
    ///
    /// The rung belongs to the *order*, not to the strategy that shaped it:
    /// the strategy was the template, the order carries a copy, and hauling
    /// this line edits the copy. Nothing is written back to the named
    /// ladder, so the next order still rests with what the trader saved.
    ///
    /// A filled position needs no such state — its rungs are working orders
    /// by then, and their own lines already mean "reprice".
    Rung {
        order: OrderId,
        index: usize,
        leg: Leg,
    },
}

/// One frame's answer for one working order's in-plot tag: computed by the
/// input pass, read by the paint *and* by the press.
///
/// A shared **value**, not a shared formula. The two sides are handed
/// different pointers (`hover_pos` for the paint, `latest_pos` for the
/// press) and different rects (the whole chart vs. the band left of the
/// tape lane), so asking them to recompute the same predicate is asking
/// them to disagree — and a ✕ that one side paints and the other side does
/// not is a cancel the trader never saw coming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenTag {
    pub key: TagKey,
    /// The ✕ is painted with the full statement, so a press may act on it.
    /// False while the order is being dragged: a moving order offers no
    /// cancel, and its tag is on a different row from its resting price.
    pub cancel: bool,
}

/// Whose tag an [`OpenTag`] opens: a working order's, or one leg of a
/// bracket — the same two states, one contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagKey {
    Order(OrderId),
    Leg(BracketTarget, Leg),
}

/// A painted overlay control: a tag's ✕ or a bracket handle. Found from
/// this frame's own geometry, never from a cached rect — the input pass runs
/// before the draw, and an immediate-mode overlay control is pressed against
/// where it was actually painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperControl {
    /// ✕ on the position tag: exit at the next print.
    ClosePosition,
    /// ✕ on a protective leg's tag: clear that leg, keeping the other.
    ClearLeg { owner: BracketTarget, leg: Leg },
    /// ✕ on a working order's tag: cancel it.
    CancelOrder(OrderId),
    /// Labelled `SL`/`TP` handle beside a line that owns brackets: the
    /// press starts a create-drag for that leg.
    Handle { owner: BracketTarget, leg: Leg },
    /// ✕ on one rung of a resting entry's ladder: clear that rung's leg and
    /// leave every other rung alone. A rung with neither leg left is dropped
    /// — a part that protects nothing is not a part.
    ClearRung {
        order: OrderId,
        index: usize,
        leg: Leg,
    },
}

/// The next chart click places this entry (`Limit` or `Stop` only — a
/// market order needs no price and fires straight from its button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmedPlacement {
    pub side: Side,
    pub kind: EntryKind,
}

/// The cursor that announces what is under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorHint {
    /// A painted control, or anywhere a real aim is up.
    PointingHand,
    /// A line a drag moves, or an entry line a drag grows a leg from.
    ResizeVertical,
    /// A fully bracketed entry line: history, it never moves.
    NotAllowed,
}

/// Everything the input pass needs from the frame, in plain values. The
/// host gathers it, so the desk never reads raw input state itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartFrame {
    /// The interactive band the press is tested against.
    pub chart: Bounds,
    pub pointer: Option<Point>,
    pub primary_pressed: bool,
    pub primary_down: bool,
    pub primary_released: bool,
    /// The frame's held modifiers — what the cmd-trading gesture reads.
    pub keys: HeldKeys,
    /// Whether something the pane owns already holds this pixel — a
    /// drawing a press would grab, or the canvas's own chrome. The aim is
    /// the *last* claimant on the canvas, so it paints nothing and places
    /// nothing there.
    pub canvas_claimed: bool,
    /// This frame's wheel travel over the chart, in pixels.
    pub scroll_y: f32,
    /// The wheel was *pressed* this frame.
    pub middle_pressed: bool,
    /// Whether the paper layer is painted this frame. Switched off, its
    /// lines and tags are unpainted — so they take no press either.
    pub layer_visible: bool,
}

/// What a press asked of the account. The desk decides; the host carries it
/// out through the account's own funnel, so the journal, the toast and the
/// report hear about it the way they hear about every other order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChartCommand {
    /// Exit the position at the next print.
    ClosePosition,
    /// Set (or, with `None`, clear) one leg of an owner's bracket.
    AmendLeg {
        owner: BracketTarget,
        leg: Leg,
        price: Option<Decimal>,
    },
    /// Set (or clear) one rung's leg of a resting ladder.
    AmendRung {
        order: OrderId,
        index: usize,
        leg: Leg,
        price: Option<Decimal>,
    },
    /// Cancel a working order.
    CancelOrder(OrderId),
    /// Reprice a working order.
    AmendOrder { id: OrderId, price: Decimal },
    /// Rest an entry at the aimed price, with the ticket's protection.
    PlaceResting {
        side: Side,
        kind: EntryKind,
        raw_price: f64,
    },
    /// The armed click: rest the armed entry at the clicked price, and
    /// disarm only if the venue took it.
    PlaceArmed {
        armed: ArmedPlacement,
        raw_price: f64,
    },
}

/// What one input pass decided: whether paper trading owns the gesture this
/// frame (the chart must not pan and the drawings must not select under
/// it), and the command a press asked for, if any.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputOutcome {
    pub owned: bool,
    pub command: Option<ChartCommand>,
}

impl InputOutcome {
    pub(super) fn claimed(command: Option<ChartCommand>) -> Self {
        Self {
            owned: true,
            command,
        }
    }
}

/// The chart gesture's per-frame state.
#[derive(Debug, Clone, PartialEq)]
pub struct Gesture {
    /// The line in the hand, if any.
    pub drag: PaperDrag,
    /// Where the hand is holding it, as a raw price.
    pub drag_price: Option<f64>,
    /// This frame's cmd preview — input computes, paint reads, one
    /// geometry both sides.
    pub cmd_preview: Option<CmdPreview>,
    /// This frame's opened order tags — same contract as `cmd_preview`:
    /// input computes, paint and press both read. Empty is the common
    /// case, so this allocates nothing on an ordinary frame.
    pub open_tags: Vec<OpenTag>,
    /// Whether this frame paints the paper layer. Same contract again, and
    /// the gate lives *here* so that every reader honours it: an unpainted
    /// line offers no cursor, no control and no press, whoever asks.
    pub layer_visible: bool,
    /// Harness override: paint the preview for this side, optionally at a
    /// stated x, with nobody at the keyboard (`QUANTICK_CMD_PREVIEW`).
    pub cmd_preview_force: Option<CmdPreviewForce>,
    /// Harness override: every resting order's tag opens, with nobody at
    /// the mouse (`QUANTICK_PAPER_ORDER_HOVER`).
    pub order_hover_force: bool,
    /// The working order hovered in the dock this frame — its chart line
    /// lifts, so one hover reads on both surfaces. Cleared after the chart
    /// consumed it (the host's `settle` runs last in the frame).
    pub hovered_order: Option<OrderId>,
    /// Whether the ruler spent this frame's wheel travel, so the chart's
    /// zoom can leave it alone.
    pub scroll_consumed: bool,
}

impl Default for Gesture {
    fn default() -> Self {
        Self {
            drag: PaperDrag::None,
            drag_price: None,
            cmd_preview: None,
            open_tags: Vec::new(),
            layer_visible: true,
            cmd_preview_force: None,
            order_hover_force: false,
            hovered_order: None,
            scroll_consumed: false,
        }
    }
}

impl Gesture {
    /// Whether a chart gesture is in flight — a line being dragged, a
    /// bracket leg being pulled into existence.
    #[must_use]
    pub fn active(&self) -> bool {
        self.drag != PaperDrag::None
    }

    /// Drop the line in the hand without submitting it; answers whether
    /// there was one.
    pub fn drop_drag(&mut self) -> bool {
        self.drag_price = None;
        std::mem::take(&mut self.drag) != PaperDrag::None
    }

    /// Whether a *real* aim is on screen — one a held key put there, not
    /// the capture hook.
    #[must_use]
    pub fn real_aim(&self) -> bool {
        self.cmd_preview.is_some_and(|preview| !preview.forced)
    }

    /// This frame's answer for one tag, or `None` while it rests as a
    /// pill. See [`OpenTag`].
    #[must_use]
    pub fn open_tag(&self, key: TagKey) -> Option<OpenTag> {
        self.open_tags.iter().copied().find(|tag| tag.key == key)
    }

    /// Where `QUANTICK_PAPER_ORDER_HOVER` parks the hand a capture run does
    /// not have — on the first working order's line, in the tag column.
    ///
    /// The hook forces the *tag* open, but the bracket handles are drawn
    /// only where a pointer actually is, so that a pane the hand is not on
    /// never offers a press it will not take. Without a parked pointer the
    /// handles would be unreachable from a scripted run — the ParkedHand
    /// problem the aim's own `CmdPreviewForce` already solves this way.
    ///
    /// Only the pane feeding paper input asks, so parking one hand cannot
    /// put handles on two charts. It paints and never places: this is read
    /// by the draw, and the press side reads the real pointer alone.
    #[must_use]
    pub fn forced_hover_pointer(
        &self,
        account: &PaperAccount,
        chart: Bounds,
        tag_right: f32,
        axis: &dyn PriceAxis,
    ) -> Option<Point> {
        if !self.order_hover_force {
            return None;
        }
        // The first order whose line this pane can actually show. Not
        // simply the first order: panes hold different price ranges, and a
        // hand parked on a line that is off-range here would be a hand on
        // nothing — the handles would stay unreachable on exactly the pane
        // the capture was pointed at.
        account
            .venue()
            .working_orders()
            .iter()
            .filter_map(|order| order.price)
            .map(|level| axis.y(level.to_f64().unwrap_or_default()))
            .find(|y| *y >= chart.top() && *y <= chart.bottom())
            .map(|y| Point::new(tag_right - TAG_GAP_PX - TAG_BUTTON_PX / 2.0, y))
    }

    /// The overlay control under the pointer, computed from this frame's
    /// axis and simulator state — never a cached pixel rect, which goes
    /// stale between paint and press the moment a live chart autoscales.
    /// Priority follows the draw stack: working orders on top, then the
    /// take profit, the stop, the position's ✕, and last the bracket
    /// handles beside the entry line.
    #[must_use]
    pub fn control_at(
        &self,
        account: &PaperAccount,
        pointer: Point,
        chart: Bounds,
        axis: &dyn PriceAxis,
    ) -> Option<PaperControl> {
        let tag_right = chart.right();
        // A line outside the visible price range paints no tag, so it
        // offers no control either — same gate the paint applies.
        let visible_center = |price: Decimal| {
            let y = axis.y(price.to_f64().unwrap_or_default());
            (y >= chart.top() && y <= chart.bottom())
                .then(|| clamp_tag_center(y, chart.top(), chart.bottom()))
        };
        for order in account.venue().working_orders().iter().rev() {
            // A tag that paints no ✕ offers none: the press reads the very
            // value the paint read, rather than recomputing a predicate
            // from a different pointer and a different rect.
            if let Some(level) = order.price
                && let Some(center_y) = visible_center(level)
                && self
                    .open_tag(TagKey::Order(order.id))
                    .is_some_and(|tag| tag.cancel)
                && close_button_rect(tag_right, center_y).contains(pointer)
            {
                return Some(PaperControl::CancelOrder(order.id));
            }
        }
        // Legs and handles, for every owner that has them. Working orders
        // first and newest-first, matching the draw stack above; the
        // position last, because its legs are the ones a trader reaches for
        // least often while an entry is still resting on top of them.
        for owner in account.bracket_owners() {
            let Some((side, reference, bracket, _)) = account.bracket_owner(owner) else {
                continue;
            };
            // A ladder offers no *whole-bracket* handle: one drag there
            // would replace every rung with a single level. Its rungs are
            // reachable one at a time instead, each with its own cross,
            // tested below - the trader's order is the trader's to move.
            if bracket.is_laddered() {
                if let BracketTarget::Order(id) = owner
                    && let Some(control) =
                        rung_control_at(id, bracket, pointer, tag_right, &visible_center)
                {
                    return Some(control);
                }
                continue;
            }
            let reference_center = visible_center(reference);
            for leg in [Leg::TakeProfit, Leg::StopLoss] {
                match leg.level(bracket) {
                    // A leg that exists offers its cross — while its tag is
                    // open, which is exactly while the cross is painted.
                    Some(level) => {
                        if let Some(center_y) = visible_center(level)
                            && self
                                .open_tag(TagKey::Leg(owner, leg))
                                .is_some_and(|tag| tag.cancel)
                            && close_button_rect(tag_right, center_y).contains(pointer)
                        {
                            return Some(PaperControl::ClearLeg { owner, leg });
                        }
                    }
                    // A leg that does not offers its handle. Same
                    // orientation mapping as the paint: the hit-test and the
                    // pixels must name the same handle, or a press acts on
                    // the leg the trader was not looking at.
                    None => {
                        if let Some(center_y) = reference_center
                            && bracket_handle_rect(
                                tag_right,
                                center_y,
                                leg.sits_above_entry(side) != axis.is_inverted(),
                            )
                            .contains(pointer)
                        {
                            return Some(PaperControl::Handle { owner, leg });
                        }
                    }
                }
            }
        }
        let position = account.venue().position()?;
        let entry_center = visible_center(position.avg_price)?;
        if close_button_rect(tag_right, entry_center).contains(pointer) {
            return Some(PaperControl::ClosePosition);
        }
        None
    }

    /// Which resting orders state themselves in full this frame, and which
    /// of those offer their ✕. At rest a tag is a compact pill, so the
    /// candles behind the most recent price stay readable; it opens under
    /// the pointer, while its dock row is hovered, and for as long as it is
    /// being dragged — a trader repricing an order needs every field of it,
    /// though a moving order offers no cancel.
    ///
    /// Computed once, from the pointer and the rect the *press* uses, and
    /// then read by both sides (see [`OpenTag`]).
    ///
    /// Per-frame path: the buffer is taken and refilled rather than rebuilt,
    /// so hovering an order costs no allocation once its capacity is up.
    pub fn refresh_open_tags(
        &mut self,
        account: &PaperAccount,
        frame: &ChartFrame,
        axis: Option<&dyn PriceAxis>,
    ) {
        let mut open = std::mem::take(&mut self.open_tags);
        open.clear();
        self.fill_open_tags(&mut open, account, frame, axis);
        self.open_tags = open;
    }

    /// See [`Self::refresh_open_tags`] — split out so the order list and
    /// the buffer are never borrowed from `self` at the same time.
    fn fill_open_tags(
        &self,
        open: &mut Vec<OpenTag>,
        account: &PaperAccount,
        frame: &ChartFrame,
        axis: Option<&dyn PriceAxis>,
    ) {
        if !frame.layer_visible {
            return;
        }
        let Some(axis) = axis else {
            return;
        };
        for order in account.venue().working_orders() {
            let Some(level) = order.price else { continue };
            let dragged = self.drag == PaperDrag::Order(order.id);
            let price = if dragged {
                self.drag_price
                    .unwrap_or_else(|| level.to_f64().unwrap_or_default())
            } else {
                level.to_f64().unwrap_or_default()
            };
            let expanded = dragged
                || self.hovered_order == Some(order.id)
                || self.order_hover_force
                || frame
                    .pointer
                    .is_some_and(|pointer| tag_row_hit(pointer, axis.y(price), frame.chart));
            if expanded {
                open.push(OpenTag {
                    key: TagKey::Order(order.id),
                    cancel: !dragged,
                });
            }
        }
        self.fill_open_legs(open, account, frame.pointer, frame.chart, axis);
    }

    /// The leg tags this frame opens, appended to the order tags'.
    ///
    /// A leg opens when the pointer is on its row — the rule an order's tag
    /// follows (`tag_row_hit`) — or when the capture hook forces every tag
    /// open. A leg in the hand is left out: its drag paints the open form
    /// itself, and a moving leg offers no ✕. A ladder's rungs keep their own
    /// tags.
    fn fill_open_legs(
        &self,
        open: &mut Vec<OpenTag>,
        account: &PaperAccount,
        pointer: Option<Point>,
        chart: Bounds,
        axis: &dyn PriceAxis,
    ) {
        for owner in account.bracket_owners() {
            let Some((_, _, bracket, _)) = account.bracket_owner(owner) else {
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
                let y = axis.y(level.to_f64().unwrap_or_default());
                if self.order_hover_force || pointer.is_some_and(|at| tag_row_hit(at, y, chart)) {
                    open.push(OpenTag {
                        key: TagKey::Leg(owner, leg),
                        cancel: true,
                    });
                }
            }
        }
    }

    /// Turn a pending entry-line press into the leg the pull chose, once it
    /// travelled far enough to mean it.
    pub(super) fn decide_pending_leg(
        &mut self,
        account: &PaperAccount,
        pointer_y: f32,
        axis: &dyn PriceAxis,
    ) {
        let Some(position) = account.venue().position() else {
            self.drag = PaperDrag::Blocked;
            return;
        };
        let entry_y = axis.y(position.avg_price.to_f64().unwrap_or_default());
        let delta = pointer_y - entry_y;
        if delta.abs() < CREATE_DECIDE_THRESHOLD_PX {
            return;
        }
        // The leg is chosen by *price*, not by screen direction: on an
        // inverted chart up is the loss side for a long, and reading pixels
        // would hand the pull the wrong leg.
        let above_entry =
            axis.price_at(pointer_y) > position.avg_price.to_f64().unwrap_or_default();
        let profit_side = match position.side {
            Side::Buy => above_entry,
            Side::Sell => !above_entry,
        };
        let leg = if profit_side {
            Leg::TakeProfit
        } else {
            Leg::StopLoss
        };
        let bracket = Bracket::whole(position.stop_loss, position.take_profit);
        // A side whose leg already exists stays blocked: that leg's own
        // line is its handle.
        self.drag = if leg.level(bracket).is_none() {
            PaperDrag::CreateLeg {
                owner: BracketTarget::Position,
                leg,
            }
        } else {
            PaperDrag::Blocked
        };
    }

    /// The cursor that announces what is under the pointer (audit M3/M4):
    /// a pointing hand over a painted control, vertical-resize over a
    /// draggable order/stop/target line and over an entry line with a
    /// missing bracket leg (dragging away from it creates that leg),
    /// not-allowed over a fully bracketed entry — the average entry itself
    /// is history and never moves. `None` away from everything, so the
    /// caller falls through to the drawings' own cursors.
    #[must_use]
    pub fn hover_cursor(
        &self,
        account: &PaperAccount,
        pointer: Point,
        chart: Bounds,
        axis: &dyn PriceAxis,
    ) -> Option<CursorHint> {
        if !self.layer_visible {
            return None;
        }
        if self.control_at(account, pointer, chart, axis).is_some() {
            return Some(CursorHint::PointingHand);
        }
        // While a *real* aim is painted, the whole plot is the click, so
        // the hand says so everywhere — and only there: the aim already
        // stood down over every line and control this function would
        // otherwise announce, so the two can no longer contradict.
        if self.real_aim() {
            return Some(CursorHint::PointingHand);
        }
        match line_at(account, pointer, axis)? {
            PaperDrag::Blocked => Some(CursorHint::NotAllowed),
            PaperDrag::Leg { .. }
            | PaperDrag::Rung { .. }
            | PaperDrag::Order(_)
            | PaperDrag::CreatePending => Some(CursorHint::ResizeVertical),
            PaperDrag::None | PaperDrag::CreateLeg { .. } => None,
        }
    }
}

/// The ✕ of whichever rung the pointer is over, if any.
///
/// Separate from the whole-bracket controls because a rung is addressed
/// by index: clearing one must leave the others exactly where they are,
/// which a leg-shaped control cannot say.
fn rung_control_at(
    order: OrderId,
    bracket: Bracket,
    pointer: Point,
    tag_right: f32,
    visible_center: &impl Fn(Decimal) -> Option<f32>,
) -> Option<PaperControl> {
    for (index, part) in bracket.parts().enumerate() {
        for (level, leg) in [
            (part.stop_loss, Leg::StopLoss),
            (part.take_profit, Leg::TakeProfit),
        ] {
            if let Some(level) = level
                && let Some(center_y) = visible_center(level)
                && close_button_rect(tag_right, center_y).contains(pointer)
            {
                return Some(PaperControl::ClearRung { order, index, leg });
            }
        }
    }
    None
}

/// Which line sits under the pointer, in draw-stack priority: pending
/// orders first (they draw on top), then take profit, stop loss, and
/// the entry line — which starts a bracket-creating drag while a leg is
/// missing, and blocks the gesture once both exist.
#[must_use]
pub fn line_at(account: &PaperAccount, pointer: Point, axis: &dyn PriceAxis) -> Option<PaperDrag> {
    let near = |price: Decimal| {
        let y = axis.y(price.to_f64().unwrap_or_default());
        (pointer.y - y).abs() <= LINE_GRAB_RADIUS_PX
    };
    for order in account.venue().working_orders().iter().rev() {
        if let Some(level) = order.price
            && near(level)
        {
            return Some(PaperDrag::Order(order.id));
        }
    }
    // A resting entry's rungs, newest order first, matching the draw
    // stack. They are the trader's to move: the strategy was the
    // template and this order carries a copy of it.
    for entry in account
        .venue()
        .working_orders()
        .iter()
        .rev()
        // Only a ladder has rungs. A whole bracket's two levels stay
        // `Leg`s, which is the grammar every other surface already
        // speaks for them.
        .filter(|order| !order.is_protective() && order.bracket.is_laddered())
    {
        for (index, part) in entry.bracket.parts().enumerate() {
            for (level, leg) in [
                (part.take_profit, Leg::TakeProfit),
                (part.stop_loss, Leg::StopLoss),
            ] {
                if level.is_some_and(near) {
                    return Some(PaperDrag::Rung {
                        order: entry.id,
                        index,
                        leg,
                    });
                }
            }
        }
    }
    for owner in account.bracket_owners() {
        let Some((.., bracket, _)) = account.bracket_owner(owner) else {
            continue;
        };
        for leg in [Leg::TakeProfit, Leg::StopLoss] {
            if let Some(level) = leg.level(bracket)
                && near(level)
            {
                return Some(PaperDrag::Leg { owner, leg });
            }
        }
    }
    let position = account.venue().position()?;
    if near(position.avg_price) {
        let creatable = position.stop_loss.is_none() || position.take_profit.is_none();
        return Some(if creatable {
            PaperDrag::CreatePending
        } else {
            PaperDrag::Blocked
        });
    }
    None
}
