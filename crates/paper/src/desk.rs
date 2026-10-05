//! The trading desk: the deciding half of the ticket and the chart gesture.
//!
//! [`crate::PaperAccount`] is the money path; this is what stands between a
//! trader's hand and it. What the ticket's boxes say ([`ticket`]), how far
//! the ruler is wound ([`ruler`]), what a held key aims ([`cmd`]), which line
//! or ✕ a press lands on and what it asks for ([`gesture`]), and the words a
//! bracket leg wears ([`leg_tag`]) — over the plain geometry both the paint
//! and the press read ([`geometry`]).
//!
//! A functional core: the host hands over the frame as values
//! ([`ChartFrame`], a [`geometry::PriceAxis`]), the desk updates its own
//! state and answers with the [`ChartCommand`] a press asked for, and the
//! host carries that out through the account. No toolkit type crosses in
//! either direction, so every rule here is tested without a window, and the
//! chart's paint and a second operator's call can only ever read the one
//! state this holds.

use quantick_engine::Side;
use quantick_sim::{Bracket, EntryKind};
use rust_decimal::Decimal;

use crate::PaperAccount;
use crate::account::AccountEnv;
use crate::risk_sizing::RiskState;

pub mod cmd;
pub mod geometry;
pub mod gesture;
pub mod leg_tag;
pub mod ruler;
pub mod ticket;

#[cfg(test)]
mod tests;

pub use cmd::{
    CmdEntryKind, CmdModifier, CmdPreview, CmdPreviewForce, CmdTradingSettings, HeldKeys,
    resolve_cmd_kind,
};
pub use geometry::{Bounds, Point, PriceAxis};
pub use gesture::{
    ArmedPlacement, ChartCommand, ChartFrame, CursorHint, Gesture, InputOutcome, OpenTag,
    PaperControl, PaperDrag, TagKey, line_at,
};
pub use ruler::Ruler;
pub use ticket::Ticket;

/// Whether the strategy editor is up, what it has open and whether it holds
/// an edit not yet saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StrategyEditor {
    /// Whether the editor window is up.
    pub open: bool,
    /// An edit inside the editor that has not been saved yet.
    ///
    /// A name is typed one character at a time and a drag value fires every
    /// frame it is held; persisting each of those would read, parse and
    /// rewrite the sidecar - and clone the list into every tab - dozens of
    /// times for one word, on the UI thread. The edits live in memory and
    /// the save happens when the editor closes, which is also when the
    /// trader has finished saying what they meant.
    pub dirty: bool,
    /// Which strategy the editor has open; `None` while the list is empty.
    pub editing: Option<usize>,
}

impl StrategyEditor {
    /// Open the editor on the strategy the ticket is armed with, else the
    /// first of `count`.
    pub fn open_on(&mut self, selected: Option<usize>, count: usize) {
        self.open = true;
        self.editing = selected.or(if count == 0 { None } else { Some(0) });
    }

    /// Opening onto a blank right pane while the list holds strategies is
    /// an editor that looks broken. Whatever route opened it - the ticket's
    /// button, or the launch hook, which has no click to carry a choice - it
    /// opens on the one the ticket is armed with, else the first.
    pub fn settle_editing(&mut self, selected: Option<usize>, count: usize) {
        if self.editing.is_none() && count != 0 {
            self.editing = Some(selected.unwrap_or(0));
        }
    }

    /// A strategy was appended; the list now holds `count`.
    pub fn added(&mut self, count: usize) {
        self.editing = count.checked_sub(1);
        self.dirty = true;
    }

    /// The strategy at `index` was removed; the list now holds `count`.
    pub fn removed(&mut self, index: usize, count: usize) {
        self.editing = if count == 0 {
            None
        } else {
            Some(index.min(count - 1))
        };
        self.dirty = true;
    }

    /// Close the editor. Closing is the save point: answers whether there
    /// is an edit to persist, once.
    pub fn close(&mut self) -> bool {
        self.open = false;
        std::mem::take(&mut self.dirty)
    }
}

