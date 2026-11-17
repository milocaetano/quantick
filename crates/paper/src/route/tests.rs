// The route's rules, one test each, decided without a window or a clock:
// the time is a number the test hands over. The desk tests drive a broker
// double (a simulator wearing fixed account facts) through the same port the
// window uses.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use quantick_engine::{Side, Trade};
use quantick_sim::Simulator;
use quantick_trading::{
    AccountFacts, Bracket, BracketTarget, BrokerVenue, CloseAmount, ClosedTrade, LinkState,
    MarginMode, Order, OrderId, OrderIntent, Position, RejectReason, TradeMode, TradingVenue,
    VenueEvent, VenueNotice,
};
use rust_decimal::Decimal;

use super::*;
use crate::PaperAccount;
use crate::scratch::ScratchDir;

fn armed_facts(account: Route) -> RouteFacts {
    RouteFacts {
        broker_enabled: true,
        source_routes: true,
        account: Some(account),
        link_up: true,
        exposure: false,
    }
}

#[test]
fn every_run_starts_on_the_simulator() {
    let route = AccountRoute::new();
    assert_eq!(route.route(), Route::Sim);
    assert_eq!(route.hold_progress(0), None);
    assert_eq!(route.last_disarm(), None);
}

#[test]
fn real_arms_only_after_the_full_hold() {
    let facts = armed_facts(Route::Real);
    let mut route = AccountRoute::new();
    route.hold_start(1_000, &facts).expect("REAL is available");
    assert_eq!(route.hold_update(1_500, &facts), RouteChange::None);
    assert_eq!(route.hold_progress(1_500), Some(0.5));
    assert_eq!(route.route(), Route::Sim);
    assert_eq!(
        route.hold_update(1_000 + HOLD_TO_ARM_MS, &facts),
        RouteChange::ToBroker(Route::Real)
    );
    assert_eq!(route.route(), Route::Real);
    assert_eq!(route.hold_progress(3_000), None);
}

#[test]
fn releasing_before_the_hold_completes_arms_nothing() {
    let facts = armed_facts(Route::Real);
    let mut route = AccountRoute::new();
    route.hold_start(0, &facts).expect("REAL is available");
    assert_eq!(
        route.hold_release(HOLD_TO_ARM_MS - 1, &facts),
        RouteChange::None
    );
    assert_eq!(route.route(), Route::Sim);
    // The next frame's update has no hold left to complete.
    assert_eq!(route.hold_update(5_000, &facts), RouteChange::None);
    assert_eq!(route.route(), Route::Sim);
}

#[test]
fn demo_is_one_action_and_real_is_never_a_named_call() {
    let mut route = AccountRoute::new();
    let demo = armed_facts(Route::Demo);
    assert_eq!(
        route.request(Route::Demo, Actor::Trader, &demo, None),
        Ok(RouteChange::ToBroker(Route::Demo))
    );
    let mut route = AccountRoute::new();
    let real = armed_facts(Route::Real);
    assert_eq!(
        route.request(Route::Real, Actor::Trader, &real, None),
        Err(Refusal::RealNeedsHold)
    );
    assert_eq!(route.route(), Route::Sim);
}

#[test]
fn an_agent_never_arms_but_may_go_back_to_the_simulator() {
    let mut route = AccountRoute::new();
    for kind in [Route::Demo, Route::Real] {
        let refused = route.request(kind, Actor::Agent, &armed_facts(kind), None);
        assert_eq!(refused, Err(Refusal::AgentCannotArm));
        assert_eq!(refused.unwrap_err().code(), "route.agent_cannot_arm");
    }
    route
        .request(Route::Demo, Actor::Trader, &armed_facts(Route::Demo), None)
        .expect("the trader arms DEMO");
    assert_eq!(
        route.request(Route::Sim, Actor::Agent, &armed_facts(Route::Demo), None),
        Ok(RouteChange::ToSim { flatten: false })
    );
}

