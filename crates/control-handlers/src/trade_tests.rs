use quantick_control_host::actions::ActionRegistry;
use quantick_control_host::authority::TRADE_EFFECT_ID;
use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeWindow};

fn docked() -> ActionRegistry<FakeWindow, FakeAccess> {
    let mut registry = ActionRegistry::new();
    register(&mut registry).unwrap();
    registry
}

#[test]
fn the_trade_actions_register_and_validate_their_schemas() {
    let registry = docked();
    for id in [
        PLACE_CAPABILITY_ID,
        BRACKET_CAPABILITY_ID,
        CANCEL_CAPABILITY_ID,
    ] {
        let action = registry
            .lookup(id, CAPABILITY_VERSION)
            .unwrap_or_else(|| panic!("{id} is registered"));
        assert_eq!(action.descriptor.effect.as_str(), TRADE_EFFECT_ID);
        assert!(!action.descriptor.read_only, "{id} changes the account");
        assert!(
            action
                .descriptor
                .required_permissions
                .iter()
                .any(|permission| permission.as_str() == TRADE_PERMISSION_ID),
            "{id} sits behind the trade permission and nothing softer"
        );
    }

    let place = registry.lookup(PLACE_CAPABILITY_ID, 1).unwrap();
    place
        .input
        .validate(&json!({
            "side": "buy",
            "kind": "limit",
            "quantity": "2",
            "price": "95.5",
            "stop_loss": "90"
        }))
        .unwrap();
    assert!(
        place
            .input
            .validate(&json!({ "side": "buy", "kind": "limit" }))
            .is_err(),
        "quantity is not optional"
    );
    assert!(
        place
            .input
            .validate(&json!({
                "side": "buy",
                "kind": "limit",
                "quantity": "1",
                "price": 95.5
            }))
            .is_err(),
        "a price is a decimal string, never a JSON double"
    );
    assert!(
        place
            .input
            .validate(&json!({ "side": "sideways", "kind": "limit", "quantity": "1" }))
            .is_err()
    );
}

/// The one thing that must stay true while no profile may trade: the
/// permission is real, and it is not the annotate tier's.
#[test]
fn trading_never_borrows_the_annotate_permission() {
    let registry = docked();
    let cancel = registry.lookup(CANCEL_CAPABILITY_ID, 1).unwrap();
    assert!(
        !cancel.descriptor.destructive,
        "a working order is an instruction, not the trader's work"
    );
    assert!(
        cancel.descriptor.risk_reducing,
        "and taking one off the book can only ever reduce exposure"
    );
    assert!(
        !cancel
            .descriptor
            .required_permissions
            .iter()
            .any(|permission| permission.as_str().starts_with("annotate")),
        "annotate promises it never affects a position; a trade cannot borrow it"
    );
}

/// Without a chart open there is no desk, and every trade action says so
/// in the words the ticket uses.
#[test]
fn a_trade_without_a_chart_open_is_refused_before_it_journals() {
    let mut window = FakeWindow::new(Vec::new());
    let mut access = FakeAccess::with_budget(0);
    let refusal = cancel_order(
        &mut window,
        &mut access,
        &FakeWindow::trader(),
        &json!({ "order_id": 7 }),
    )
    .expect_err("no desk");
    assert_eq!(refusal, no_chart_open());
    assert!(
        access.events.is_empty(),
        "a refused call before the desk records nothing"
    );
}

/// One paper desk, as the window's adapter hands the active tab's over: an
/// account marked at 100 and the desk's ruler and ticket around it.
struct PaperDesk {
    account: quantick_paper::PaperAccount,
    desk: quantick_paper::desk::Desk,
}