/// The desk's whole state: the typed ticket, the ruler, the cmd gesture's
/// settings and the armed click, the chart gesture and the strategy editor.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Desk {
    /// The order-entry form, as typed.
    pub ticket: Ticket,
    /// How far the wheel has wound the projected bracket, and each
    /// instrument's step.
    pub ruler: Ruler,
    /// Cmd trading: the toggle and its two key bindings (app-wide; the
    /// host persists and fans out changes).
    pub cmd_trading: CmdTradingSettings,
    /// The entry the next chart click places, if one is armed.
    pub armed: Option<ArmedPlacement>,
    /// The chart gesture's per-frame state.
    pub gesture: Gesture,
    /// The strategy editor's window state.
    pub strategy_editor: StrategyEditor,
}

impl Desk {
    /// Install cmd-trading settings — the host's fan-out on boot and on a
    /// change made in any tab (one gesture, one meaning, everywhere).
    pub fn set_cmd_trading(&mut self, settings: CmdTradingSettings) {
        self.cmd_trading = settings;
        if !settings.enabled {
            self.gesture.cmd_preview = None;
        }
    }

    /// How far one notch walks this instrument's ruler, in points: what the
    /// trader typed for the account's symbol, else the derived default.
    #[must_use]
    pub fn ruler_step(&self, account: &PaperAccount) -> Decimal {
        self.ruler
            .step_for(account.symbol(), account.derived_ruler_step())
    }

    /// The ruler's projected stop and target around an aimed entry; see
    /// [`Ruler::levels`].
    #[must_use]
    pub fn ruler_levels(
        &self,
        account: &PaperAccount,
        side: Side,
        price: Decimal,
    ) -> (Option<Decimal>, Option<Decimal>) {
        self.ruler.levels(side, price, self.ruler_step(account))
    }

    /// Everything the account needs from the ticket for one call.
    ///
    /// Built per call and never kept: a stored copy would answer for the
    /// form the trader used to have typed, which is `ReportEnv`'s reason too.
    #[must_use]
    pub fn account_env(&self, account: &PaperAccount, side: Side, price: Decimal) -> AccountEnv {
        AccountEnv {
            ruler_levels: match self.ruler_levels(account, side, price) {
                (Some(stop), Some(target)) => Some((stop, target)),
                _ => None,
            },
            form: self.ticket.form(),
        }
    }

    /// The reading half of [`Ticket::parse_bracket`]: the same arithmetic
    /// with no complaint, so the projection can ask what the ticket says
    /// without putting a message on screen every frame.
    #[must_use]
    pub fn ticket_bracket(&self, side: Side, reference: Decimal) -> Bracket {
        self.ticket.form().bracket(side, reference)
    }

    /// The risk read, and whether it blocks an entry: the one read the
    /// entry button, the ticket's own line and the control plane share,
    /// taken at the mark (zero while there is none) as a buy.
    #[must_use]
    pub fn risk_report(&self, account: &PaperAccount) -> (RiskState, bool) {
        let reference = account.mark_price().unwrap_or_default();
        account.risk_report(&self.account_env(account, Side::Buy, reference))
    }

    /// The entry button's label for `side`; see [`ticket::entry_label`].
    ///
    /// The size the press would actually send. The risk-derived quantity
    /// is written into the field by the ticket, so a toolbar reading the
    /// field while the dock is closed promised a size the click did not
    /// send - the button naming one number and the order carrying
    /// another is the plainest kind of lie this surface can tell.
    #[must_use]
    pub fn entry_label(&self, account: &PaperAccount, side: Side) -> String {
        let derived = self.risk_report(account).0.derived_quantity();
        ticket::entry_label(
            side,
            derived.or_else(|| self.ticket.quantity_preview()),
            account.venue().position(),
        )
    }

    /// Cancel the transient chart interaction — an armed placement, else a
    /// grabbed line (dropped without submitting). Answers whether there was
    /// something to cancel, so an escape stack spends exactly one layer.
    pub fn cancel_gesture(&mut self) -> bool {
        self.armed.take().is_some() || self.gesture.drop_drag()
    }

    /// The source rebuilt its timeline: nothing armed, nothing in the hand.
    pub fn reset_timeline(&mut self) {
        self.armed = None;
        self.gesture.drop_drag();
    }