#[test]
fn agent_broker_orders_follow_the_config() {
    let mut route = AccountRoute::new();
    assert_eq!(route.write_block(Actor::Agent, false), None, "SIM is open");
    route
        .select_demo(Actor::Trader, &armed_facts(Route::Demo))
        .expect("DEMO arms");
    assert_eq!(
        route.write_block(Actor::Agent, false),
        Some(Refusal::AgentOrdersOff)
    );
    assert_eq!(route.write_block(Actor::Agent, true), None);
    assert_eq!(route.write_block(Actor::Trader, false), None);
}

#[test]
fn a_source_that_cannot_route_locks_and_disarms() {
    let mut replay = armed_facts(Route::Real);
    replay.source_routes = false;
    let mut route = AccountRoute::new();
    assert_eq!(
        route.hold_start(0, &replay),
        Err(Refusal::SourceCannotRoute)
    );
    route
        .select_demo(Actor::Trader, &armed_facts(Route::Demo))
        .expect("DEMO arms on a live chart");
    let mut moved = armed_facts(Route::Demo);
    moved.source_routes = false;
    assert_eq!(
        route.observe(&moved, 7),
        RouteChange::ToSim { flatten: false }
    );
    assert_eq!(route.last_disarm(), Some((Disarm::SourceCannotRoute, 7)));
}

#[test]
fn a_lost_link_disarms_without_closing_anything() {
    let mut route = AccountRoute::new();
    let facts = armed_facts(Route::Real);
    route.hold_start(0, &facts).expect("REAL is available");
    route.hold_update(HOLD_TO_ARM_MS, &facts);
    assert_eq!(
        route.on_notice(&VenueNotice::LinkLost, 9_000),
        RouteChange::ToSim { flatten: false }
    );
    assert_eq!(route.route(), Route::Sim);
    assert_eq!(route.last_disarm(), Some((Disarm::LinkLost, 9_000)));
}

#[test]
fn a_login_change_or_a_refused_account_disarms() {
    for (notice, reason) in [
        (VenueNotice::AccountChanged, Disarm::AccountChanged),
        (
            VenueNotice::AccountRefused("hedging accounts are refused"),
            Disarm::AccountRefused("hedging accounts are refused"),
        ),
    ] {
        let mut route = AccountRoute::new();
        route
            .select_demo(Actor::Trader, &armed_facts(Route::Demo))
            .expect("DEMO arms");
        assert_eq!(
            route.on_notice(&notice, 1),
            RouteChange::ToSim { flatten: false }
        );
        assert_eq!(route.last_disarm(), Some((reason, 1)));
    }
    // A terminal that now reports the other kind of account disarms too.
    let mut route = AccountRoute::new();
    route
        .select_demo(Actor::Trader, &armed_facts(Route::Demo))
        .expect("DEMO arms");
    assert_eq!(
        route.observe(&armed_facts(Route::Real), 2),
        RouteChange::ToSim { flatten: false }
    );
    assert_eq!(route.last_disarm(), Some((Disarm::AccountChanged, 2)));
}

#[test]
fn leaving_with_exposure_asks_how() {
    let mut facts = armed_facts(Route::Demo);
    let mut route = AccountRoute::new();
    route.select_demo(Actor::Trader, &facts).expect("DEMO arms");
    facts.exposure = true;
    assert_eq!(
        route.select_sim(&facts, None),
        Err(Refusal::ExposureNeedsChoice)
    );
    assert!(route.leaving());
    assert_eq!(route.route(), Route::Demo, "nothing moved while asking");
    assert_eq!(
        route.select_sim(&facts, Some(Leave::LeaveAtBroker)),
        Ok(RouteChange::ToSim { flatten: false })
    );
    assert!(!route.leaving());
    route
        .select_demo(Actor::Trader, &facts)
        .expect("DEMO again");
    assert_eq!(
        route.select_sim(&facts, Some(Leave::FlattenFirst)),
        Ok(RouteChange::ToSim { flatten: true })
    );
}

// --- the desk, over a broker double -------------------------------------

/// What the test changes about the broker after handing it over.
#[derive(Default)]
struct Script {
    link: Option<LinkState>,
    said: Vec<VenueEvent>,
    tickets: BTreeMap<OrderId, u64>,
}

/// A simulator wearing a broker's reads.
struct FakeBroker {
    sim: Simulator,
    facts: AccountFacts,
    script: Rc<RefCell<Script>>,
}

