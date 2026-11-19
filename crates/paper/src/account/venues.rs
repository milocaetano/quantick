//! The two venues an account can route to, and which one orders go to.
//!
//! The simulator is always there. A broker venue is attached when a broker
//! route is armed, and stays attached — *parked* — after the route goes back
//! to the simulator while it still holds a position or orders, so their
//! plates never leave the chart. Everything that reads "the venue" reads the
//! one orders go to, through `Deref`, so the money path did not learn a
//! second word for it.

use std::ops::{Deref, DerefMut};
use std::path::PathBuf;

use quantick_engine::Trade;
use quantick_sim::history;
use quantick_trading::{
    Bracket, BracketTarget, BrokerVenue, CloseAmount, ClosedTrade, Order, OrderId, OrderIntent,
    Position, RejectReason, TradeMode, TradingVenue, VenueEvent, VenueNotice,
};
use rust_decimal::Decimal;

use super::PaperAccount;
use crate::format::sanitize_symbol;
use crate::route::Route;

/// The folder broker round trips are journaled under, beside the paper one:
/// a practice fill and a broker fill never share a file.
pub use crate::report::journals::BROKER_TRADES_FOLDER;

/// The simulator, and a broker venue when one is attached.
pub(crate) struct Venues {
    sim: Box<dyn TradingVenue>,
    broker: Option<Guarded>,
}

/// An attached broker venue and whether orders are routed to it.
///
/// Parked (not routed) it still answers every read and every write that only
/// takes risk off - a close, a cancel, a protective price - but refuses a new
/// entry: the trader chose the simulator, and a press must never open a
/// broker position they no longer see as the route.
pub(crate) struct Guarded {
    venue: Box<dyn BrokerVenue>,
    routed: bool,
    /// What the route must hear about the link and the account, from
    /// whichever call drained it - a print drains the venue too - until
    /// [`PaperAccount::poll_broker`] hands it over.
    for_route: Vec<VenueNotice>,
}

/// Notices kept for the route at most; the route reads them every frame.
const ROUTE_NOTICE_CAP: usize = 16;

impl Guarded {
    /// Keep the notices the route acts on, and pass `events` through.
    fn keep(&mut self, events: Vec<VenueEvent>) -> Vec<VenueEvent> {
        for event in &events {
            if let VenueEvent::Notice(
                notice @ (VenueNotice::LinkLost
                | VenueNotice::AccountChanged
                | VenueNotice::AccountRefused(_)),
            ) = event
            {
                if self.for_route.len() == ROUTE_NOTICE_CAP {
                    self.for_route.remove(0);
                }
                self.for_route.push(notice.clone());
            }
        }
        events
    }
}

/// Why an entry is refused while the broker venue is parked with exposure.
pub const PARKED_ENTRY_BLOCK: &str = "orders are still open at the broker - close them, or arm the account again, before trading here";

impl Venues {
    pub(crate) fn new(sim: Box<dyn TradingVenue>) -> Self {
        Self { sim, broker: None }
    }

    /// The venue orders go to, read-only.
    pub(crate) fn active(&self) -> &dyn TradingVenue {
        &**self
    }

    /// The venue orders go to.
    pub(crate) fn active_mut(&mut self) -> &mut dyn TradingVenue {
        &mut **self
    }

    /// Whether the broker venue is the one shown and written to: routed, or
    /// parked while it still holds something.
    fn broker_shown(&self) -> bool {
        self.broker
            .as_ref()
            .is_some_and(|broker| broker.routed || !broker.venue.is_idle())
    }
}

impl Deref for Venues {
    type Target = dyn TradingVenue;

    fn deref(&self) -> &Self::Target {
        match &self.broker {
            Some(broker) if self.broker_shown() => broker,
            _ => self.sim.as_ref(),
        }
    }
}

impl DerefMut for Venues {
    fn deref_mut(&mut self) -> &mut Self::Target {
        if self.broker_shown()
            && let Some(broker) = self.broker.as_mut()
        {
            return broker;
        }
        self.sim.as_mut()
    }
}

