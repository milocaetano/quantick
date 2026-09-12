//! The pointer on the chart: what it is over, and what a press does to it.
//!
//! One pass per frame decides the control under the pointer, the cursor it
//! deserves and the drag it may start or finish. It never draws - the
//! geometry it hit-tests comes from [`super::paint`], so the control a press
//! finds is the one the frame painted.

use eframe::egui;
use quantick_engine::Side;
use quantick_sim::{Bracket, BracketTarget, EntryKind, OrderId};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::paint_ctx::{bracket_handle_rect, clamp_tag_center, close_button_rect, tag_row_hit};
use super::{
    ArmedPlacement, CREATE_DECIDE_THRESHOLD_PX, ChartInput, LINE_GRAB_RADIUS_PX, Leg, OpenTag,
    PaperControl, PaperDrag, PaperTrading, TAG_BUTTON_PX, TAG_GAP_PX, TagKey,
};
use crate::chart::PriceScale;

impl PaperTrading {
    /// Route pointer input to the simulated lines. Returns true when paper
    /// trading owns the gesture this frame — the chart must not pan and the
    /// drawings must not select under it.
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
        chart: egui::Rect,
        tag_right: f32,
        scale: &PriceScale,
    ) -> Option<egui::Pos2> {
        if !self.order_hover_force {
            return None;
        }
        // The first order whose line this pane can actually show. Not
        // simply the first order: panes hold different price ranges, and a
        // hand parked on a line that is off-range here would be a hand on
        // nothing — the handles would stay unreachable on exactly the pane
        // the capture was pointed at.
        self.account
            .venue
            .working_orders()
            .iter()
            .filter_map(|order| order.price)
            .map(|level| scale.y(level.to_f64().unwrap_or_default()))
            .find(|y| *y >= chart.top() && *y <= chart.bottom())
            .map(|y| egui::pos2(tag_right - TAG_GAP_PX - TAG_BUTTON_PX / 2.0, y))
    }

    /// Whether a chart gesture this module owns is in flight — a line being
    /// dragged, a bracket leg being pulled into existence.
    ///
    /// The tab asks so it can keep the gesture with the pane that started
    /// it: the price under a grabbed line is read against one pane's scale,
    /// and letting the pointer wander into a neighbour mid-drag would
    /// reprice the order to whatever that pane's scale says.
    #[must_use]
    pub fn gesture_active(&self) -> bool {
        self.drag != PaperDrag::None
    }

    /// Cancel the transient chart interaction — an armed placement or a
    /// grabbed line (dropped without submitting). Called from the app's
    /// escape stack; returns true when there was something to cancel, so
    /// the stack spends exactly one layer on it.
    pub fn cancel_interaction(&mut self) -> bool {
        if self.account.armed.take().is_some() {
            return true;
        }
        if self.drag != PaperDrag::None {
            self.drag = PaperDrag::None;
            self.drag_price = None;
            return true;
        }
        if self.account.report.clear_selected_trade() {
            return true;
        }
        // The ruler is the *last* layer, not the first. It is a standing
        // preference rather than a gesture left half-finished, and putting
        // it in front shadowed the two things Escape was already for: a
        // trader with an order armed presses Escape to disarm it, and must
        // not lose their distance instead.
        self.clear_ruler()
    }

    pub fn handle_chart_input(&mut self, input: &ChartInput<'_>) -> bool {
        // Three per-frame facts, in dependency order: whether the layer
        // paints at all, which tags are open (the aim yields to an open ✕,
        // so this comes before it), then the cmd preview. All three are
        // written here, and read by `draw_layer`, by the press below and
        // by `hover_cursor` — one value each, never a formula re-run
        // against a different pointer.
        self.layer_visible = input.layer_visible;
        self.refresh_open_tags(input);
        self.cmd_preview = self.compute_cmd_preview(input);

        // The ruler. With the aim up, the wheel walks the projected stop and
        // target out from the pointer, one tick per notch, the same distance
        // on both sides - so what is on screen before the click is the trade
        // at 1:1, and the trader can see whether that distance is worth
        // taking. A forced aim is a capture fixture with no hand behind it
        // and never spends the wheel; a selected strategy owns the distances
        // and hands the wheel back to the chart.
        // Pressing the wheel puts the ruler away. Only while an aim is up,
        // so a middle-click anywhere else stays whatever it already was.
        if input.middle_pressed
            && self.cmd_preview.is_some_and(|preview| !preview.forced)
            && self.clear_ruler()
        {
            self.cmd_preview = self.compute_cmd_preview(input);
        }
        self.scroll_consumed = false;
        if input.scroll_y.abs() > f32::EPSILON
            && self.cmd_preview.is_some_and(|preview| !preview.forced)
        {
            self.step_ruler(input.scroll_y);
            self.ruler_rolled = true;
            self.scroll_consumed = true;
            // The aim now carries its bracket; recompute so the paint and
            // the press read one value rather than two.
            self.cmd_preview = self.compute_cmd_preview(input);
        }

        // Nothing paper is painted while its layer is off, so nothing
        // paper takes the press: an invisible line is not a control.
        if !input.layer_visible {
            self.drag = PaperDrag::None;
            return false;
        }

        // An overlay control (a tag's ✕, a bracket handle) takes the press
        // before *everything*: before the armed click — arming an order must
        // never eat the ✕ under the pointer — and before the line grab,
        // which is what made "close this order" read as "drag this order".
        // The hit is geometric, from this frame's own scale and state: a
        // pixel rect cached from the last paint goes stale the moment a
        // live chart autoscales, and the press then slips onto the line.
        if input.primary_pressed
            && self.drag == PaperDrag::None
            && let Some(pointer) = input.pointer
            && let Some(scale) = input.scale
            && let Some(control) = self.control_at(pointer, input.chart, scale)
        {
            match control {
                PaperControl::ClosePosition => self.account.close_position(),
                PaperControl::ClearLeg { owner, leg } => self.account.amend_leg(owner, leg, None),
                PaperControl::ClearRung { order, index, leg } => {
                    self.account.amend_rung(order, index, leg, None);
                }
                PaperControl::CancelOrder(id) => {
                    let events = self.account.venue.cancel(id);
                    self.account.handle_events(events);
                }
                PaperControl::Handle { owner, leg } => {
                    self.drag = PaperDrag::CreateLeg { owner, leg };
                    self.drag_price = Some(scale.price_at(pointer.y));
                }
            }
            return true;
        }

        // The aimed order. There is no separate target to hit: a label
        // that rides the pointer can never be landed on — move toward it
        // and it moves with you — so the *held modifier* is the deliberate
        // act and the label beside the cursor is the statement of what
        // this click will do. It is also the *last* claimant on the
        // canvas: `compute_cmd_preview` has already stood the aim down
        // wherever an overlay control, a paper line, an armed placement or
        // an annotation holds the pixel, so a preview existing here means
        // nothing else wanted this press. A forced aim is a capture
        // fixture with no hand behind it and never places.
        if input.primary_pressed
            && self.drag == PaperDrag::None
            && let Some(preview) = self.cmd_preview
            && !preview.forced
        {
            self.place_resting(preview.side, preview.kind, preview.raw_price);
            return true;
        }

        // An armed placement takes the next chart click.
        if let Some(armed) = self.account.armed
            && input.primary_pressed
            && let Some(pointer) = input.pointer
            && input.chart.contains(pointer)
            && let Some(scale) = input.scale
        {
            self.place_armed(armed, scale.price_at(pointer.y));
            return true;
        }

        // Grab a line.
        if input.primary_pressed
            && self.drag == PaperDrag::None
            && let Some(pointer) = input.pointer
            && input.chart.contains(pointer)
            && let Some(scale) = input.scale
            && let Some(target) = self.line_at(pointer, scale)
        {
            self.drag = target;
            self.drag_price = Some(scale.price_at(pointer.y));
            return true;
        }

        // Follow the pointer while dragging. A press that started on the
        // entry line commits to one bracket leg on its first real pull:
        // towards the profit side it creates the take profit, towards the
        // losing side the stop — and a side whose leg already exists stays
        // blocked (that leg's own line is its handle).
        if input.primary_down && self.drag != PaperDrag::None {
            if let (Some(pointer), Some(scale)) = (input.pointer, input.scale) {
                let y = pointer.y.clamp(input.chart.top(), input.chart.bottom());
                self.drag_price = Some(scale.price_at(y));
                if self.drag == PaperDrag::CreatePending {
                    self.decide_pending_leg(y, scale);
                }
            }
            return true;
        }

        // Drop: submit the new price; the simulator answers (a rejection
        // snaps the line back and the toast explains why). Creating a leg
        // and repricing it are the same command — the bracket is replaced
        // wholesale either way.
        if input.primary_released && self.drag != PaperDrag::None {
            let drag = std::mem::take(&mut self.drag);
            if let Some(price) = self.drag_price.take() {
                let price = self.account.snap(price);
                match drag {
                    PaperDrag::Leg { owner, leg } | PaperDrag::CreateLeg { owner, leg } => {
                        self.account.amend_leg(owner, leg, Some(price));
                    }
                    PaperDrag::Order(id) => {
                        let events = self.account.venue.amend_price(id, price);
                        self.account.handle_events(events);
                    }
                    PaperDrag::Rung { order, index, leg } => {
                        self.account.amend_rung(order, index, leg, Some(price));
                    }
                    PaperDrag::None | PaperDrag::Blocked | PaperDrag::CreatePending => {}
                }
            }
            return true;
        }

        self.drag != PaperDrag::None
    }

    /// The overlay control under the pointer, computed from this frame's
    /// scale and simulator state — never a cached pixel rect, which goes
    /// stale between paint and press the moment a live chart autoscales.
    /// Priority follows the draw stack: working orders on top, then the
    /// take profit, the stop, the position's ✕, and last the bracket
    /// handles beside the entry line.
    pub(super) fn control_at(
        &self,
        pointer: egui::Pos2,
        chart: egui::Rect,
        scale: &PriceScale,
    ) -> Option<PaperControl> {
        let tag_right = chart.right();
        // A line outside the visible price range paints no tag, so it
        // offers no control either — same gate the paint applies.
        let visible_center = |price: Decimal| {
            let y = scale.y(price.to_f64().unwrap_or_default());
            (y >= chart.top() && y <= chart.bottom())
                .then(|| clamp_tag_center(y, chart.top(), chart.bottom()))
        };
        for order in self.account.venue.working_orders().iter().rev() {
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
        for owner in self.account.bracket_owners() {
            let Some((side, reference, bracket, _)) = self.account.bracket_owner(owner) else {
                continue;
            };
            // A ladder offers no *whole-bracket* handle: one drag there
            // would replace every rung with a single level. Its rungs are
            // reachable one at a time instead, each with its own cross,
            // tested below - the trader's order is the trader's to move.
            if bracket.is_laddered() {
                if let BracketTarget::Order(id) = owner
                    && let Some(control) =
                        self.rung_control_at(id, bracket, pointer, tag_right, &visible_center)
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
                                leg.sits_above_entry(side) != scale.is_inverted(),
                            )
                            .contains(pointer)
                        {
                            return Some(PaperControl::Handle { owner, leg });
                        }
                    }
                }
            }
        }
        let position = self.account.venue.position()?;
        let entry_center = visible_center(position.avg_price)?;
        if close_button_rect(tag_right, entry_center).contains(pointer) {
            return Some(PaperControl::ClosePosition);
        }
        None
    }

    /// The ✕ of whichever rung the pointer is over, if any.
    ///
    /// Separate from the whole-bracket controls because a rung is addressed
    /// by index: clearing one must leave the others exactly where they are,
    /// which a leg-shaped control cannot say.
    fn rung_control_at(
        &self,
        order: OrderId,
        bracket: Bracket,
        pointer: egui::Pos2,
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
    fn refresh_open_tags(&mut self, input: &ChartInput<'_>) {
        let mut open = std::mem::take(&mut self.open_tags);
        open.clear();
        self.fill_open_tags(&mut open, input);
        self.open_tags = open;
    }

    /// See [`Self::refresh_open_tags`] — split out so the order list and
    /// the buffer are never borrowed from `self` at the same time.
    fn fill_open_tags(&self, open: &mut Vec<OpenTag>, input: &ChartInput<'_>) {
        if !input.layer_visible {
            return;
        }
        let Some(scale) = input.scale else {
            return;
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
            let expanded = dragged
                || self.hovered_order == Some(order.id)
                || self.order_hover_force
                || input
                    .pointer
                    .is_some_and(|pointer| tag_row_hit(pointer, scale.y(price), input.chart));
            if expanded {
                open.push(OpenTag {
                    key: TagKey::Order(order.id),
                    cancel: !dragged,
                });
            }
        }
        self.fill_open_legs(open, input.pointer, input.chart, scale);
    }

    /// This frame's answer for one tag, or `None` while it rests as a
    /// pill. See [`OpenTag`].
    pub(super) fn open_tag(&self, key: TagKey) -> Option<OpenTag> {
        self.open_tags.iter().copied().find(|tag| tag.key == key)
    }

    /// Turn a pending entry-line press into the leg the pull chose, once it
    /// travelled far enough to mean it.
    fn decide_pending_leg(&mut self, pointer_y: f32, scale: &PriceScale) {
        let Some(position) = self.account.venue.position() else {
            self.drag = PaperDrag::Blocked;
            return;
        };
        let entry_y = scale.y(position.avg_price.to_f64().unwrap_or_default());
        let delta = pointer_y - entry_y;
        if delta.abs() < CREATE_DECIDE_THRESHOLD_PX {
            return;
        }
        // The leg is chosen by *price*, not by screen direction: on an
        // inverted chart up is the loss side for a long, and reading pixels
        // would hand the pull the wrong leg.
        let above_entry =
            scale.price_at(pointer_y) > position.avg_price.to_f64().unwrap_or_default();
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
        pointer: egui::Pos2,
        chart: egui::Rect,
        scale: &PriceScale,
    ) -> Option<egui::CursorIcon> {
        if !self.layer_visible {
            return None;
        }
        if self.control_at(pointer, chart, scale).is_some() {
            return Some(egui::CursorIcon::PointingHand);
        }
        // While a *real* aim is painted, the whole plot is the click, so
        // the hand says so everywhere — and only there: the aim already
        // stood down over every line and control this function would
        // otherwise announce, so the two can no longer contradict.
        if self.cmd_preview.is_some_and(|preview| !preview.forced) {
            return Some(egui::CursorIcon::PointingHand);
        }
        match self.line_at(pointer, scale)? {
            PaperDrag::Blocked => Some(egui::CursorIcon::NotAllowed),
            PaperDrag::Leg { .. }
            | PaperDrag::Rung { .. }
            | PaperDrag::Order(_)
            | PaperDrag::CreatePending => Some(egui::CursorIcon::ResizeVertical),
            PaperDrag::None | PaperDrag::CreateLeg { .. } => None,
        }
    }

    /// Which line sits under the pointer, in draw-stack priority: pending
    /// orders first (they draw on top), then take profit, stop loss, and
    /// the entry line — which starts a bracket-creating drag while a leg is
    /// missing, and blocks the gesture once both exist.
    pub(super) fn line_at(&self, pointer: egui::Pos2, scale: &PriceScale) -> Option<PaperDrag> {
        let near = |price: Decimal| {
            let y = scale.y(price.to_f64().unwrap_or_default());
            (pointer.y - y).abs() <= LINE_GRAB_RADIUS_PX
        };
        for order in self.account.venue.working_orders().iter().rev() {
            if let Some(level) = order.price
                && near(level)
            {
                return Some(PaperDrag::Order(order.id));
            }
        }
        // A resting entry's rungs, newest order first, matching the draw
        // stack. They are the trader's to move: the strategy was the
        // template and this order carries a copy of it.
        for entry in self
            .account
            .venue
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
        for owner in self.account.bracket_owners() {
            let Some((.., bracket, _)) = self.account.bracket_owner(owner) else {
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
        let position = self.account.venue.position()?;
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

    /// The armed click: place at the clicked price and disarm on success.
    /// Stays armed on a rejection — the toast explains where the order may
    /// sit, and the user clicks again.
    fn place_armed(&mut self, armed: ArmedPlacement, raw_price: f64) {
        if self.place_resting(armed.side, armed.kind, raw_price) {
            self.account.armed = None;
        }
    }

    /// Rest a limit/stop entry at `raw_price` with the ticket's quantity
    /// and offsets; returns whether the simulator accepted it.
    pub(super) fn place_resting(&mut self, side: Side, kind: EntryKind, raw_price: f64) -> bool {
        let price = self.account.snap(raw_price);
        // The offsets are read here because an unreadable one is a message
        // beside the box the trader typed in. Everything after is placement.
        let Some(ticket) = self.parse_bracket(side, price) else {
            return false;
        };
        let env = self.account_env(side, price);
        self.account.place_resting(side, kind, price, ticket, &env)
    }

    /// Whether the ruler spent this frame's wheel travel. The chart asks
    /// before zooming: one wheel, one meaning at a time.
    #[must_use]
    pub fn consumed_scroll(&self) -> bool {
        self.scroll_consumed
    }

    /// Whether an aim is on screen this frame.
    ///
    /// The pointer compass asks before writing its own price on the axis:
    /// the aim already puts one there, on the same pixel, and two chips
    /// stacked on one pixel is not two facts. Same rule the crosshair tool
    /// already earns.
    #[must_use]
    pub fn aiming(&self) -> bool {
        self.cmd_preview.is_some()
    }
}