impl TradeDesk for PaperDesk {
    fn account(&self) -> &PaperAccount {
        &self.account
    }
    fn account_mut(&mut self) -> &mut PaperAccount {
        &mut self.account
    }
    fn ruler_ticks(&self) -> u32 {
        self.desk.ruler.notches
    }
    fn set_ruler_ticks(&mut self, ticks: u32) {
        self.desk.ruler.set_ticks(ticks);
    }
    fn clear_ruler(&mut self) {
        self.desk.ruler.clear();
    }
    fn mark_price(&self) -> Option<Decimal> {
        self.account.mark_price()
    }
    fn working_orders(&self) -> &[Order] {
        self.account.working_orders()
    }
    fn armed_bracket(&self, side: Side, reference: Decimal, quantity: Decimal) -> Bracket {
        let env = self.desk.account_env(&self.account, side, reference);
        self.account.armed_bracket(side, reference, quantity, &env)
    }
    fn risk_report(&self) -> (RiskState, bool) {
        self.desk.risk_report(&self.account)
    }
    fn position_summary(&self) -> Option<PositionSummary> {
        self.account.position_summary()
    }
    fn selected_trade_index(&self) -> Option<usize> {
        None
    }
}

/// A window whose active tab holds `desk`.
struct DeskWindow {
    desk: PaperDesk,
}

impl TradePort for DeskWindow {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        Some(&self.desk)
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        Some(&mut self.desk)
    }
    fn persist_order_strategies(&mut self) {}
    fn persist_risk_settings(&mut self) {}
}

