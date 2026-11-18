//! New entries pause while the host says the chart is out of sync with its
//! feed, on the simulator and on a real broker route alike; exits never do.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use quantick_engine::{Side, Trade};
use quantick_paper::PaperAccount;
use quantick_paper::desk::ChartCommand;
use quantick_sim::{
    Bracket, BracketTarget, CloseAmount, ClosedTrade, EntryKind, Order, OrderId, OrderIntent,
    Position, Simulator, TradingVenue, VenueEvent,
};
use quantick_sources::sync::{Desync, LinkState, SyncLimits, SyncReading};
use quantick_trading::{AccountFacts, BrokerVenue, LinkState as BrokerLink, MarginMode, TradeMode};
use rust_decimal::Decimal;

use crate::paper::{TabPaper, TradeCommand};

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn late() -> Desync {
    Desync {
        reading: SyncReading::Latency,
        link: LinkState::Up,
        latency_ms: Some(900),
        latency_hop: Some("MT5"),
        tape_age_ms: Some(40),
        limits: SyncLimits::default(),
        notice: None,
    }
}

/// What the broker was asked to do, in order.
type Calls = Rc<RefCell<Vec<&'static str>>>;

/// A real-money broker account over the simulator, recording every write.
struct RealBroker {
    sim: Simulator,
    facts: AccountFacts,
    calls: Calls,
}

impl RealBroker {
    fn note(&self, call: &'static str) {
        self.calls.borrow_mut().push(call);
    }
}

impl TradingVenue for RealBroker {
    fn submit(&mut self, intent: OrderIntent) -> Vec<VenueEvent> {
        self.note("submit");
        self.sim.submit(intent)
    }
    fn amend_price(&mut self, id: OrderId, price: Decimal) -> Vec<VenueEvent> {
        self.note("amend_price");
        self.sim.amend_price(id, price)
    }
    fn amend_bracket(&mut self, target: BracketTarget, bracket: Bracket) -> Vec<VenueEvent> {
        self.note("amend_bracket");
        self.sim.amend_bracket(target, bracket)
    }
    fn cancel(&mut self, id: OrderId) -> Vec<VenueEvent> {
        self.note("cancel");
        self.sim.cancel(id)
    }
    fn close(&mut self, amount: CloseAmount) -> Vec<VenueEvent> {
        self.note("close");
        self.sim.close(amount)
    }
    fn flatten(&mut self) -> Vec<VenueEvent> {
        self.note("flatten");
        self.sim.flatten()
    }
    fn on_trade(&mut self, trade: &Trade) -> Vec<VenueEvent> {
        self.sim.on_trade(trade)
    }
    fn seed(&mut self, trade: &Trade) {
        self.sim.seed(trade);
    }
    fn reset(&mut self) -> Vec<VenueEvent> {
        Vec::new()
    }
    fn mark_price(&self) -> Option<Decimal> {
        self.sim.mark_price()
    }
    fn mark_timestamp_ms(&self) -> Option<i64> {
        self.sim.mark_timestamp_ms()
    }
    fn position(&self) -> Option<&Position> {
        self.sim.position()
    }
    fn working_orders(&self) -> &[Order] {
        self.sim.working_orders()
    }
    fn in_flight(&self) -> usize {
        self.sim.in_flight()
    }
    fn in_flight_entries(&self, out: &mut Vec<OrderId>) {
        self.sim.in_flight_entries(out);
    }
    fn closed_trades(&self) -> &[ClosedTrade] {
        self.sim.closed_trades()
    }
    fn realized_points(&self) -> Decimal {
        self.sim.realized_points()
    }
}

impl BrokerVenue for RealBroker {
    fn poll(&mut self) -> Vec<VenueEvent> {
        Vec::new()
    }
    fn set_symbol(&mut self, _symbol: &str) -> Vec<VenueEvent> {
        Vec::new()
    }
    fn account(&self) -> Option<&AccountFacts> {
        Some(&self.facts)
    }
    fn link_state(&self) -> BrokerLink {
        BrokerLink::Up
    }
    fn entry_block(&self) -> Option<&'static str> {
        None
    }
    fn foreign_counts(&self) -> (usize, usize) {
        (0, 0)
    }
    fn order_ref(&self, _id: OrderId) -> Option<u64> {
        None
    }
    fn position_ref(&self) -> Option<u64> {
        None
    }
    fn is_acknowledged(&self, _id: OrderId) -> bool {
        true
    }
}

fn sim() -> TabPaper {
    TabPaper::new(PaperAccount::with_trades_dir(PathBuf::new()))
}