impl TradingVenue for Guarded {
    fn submit(&mut self, intent: OrderIntent) -> Vec<VenueEvent> {
        if !self.routed {
            return vec![VenueEvent::Rejected(RejectReason::Blocked(
                PARKED_ENTRY_BLOCK,
            ))];
        }
        let events = self.venue.submit(intent);
        self.keep(events)
    }

    fn amend_price(&mut self, id: OrderId, price: Decimal) -> Vec<VenueEvent> {
        let events = self.venue.amend_price(id, price);
        self.keep(events)
    }

    fn amend_bracket(&mut self, target: BracketTarget, bracket: Bracket) -> Vec<VenueEvent> {
        let events = self.venue.amend_bracket(target, bracket);
        self.keep(events)
    }

    fn cancel(&mut self, id: OrderId) -> Vec<VenueEvent> {
        let events = self.venue.cancel(id);
        self.keep(events)
    }

    fn close(&mut self, amount: CloseAmount) -> Vec<VenueEvent> {
        let events = self.venue.close(amount);
        self.keep(events)
    }

    fn flatten(&mut self) -> Vec<VenueEvent> {
        let events = self.venue.flatten();
        self.keep(events)
    }

    fn on_trade(&mut self, trade: &Trade) -> Vec<VenueEvent> {
        let events = self.venue.on_trade(trade);
        self.keep(events)
    }

    fn seed(&mut self, trade: &Trade) {
        self.venue.seed(trade);
    }

    fn reset(&mut self) -> Vec<VenueEvent> {
        let events = self.venue.reset();
        self.keep(events)
    }

    fn mark_price(&self) -> Option<Decimal> {
        self.venue.mark_price()
    }

    fn mark_timestamp_ms(&self) -> Option<i64> {
        self.venue.mark_timestamp_ms()
    }

    fn position(&self) -> Option<&Position> {
        self.venue.position()
    }

    fn working_orders(&self) -> &[Order] {
        self.venue.working_orders()
    }

    fn in_flight(&self) -> usize {
        self.venue.in_flight()
    }

    fn in_flight_entries(&self, out: &mut Vec<OrderId>) {
        self.venue.in_flight_entries(out);
    }

    fn in_flight_entry(&self, id: OrderId) -> Option<&Order> {
        self.venue.in_flight_entry(id)
    }

    fn closed_trades(&self) -> &[ClosedTrade] {
        self.venue.closed_trades()
    }

    fn realized_points(&self) -> Decimal {
        self.venue.realized_points()
    }
}

/// Where broker round trips are written: one file per run, under the
/// symbol's folder, named from the first close's venue time.
#[derive(Debug, Default)]
pub(crate) struct BrokerJournal {
    path: Option<PathBuf>,
    warned: bool,
}

impl PaperAccount {
    /// Route orders to `venue`, the broker the trader armed. It trades the
    /// account's symbol from now on; nothing it already holds is touched.
    pub fn attach_broker(&mut self, mut venue: Box<dyn BrokerVenue>) {
        let events = if self.symbol.is_empty() {
            Vec::new()
        } else {
            venue.set_symbol(&self.symbol)
        };
        self.venue.broker = Some(Guarded {
            venue,
            routed: true,
            for_route: Vec::new(),
        });
        self.handle_events(events);
    }

    /// Route orders back to the simulator, keeping the broker venue in view
    /// while it holds anything.
    pub fn park_broker(&mut self) {
        if let Some(broker) = self.venue.broker.as_mut() {
            broker.routed = false;
        }
    }

    /// Route orders to the attached broker venue again.
    pub fn resume_broker(&mut self) -> bool {
        let Some(broker) = self.venue.broker.as_mut() else {
            return false;
        };
        broker.routed = true;
        true
    }

    /// Hand the broker venue back, routing orders to the simulator.
    pub fn detach_broker(&mut self) -> Option<Box<dyn BrokerVenue>> {
        self.venue.broker.take().map(|broker| broker.venue)
    }

    /// The broker venue attached to this account, routed to or parked.
    #[must_use]
    pub fn broker(&self) -> Option<&dyn BrokerVenue> {
        self.venue
            .broker
            .as_ref()
            .map(|broker| broker.venue.as_ref())
    }