    /// Rest a limit/stop entry at `raw_price` with the ticket's quantity
    /// and offsets; answers whether the simulator accepted it.
    ///
    /// # Errors
    ///
    /// The ticket's complaint about an offset that does not parse, for the
    /// host to show beside the box; nothing reaches the venue then.
    pub fn place_resting(
        &self,
        account: &mut PaperAccount,
        side: Side,
        kind: EntryKind,
        raw_price: f64,
    ) -> Result<bool, String> {
        let price = account.snap(raw_price);
        // The offsets are read here because an unreadable one is a message
        // beside the box the trader typed in. Everything after is placement.
        let ticket = self.ticket.parse_bracket(side, price)?;
        let env = self.account_env(account, side, price);
        Ok(account.place_resting(side, kind, price, ticket, &env))
    }

    /// Carry out what a press asked for, through the account's own funnel.
    ///
    /// # Errors
    ///
    /// The ticket's complaint when a placement's offsets do not parse; see
    /// [`Self::place_resting`].
    pub fn carry_out(
        &mut self,
        account: &mut PaperAccount,
        command: ChartCommand,
    ) -> Result<(), String> {
        match command {
            ChartCommand::ClosePosition => account.close_position(),
            ChartCommand::AmendLeg { owner, leg, price } => account.amend_leg(owner, leg, price),
            ChartCommand::AmendRung {
                order,
                index,
                leg,
                price,
            } => account.amend_rung(order, index, leg, price),
            ChartCommand::CancelOrder(id) => {
                account.cancel_order(id);
            }
            ChartCommand::AmendOrder { id, price } => {
                account.amend_order_price(id, price);
            }
            ChartCommand::PlaceResting {
                side,
                kind,
                raw_price,
            } => {
                self.place_resting(account, side, kind, raw_price)?;
            }
            // The armed click: place at the clicked price and disarm on
            // success. Stays armed on a rejection — the toast explains where
            // the order may sit, and the user clicks again.
            ChartCommand::PlaceArmed { armed, raw_price } => {
                if self.place_resting(account, armed.side, armed.kind, raw_price)? {
                    self.armed = None;
                }
            }
        }
        Ok(())
    }

    /// The aim the frame's pointer and held keys describe, or `None` where
    /// there is none to show.
    #[must_use]
    pub fn compute_cmd_preview(
        &self,
        account: &PaperAccount,
        frame: &ChartFrame,
        axis: Option<&dyn PriceAxis>,
    ) -> Option<CmdPreview> {
        if !self.cmd_trading.enabled || !frame.layer_visible {
            return None;
        }
        // A drawing a press would grab, or the canvas's own chrome. The
        // buy modifier is Shift by default — the very key that levels a
        // channel corner — so sweeping across a drawn line blinks the aim
        // off for its grab band, in step with the move cursor the drawings
        // put up.
        if frame.canvas_claimed {
            return None;
        }
        // An armed limit/stop is an intent already stated, with its own
        // hint on screen; a modifier resting under the hand must not turn
        // that click into a different order and leave the ticket armed.
        if self.armed.is_some() {
            return None;
        }
        let axis = axis?;
        let (pointer, side, forced) = match self.gesture.cmd_preview_force {
            // The harness has no hand; park the pointer mid-chart, or at
            // the x the hook stated — which is the whole point of a run
            // capturing where the label rides, so it wins over a stray
            // real pointer that in such a run is nobody's aim.
            Some(force) => {
                let pointer = match force.x_fraction {
                    Some(fraction) => Point::new(
                        frame.chart.left() + frame.chart.width() * fraction,
                        frame
                            .pointer
                            .map_or_else(|| frame.chart.center().y, |pointer| pointer.y),
                    ),
                    None => frame.pointer.unwrap_or(frame.chart.center()),
                };
                (pointer, force.side, true)
            }
            None => {
                let pointer = frame.pointer?;
                let side = self.cmd_trading.aimed_side(frame.keys)?;
                (pointer, side, false)
            }
        };
        if !frame.chart.contains(pointer) {
            return None;
        }
        // The desk's own furniture outranks the aim, the same way an
        // annotation does: an ✕ or a bracket handle under the pointer, and
        // any line a press would grab. Otherwise holding the modifier
        // while reaching for a stop would rest a new order on top of it,
        // with the hand cursor promising exactly that.
        if self
            .gesture
            .control_at(account, pointer, frame.chart, axis)
            .is_some()
            || line_at(account, pointer, axis).is_some()
        {
            return None;
        }
        let mark = account.venue().mark_price()?;
        let raw_price = axis.price_at(pointer.y);
        let price = account.snap(raw_price);
        // The context menu's own validity table, plus the trader's stated
        // kind. `None` stands the aim down rather than substituting the
        // other kind — see `resolve_cmd_kind`.
        let kind = resolve_cmd_kind(self.cmd_trading.kind, side, price, mark)?;
        let quantity = self.ticket.quantity_preview().unwrap_or(Decimal::ONE);
        let ticket = self.ticket_bracket(side, price);
        Some(CmdPreview {
            side,
            kind,
            price,
            raw_price,
            pointer,
            forced,
            bracket: account.aim_bracket(
                side,
                price,
                quantity,
                ticket,
                &self.account_env(account, side, price),
            ),
            ruler_ticks: self.ruler.notches,
        })
    }

