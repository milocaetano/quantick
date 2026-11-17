//! The broker venue's decisions, with no I/O: intents become request lines,
//! bridge lines become [`VenueEvent`]s and read state.
//!
//! Nothing in here reads a clock, opens a socket or spawns a thread. Lines
//! come in through [`BrokerCore::on_line`] and the link's comings and goings
//! through [`BrokerCore::on_disconnected`]; request lines leave through
//! [`BrokerCore::take_outbox`]. The same lines in give the same events out,
//! which is what lets every rule below be tested with a scripted bridge.
//!
//! # Whose state the reads are
//!
//! The broker's. A position, a working order, a protective price: each read
//! is the last snapshot the terminal pushed, plus what this side has sent and
//! not yet heard back about — an order placed but not acknowledged, a price
//! moved but not confirmed. Those are marked (see
//! [`BrokerCore::is_acknowledged`]) so a surface can draw them as provisional
//! rather than as fact.

mod apply;

use std::collections::{BTreeMap, BTreeSet};

use quantick_engine::{Side, Trade};
use quantick_trading::{
    Bracket, BracketTarget, CancelReason, CloseAmount, ClosedTrade, EntryKind, Order, OrderId,
    OrderIntent, OrderRole, Position, RejectReason, VenueEvent, VenueNotice,
};
use rust_decimal::Decimal;

use crate::account::AccountFacts;
use crate::wire::{Request, Stamp};

/// The link is down: nothing can reach the terminal.
pub const LINK_DOWN: &str = "the broker bridge is not connected - orders cannot reach the terminal; start the execution bridge, or trade on Simulado";
/// Connected, but the terminal has not reported this symbol's state yet.
pub const NO_STATE: &str =
    "the broker has not reported this symbol's account state yet - wait a moment and try again";
/// The terminal is not logged into any account.
pub const NO_ACCOUNT: &str =
    "the terminal is not logged into an account - log in at the terminal first";
/// The account does not net positions.
pub const NOT_NETTING: &str = "this account hedges positions (or reports a mode Quantick does not know) - Quantick trades netting accounts only; log into a netting account";
/// The bridge speaks another protocol version.
pub const INCOMPATIBLE: &str = "the execution bridge speaks another protocol version - update bridge/mt5 to match this build of Quantick";
/// Another program's position or order rests on the symbol.
pub const FOREIGN_EXPOSURE: &str = "another program's position or order is open on this symbol at the broker - new orders are blocked until it is gone; close it at the terminal";
/// A multi-part exit ladder.
pub const NO_LADDER: &str = "the broker holds one stop and one target per position or order - merge the exit ladder into a single bracket";
/// A cancel-at price, which no broker order type carries.
pub const NO_CANCEL_AT: &str =
    "the broker has no native cancel-at price - place the limit without one";
/// A fill-only-when-flat condition, which no broker order type carries.
pub const NO_FLAT_ONLY: &str =
    "the broker cannot hold an order that fills only while flat - place it without that condition";
/// An order with no broker ticket yet.
pub const NOT_ACKNOWLEDGED: &str =
    "the broker has not acknowledged this order yet - wait for its ticket, then change it";
/// An order whose last change is still unanswered.
pub const CHANGE_PENDING: &str =
    "the broker has not answered the last change to this order yet - wait for it";
/// A position whose last stop/target change is still unanswered.
pub const STOPS_PENDING: &str =
    "the broker has not answered the last stop/target change yet - wait for it";

/// MetaTrader's return codes that mean the request was carried out.
/// `10025` is "no changes": the terminal already held exactly that.
const ACCEPTED_RETCODES: [i64; 4] = [10008, 10009, 10010, 10025];

fn accepted(retcode: Option<i64>) -> bool {
    retcode.is_some_and(|code| ACCEPTED_RETCODES.contains(&code))
}