impl TradingVenue for FakeBroker {
    fn submit(&mut self, intent: OrderIntent) -> Vec<VenueEvent> {
        self.sim.submit(intent)
    }
    fn amend_price(&mut self, id: OrderId, price: Decimal) -> Vec<VenueEvent> {
        self.sim.amend_price(id, price)
    }
    fn amend_bracket(&mut self, target: BracketTarget, bracket: Bracket) -> Vec<VenueEvent> {
        self.sim.amend_bracket(target, bracket)
    }
    fn cancel(&mut self, id: OrderId) -> Vec<VenueEvent> {
        self.sim.cancel(id)
    }
    fn close(&mut self, amount: CloseAmount) -> Vec<VenueEvent> {
        self.sim.close(amount)
    }
    fn flatten(&mut self) -> Vec<VenueEvent> {
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

impl BrokerVenue for FakeBroker {
    fn poll(&mut self) -> Vec<VenueEvent> {
        std::mem::take(&mut self.script.borrow_mut().said)
    }
    fn set_symbol(&mut self, _symbol: &str) -> Vec<VenueEvent> {
        Vec::new()
    }
    fn account(&self) -> Option<&AccountFacts> {
        // A terminal that has not said hello has not said which account.
        (self.script.borrow().link != Some(LinkState::NeverConnected)).then_some(&self.facts)
    }
    fn link_state(&self) -> LinkState {
        self.script.borrow().link.unwrap_or(LinkState::Up)
    }
    fn entry_block(&self) -> Option<&'static str> {
        None
    }
    fn foreign_counts(&self) -> (usize, usize) {
        (1, 0)
    }
    fn order_ref(&self, id: OrderId) -> Option<u64> {
        self.script.borrow().tickets.get(&id).copied()
    }
    fn position_ref(&self) -> Option<u64> {
        Some(88_412_094)
    }
    fn is_acknowledged(&self, id: OrderId) -> bool {
        self.script.borrow().tickets.contains_key(&id)
    }
}

fn broker(mode: TradeMode) -> (Box<dyn BrokerVenue>, Rc<RefCell<Script>>) {
    let script = Rc::new(RefCell::new(Script::default()));
    let venue = FakeBroker {
        sim: Simulator::new(),
        facts: AccountFacts {
            login: 51_234_567,
            server: "XPMT5-PRD".to_owned(),
            company: "XP Investimentos".to_owned(),
            trade_mode: mode,
            margin_mode: MarginMode::Exchange,
            balance: Some(Decimal::new(1_234_000, 2)),
            currency: "BRL".to_owned(),
        },
        script: Rc::clone(&script),
    };
    (Box::new(venue), script)
}

/// Two tabs, keyed 1 and 2, the first active.
struct Tabs {
    active: u64,
    accounts: BTreeMap<u64, PaperAccount>,
    _dir: ScratchDir,
}

impl Tabs {
    fn new() -> Self {
        let dir = ScratchDir::new("route-desk");
        let mut accounts = BTreeMap::new();
        for key in [1, 2] {
            let mut account =
                PaperAccount::with_trades_dir(dir.path().join(format!("paper-{key}")));
            account.set_symbol("WINZ26");
            accounts.insert(key, account);
        }
        Self {
            active: 1,
            accounts,
            _dir: dir,
        }
    }

    fn active(&mut self) -> &mut PaperAccount {
        self.accounts.get_mut(&self.active).expect("active tab")
    }
}

impl AccountsPort for Tabs {
    fn active_tab(&self) -> u64 {
        self.active
    }
    fn account(&self, key: u64) -> Option<&PaperAccount> {
        self.accounts.get(&key)
    }
    fn account_mut(&mut self, key: u64) -> Option<&mut PaperAccount> {
        self.accounts.get_mut(&key)
    }
}

const LIVE: SourceFacts = SourceFacts {
    broker_enabled: true,
    source_routes: true,
    replay: false,
};

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small ids") * 1_000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn armed(mode: TradeMode) -> (BrokerDesk, Tabs, Rc<RefCell<Script>>) {
    let (venue, script) = broker(mode);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    match mode {
        TradeMode::Demo => desk
            .request(&mut tabs, LIVE, (Route::Demo, Actor::Trader, None))
            .expect("DEMO arms in one click"),
        _ => {
            desk.hold_start(&tabs, LIVE, 0).expect("REAL is available");
            desk.hold(&mut tabs, LIVE, HOLD_TO_ARM_MS, false);
        }
    }
    (desk, tabs, script)
}