    /// Route one frame's pointer to the simulated lines.
    ///
    /// The answer says whether paper trading owns the gesture this frame —
    /// the chart must not pan and the drawings must not select under it —
    /// and carries the command a press asked for, which the host then
    /// applies to the account.
    pub fn handle_input(
        &mut self,
        account: &PaperAccount,
        frame: &ChartFrame,
        axis: Option<&dyn PriceAxis>,
    ) -> InputOutcome {
        // Three per-frame facts, in dependency order: whether the layer
        // paints at all, which tags are open (the aim yields to an open ✕,
        // so this comes before it), then the cmd preview. All three are
        // written here, and read by the paint, by the press below and by
        // `hover_cursor` — one value each, never a formula re-run against a
        // different pointer.
        self.gesture.layer_visible = frame.layer_visible;
        self.gesture.refresh_open_tags(account, frame, axis);
        self.gesture.cmd_preview = self.compute_cmd_preview(account, frame, axis);

        // The ruler. With the aim up, the wheel walks the projected stop and
        // target out from the pointer, one step per notch, the same distance
        // on both sides - so what is on screen before the click is the trade
        // at 1:1, and the trader can see whether that distance is worth
        // taking. A forced aim is a capture fixture with no hand behind it
        // and never spends the wheel.
        // Pressing the wheel puts the ruler away. Only while an aim is up,
        // so a middle-click anywhere else stays whatever it already was.
        if frame.middle_pressed && self.gesture.real_aim() && self.ruler.clear() {
            self.gesture.cmd_preview = self.compute_cmd_preview(account, frame, axis);
        }
        self.gesture.scroll_consumed = false;
        if frame.scroll_y.abs() > f32::EPSILON && self.gesture.real_aim() {
            self.ruler.step(frame.scroll_y);
            self.ruler.rolled = true;
            self.gesture.scroll_consumed = true;
            // The aim now carries its bracket; recompute so the paint and
            // the press read one value rather than two.
            self.gesture.cmd_preview = self.compute_cmd_preview(account, frame, axis);
        }

        // Nothing paper is painted while its layer is off, so nothing
        // paper takes the press: an invisible line is not a control.
        if !frame.layer_visible {
            self.gesture.drag = PaperDrag::None;
            return InputOutcome {
                owned: false,
                command: None,
            };
        }
        self.press(account, frame, axis)
    }