/// What the venue is told once, at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerConfig {
    /// The symbol whose orders and position this venue trades and reads.
    pub symbol: String,
    /// Stamped on every order; positions and orders carrying any other
    /// magic are someone else's. Never 0: that is the terminal's own hand.
    pub magic: u64,
    /// Slippage allowed on a market order, in points.
    pub deviation_points: u32,
    /// Unique per run of the application: it prefixes every request id, so
    /// a bridge that outlived a restart can never answer a new request from
    /// an old one's memory. Chosen by whoever owns a clock.
    pub session_tag: String,
}

/// Whether the execution channel is up: the venue-neutral state.
pub use quantick_trading::broker::LinkState;

/// A position on the symbol that carries another magic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignPosition {
    pub ticket: u64,
    pub side: Side,
    pub quantity: Decimal,
    pub avg_price: Decimal,
    pub magic: u64,
}

/// A working order on the symbol that carries another magic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignOrder {
    pub ticket: u64,
    pub side: Option<Side>,
    /// The terminal's word for it: `limit`, `stop`, `stop_limit`, ...
    pub kind: String,
    pub quantity: Decimal,
    pub price: Decimal,
    pub magic: u64,
}

/// Everything on the symbol this venue did not place and will never touch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForeignExposure {
    pub positions: Vec<ForeignPosition>,
    pub orders: Vec<ForeignOrder>,
}

impl ForeignExposure {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty() && self.orders.is_empty()
    }
}

/// What one unanswered request was for.
#[derive(Debug, Clone)]
enum Pending {
    Watch,
    Submit(OrderId),
    Cancel {
        id: OrderId,
        reason: CancelReason,
    },
    Modify {
        id: OrderId,
        previous: Box<Order>,
    },
    PositionStops {
        previous: (Option<Decimal>, Option<Decimal>),
    },
    Close,
}

/// A request sent and not yet answered, and the line to resend it with.
#[derive(Debug, Clone)]
struct Unanswered {
    pending: Pending,
    line: String,
}

/// The broker venue's state machine. See the module doc.
#[derive(Debug)]
pub struct BrokerCore {
    config: BrokerConfig,
    link: LinkState,
    incompatible: bool,
    /// The bridge process last seen, by its hello.
    instance: Option<String>,
    /// The account the terminal is logged into now.
    account: Option<AccountFacts>,
    /// The last account ever seen, kept through a log-out so a log-in to a
    /// different one is still noticed.
    known_account: Option<AccountFacts>,
    /// A state for this symbol arrived since the link last came up.
    has_state: bool,
    mark: Mark,
    next_order: u64,
    next_request: u64,
    /// Own resting orders, in placement order.
    working: Vec<Order>,
    /// Own market entries between the click and their fill.
    entries: Vec<Order>,
    position: Option<Position>,
    position_ticket: Option<u64>,
    foreign: ForeignExposure,
    ids: Tickets,
    changing: BTreeSet<OrderId>,
    /// Cancels asked for before the order had a ticket to cancel by.
    cancel_on_ack: BTreeMap<OrderId, CancelReason>,
    stops_pending: bool,
    requests: BTreeMap<u64, Unanswered>,
    outbox: Vec<String>,
    /// Close orders the terminal accepted, whose deal has not arrived yet.
    closing: BTreeSet<u64>,
    deals: DealCursor,
    ledger: Ledger,
    malformed_lines: usize,
}

/// The last print the venue was shown.
#[derive(Debug, Default)]
struct Mark {
    price: Option<Decimal>,
    ms: i64,
    agg_id: u64,
}

/// Own orders and the broker tickets they go by, both ways.
#[derive(Debug, Default)]
struct Tickets {
    by_id: BTreeMap<OrderId, u64>,
    by_ticket: BTreeMap<u64, OrderId>,
    /// Orders the broker has confirmed holding.
    acked: BTreeSet<OrderId>,
    /// Own orders that stopped working here (filled, cancelled, refused).
    /// A snapshot taken a moment before the broker removed one must not
    /// bring it back.
    retired: BTreeSet<OrderId>,
}