/// A desk marked at 100. Nothing here closes a position, so nothing is
/// journaled - and the folder sits under this source file, where no
/// directory can be made, so a write could never land anywhere.
fn desk_window() -> DeskWindow {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/trade_tests.rs/trades");
    let mut account = quantick_paper::PaperAccount::with_trades_dir(dir);
    account.set_symbol("TRADEX");
    account.seed(&quantick_engine::Trade {
        agg_id: 0,
        timestamp_ms: 0,
        price: Decimal::from(100),
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
    DeskWindow {
        desk: PaperDesk {
            account,
            desk: quantick_paper::desk::Desk::default(),
        },
    }
}

/// The second operator places an order, brackets it and takes it away,
/// and reads each step back from the answer; a refusal comes back in the
/// venue's own words and changes nothing.
#[test]
fn an_order_is_placed_bracketed_refused_and_cancelled_by_name() {
    let mut window = desk_window();
    let mut access = FakeAccess::with_budget(0);
    let trader = FakeWindow::trader();

    let placed = place_order(
        &mut window,
        &mut access,
        &trader,
        &json!({ "side": "buy", "kind": "limit", "quantity": "2", "price": "99" }),
    )
    .expect("the call dispatches");
    assert_eq!(placed["accepted"], true, "{placed}");
    assert_eq!(placed["simulated"], true, "every result says so");
    assert_eq!(placed["working_orders"][0]["kind"], "limit");
    assert_eq!(placed["working_orders"][0]["quantity"], "2");
    let order_id = placed["order_id"].as_u64().expect("the venue named it");

    let bracketed = bracket_order(
        &mut window,
        &mut access,
        &trader,
        &json!({ "order_id": order_id, "stop_loss": "98", "take_profit": "101" }),
    )
    .expect("the call dispatches");
    assert_eq!(bracketed["accepted"], true, "{bracketed}");
    assert_eq!(bracketed["working_orders"][0]["stop_loss"], "98");
    assert_eq!(bracketed["working_orders"][0]["take_profit"], "101");

    let refused = bracket_order(
        &mut window,
        &mut access,
        &trader,
        &json!({ "order_id": order_id, "stop_loss": "100" }),
    )
    .expect("the call dispatches");
    assert_eq!(refused["accepted"], false);
    assert!(
        refused["rejected_because"]
            .as_str()
            .is_some_and(|reason| reason.contains("stop loss")),
        "the reason teaches: {refused}"
    );
    assert_eq!(
        refused["working_orders"][0]["stop_loss"], "98",
        "a refusal changed nothing"
    );

    let cancelled = cancel_order(
        &mut window,
        &mut access,
        &trader,
        &json!({ "order_id": order_id }),
    )
    .expect("the call dispatches");
    assert_eq!(cancelled["accepted"], true, "{cancelled}");
    assert_eq!(cancelled["working_orders"], json!([]), "nothing works now");
}

/// Every placement journals who asked and what the venue answered, a
/// refusal included, so an operator's order is never mistaken for one the
/// trader placed by hand.
#[test]
fn a_placement_journals_its_actor_and_the_venues_answer() {
    let mut window = desk_window();
    let mut access = FakeAccess::with_budget(0);
    let trader = FakeWindow::trader();
    for price in ["99", "101"] {
        place_order(
            &mut window,
            &mut access,
            &trader,
            &json!({ "side": "buy", "kind": "limit", "quantity": "1", "price": price }),
        )
        .expect("the call dispatches");
    }

    let placed: Vec<_> = access
        .events
        .iter()
        .filter(|event| event.kind.as_str() == PLACE_EVENT_KIND)
        .collect();
    assert_eq!(placed.len(), 2, "the acceptance and the refusal");
    assert_eq!(placed[0].payload["accepted"], true);
    assert!(placed[0].payload["order_id"].is_u64());
    assert_eq!(placed[0].payload["simulated"], true);
    assert_eq!(
        placed[0].actor.as_ref().expect("an actor is recorded").kind,
        quantick_control::wire::ActorKind::HumanUi
    );
    assert_eq!(placed[1].payload["accepted"], false);
    assert!(
        placed[1].payload["rejected_because"]
            .as_str()
            .is_some_and(|reason| reason.contains("fill immediately")),
        "the venue's own words: {}",
        placed[1].payload
    );
    assert_eq!(placed[1].payload["asked"]["side"], "buy");
}

/// A desk on WIN$N marked at 140,000, with the money declared (0.20 BRL a
/// point), a 100 BRL risk per trade and the lock on.
fn locked_win_window() -> DeskWindow {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/trade_tests.rs/trades");
    let mut account = quantick_paper::PaperAccount::with_trades_dir(dir);
    account.set_symbol("WIN$N");
    account.seed(&quantick_engine::Trade {
        agg_id: 0,
        timestamp_ms: 0,
        price: Decimal::from(140_000),
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
    let money = quantick_sim::InstrumentMoney {
        point_value: Decimal::new(20, 2),
        size_step: Decimal::ONE,
        min_size: Decimal::ONE,
        max_size: None,
        currency: quantick_sim::Currency::new("BRL").expect("BRL"),
        source: quantick_sim::MoneySource::Declared,
    };
    account.set_instrument_money([("WIN$N".to_owned(), money)].into_iter().collect());
    account.set_risk_settings(quantick_paper::risk_sizing::RiskSettings {
        basis: quantick_paper::risk_sizing::RiskBasis::Amount,
        amount: Decimal::from(100),
        amount_currency: None,
        percent: Decimal::ZERO,
        lock: true,
    });
    DeskWindow {
        desk: PaperDesk {
            account,
            desk: quantick_paper::desk::Desk::default(),
        },
    }
}

/// The lock holds on the named path too: an order whose own stop risks
/// more than the budget is refused as an invalid request carrying the
/// sentence the ticket shows, and nothing reaches the venue. A ceiling the
/// ticket enforced and this call did not would stand only while a human
/// is clicking.
#[test]
fn the_lock_refuses_a_named_order_over_the_budget() {
    let mut window = locked_win_window();
    let mut access = FakeAccess::with_budget(0);
    // 4000 points x 0.20 x 10 = 8,000 BRL against a budget of 100.
    let same = OrderIntent::limit(Side::Buy, Decimal::from(10), Decimal::from(139_000))
        .with_bracket(Bracket::whole(Some(Decimal::from(135_000)), None));
    let sentence = window
        .desk
        .account
        .risk_refusal_for(&same)
        .expect("the lock names the over-budget order");
    assert!(sentence.contains("8000 BRL"), "{sentence}");
    assert!(sentence.contains("turn the lock off"), "{sentence}");

    let refusal = place_order(
        &mut window,
        &mut access,
        &FakeWindow::trader(),
        &json!({
            "side": "buy",
            "kind": "limit",
            "quantity": "10",
            "price": "139000",
            "stop_loss": "135000",
        }),
    )
    .expect_err("the lock refuses the call");
    assert_eq!(refusal, ControlError::invalid_request(sentence));
    assert!(
        window.desk.account.working_orders().is_empty(),
        "nothing rests at the venue"
    );
    assert!(window.desk.account.is_flat(), "nothing was filled");
}
