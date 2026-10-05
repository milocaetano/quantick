//! The pointer on the chart, at the window's edge.
//!
//! What a press lands on and what it asks for is decided by the desk
//! (`quantick_paper::desk`); this module is the shell around it. It reads
//! the window's input into the desk's plain values, carries out the command
//! the desk answers with through the account's own funnel, and turns the
//! desk's cursor and geometry back into the window's types.

use eframe::egui;
use quantick_engine::Side;
#[cfg(test)]
use quantick_paper::desk::PaperDrag;
use quantick_paper::desk::{ChartCommand, ChartFrame, CursorHint, HeldKeys, PriceAxis};
use quantick_sim::EntryKind;

#[cfg(test)]
use super::PaperControl;
use super::paint_ctx::{bounds, point, pos};
use super::{ChartInput, PaperTrading};
use crate::chart::PriceScale;

/// The chart's scale, as the desk's price axis: the one price-to-pixel law,
/// lent rather than copied.
pub(super) struct Axis<'a>(pub(super) &'a PriceScale);

impl PriceAxis for Axis<'_> {
    fn y(&self, price: f64) -> f32 {
        self.0.y(price)
    }

    fn price_at(&self, y: f32) -> f64 {
        self.0.price_at(y)
    }

    fn is_inverted(&self) -> bool {
        self.0.is_inverted()
    }
}

impl ChartInput<'_> {
    /// The frame, in the desk's plain values.
    fn frame(&self) -> ChartFrame {
        ChartFrame {
            chart: bounds(self.chart),
            pointer: self.pointer.map(point),
            primary_pressed: self.primary_pressed,
            primary_down: self.primary_down,
            primary_released: self.primary_released,
            keys: HeldKeys {
                shift: self.modifiers.shift,
                command: self.modifiers.command,
                alt: self.modifiers.alt,
            },
            canvas_claimed: self.canvas_claimed,
            scroll_y: self.scroll_y,
            middle_pressed: self.middle_pressed,
            layer_visible: self.layer_visible,
        }
    }
}

impl PaperTrading {
    /// Where `QUANTICK_PAPER_ORDER_HOVER` parks the hand a capture run does
    /// not have; see `Gesture::forced_hover_pointer`.
    #[must_use]
    pub fn forced_hover_pointer(
        &self,
        chart: egui::Rect,
        tag_right: f32,
        scale: &PriceScale,
    ) -> Option<egui::Pos2> {
        self.desk
            .gesture
            .forced_hover_pointer(&self.account, bounds(chart), tag_right, &Axis(scale))
            .map(pos)
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
        self.desk.gesture.active()
    }

    /// Cancel the transient chart interaction — an armed placement or a
    /// grabbed line (dropped without submitting). Called from the app's
    /// escape stack; returns true when there was something to cancel, so
    /// the stack spends exactly one layer on it.
    pub fn cancel_interaction(&mut self) -> bool {
        if self.desk.cancel_gesture() {
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

    /// Route pointer input to the simulated lines. Returns true when paper
    /// trading owns the gesture this frame — the chart must not pan and the
    /// drawings must not select under it.
    pub fn handle_chart_input(&mut self, input: &ChartInput<'_>) -> bool {
        let axis = input.scale.map(Axis);
        let outcome = self.desk.handle_input(
            &self.account,
            &input.frame(),
            axis.as_ref().map(|axis| axis as &dyn PriceAxis),
        );
        if let Some(command) = outcome.command {
            self.apply(command);
        }
        outcome.owned
    }

    /// Carry out what a press asked for, through the account's own funnel.
    fn apply(&mut self, command: ChartCommand) {
        match command {
            ChartCommand::ClosePosition => self.account.close_position(),
            ChartCommand::AmendLeg { owner, leg, price } => {
                self.account.amend_leg(owner, leg, price);
            }
            ChartCommand::AmendRung {
                order,
                index,
                leg,
                price,
            } => self.account.amend_rung(order, index, leg, price),
            ChartCommand::CancelOrder(id) => {
                let events = self.account.venue_mut().cancel(id);
                self.account.handle_events(events);
            }
            ChartCommand::AmendOrder { id, price } => {
                let events = self.account.venue_mut().amend_price(id, price);
                self.account.handle_events(events);
            }
            ChartCommand::PlaceResting {
                side,
                kind,
                raw_price,
            } => {
                self.place_resting(side, kind, raw_price);
            }
            // The armed click: place at the clicked price and disarm on
            // success. Stays armed on a rejection — the toast explains where
            // the order may sit, and the user clicks again.
            ChartCommand::PlaceArmed { armed, raw_price } => {
                if self.place_resting(armed.side, armed.kind, raw_price) {
                    self.desk.armed = None;
                }
            }
        }
    }

    /// The overlay control under the pointer; see `Gesture::control_at`.
    #[cfg(test)]
    pub(super) fn control_at(
        &self,
        pointer: egui::Pos2,
        chart: egui::Rect,
        scale: &PriceScale,
    ) -> Option<PaperControl> {
        self.desk
            .gesture
            .control_at(&self.account, point(pointer), bounds(chart), &Axis(scale))
    }

    /// Which line sits under the pointer; see `quantick_paper::desk::line_at`.
    #[cfg(test)]
    pub(super) fn line_at(&self, pointer: egui::Pos2, scale: &PriceScale) -> Option<PaperDrag> {
        quantick_paper::desk::line_at(&self.account, point(pointer), &Axis(scale))
    }

    /// The cursor that announces what is under the pointer; `None` away
    /// from everything, so the caller falls through to the drawings' own
    /// cursors. See `Gesture::hover_cursor`.
    #[must_use]
    pub fn hover_cursor(
        &self,
        pointer: egui::Pos2,
        chart: egui::Rect,
        scale: &PriceScale,
    ) -> Option<egui::CursorIcon> {
        self.desk
            .gesture
            .hover_cursor(&self.account, point(pointer), bounds(chart), &Axis(scale))
            .map(|hint| match hint {
                CursorHint::PointingHand => egui::CursorIcon::PointingHand,
                CursorHint::ResizeVertical => egui::CursorIcon::ResizeVertical,
                CursorHint::NotAllowed => egui::CursorIcon::NotAllowed,
            })
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
        self.desk.gesture.scroll_consumed
    }

    /// Whether an aim is on screen this frame.
    ///
    /// The pointer compass asks before writing its own price on the axis:
    /// the aim already puts one there, on the same pixel, and two chips
    /// stacked on one pixel is not two facts. Same rule the crosshair tool
    /// already earns.
    #[must_use]
    pub fn aiming(&self) -> bool {
        self.desk.gesture.cmd_preview.is_some()
    }
}