impl Tickets {
    fn bind(&mut self, id: OrderId, ticket: u64) {
        self.by_id.insert(id, ticket);
        self.by_ticket.insert(ticket, id);
        self.acked.insert(id);
    }

    fn ticket(&self, id: OrderId) -> Option<u64> {
        self.by_id.get(&id).copied()
    }

    fn order(&self, ticket: u64) -> Option<OrderId> {
        self.by_ticket.get(&ticket).copied()
    }
}

/// Which deals have been turned into events already.
#[derive(Debug, Default)]
struct DealCursor {
    seen: BTreeSet<u64>,
    /// The first state's deals are history from before this session and
    /// produce no events; after it, every unseen deal is news.
    baselined: bool,
}

/// Round trips completed this session.
#[derive(Debug, Default)]
struct Ledger {
    closed: Vec<ClosedTrade>,
    realized: Decimal,
}

impl BrokerCore {
    #[must_use]
    pub fn new(config: BrokerConfig) -> Self {
        Self {
            config,
            link: LinkState::NeverConnected,
            incompatible: false,
            instance: None,
            account: None,
            known_account: None,
            has_state: false,
            mark: Mark::default(),
            next_order: 1,
            next_request: 1,
            working: Vec::new(),
            entries: Vec::new(),
            position: None,
            position_ticket: None,
            foreign: ForeignExposure::default(),
            ids: Tickets::default(),
            changing: BTreeSet::new(),
            cancel_on_ack: BTreeMap::new(),
            stops_pending: false,
            requests: BTreeMap::new(),
            outbox: Vec::new(),
            closing: BTreeSet::new(),
            deals: DealCursor::default(),
            ledger: Ledger::default(),
            malformed_lines: 0,
        }
    }

    // --- gates ------------------------------------------------------------