fn real() -> (TabPaper, Calls) {
    let calls = Calls::default();
    let mut account = PaperAccount::with_trades_dir(PathBuf::new());
    account.attach_broker(Box::new(RealBroker {
        sim: Simulator::new(),
        facts: AccountFacts {
            login: 51_234_567,
            server: "XPMT5-PRD".to_owned(),
            company: "XP Investimentos".to_owned(),
            trade_mode: TradeMode::Real,
            margin_mode: MarginMode::Exchange,
            balance: None,
            currency: "BRL".to_owned(),
        },
        calls: Rc::clone(&calls),
    }));
    let paper = TabPaper::new(account);
    assert_eq!(paper.account.route_kind().word(), "REAL");
    (paper, calls)
}

/// A long of one filled at 100 and a resting buy limit at 90, both placed
/// before the feed fell behind.
fn exposed(paper: &mut TabPaper) -> OrderId {
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    paper.working_orders()[0].id
}

/// Every door an entry can come through, with the gate set.
fn entries_refused(paper: &mut TabPaper) {
    paper.set_trade_gate(Some(late()));
    let intent = OrderIntent::market(Side::Buy, Decimal::ONE);
    for command in [
        TradeCommand::Market { side: Side::Sell },
        TradeCommand::Place {
            intent: Box::new(intent),
        },
        TradeCommand::Reverse,
    ] {
        let outcome = paper.execute(command.clone());
        assert!(outcome.events.is_empty(), "{command:?} reached the venue");
        let refusal = outcome.refusal.expect("an entry is refused");
        assert!(refusal.contains("prints arrive 900 ms late"), "{refusal}");
        assert_eq!(outcome.gate, Some(late()), "a caller can tell why");
        let toast = paper.account.peek_toast().cloned().unwrap_or_default();
        assert!(toast.ends_with(&refusal), "toasted as refused: {toast}");
    }
    let orders = paper.working_orders().len();
    assert!(!paper.place_resting(Side::Buy, EntryKind::Limit, 80.0));
    let armed = ChartCommand::PlaceResting {
        side: Side::Buy,
        kind: EntryKind::Limit,
        raw_price: 85.0,
    };
    assert_eq!(paper.carry_out(armed), Ok(()));
    assert_eq!(paper.working_orders().len(), orders, "no chart placement");
}

/// Exits, cancels and amends still work with the gate set.
fn exits_pass(paper: &mut TabPaper, order: OrderId) {
    let protect = Bracket::whole(Some(Decimal::from(80)), None);
    for command in [
        TradeCommand::Amend {
            id: order,
            bracket: Box::new(protect),
        },
        TradeCommand::Cancel { id: order },
        TradeCommand::CancelAll,
        TradeCommand::Close,
        TradeCommand::Flatten,
    ] {
        let outcome = paper.execute(command.clone());
        assert_eq!(outcome.refusal, None, "{command:?} was refused");
        assert_eq!(outcome.gate, None);
    }
    paper.on_trade(&print(2, 101));
    assert!(paper.is_flat(), "the long closed");
    assert!(paper.working_orders().is_empty(), "the limit is gone");
}

#[test]
fn a_feed_out_of_sync_refuses_entries_on_the_simulator_and_lets_exits_through() {
    let mut paper = sim();
    let order = exposed(&mut paper);
    entries_refused(&mut paper);
    assert!(
        paper
            .account
            .peek_toast()
            .is_some_and(|toast| toast.starts_with("SIM: new entries paused")),
    );
    exits_pass(&mut paper, order);
}

#[test]
fn a_feed_out_of_sync_refuses_entries_on_a_real_route_and_lets_exits_through() {
    let (mut paper, calls) = real();
    let order = exposed(&mut paper);
    calls.borrow_mut().clear();
    entries_refused(&mut paper);
    assert!(
        calls.borrow().is_empty(),
        "nothing reached the broker: {:?}",
        calls.borrow()
    );
    assert!(
        paper
            .account
            .peek_toast()
            .is_some_and(|toast| toast.starts_with("REAL: new entries paused")),
    );
    exits_pass(&mut paper, order);
    let calls = calls.borrow();
    for exit in ["amend_bracket", "cancel", "close"] {
        assert!(
            calls.contains(&exit),
            "{exit} reached the broker: {calls:?}"
        );
    }
}

#[test]
fn entries_resume_once_the_gate_lifts() {
    let mut paper = sim();
    paper.seed(&print(0, 100));
    paper.set_trade_gate(Some(late()));
    assert_eq!(paper.trade_gate(), Some(&late()));
    paper.set_trade_gate(None);
    assert_eq!(paper.trade_gate(), None);
    let outcome = paper.execute(TradeCommand::Market { side: Side::Buy });
    assert_eq!(outcome.refusal, None);
    assert!(!outcome.events.is_empty(), "the entry reached the venue");
}