/// A long of one at 100 on the active account's venue.
fn go_long(account: &mut PaperAccount) {
    account.seed(&print(0, 100));
    let sent = account
        .venue_mut()
        .submit(OrderIntent::market(Side::Buy, Decimal::ONE));
    account.handle_events(sent);
    account.on_trade(&print(1, 100));
}

#[test]
fn arming_routes_the_active_chart_to_the_broker() {
    let (desk, mut tabs, _script) = armed(TradeMode::Real);
    assert_eq!(desk.route().route(), Route::Real);
    assert!(tabs.active().broker_routed());
    assert_eq!(tabs.active().route_kind(), Route::Real);
    let sheet = desk.sheet(&tabs, LIVE, None, HOLD_TO_ARM_MS);
    assert_eq!(sheet.title, "TRADING · REAL");
    assert_eq!(sheet.notice.text, "XP 51234567 · REAL");
    assert_eq!(sheet.notice.tone, RouteTone::Real);
}

#[test]
fn the_held_real_row_shows_its_facts_and_progress() {
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    desk.hold_start(&tabs, LIVE, 0).expect("REAL is available");
    let money = quantick_trading::InstrumentMoney::declared(
        ("BRL", Decimal::new(20, 2), Decimal::ONE),
        None,
    )
    .expect("a declaration");
    let mut book = crate::risk_sizing::InstrumentBook::new();
    book.insert("WINZ26".to_owned(), money);
    tabs.active().set_instrument_money(book);
    let sheet = desk.sheet(&tabs, LIVE, Some(Decimal::ONE), 620);
    let real = &sheet.rows[1];
    assert_eq!(real.title, "XP · 51234567 · REAL");
    assert_eq!(real.hint, "holding…");
    assert_eq!(real.gesture, RowGesture::Hold);
    assert_eq!(real.progress, Some(0.62));
    assert_eq!(
        real.subs,
        [
            "real money · balance 12340.00 BRL",
            "WINZ26 · 0.20 BRL/pt · qty 1",
            "you: flat · other robots: 1 position",
            "Stop and target rest on the server.",
        ]
    );
    assert_eq!(sheet.rows[0].hint, "in use");
}

#[test]
fn a_replay_locks_the_selector_with_the_reason() {
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    let replay = SourceFacts {
        broker_enabled: true,
        source_routes: false,
        replay: true,
    };
    assert_eq!(
        desk.hold_start(&tabs, replay, 0),
        Err(Refusal::SourceCannotRoute)
    );
    let sheet = desk.sheet(&tabs, replay, None, 0);
    assert!(sheet.locked);
    assert_eq!(sheet.selector, "Simulated · replay");
    assert_eq!(sheet.rows[1].state, RowState::Off);
    assert_eq!(
        sheet.footer.as_deref(),
        Some("This source does not route orders.")
    );
    assert_eq!(sheet.rows[0].subs, ["Replay of WINZ26."]);
    desk.frame(&mut tabs, replay, 0);
    assert_eq!(desk.route().route(), Route::Sim);
}