    /// Why no command can reach the broker right now, if one cannot.
    #[must_use]
    pub fn write_block(&self) -> Option<&'static str> {
        if self.incompatible {
            return Some(INCOMPATIBLE);
        }
        if self.link != LinkState::Up {
            return Some(LINK_DOWN);
        }
        if !self.has_state {
            return Some(NO_STATE);
        }
        match &self.account {
            None => Some(NO_ACCOUNT),
            Some(account) if !account.margin_mode.nets() => Some(NOT_NETTING),
            Some(_) => None,
        }
    }

    /// Why a new entry would be refused right now, if it would: everything
    /// [`Self::write_block`] says, plus another program's exposure on the
    /// symbol, which a netting account would merge an entry into.
    #[must_use]
    pub fn entry_block(&self) -> Option<&'static str> {
        self.write_block()
            .or_else(|| (!self.foreign.is_empty()).then_some(FOREIGN_EXPOSURE))
    }

    fn blocked(sentence: &'static str) -> Vec<VenueEvent> {
        vec![VenueEvent::Rejected(RejectReason::Blocked(sentence))]
    }

    // --- writes -----------------------------------------------------------

    pub fn submit(&mut self, intent: OrderIntent) -> Vec<VenueEvent> {
        if let Some(sentence) = self.entry_block() {
            return Self::blocked(sentence);
        }
        if intent.quantity <= Decimal::ZERO {
            return vec![VenueEvent::Rejected(RejectReason::QuantityNotPositive)];
        }
        if intent.price.is_some_and(|price| price <= Decimal::ZERO) {
            return vec![VenueEvent::Rejected(RejectReason::PriceNotPositive)];
        }
        if intent.bracket.is_laddered() {
            return Self::blocked(NO_LADDER);
        }
        if intent.cancel_at.is_some() {
            return Self::blocked(NO_CANCEL_AT);
        }
        if intent.flat_only {
            return Self::blocked(NO_FLAT_ONLY);
        }
        let id = OrderId(self.next_order);
        self.next_order = self.next_order.saturating_add(1);
        let order = Order {
            id,
            side: intent.side,
            kind: intent.kind,
            price: intent.price,
            quantity: intent.quantity,
            bracket: intent.bracket,
            cancel_at: None,
            flat_only: false,
            placed_ms: self.mark.ms,
            role: OrderRole::Entry,
            oco: None,
            reduce_only: false,
        };
        let kind = match intent.kind {
            EntryKind::Market => "market",
            EntryKind::Limit => "limit",
            EntryKind::Stop => "stop",
        };
        self.queue(
            Pending::Submit(id),
            &Request::Submit {
                symbol: self.config.symbol.clone(),
                side: intent.side,
                kind,
                volume: intent.quantity,
                price: intent.price,
                sl: intent.bracket.stop_loss(),
                tp: intent.bracket.take_profit(),
            },
        );
        if intent.rests() {
            self.working.push(order.clone());
        } else {
            self.entries.push(order.clone());
        }
        vec![VenueEvent::Placed(order)]
    }

    pub fn amend_price(&mut self, id: OrderId, price: Decimal) -> Vec<VenueEvent> {
        if price <= Decimal::ZERO {
            return vec![VenueEvent::Rejected(RejectReason::PriceNotPositive)];
        }
        self.change_order(id, |order| order.price = Some(price))
    }

    pub fn amend_bracket(&mut self, target: BracketTarget, bracket: Bracket) -> Vec<VenueEvent> {
        if let Some(sentence) = self.write_block() {
            return Self::blocked(sentence);
        }
        if bracket.is_laddered() {
            return Self::blocked(NO_LADDER);
        }
        match target {
            BracketTarget::Order(id) => self.change_order(id, |order| order.bracket = bracket),
            BracketTarget::Position => self.change_position_stops(bracket),
        }
    }

    /// Change a working order here first, then ask the broker to agree.
    fn change_order(&mut self, id: OrderId, change: impl FnOnce(&mut Order)) -> Vec<VenueEvent> {
        if let Some(sentence) = self.write_block() {
            return Self::blocked(sentence);
        }
        let Some(index) = self.working.iter().position(|order| order.id == id) else {
            return vec![VenueEvent::Rejected(RejectReason::UnknownOrder(id))];
        };
        let Some(ticket) = self.ids.ticket(id) else {
            return Self::blocked(NOT_ACKNOWLEDGED);
        };
        if self.changing.contains(&id) {
            return Self::blocked(CHANGE_PENDING);
        }
        let previous = self.working[index].clone();
        let order = &mut self.working[index];
        change(order);
        let (Some(price), changed) = (order.price, order.clone()) else {
            return vec![VenueEvent::Rejected(RejectReason::UnknownOrder(id))];
        };
        self.queue(
            Pending::Modify {
                id,
                previous: Box::new(previous),
            },
            &Request::Modify {
                order: ticket,
                price,
                sl: changed.bracket.stop_loss(),
                tp: changed.bracket.take_profit(),
            },
        );
        self.changing.insert(id);
        vec![VenueEvent::Updated(changed)]
    }

    fn change_position_stops(&mut self, bracket: Bracket) -> Vec<VenueEvent> {
        let (Some(position), Some(ticket)) = (self.position.as_mut(), self.position_ticket) else {
            return vec![VenueEvent::Rejected(RejectReason::NoPosition)];
        };
        if self.stops_pending {
            return Self::blocked(STOPS_PENDING);
        }
        let previous = (position.stop_loss, position.take_profit);
        position.stop_loss = bracket.stop_loss();
        position.take_profit = bracket.take_profit();
        self.stops_pending = true;
        self.queue(
            Pending::PositionStops { previous },
            &Request::PositionStops {
                symbol: self.config.symbol.clone(),
                position: ticket,
                sl: bracket.stop_loss(),
                tp: bracket.take_profit(),
            },
        );
        vec![VenueEvent::BracketSet {
            stop_loss: bracket.stop_loss(),
            take_profit: bracket.take_profit(),
        }]
    }

    pub fn cancel(&mut self, id: OrderId) -> Vec<VenueEvent> {
        if let Some(sentence) = self.write_block() {
            return Self::blocked(sentence);
        }
        if !self.working.iter().any(|order| order.id == id) {
            return vec![VenueEvent::Rejected(RejectReason::UnknownOrder(id))];
        }
        self.cancel_working(id, CancelReason::User);
        Vec::new()
    }

    /// Ask the broker to remove an own order now, or as soon as it has a
    /// ticket to remove it by.
    fn cancel_working(&mut self, id: OrderId, reason: CancelReason) {
        match self.ids.ticket(id) {
            Some(ticket) => self.queue(
                Pending::Cancel { id, reason },
                &Request::Cancel { order: ticket },
            ),
            None => {
                self.cancel_on_ack.insert(id, reason);
            }
        }
    }

    pub fn close(&mut self, amount: CloseAmount) -> Vec<VenueEvent> {
        if let Some(sentence) = self.write_block() {
            return Self::blocked(sentence);
        }
        let (Some(position), Some(ticket)) = (self.position.as_ref(), self.position_ticket) else {
            return vec![VenueEvent::Rejected(RejectReason::NoPosition)];
        };
        let volume = match amount {
            CloseAmount::All => position.quantity,
            CloseAmount::Partial(quantity) if quantity <= Decimal::ZERO => {
                return vec![VenueEvent::Rejected(RejectReason::QuantityNotPositive)];
            }
            CloseAmount::Partial(quantity) => quantity.min(position.quantity),
        };
        self.queue(
            Pending::Close,
            &Request::Close {
                position: ticket,
                volume,
            },
        );
        Vec::new()
    }

    pub fn flatten(&mut self) -> Vec<VenueEvent> {
        if let Some(sentence) = self.write_block() {
            return Self::blocked(sentence);
        }
        let ids: Vec<OrderId> = self.working.iter().map(|order| order.id).collect();
        for id in ids {
            self.cancel_working(id, CancelReason::Flatten);
        }
        if self.position.is_some() {
            return self.close(CloseAmount::All);
        }
        Vec::new()
    }

    /// The chart's timeline is gone; the broker's account is not. Nothing is
    /// closed or cancelled, and the event says so.
    pub fn reset(&mut self) -> Vec<VenueEvent> {
        self.mark.price = None;
        vec![self.left_open()]
    }

    fn left_open(&self) -> VenueEvent {
        VenueEvent::Notice(VenueNotice::ResetLeftOpen {
            working: self.working.len(),
            position: self.position.is_some(),
        })
    }

    /// Trade and read another symbol. What rests on the old one stays at the
    /// broker, untouched, and the event says so.
    pub fn set_symbol(&mut self, symbol: &str) -> Vec<VenueEvent> {
        if symbol == self.config.symbol {
            return Vec::new();
        }
        let events = vec![self.left_open()];
        self.forget_symbol_view();
        self.config.symbol = symbol.to_owned();
        self.mark.price = None;
        if self.link == LinkState::Up {
            self.queue(
                Pending::Watch,
                &Request::Watch {
                    symbol: symbol.to_owned(),
                },
            );
        }
        events
    }

    /// Drop everything known about orders and the position on this symbol.
    /// Unanswered requests stay: their replies still arrive, and find
    /// nothing left to act on.
    fn forget_symbol_view(&mut self) {
        self.working.clear();
        self.entries.clear();
        self.position = None;
        self.position_ticket = None;
        self.foreign = ForeignExposure::default();
        self.ids = Tickets::default();
        self.changing.clear();
        self.cancel_on_ack.clear();
        self.stops_pending = false;
        self.closing.clear();
        self.deals = DealCursor::default();
        self.has_state = false;
    }

    fn queue(&mut self, pending: Pending, request: &Request) {
        let number = self.next_request;
        self.next_request = self.next_request.saturating_add(1);
        let id = format!("{}-{number}", self.config.session_tag);
        let line = request.encode(
            &id,
            Stamp {
                magic: self.config.magic,
                deviation_points: self.config.deviation_points,
            },
        );
        self.outbox.push(line.clone());
        self.requests.insert(number, Unanswered { pending, line });
    }

    /// Request lines waiting for the link, oldest first.
    pub fn take_outbox(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outbox)
    }

    // --- the market -------------------------------------------------------

    pub fn seed(&mut self, trade: &Trade) {
        self.mark = Mark {
            price: Some(trade.price),
            ms: trade.timestamp_ms,
            agg_id: trade.agg_id,
        };
        if let Some(position) = self.position.as_mut() {
            position.observe(trade.price);
        }
    }

    // --- reads ------------------------------------------------------------

    #[must_use]
    pub fn mark_price(&self) -> Option<Decimal> {
        self.mark.price
    }

    #[must_use]
    pub fn mark_timestamp_ms(&self) -> Option<i64> {
        self.mark.price.map(|_| self.mark.ms)
    }

    #[must_use]
    pub fn position(&self) -> Option<&Position> {
        self.position.as_ref()
    }

    #[must_use]
    pub fn working_orders(&self) -> &[Order] {
        &self.working
    }

    /// Market entries awaiting their fill, plus closes awaiting theirs.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        let closes = self
            .requests
            .values()
            .filter(|request| matches!(request.pending, Pending::Close))
            .count();
        self.entries.len() + closes + self.closing.len()
    }

    pub fn in_flight_entries(&self, out: &mut Vec<OrderId>) {
        out.extend(self.entries.iter().map(|order| order.id));
    }

    #[must_use]
    pub fn closed_trades(&self) -> &[ClosedTrade] {
        &self.ledger.closed
    }

    #[must_use]
    pub fn realized_points(&self) -> Decimal {
        self.ledger.realized
    }

    /// The broker's ticket for an own order, once it has one.
    #[must_use]
    pub fn ticket(&self, id: OrderId) -> Option<u64> {
        self.ids.ticket(id)
    }

    /// Whether the broker holds this order exactly as shown: it has a
    /// ticket and no change to it is still unanswered. A surface draws
    /// anything else as provisional.
    #[must_use]
    pub fn is_acknowledged(&self, id: OrderId) -> bool {
        self.ids.acked.contains(&id) && !self.changing.contains(&id)
    }

    /// Whether the position's protective prices are the broker's, rather
    /// than a change still on its way there.
    #[must_use]
    pub fn position_stops_acknowledged(&self) -> bool {
        !self.stops_pending
    }

    /// The broker's ticket for the open position.
    #[must_use]
    pub fn position_ticket(&self) -> Option<u64> {
        self.position_ticket
    }

    #[must_use]
    pub fn account(&self) -> Option<&AccountFacts> {
        self.account.as_ref()
    }

    #[must_use]
    pub fn link_state(&self) -> LinkState {
        self.link
    }

    /// Whether the reads are the last known state rather than the current
    /// one: the link is down, or it came up and no state has arrived since.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.link != LinkState::Up || !self.has_state
    }

    #[must_use]
    pub fn foreign_exposure(&self) -> &ForeignExposure {
        &self.foreign
    }

    #[must_use]
    pub fn symbol(&self) -> &str {
        &self.config.symbol
    }

    #[must_use]
    pub fn config(&self) -> &BrokerConfig {
        &self.config
    }

    /// Lines from the bridge this side could not read, this session.
    #[must_use]
    pub fn malformed_lines(&self) -> usize {
        self.malformed_lines
    }
}
