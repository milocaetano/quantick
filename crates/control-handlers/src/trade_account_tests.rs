use quantick_control::error::codes;
use quantick_control_schema::trade::PLACE_CAPABILITY_ID;
use quantick_control_schema::trade_account::{
    DISARM_CAPABILITY_ID, READ_CAPABILITY_ID, SELECT_CAPABILITY_ID,
};
use quantick_paper::route::{AccountRoute, RouteFacts};
use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeWindow};
use crate::trade::{CAPABILITY_VERSION, TradeDesk};

/// A window whose route the calls drive, with a terminal logged into a demo
/// account and no chart: every rule is the route's, so no desk is needed.
struct RouteWindow {
    route: AccountRoute,
    facts: RouteFacts,
    allow_agent_orders: bool,
}

impl RouteWindow {
    fn demo() -> Self {
        Self {
            route: AccountRoute::new(),
            facts: RouteFacts {
                broker_enabled: true,
                source_routes: true,
                account: Some(Route::Demo),
                account_key: None,
                link_up: true,
                exposure: false,
            },
            allow_agent_orders: false,
        }
    }
}

impl TradePort for RouteWindow {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        None
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        None
    }
    fn persist_order_strategies(&mut self) {}
    fn persist_risk_settings(&mut self) {}
    fn account_route(&mut self) -> Option<AccountRouteResult> {
        Some(AccountRouteResult {
            route: self.route.route().id().to_owned(),
            orders_to: String::new(),
            title: String::new(),
            broker_enabled: self.facts.broker_enabled,
            locked: false,
            accounts: Vec::new(),
            footer: None,
            leave_pending: self.route.leaving(),
        })
    }
    fn request_route(
        &mut self,
        route: Route,
        actor: Actor,
        leave: Option<Leave>,
    ) -> Result<(), Refusal> {
        self.route
            .request(route, actor, &self.facts, leave)
            .map(|_| ())
    }
    fn broker_write_block(&mut self, actor: Actor) -> Option<Refusal> {
        self.route.write_block(actor, self.allow_agent_orders)
    }
}

fn route_code(error: &ControlError) -> Option<&str> {
    error.context.details.as_ref()?["route_code"].as_str()
}

#[test]
fn the_account_calls_register_read_and_write() {
    let mut registry =
        quantick_control_host::actions::ActionRegistry::<RouteWindow, FakeAccess>::new();
    crate::trade::register(&mut registry).expect("the family docks");
    let read = registry
        .lookup(READ_CAPABILITY_ID, CAPABILITY_VERSION)
        .expect("read is registered");
    assert!(read.descriptor.read_only);
    for id in [SELECT_CAPABILITY_ID, DISARM_CAPABILITY_ID] {
        let action = registry
            .lookup(id, CAPABILITY_VERSION)
            .unwrap_or_else(|| panic!("{id} is registered"));
        assert!(!action.descriptor.read_only, "{id} changes the route");
    }
    let disarm = registry.lookup(DISARM_CAPABILITY_ID, 1).unwrap();
    assert!(disarm.descriptor.risk_reducing);
}

#[test]
fn an_agent_is_refused_arming_with_a_code_and_the_refusal_is_journaled() {
    let mut window = RouteWindow::demo();
    let mut access = FakeAccess::with_budget(0);
    let refused = select(
        &mut window,
        &mut access,
        &FakeWindow::assistant(),
        &json!({ "route": "demo" }),
    )
    .expect_err("an agent never arms");
    assert_eq!(refused.code.as_str(), codes::PERMISSION_DENIED);
    assert_eq!(route_code(&refused), Some("route.agent_cannot_arm"));
    assert_eq!(window.route.route(), Route::Sim);
    assert_eq!(access.events.len(), 1, "the refused ask is on the record");
}