    /// The press half of [`Self::handle_input`], once the frame's facts are
    /// written.
    fn press(
        &mut self,
        account: &PaperAccount,
        frame: &ChartFrame,
        axis: Option<&dyn PriceAxis>,
    ) -> InputOutcome {
        let idle = self.gesture.drag == PaperDrag::None;
        // An overlay control (a tag's ✕, a bracket handle) takes the press
        // before *everything*: before the armed click — arming an order must
        // never eat the ✕ under the pointer — and before the line grab,
        // which is what made "close this order" read as "drag this order".
        // The hit is geometric, from this frame's own axis and state: a
        // pixel rect cached from the last paint goes stale the moment a
        // live chart autoscales, and the press then slips onto the line.
        if frame.primary_pressed
            && idle
            && let Some(pointer) = frame.pointer
            && let Some(axis) = axis
            && let Some(control) = self.gesture.control_at(account, pointer, frame.chart, axis)
        {
            let command = self.press_control(control, axis.price_at(pointer.y));
            return InputOutcome::claimed(command);
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
        if frame.primary_pressed
            && idle
            && let Some(preview) = self.gesture.cmd_preview
            && !preview.forced
        {
            return InputOutcome::claimed(Some(ChartCommand::PlaceResting {
                side: preview.side,
                kind: preview.kind,
                raw_price: preview.raw_price,
            }));
        }

        // An armed placement takes the next chart click.
        if let Some(armed) = self.armed
            && frame.primary_pressed
            && let Some(pointer) = frame.pointer
            && frame.chart.contains(pointer)
            && let Some(axis) = axis
        {
            return InputOutcome::claimed(Some(ChartCommand::PlaceArmed {
                armed,
                raw_price: axis.price_at(pointer.y),
            }));
        }

        // Grab a line.
        if frame.primary_pressed
            && idle
            && let Some(pointer) = frame.pointer
            && frame.chart.contains(pointer)
            && let Some(axis) = axis
            && let Some(target) = line_at(account, pointer, axis)
        {
            self.gesture.drag = target;
            self.gesture.drag_price = Some(axis.price_at(pointer.y));
            return InputOutcome::claimed(None);
        }

        // Follow the pointer while dragging. A press that started on the
        // entry line commits to one bracket leg on its first real pull:
        // towards the profit side it creates the take profit, towards the
        // losing side the stop — and a side whose leg already exists stays
        // blocked (that leg's own line is its handle).
        if frame.primary_down && !idle {
            if let (Some(pointer), Some(axis)) = (frame.pointer, axis) {
                self.follow(account, pointer, frame.chart, axis);
            }
            return InputOutcome::claimed(None);
        }

        // Drop: submit the new price; the simulator answers (a rejection
        // snaps the line back and the toast explains why). Creating a leg
        // and repricing it are the same command — the bracket is replaced
        // wholesale either way.
        if frame.primary_released && !idle {
            return InputOutcome::claimed(self.release(account));
        }

        InputOutcome {
            owned: self.gesture.drag != PaperDrag::None,
            command: None,
        }
    }

    /// Carry the line in the hand to the pointer, held inside the chart,
    /// and let a pending entry-line press decide its leg.
    fn follow(
        &mut self,
        account: &PaperAccount,
        pointer: Point,
        chart: Bounds,
        axis: &dyn PriceAxis,
    ) {
        let y = pointer.y.clamp(chart.top(), chart.bottom());
        self.gesture.drag_price = Some(axis.price_at(y));
        if self.gesture.drag == PaperDrag::CreatePending {
            self.gesture.decide_pending_leg(account, y, axis);
        }
    }

    /// What a press on an overlay control asks for. A handle asks for
    /// nothing yet: it puts its leg in the hand at `price`, and the release
    /// asks.
    fn press_control(&mut self, control: PaperControl, price: f64) -> Option<ChartCommand> {
        match control {
            PaperControl::ClosePosition => Some(ChartCommand::ClosePosition),
            PaperControl::ClearLeg { owner, leg } => Some(ChartCommand::AmendLeg {
                owner,
                leg,
                price: None,
            }),
            PaperControl::ClearRung { order, index, leg } => Some(ChartCommand::AmendRung {
                order,
                index,
                leg,
                price: None,
            }),
            PaperControl::CancelOrder(id) => Some(ChartCommand::CancelOrder(id)),
            PaperControl::Handle { owner, leg } => {
                self.gesture.drag = PaperDrag::CreateLeg { owner, leg };
                self.gesture.drag_price = Some(price);
                None
            }
        }
    }

    /// Let go of the line in the hand: what it asks for at the snapped
    /// price it was dropped on, if anything.
    fn release(&mut self, account: &PaperAccount) -> Option<ChartCommand> {
        let drag = std::mem::take(&mut self.gesture.drag);
        let price = account.snap(self.gesture.drag_price.take()?);
        match drag {
            PaperDrag::Leg { owner, leg } | PaperDrag::CreateLeg { owner, leg } => {
                Some(ChartCommand::AmendLeg {
                    owner,
                    leg,
                    price: Some(price),
                })
            }
            PaperDrag::Order(id) => Some(ChartCommand::AmendOrder { id, price }),
            PaperDrag::Rung { order, index, leg } => Some(ChartCommand::AmendRung {
                order,
                index,
                leg,
                price: Some(price),
            }),
            PaperDrag::None | PaperDrag::Blocked | PaperDrag::CreatePending => None,
        }
    }
}