    /// The attached broker venue, to act on.
    pub(super) fn venue_broker_mut(&mut self) -> Option<&mut (dyn BrokerVenue + 'static)> {
        self.venue
            .broker
            .as_mut()
            .map(|broker| broker.venue.as_mut())
    }

    /// The number a plate wears for `owner`: the broker's ticket while the
    /// broker venue is shown and has one, else `None` (the plate keeps the
    /// account's own `#n`).
    #[must_use]
    pub fn venue_ref(&self, owner: quantick_trading::BracketTarget) -> Option<u64> {
        let broker = self.broker().filter(|_| self.broker_shown())?;
        match owner {
            quantick_trading::BracketTarget::Order(id) => broker.order_ref(id),
            quantick_trading::BracketTarget::Position => broker.position_ref(),
        }
    }

    /// The id a working order's plate shows: the broker's ticket once it
    /// has one, else the account's own number.
    #[must_use]
    pub fn plate_id(&self, id: OrderId) -> u64 {
        self.venue_ref(quantick_trading::BracketTarget::Order(id))
            .unwrap_or(id.0)
    }

    /// Whether a working order's plate is drawn provisional: sent to the
    /// broker, not yet confirmed as it stands.
    #[must_use]
    pub fn awaiting_ack(&self, id: OrderId) -> bool {
        self.broker_shown()
            && self
                .broker()
                .is_some_and(|broker| !broker.is_acknowledged(id))
    }

    /// Whether orders go to the broker venue.
    #[must_use]
    pub fn broker_routed(&self) -> bool {
        self.venue
            .broker
            .as_ref()
            .is_some_and(|broker| broker.routed)
    }

    /// Whether the broker venue is the one on the chart: routed, or parked
    /// with a position or orders still open there.
    #[must_use]
    pub fn broker_shown(&self) -> bool {
        self.venue.broker_shown()
    }

    /// A parked broker venue that still holds a position or orders: its
    /// plates stay on the chart, read-only.
    #[must_use]
    pub fn parked_broker(&self) -> Option<&dyn BrokerVenue> {
        if self.broker_routed() {
            return None;
        }
        self.broker().filter(|broker| !broker.is_idle())
    }

    /// Whose orders the account shows and labels now: the simulator, or the
    /// broker venue (routed, or parked with exposure) in its own word for
    /// the account.
    #[must_use]
    pub fn route_kind(&self) -> Route {
        if !self.broker_shown() {
            return Route::Sim;
        }
        self.broker()
            .and_then(BrokerVenue::account)
            .map_or(Route::Real, |facts| Route::of_mode(facts.trade_mode))
    }