#[test]
fn a_lost_link_with_a_position_parks_it_in_view_and_refuses_entries() {
    let (mut desk, mut tabs, script) = armed(TradeMode::Real);
    go_long(tabs.active());
    assert!(tabs.active().venue().position().is_some());

    script.borrow_mut().link = Some(LinkState::Lost);
    script
        .borrow_mut()
        .said
        .push(VenueEvent::Notice(VenueNotice::LinkLost));
    desk.frame(&mut tabs, LIVE, 5_000);
    assert_eq!(desk.route().route(), Route::Sim);
    assert_eq!(
        desk.take_toast().as_deref(),
        Some("SIM: no link with the broker - disarmed")
    );
    let account = tabs.active();
    assert!(!account.broker_routed());
    assert!(
        account.broker_shown(),
        "the broker position stays on the chart"
    );
    assert_eq!(account.route_kind(), Route::Real);
    assert_eq!(
        account.entry_block(),
        Some(crate::account::PARKED_ENTRY_BLOCK)
    );
    assert!(
        !account.entry_ready(),
        "the toolbar and the ticket both refuse"
    );
    let refused = account
        .venue_mut()
        .submit(OrderIntent::market(Side::Buy, Decimal::ONE));
    assert!(matches!(
        refused.as_slice(),
        [VenueEvent::Rejected(RejectReason::Blocked(_))]
    ));
    let sheet = desk.sheet(&tabs, LIVE, None, 19_000);
    assert_eq!(sheet.notice.text, "XP 51234567 · no link");
    assert_eq!(sheet.notice.dot, Dot::Hollow);
    assert_eq!(sheet.tone, RouteTone::Lost);
    assert_eq!(
        sheet.status.as_deref(),
        Some("no link with the broker for 14 s · disarmed · stop and target stay at the broker")
    );
}

#[test]
fn leaving_flat_detaches_the_broker_and_trades_the_simulator() {
    let (mut desk, mut tabs, _script) = armed(TradeMode::Demo);
    assert!(tabs.active().broker_routed());
    desk.request(&mut tabs, LIVE, (Route::Sim, Actor::Trader, None))
        .expect("flat, nothing to ask");
    let account = tabs.active();
    assert!(account.broker().is_none());
    assert_eq!(account.route_kind(), Route::Sim);
    assert!(desk.has_venue());
}

#[test]
fn leaving_with_a_position_asks_then_keeps_its_plates() {
    let (mut desk, mut tabs, _script) = armed(TradeMode::Demo);
    go_long(tabs.active());
    assert_eq!(
        desk.request(&mut tabs, LIVE, (Route::Sim, Actor::Trader, None)),
        Err(Refusal::ExposureNeedsChoice)
    );
    let sheet = desk.sheet(&tabs, LIVE, None, 0);
    assert!(sheet.leave_prompt);
    desk.request(
        &mut tabs,
        LIVE,
        (Route::Sim, Actor::Trader, Some(Leave::LeaveAtBroker)),
    )
    .expect("left at the broker");
    let account = tabs.active();
    assert!(account.broker_shown(), "the plates stay on the chart");
    assert!(account.venue().position().is_some());
    assert!(!account.broker_routed());
}

#[test]
fn switching_to_a_chart_that_cannot_route_disarms() {
    let (mut desk, mut tabs, _script) = armed(TradeMode::Demo);
    tabs.active = 2;
    let binance = SourceFacts {
        broker_enabled: true,
        source_routes: false,
        replay: false,
    };
    desk.frame(&mut tabs, binance, 3);
    assert_eq!(desk.route().route(), Route::Sim);
    assert_eq!(
        desk.route().last_disarm(),
        Some((Disarm::SourceCannotRoute, 3))
    );
    assert!(tabs.accounts[&2].broker().is_none());
}

#[test]
fn broker_closes_go_to_their_own_journal() {
    let (_desk, mut tabs, _script) = armed(TradeMode::Demo);
    let account = tabs.active();
    go_long(account);
    account.close_position();
    account.on_trade(&print(2, 103));
    let real = account.broker_trades_dir().join("WINZ26");
    assert!(
        std::fs::read_dir(&real).is_ok_and(|mut files| files.next().is_some()),
        "a broker close lands under {}",
        real.display()
    );
    assert!(
        account.journal_path().is_none(),
        "the paper journal stayed empty"
    );
    let toast = account.take_toast().unwrap_or_default();
    assert!(toast.starts_with("DEMO closed: LONG 1"), "{toast}");
}

