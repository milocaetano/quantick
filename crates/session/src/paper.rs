//! The paper desk and account a tab trades on, and the decisions that join
//! them.
//!
//! `quantick_paper` owns both halves: the [`Desk`] decides what the ticket
//! says and what a chart gesture aims, the [`PaperAccount`] places, fills,
//! sizes and journals. What sits between them is per tab and was the
//! window's: a market press resolves the ticket's offsets before the account
//! sizes the entry, a reversal takes the side from the account and the
//! protection from the ticket, a symbol switch retargets the journal and
//! moves the ruler with it, a rebuilt timeline flattens the account and
//! empties the hand. [`TabPaper`] holds the pair and makes those moves, so a
//! test drives a tab's trading without a window.
//!
//! The account is held as `A`, anything that lends a [`PaperAccount`]: the
//! account itself here and in the tests, the window's host — which keeps the
//! report and the export threads beside it — in the app. Every decision below
//! reaches only the borrowed account, so the host's extras never change what
//! a press does. Told nothing, reading nothing: no environment, file, thread
//! or clock.

use std::borrow::{Borrow, BorrowMut};
use std::collections::BTreeMap;

use quantick_engine::{Side, Trade};
use quantick_paper::PaperAccount;
use quantick_paper::account::{AccountEnv, TimelineReset};
use quantick_paper::desk::{CmdTradingSettings, Desk, TradeBar};
use quantick_paper::format::PositionSummary;
use quantick_paper::risk_sizing::RiskState;
use quantick_sim::{Bracket, ClosedTrade, Command, EntryKind, Order, VenueEvent};
use rust_decimal::Decimal;

#[path = "paper_command.rs"]
mod command;
#[path = "paper_settings.rs"]
mod settings;

pub use command::{NOTHING_TO_REVERSE, TradeCommand, TradeOutcome};
pub use settings::{SharedChange, SharedSettings};

/// What a rebuilt timeline says when it flattened a position.
pub(crate) const RESET_POSITION_TOAST: &str =
    "SIM position flattened - the timeline was rebuilt under it.";
/// What a rebuilt timeline says when it swept resting orders.
pub(crate) const RESET_ORDERS_TOAST: &str =
    "SIM orders cancelled - the timeline was rebuilt under them.";

/// What a rebuilt timeline says: the flatten before the sweep, and nothing
/// while a could-not-save warning waits in the outbox.
pub(crate) fn reset_toast(reset: TimelineReset) -> Option<&'static str> {
    if !reset.all_saved {
        None
    } else if reset.had_position {
        Some(RESET_POSITION_TOAST)
    } else if reset.had_orders {
        Some(RESET_ORDERS_TOAST)
    } else {
        None
    }
}

/// One tab's paper trading: the desk and the account it trades on.
/// See the module docs.
#[derive(Debug)]
pub struct TabPaper<A = PaperAccount> {
    /// The deciding half of the ticket and the chart gesture: what was
    /// typed, how far the ruler stands, what is aimed, armed and grabbed.
    pub desk: Desk,
    /// The money path, as its host holds it.
    pub account: A,
}

impl<A: BorrowMut<PaperAccount>> TabPaper<A> {
    /// A tab trading on `account`, with a fresh desk.
    #[must_use]
    pub fn new(account: A) -> Self {
        Self {
            desk: Desk::default(),
            account,
        }
    }

    /// The account, as its host holds it.
    pub fn account(&self) -> &A {
        &self.account
    }

    /// The account, as its host holds it, mutably.
    pub fn account_mut(&mut self) -> &mut A {
        &mut self.account
    }

    fn core(&self) -> &PaperAccount {
        self.account.borrow()
    }

    fn core_mut(&mut self) -> &mut PaperAccount {
        self.account.borrow_mut()
    }

    /// Everything the account needs from the ticket for one call; see
    /// [`Desk::account_env`].
    #[must_use]
    pub fn account_env(&self, side: Side, price: Decimal) -> AccountEnv {
        self.desk.account_env(self.core(), side, price)
    }

    /// The bracket the ticket's offsets describe, or `None` with the
    /// complaint about the text that does not parse posted as the toast.
    pub fn parse_bracket(&mut self, side: Side, reference: Decimal) -> Option<Bracket> {
        match self.desk.ticket.parse_bracket(side, reference) {
            Ok(bracket) => Some(bracket),
            Err(message) => {
                self.show_toast(message);
                None
            }
        }
    }

    /// Post an acknowledgement for the window to show. Newest wins: the
    /// account holds one slot, and the host drains it into the one surface
    /// that owns the clock and the position.
    pub fn show_toast(&mut self, message: String) {
        self.core_mut().set_toast(message);
    }