    /// Why an entry is refused now on the broker shown, if one is: parked
    /// exposure the trader left there, or the routed venue's own block. The
    /// toolbar and the ticket read this one answer.
    #[must_use]
    pub fn entry_block(&self) -> Option<&'static str> {
        let broker = self.broker()?;
        if !self.broker_routed() {
            return self.broker_shown().then_some(PARKED_ENTRY_BLOCK);
        }
        broker.entry_block()
    }

    /// Whether an entry button may be pressed: a price to trade against and
    /// no broker block.
    #[must_use]
    pub fn entry_ready(&self) -> bool {
        self.ready() && self.entry_block().is_none()
    }

    /// The chip every position surface leads with, in the route's word:
    /// `SIM LONG 2`, `REAL SHORT 1`; `None` while flat.
    #[must_use]
    pub fn position_chip(&self) -> Option<String> {
        let position = self.venue().position()?;
        let side = crate::format::position_word(position.side);
        let quantity = crate::format::fmt_decimal(position.quantity);
        Some(format!("{} {side} {quantity}", self.route_kind().word()))
    }

    /// The tone the chart's broker marks wear: the route's colour while a
    /// broker venue is shown, lost while its link is down.
    #[must_use]
    pub fn route_tone(&self) -> crate::route::RouteTone {
        use crate::route::RouteTone;
        match (
            self.route_kind(),
            self.broker().map(BrokerVenue::link_state),
        ) {
            (Route::Sim, _) => RouteTone::Sim,
            (_, Some(quantick_trading::LinkState::Lost)) => RouteTone::Lost,
            (Route::Demo, _) => RouteTone::Demo,
            (Route::Real, _) => RouteTone::Real,
        }
    }

    /// Drain what the attached broker said, routed or parked, hand it the
    /// time `now_ms` (a silent link is declared lost there), and return its
    /// notices for the route to read.
    pub fn poll_broker(&mut self, now_ms: i64) -> Vec<VenueNotice> {
        let route = self.route_kind();
        let Some(guarded) = self.venue.broker.as_mut() else {
            return Vec::new();
        };
        // What another drain took off first, in the order it was said.
        let mut notices = std::mem::take(&mut guarded.for_route);
        let broker = guarded.venue.as_mut();
        let mut events = broker.poll();
        events.extend(broker.tick(now_ms));
        notices.extend(notices_of(&events));
        if route.is_broker() {
            self.handle_events_from(events, route);
        } else {
            // Parked: its closes still belong in the broker's journal, its
            // refusals and notices in the log, and nothing it says may reach
            // the simulator's bot buffer.
            for event in events {
                super::broker_log::log_broker_event(&event, "parked", &self.symbol);
                if let VenueEvent::Closed {
                    trade,
                    broker_account: Some(mode),
                } = event
                {
                    self.journal_broker(&trade, mode);
                    self.outbox.journal_changed = true;
                }
            }
        }
        notices
    }

    /// The folder broker round trips go to: `real-trades` beside the paper
    /// trades folder.
    #[must_use]
    pub fn broker_trades_dir(&self) -> PathBuf {
        crate::report::journals::broker_dir(&self.dir)
    }

    pub fn journal_paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.session_journal_paths()
            .iter()
            .chain(self.broker_journal.path.iter())
    }

    /// The in-memory ledger tail is always the simulator's. Broker rows are read
    /// from their journal so switching routes cannot relabel or duplicate them.
    pub fn simulator_trades(&self) -> &[ClosedTrade] {
        self.venue.sim.closed_trades()
    }

    /// Ledger tail indices name simulator trades, never the broker's separate tape marks.
    pub fn ledger_chart_selection(&self, index: Option<usize>) -> Option<usize> {
        index.filter(|index| !self.broker_shown() && *index < self.simulator_trades().len())
    }

    pub fn execution_source(&self) -> history::SessionSource {
        if self.broker_shown() {
            history::SessionSource::Broker(
                self.broker()
                    .and_then(BrokerVenue::account)
                    .map_or(TradeMode::Unknown, |facts| facts.trade_mode),
            )
        } else {
            self.session_source()
        }
    }

    /// Append one broker round trip to its own journal. Returns whether the
    /// write landed.
    pub(super) fn journal_broker(&mut self, trade: &ClosedTrade, mode: TradeMode) -> bool {
        if self.symbol.is_empty() {
            return false;
        }
        let folder = self.broker_trades_dir().join(sanitize_symbol(&self.symbol));
        if self
            .broker_journal
            .path
            .as_ref()
            .is_some_and(|path| path.parent() != Some(folder.as_path()))
        {
            self.broker_journal = BrokerJournal::default();
        }
        let path = self.broker_journal.path.get_or_insert_with(|| {
            folder.join(format!(
                "{}.{}",
                super::utc_compact(trade.closed_ms),
                history::FILE_EXTENSION
            ))
        });
        let mut text = String::new();
        if !path.exists() {
            text.push_str(&history::write_header(
                &self.symbol,
                history::SessionSource::Broker(TradeMode::Unknown),
            ));
        }
        text.push_str(history::write_account(mode));
        text.push_str(&history::write_trade(trade));
        let written = std::fs::create_dir_all(&folder).and_then(|()| {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&*path)
                .and_then(|mut file| file.write_all(text.as_bytes()))
        });
        if let Err(error) = &written
            && !self.broker_journal.warned
        {
            self.broker_journal.warned = true;
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "BROKER_TRADE_JOURNAL_FAILED",
                path = %path.display(),
                %error,
                action = "trade_not_saved",
                "could not append to the broker trade history"
            );
        }
        written.is_ok()
    }
}

fn notices_of(events: &[VenueEvent]) -> Vec<VenueNotice> {
    events
        .iter()
        .filter_map(|event| match event {
            VenueEvent::Notice(notice) => Some(notice.clone()),
            _ => None,
        })
        .collect()
}