#[test]
fn plates_name_the_broker_ticket_once_it_has_one() {
    let (_desk, mut tabs, script) = armed(TradeMode::Demo);
    let account = tabs.active();
    account.seed(&print(0, 100));
    let placed = account.venue_mut().submit(OrderIntent::limit(
        Side::Buy,
        Decimal::ONE,
        Decimal::from(90),
    ));
    account.handle_events(placed);
    let id = account.venue().working_orders()[0].id;
    assert!(account.awaiting_ack(id), "sent, not yet held");
    assert_eq!(account.plate_id(id), id.0);
    script.borrow_mut().tickets.insert(id, 88_412_096);
    assert!(!account.awaiting_ack(id));
    assert_eq!(account.plate_id(id), 88_412_096);
    assert_eq!(account.venue_ref(BracketTarget::Position), Some(88_412_094));
}

#[test]
fn automated_strategies_never_reach_a_broker() {
    let (_desk, mut tabs, _script) = armed(TradeMode::Demo);
    let events = tabs
        .active()
        .apply_strategy_command(quantick_sim::Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
        });
    assert_eq!(
        events,
        [VenueEvent::Rejected(RejectReason::Blocked(
            crate::account::STRATEGIES_SIM_ONLY
        ))]
    );
}

#[test]
fn a_press_on_the_real_row_arms_only_when_the_hold_completes() {
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    desk.press(&mut tabs, LIVE, RoutePress::OpenList, 0);
    desk.press(&mut tabs, LIVE, RoutePress::HoldStart, 0);
    desk.frame(&mut tabs, LIVE, HOLD_TO_ARM_MS / 2);
    assert_eq!(desk.route().route(), Route::Sim);
    desk.press(&mut tabs, LIVE, RoutePress::HoldRelease, HOLD_TO_ARM_MS / 2);
    desk.frame(&mut tabs, LIVE, 5 * HOLD_TO_ARM_MS);
    assert_eq!(desk.route().route(), Route::Sim, "released early");

    desk.press(&mut tabs, LIVE, RoutePress::HoldStart, 10_000);
    desk.frame(&mut tabs, LIVE, 10_000 + HOLD_TO_ARM_MS);
    assert_eq!(desk.route().route(), Route::Real, "held through");
    assert!(!desk.sheet_open, "the list closes once armed");
}

#[test]
fn a_staged_scene_arms_as_the_trader_would_and_drops_its_link_once() {
    let scene = Scene::parse(Some("lost"), Some("1"), None).expect("a scene");
    assert!(scene.needs_broker() && scene.stands_in_live() && scene.lost);
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    assert!(!desk.stage(&mut tabs, LIVE, &scene, 5_000), "arming first");
    assert!(desk.sheet_open);
    assert_eq!(desk.route().route(), Route::Real, "through the full hold");
    assert!(
        desk.stage(&mut tabs, LIVE, &scene, 5_016),
        "then the link drops"
    );
    assert_eq!(Scene::parse(None, None, None), None);
    let idle = Scene::parse(Some("idle"), None, None).expect("a scene");
    assert!(idle.needs_broker() && !idle.stands_in_live());
}

// --- the session, over a connector double --------------------------------

/// The tabs as a window hands them to the session: the config and chart
/// facts, a clock the test sets, and the toasts it was told.
struct Host {
    tabs: Tabs,
    source: SourceFacts,
    now_ms: i64,
    toasts: Vec<String>,
}

impl Host {
    fn new(source: SourceFacts) -> Self {
        Self {
            tabs: Tabs::new(),
            source,
            now_ms: 0,
            toasts: Vec::new(),
        }
    }
}

impl AccountsPort for Host {
    fn active_tab(&self) -> u64 {
        self.tabs.active_tab()
    }
    fn account(&self, key: u64) -> Option<&PaperAccount> {
        self.tabs.account(key)
    }
    fn account_mut(&mut self, key: u64) -> Option<&mut PaperAccount> {
        self.tabs.account_mut(key)
    }
}

impl RouteHost for Host {
    fn source(&self) -> SourceFacts {
        self.source
    }
    fn now_ms(&self) -> i64 {
        self.now_ms
    }
    fn ticket_quantity(&self) -> Option<Decimal> {
        None
    }
    fn toast(&mut self, text: String) {
        self.toasts.push(text);
    }
}