#[test]
fn real_is_refused_to_every_named_call_and_demo_reaches_the_trader() {
    let mut window = RouteWindow::demo();
    let mut access = FakeAccess::with_budget(0);
    let refused = select(
        &mut window,
        &mut access,
        &FakeWindow::trader(),
        &json!({ "route": "real" }),
    )
    .expect_err("REAL needs the hold");
    assert_eq!(route_code(&refused), Some("route.real_needs_hold"));
    let armed = select(
        &mut window,
        &mut access,
        &FakeWindow::trader(),
        &json!({ "route": "demo" }),
    )
    .expect("the trader arms DEMO by name");
    assert_eq!(armed["route"], "demo");
}

#[test]
fn an_agent_may_disarm_and_its_broker_orders_are_refused_before_the_desk() {
    let mut window = RouteWindow::demo();
    let mut access = FakeAccess::with_budget(0);
    let trader = FakeWindow::trader();
    select(
        &mut window,
        &mut access,
        &trader,
        &json!({ "route": "demo" }),
    )
    .expect("armed");

    let mut registry =
        quantick_control_host::actions::ActionRegistry::<RouteWindow, FakeAccess>::new();
    crate::trade::register(&mut registry).expect("the family docks");
    let place = registry.lookup(PLACE_CAPABILITY_ID, 1).unwrap();
    let refused = (place.handler)(
        &mut window,
        &mut access,
        &FakeWindow::assistant(),
        &json!({ "side": "buy", "kind": "market", "quantity": "1" }),
    )
    .expect_err("allow_agent_orders is off");
    assert_eq!(route_code(&refused), Some("route.agent_orders_off"));

    let disarmed = disarm(
        &mut window,
        &mut access,
        &FakeWindow::assistant(),
        &json!({}),
    )
    .expect("an agent may go back to the simulator");
    assert_eq!(disarmed["route"], "sim");
}

#[test]
fn an_unknown_route_is_an_invalid_request() {
    let mut window = RouteWindow::demo();
    let mut access = FakeAccess::with_budget(0);
    let refused = select(
        &mut window,
        &mut access,
        &FakeWindow::trader(),
        &json!({ "route": "live" }),
    )
    .expect_err("no such route");
    assert_eq!(refused.code.as_str(), codes::INVALID_REQUEST);
}

/// A host with no tabs and broker routes off: the session's own answers.
struct NoTabs;

impl quantick_paper::route::AccountsPort for NoTabs {
    fn active_tab(&self) -> u64 {
        0
    }
    fn account(&self, _key: u64) -> Option<&quantick_paper::PaperAccount> {
        None
    }
    fn account_mut(&mut self, _key: u64) -> Option<&mut quantick_paper::PaperAccount> {
        None
    }
}

impl quantick_paper::route::RouteHost for NoTabs {
    fn source(&self) -> quantick_paper::route::SourceFacts {
        quantick_paper::route::SourceFacts::default()
    }
    fn now_ms(&self) -> i64 {
        0
    }
    fn ticket_quantity(&self) -> Option<rust_decimal::Decimal> {
        None
    }
    fn toast(&mut self, _text: String) {}
}

/// A window that answers only `route_call`: the three calls go through it.
struct SessionWindow(quantick_paper::route::BrokerSession);

impl TradePort for SessionWindow {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        None
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        None
    }
    fn persist_order_strategies(&mut self) {}
    fn persist_risk_settings(&mut self) {}
    fn route_call(&mut self) -> Option<quantick_paper::route::RouteCall<'_>> {
        Some(quantick_paper::route::RouteCall::new(
            &mut self.0,
            Box::new(NoTabs),
        ))
    }
}

#[test]
fn the_calls_reach_the_route_session_through_its_call() {
    let mut window = SessionWindow(quantick_paper::route::BrokerSession::default());
    let mut access = FakeAccess::with_budget(0);
    let trader = FakeWindow::trader();
    let now = read(&mut window, &mut access, &trader, &json!({})).expect("the read answers");
    assert_eq!(now["route"], "sim");
    assert_eq!(now["broker_enabled"], false);
    let refused = select(
        &mut window,
        &mut access,
        &trader,
        &json!({ "route": "demo" }),
    )
    .expect_err("broker routes are off");
    assert_eq!(route_code(&refused), Some("route.broker_off"));
    assert!(
        window
            .route_call()
            .is_some_and(|call| call.write_block(Actor::Agent).is_none())
    );
}
