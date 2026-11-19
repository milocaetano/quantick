// The route's rules, one test each, decided without a window or a clock:
// the time is a number the test hands over. The desk tests drive a broker
// double (a simulator wearing fixed account facts) through the same port the
// window uses.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use quantick_engine::{Side, Trade};
use quantick_trading::{
    Bracket, BracketTarget, BrokerVenue, CloseAmount, LinkState, OrderId, OrderIntent,
    RejectReason, TradeMode, VenueEvent, VenueNotice,
};
use rust_decimal::Decimal;

use super::*;
use crate::PaperAccount;
use crate::fake_broker::{Script, broker};
use crate::scratch::ScratchDir;

fn armed_facts(account: Route) -> RouteFacts {
    RouteFacts {
        broker_enabled: true,
        source_routes: true,
        account: Some(account),
        account_key: None,
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
            .request(
                &mut tabs,
                LIVE,
                RouteRequest::new(Route::Demo, Actor::Trader, None),
            )
            .expect("DEMO arms in one click"),
        _ => {
            desk.hold_start(&tabs, LIVE, 0).expect("REAL is available");
            desk.hold_still(&mut tabs, LIVE, HOLD_TO_ARM_MS);
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

#[path = "desk_tests.rs"]
mod desk_tests;
#[path = "session_tests.rs"]
mod session_tests;