/// What the connector double did, shared with the test.
#[derive(Default)]
struct Wiring {
    built: usize,
    severed: bool,
    script: Option<Rc<RefCell<Script>>>,
}

/// Builds a broker of `mode`, or nothing when `mode` is `None` (the port is
/// taken).
struct Connector {
    mode: Option<TradeMode>,
    link: Option<LinkState>,
    wiring: Rc<RefCell<Wiring>>,
}

impl quantick_trading::BrokerConnector for Connector {
    fn connect(&mut self, _symbol: &str) -> Option<Box<dyn BrokerVenue>> {
        let (venue, script) = broker(self.mode?);
        script.borrow_mut().link = self.link;
        let mut wiring = self.wiring.borrow_mut();
        wiring.built += 1;
        wiring.script = Some(script);
        Some(venue)
    }
    fn sever(&mut self) {
        self.wiring.borrow_mut().severed = true;
    }
}

fn session(
    mode: Option<TradeMode>,
    link: Option<LinkState>,
) -> (BrokerSession, Rc<RefCell<Wiring>>) {
    let wiring = Rc::new(RefCell::new(Wiring::default()));
    let connector = Connector {
        mode,
        link,
        wiring: Rc::clone(&wiring),
    };
    (BrokerSession::new(Box::new(connector), false), wiring)
}

#[test]
fn the_list_builds_the_venue_once_and_only_while_broker_routes_are_offered() {
    let (mut session, wiring) = session(Some(TradeMode::Real), None);
    let mut host = Host::new(SourceFacts::default());
    session.press(&mut host, RoutePress::OpenList);
    session.frame(&mut host);
    assert_eq!(wiring.borrow().built, 0, "broker routes are off");
    assert!(session.sheet().is_none());

    host.source = LIVE;
    session.frame(&mut host);
    assert_eq!(
        wiring.borrow().built,
        0,
        "nothing is built until the list opens"
    );
    session.press(&mut host, RoutePress::ToggleList);
    session.press(&mut host, RoutePress::ToggleList);
    session.press(&mut host, RoutePress::OpenList);
    assert_eq!(wiring.borrow().built, 1, "one terminal, one venue");
    let sheet = session.sheet().expect("broker routes are offered");
    assert!(sheet.open);
    assert_eq!(sheet.rows[1].title, "XP · 51234567 · REAL");
}

#[test]
fn the_broker_row_says_connecting_until_the_terminal_names_its_account() {
    let (mut session, _wiring) = session(Some(TradeMode::Real), Some(LinkState::NeverConnected));
    let mut host = Host::new(LIVE);
    session.press(&mut host, RoutePress::OpenList);
    let row = &session.sheet().expect("offered").rows[1];
    assert_eq!(row.hint, "connecting…");
    assert_eq!(row.state, RowState::Off);

    let (mut session, _wiring) = session_without_terminal();
    session.press(&mut host, RoutePress::OpenList);
    let row = &session.sheet().expect("offered").rows[1];
    assert_eq!(row.subs, ["not connected"]);
}

fn session_without_terminal() -> (BrokerSession, Rc<RefCell<Wiring>>) {
    session(None, None)
}

#[test]
fn a_named_demo_request_builds_the_venue_and_arms_for_the_trader_only() {
    let (mut session, wiring) = session(Some(TradeMode::Demo), None);
    let mut host = Host::new(LIVE);
    session.frame(&mut host);
    let mut call = RouteCall::new(&mut session, Box::new(Host::new(LIVE)));
    assert_eq!(
        call.request((Route::Demo, Actor::Agent, None)),
        Err(Refusal::AgentCannotArm)
    );
    assert_eq!(call.request((Route::Demo, Actor::Trader, None)), Ok(()));
    let (sheet, route, offered) = call.read();
    assert_eq!((route, offered), (Route::Demo, true));
    assert_eq!(sheet.notice.tone, RouteTone::Demo);
    assert_eq!(
        call.write_block(Actor::Agent),
        Some(Refusal::AgentOrdersOff)
    );
    assert_eq!(wiring.borrow().built, 1);
}

/// A host whose capture staged `scene`, over a connector wired to the test.
struct StagedHost(Host, Option<Scene>, Rc<RefCell<Wiring>>);