    /// Take the acknowledgement waiting to be shown, once.
    pub fn take_toast(&mut self) -> Option<String> {
        self.core_mut().take_toast()
    }

    /// Rest a limit/stop entry at `raw_price` with the ticket's quantity
    /// and offsets; answers whether the simulator accepted it, and toasts
    /// the ticket's complaint when an offset does not parse.
    pub fn place_resting(&mut self, side: Side, kind: EntryKind, raw_price: f64) -> bool {
        let Self { desk, account } = self;
        match desk.place_resting(account.borrow_mut(), side, kind, raw_price) {
            Ok(placed) => placed,
            Err(message) => {
                self.show_toast(message);
                false
            }
        }
    }

    /// Walk the typed quantity by `notches` of the instrument's own size
    /// step, never below its minimum; see `Ticket::step_quantity`.
    pub fn step_quantity(&mut self, notches: Decimal) {
        let (unit, floor) = self
            .core()
            .symbol_money()
            .map_or((Decimal::ONE, Decimal::ONE), |money| {
                (money.size_step, money.min_size)
            });
        self.desk.ticket.step_quantity(notches, unit, floor);
    }

    /// What pressing this side's entry would do to the open position —
    /// `SELL 1 (closes)`, `SELL 5 (reverses to short 4)`.
    #[must_use]
    pub fn entry_label(&self, side: Side) -> String {
        self.desk.entry_label(self.core(), side)
    }

    /// The quantity and protective offsets an entry press will use.
    #[must_use]
    pub fn entry_hover(&self, side: Side) -> String {
        self.desk.ticket.entry_hover(side)
    }

    /// The toolbar TRADE group's words, from one risk read.
    #[must_use]
    pub fn trade_bar(&self, reverse_key: &str) -> TradeBar {
        self.desk.trade_bar(self.core(), reverse_key)
    }

    /// What the open position's buttons may do now: one answer for the
    /// toolbar, the Trading panel and the chart HUD.
    #[must_use]
    pub fn position_actions(&self) -> quantick_paper::desk::PositionActions {
        self.desk.position_actions(self.core())
    }

    /// Whether a chart gesture is in the hand — a line being dragged, a leg
    /// being pulled — so the host keeps it with the pane that started it.
    #[must_use]
    pub fn gesture_active(&self) -> bool {
        self.desk.gesture.active()
    }

    /// Whether the ruler spent this frame's wheel travel: one wheel, one
    /// meaning at a time.
    #[must_use]
    pub fn consumed_scroll(&self) -> bool {
        self.desk.gesture.scroll_consumed
    }

    /// Whether an aim is on screen this frame.
    #[must_use]
    pub fn aiming(&self) -> bool {
        self.desk.gesture.cmd_preview.is_some()
    }

    /// Feed one live print through the simulator and act on what it did.
    pub fn on_trade(&mut self, trade: &Trade) {
        self.core_mut().on_trade(trade);
    }

    /// Seed the mark from a print without trading on it.
    pub fn seed(&mut self, trade: &Trade) {
        self.core_mut().seed(trade);
    }

    /// A market order using the ticket's quantity and offsets;
    /// [`TradeCommand::Market`].
    pub fn market(&mut self, side: Side) {
        self.send(TradeCommand::Market { side });
    }

    /// Flip the open position: one market order for twice its size;
    /// [`TradeCommand::Reverse`].
    pub fn reverse_position(&mut self) {
        self.send(TradeCommand::Reverse);
    }

    /// Close everything, now; [`TradeCommand::Flatten`]. Named on the tab as
    /// well as on the account because `Iterator::flatten` otherwise wins
    /// method resolution on a bare `.flatten()`.
    pub fn flatten(&mut self) {
        self.send(TradeCommand::Flatten);
    }

    /// Close the open position at the market; [`TradeCommand::Close`].
    pub fn close_position(&mut self) {
        self.send(TradeCommand::Close);
    }

    /// The source rebuilt its timeline (replay seek, feed/symbol switch,
    /// restart): the account sweeps, flattens at the last mark and journals
    /// it, the hand empties, and the toast says so — never silently.
    pub fn on_timeline_reset(&mut self) {
        let reset = self.core_mut().reset_timeline();
        self.desk.reset_timeline();
        if let Some(toast) = reset_toast(reset) {
            self.core_mut().set_toast(toast.to_owned());
        }
    }

    /// Follow the tab's symbol: the journal retargets and the ruler moves
    /// with the instrument. Answers whether anything changed, for a host
    /// with state of its own keyed by the symbol.
    pub fn set_symbol(&mut self, symbol: &str) -> bool {
        let Some(arriving) = self.core_mut().set_symbol(symbol) else {
            return false;
        };
        self.desk.ruler.follow_symbol(symbol, arriving);
        true
    }