impl AccountsPort for StagedHost {
    fn active_tab(&self) -> u64 {
        self.0.active_tab()
    }
    fn account(&self, key: u64) -> Option<&PaperAccount> {
        self.0.account(key)
    }
    fn account_mut(&mut self, key: u64) -> Option<&mut PaperAccount> {
        self.0.account_mut(key)
    }
}

impl RouteHost for StagedHost {
    fn source(&self) -> SourceFacts {
        self.0.source
    }
    fn now_ms(&self) -> i64 {
        self.0.now_ms
    }
    fn ticket_quantity(&self) -> Option<Decimal> {
        None
    }
    fn toast(&mut self, text: String) {
        self.0.toasts.push(text);
    }
    fn staged(&self) -> Option<Staged> {
        let connector = Connector {
            mode: Some(TradeMode::Real),
            link: None,
            wiring: Rc::clone(&self.2),
        };
        Some((self.1, Some(Box::new(connector))))
    }
}

#[test]
fn a_staged_lost_link_arms_over_a_replay_then_drops_and_disarms() {
    let scene = Scene::parse(Some("lost"), None, None);
    let replay = SourceFacts {
        broker_enabled: true,
        source_routes: false,
        replay: true,
    };
    let (mut session, configured) = session(Some(TradeMode::Demo), None);
    let staged = Rc::new(RefCell::new(Wiring::default()));
    let mut host = StagedHost(Host::new(replay), scene, Rc::clone(&staged));
    session.frame(&mut host);
    assert_eq!(session.sheet().expect("offered").tone, RouteTone::Real);
    assert_eq!(staged.borrow().built, 1, "the staged connector stands in");
    assert_eq!(configured.borrow().built, 0);
    assert!(!staged.borrow().severed);
    session.frame(&mut host);
    assert!(staged.borrow().severed, "armed, the staged link drops");
}

#[test]
fn a_staged_broker_scene_waits_for_a_mark_and_opens_at_whole_points() {
    let wiring = Rc::new(RefCell::new(Wiring::default()));
    let connect = |demo: bool, entry: Decimal| -> Box<dyn quantick_trading::BrokerConnector> {
        assert!(demo);
        assert_eq!(entry, Decimal::from(125_431));
        let (mode, link) = (Some(TradeMode::Demo), None);
        Box::new(Connector {
            mode,
            link,
            wiring: Rc::clone(&wiring),
        })
    };
    let list = [None, Some("1"), None];
    let (scene, connector) = Scene::stage(list, None, |_, _| unreachable!()).expect("no broker");
    assert!(scene.is_some_and(|scene| scene.list) && connector.is_none());
    let demo = [Some("demo"), None, None];
    assert!(Scene::stage(demo, None, |_, _| unreachable!()).is_none());
    let mark = Some(Decimal::new(1_254_306, 1));
    let (scene, connector) = Scene::stage(demo, mark, connect).expect("a mark");
    assert!(scene.is_some_and(|scene| scene.demo()) && connector.is_some());
}

#[test]
fn a_lost_link_takes_the_status_cell_while_its_account_is_shown() {
    let scene = Scene::parse(Some("real"), None, None);
    let (mut session, _configured) = session(Some(TradeMode::Demo), None);
    let staged = Rc::new(RefCell::new(Wiring::default()));
    let mut host = StagedHost(Host::new(LIVE), scene, Rc::clone(&staged));
    session.frame(&mut host);
    assert_eq!(session.status_cell(), None, "the link is up");
    go_long(host.0.tabs.active());
    let script = staged.borrow().script.clone().expect("built");
    script.borrow_mut().link = Some(LinkState::Lost);
    let lost = VenueEvent::Notice(VenueNotice::LinkLost);
    script.borrow_mut().said.push(lost);
    host.0.now_ms = 3_000;
    session.frame(&mut host);
    let status = session.sheet().and_then(|sheet| sheet.status.clone());
    assert!(status.is_some(), "the shown account says its link is down");
    let cell = status.map(|status| (status, std::cmp::Ordering::Less));
    assert_eq!(session.status_cell(), cell);
}