    /// Put the ruler at `notches`, clamped to what the wheel can reach, and
    /// answer with where it landed — the named form of rolling the wheel.
    pub fn set_ruler_ticks(&mut self, notches: u32) -> u32 {
        self.desk.ruler.set_ticks(notches)
    }

    /// How far the ruler stands from the aim, in ticks; zero when it is off.
    #[must_use]
    pub fn ruler_ticks(&self) -> u32 {
        self.desk.ruler.notches
    }

    /// Clear the ruler, so the next aim starts from the entry again.
    pub fn clear_ruler(&mut self) -> bool {
        self.desk.ruler.clear()
    }

    /// Every step the trader has named, by symbol, for the host to persist.
    #[must_use]
    pub fn ruler_steps(&self) -> &BTreeMap<String, Decimal> {
        &self.desk.ruler.steps
    }

    /// Replace the remembered steps wholesale, as the host restores them.
    pub fn set_ruler_steps(&mut self, steps: BTreeMap<String, Decimal>) {
        let Self { desk, account } = self;
        desk.ruler
            .set_steps(steps, Borrow::<PaperAccount>::borrow(account).symbol());
    }

    /// Name this instrument's step, in points; `None` puts it back on the
    /// derived default.
    pub fn set_ruler_step(&mut self, step: Option<Decimal>) {
        let Self { desk, account } = self;
        desk.ruler
            .set_step(Borrow::<PaperAccount>::borrow(account).symbol(), step);
    }

    /// Install cmd-trading settings — the host's fan-out on boot and on a
    /// change made in any tab.
    pub fn set_cmd_trading(&mut self, settings: CmdTradingSettings) {
        self.desk.set_cmd_trading(settings);
    }

    /// The cmd-trading settings, for the host to persist.
    #[must_use]
    pub fn cmd_trading(&self) -> CmdTradingSettings {
        self.desk.cmd_trading
    }

    /// Drop the frame's aim, so a stale line never keeps painting.
    pub fn clear_cmd_preview(&mut self) {
        self.desk.gesture.cmd_preview = None;
    }

    /// Whether a chart click flipped the P&L switch since the last ask.
    pub fn take_pnl_flip(&mut self) -> bool {
        self.desk.take_pnl_flip()
    }

    /// What the risk per trade says about the entry the aim is holding.
    #[must_use]
    pub fn risk_state(&self, side: Side, reference: Decimal) -> RiskState {
        self.core()
            .risk_state(side, reference, &self.account_env(side, reference))
    }

    /// The risk read the control plane and the ticket's own line share,
    /// and whether it blocks an entry.
    #[must_use]
    pub fn risk_report(&self) -> (RiskState, bool) {
        self.desk.risk_report(self.core())
    }

    /// The bracket an armed entry would carry, at this size.
    #[must_use]
    pub fn armed_bracket(&self, side: Side, reference: Decimal, quantity: Decimal) -> Bracket {
        self.core().armed_bracket(
            side,
            reference,
            quantity,
            &self.account_env(side, reference),
        )
    }

    /// The status cell's text and its sign, or `None` while flat.
    #[must_use]
    pub fn status_cell(&self) -> Option<(String, std::cmp::Ordering)> {
        self.core().status_cell()
    }

    /// The open position, as the desk's surfaces summarise it.
    #[must_use]
    pub fn position_summary(&self) -> Option<PositionSummary> {
        self.core().position_summary()
    }

    /// Whether the account holds no position.
    #[must_use]
    pub fn is_flat(&self) -> bool {
        self.core().is_flat()
    }

    /// The last mark price, if the account has seen a print.
    #[must_use]
    pub fn mark_price(&self) -> Option<Decimal> {
        self.core().mark_price()
    }

    /// This session's closed round trips, oldest first.
    #[must_use]
    pub fn session_trades(&self) -> &[ClosedTrade] {
        self.core().session_trades()
    }

    /// The resting entry orders, in placement order.
    #[must_use]
    pub fn working_orders(&self) -> &[Order] {
        self.core().working_orders()
    }
}

/// The tab's account as the strategy runner reaches it: commands go in
/// through the funnel every strategy command takes, and the flat gate is the
/// account's own.
impl<A: BorrowMut<PaperAccount>> quantick_strategy::runner::StrategyPort for TabPaper<A> {
    fn apply(&mut self, command: Command) -> Vec<VenueEvent> {
        self.core_mut().apply_strategy_command(command)
    }

    fn is_flat(&self) -> bool {
        self.core().is_flat()
    }
}
