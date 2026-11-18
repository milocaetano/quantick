use quantick_control_host::actions::ActionRegistry;
use quantick_control_host::authority::TRADE_EFFECT_ID;
use serde_json::json;

use quantick_session::paper::TabPaper;

use super::*;
use crate::test_support::{FakeAccess, FakeWindow, NoDesk, PaperDesk};

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

/// A window whose active tab holds `desk`, and every shared setting a call
/// asked it to persist.
struct DeskWindow {
    desk: PaperDesk,
    persisted: Vec<SharedChange>,
}

impl PaperOwner for DeskWindow {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        Some(&self.desk)
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        Some(&mut self.desk)
    }
    fn persist(&mut self, change: SharedChange) {
        self.persisted.push(change);
    }
    fn journal_scope(&self) -> JournalScope {
        NoDesk.journal_scope()
    }
    fn journal_timezone(&self) -> quantick_civil::TzOffset {
        NoDesk.journal_timezone()
    }
    fn edit_journal(&mut self, edit: &JournalEdit) -> Result<JournalEditOutcome, SessionEditError> {
        NoDesk.edit_journal(edit)
    }
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
        desk: PaperDesk::new(account),
        persisted: Vec::new(),
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
        desk: PaperDesk::new(account),
        persisted: Vec::new(),
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

/// One print at `price`, the tape a fill needs.
fn print(agg_id: u64, price: i64) -> quantick_engine::Trade {
    quantick_engine::Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small id") * 1_000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// Everything a trader or an operator can read back from one desk.
fn readback(paper: &TabPaper) -> String {
    format!(
        "{:?} | {:?} | {:?} | {:?} | ruler {}",
        paper.position_summary(),
        paper.working_orders(),
        paper.session_trades(),
        paper.account.peek_toast(),
        paper.ruler_ticks(),
    )
}

/// C1: the tab's own surfaces and the named call run one decision. The same
/// commands, sent to one desk the way a click sends them and to an identical
/// desk through the control handlers, leave the same account, the same
/// working orders and the same venue events behind, step by step.
#[test]
fn a_trade_command_lands_the_same_through_a_click_and_a_named_call() {
    let mut window = desk_window();
    let mut ui = desk_window().desk.paper;
    let mut access = FakeAccess::with_budget(0);
    let limit = OrderIntent::limit(Side::Buy, Decimal::from(2), Decimal::from(99));
    let market = OrderIntent::market(Side::Buy, Decimal::ONE);
    let protect = Bracket::whole(Some(Decimal::from(95)), Some(Decimal::from(110)));

    type NamedCall = Box<dyn Fn(&mut DeskWindow, &mut FakeAccess)>;
    let mut steps: Vec<(TradeCommand, NamedCall)> = vec![
        (
            TradeCommand::Place {
                intent: Box::new(limit),
            },
            Box::new(|w, a| {
                place_order(
                    w,
                    a,
                    &FakeWindow::trader(),
                    &json!({ "side": "buy", "kind": "limit", "quantity": "2", "price": "99" }),
                )
                .expect("placed");
            }),
        ),
        (
            TradeCommand::Amend {
                id: OrderId(1),
                bracket: Box::new(protect),
            },
            Box::new(|w, a| {
                bracket_order(
                    w,
                    a,
                    &FakeWindow::trader(),
                    &json!({ "order_id": 1, "stop_loss": "95", "take_profit": "110" }),
                )
                .expect("bracketed");
            }),
        ),
        (
            TradeCommand::Cancel { id: OrderId(1) },
            Box::new(|w, a| {
                cancel_order(w, a, &FakeWindow::trader(), &json!({ "order_id": 1 }))
                    .expect("cancelled");
            }),
        ),
        (
            TradeCommand::Place {
                intent: Box::new(market),
            },
            Box::new(|w, a| {
                place_order(
                    w,
                    a,
                    &FakeWindow::trader(),
                    &json!({ "side": "buy", "kind": "market", "quantity": "1" }),
                )
                .expect("placed");
            }),
        ),
        (
            TradeCommand::Reverse,
            Box::new(|w, a| {
                reverse_position(w, a, &FakeWindow::trader(), &json!({})).expect("reversed");
            }),
        ),
        (
            TradeCommand::Flatten,
            Box::new(|w, a| {
                flatten_position(w, a, &FakeWindow::trader(), &json!({})).expect("flattened");
            }),
        ),
    ];
    for (index, (command, named_call)) in steps.drain(..).enumerate() {
        let clicked = ui.execute(command.clone());
        named_call(&mut window, &mut access);
        let called = window
            .desk
            .outcomes
            .pop()
            .expect("the handler ran the command");
        assert_eq!(
            clicked, called,
            "step {index}: {command:?} answered the same"
        );
        // A print after every step, so a market order fills on both desks.
        let tape = print(u64::try_from(index).expect("small") + 1, 100);
        ui.on_trade(&tape);
        window.desk.on_trade(&tape);
        assert_eq!(
            readback(&ui),
            readback(&window.desk),
            "step {index}: {command:?} left the same desk behind"
        );
        if index == 1 {
            let amended = ui.working_orders().first().map(|order| order.bracket);
            assert_eq!(amended, Some(protect), "the amendment found order 1");
        }
    }
    assert!(ui.is_flat(), "the sequence ends flat");
    assert!(ui.working_orders().is_empty(), "and with nothing working");
}

/// C2: one call goes flat. A long position and a resting order become no
/// position and nothing working; the call journals who asked and the venue's
/// answer, as a placement does.
#[test]
fn flatten_goes_flat_with_nothing_working_in_one_call() {
    let mut window = desk_window();
    let mut access = FakeAccess::with_budget(0);
    let trader = FakeWindow::trader();
    for input in [
        json!({ "side": "buy", "kind": "market", "quantity": "2" }),
        json!({ "side": "buy", "kind": "limit", "quantity": "1", "price": "95" }),
    ] {
        place_order(&mut window, &mut access, &trader, &input).expect("placed");
    }
    window.desk.on_trade(&print(1, 100));
    assert!(!window.desk.is_flat(), "the market order filled");
    assert_eq!(window.desk.working_orders().len(), 1, "the limit rests");

    let flat = flatten_position(&mut window, &mut access, &trader, &json!({}))
        .expect("the call dispatches");
    assert_eq!(flat["accepted"], true, "{flat}");
    assert_eq!(flat["working_orders"], json!([]), "nothing works now");
    window.desk.on_trade(&print(2, 100));
    assert!(window.desk.is_flat(), "the exit filled at the next print");
    assert!(window.desk.working_orders().is_empty());
    let journalled = access
        .events
        .iter()
        .filter(|event| event.kind.as_str() == FLATTEN_EVENT_KIND)
        .count();
    assert_eq!(journalled, 1, "the flatten journals once");
}

/// C2: one call reverses. A long of 2 becomes a short of 2 at the next
/// print; while flat the call is refused, before it journals, because a
/// reversal of nothing is a caller working from a stale read.
#[test]
fn reverse_flips_the_position_in_one_call_and_refuses_while_flat() {
    let mut window = desk_window();
    let mut access = FakeAccess::with_budget(0);
    let trader = FakeWindow::trader();
    let flat = reverse_position(&mut window, &mut access, &trader, &json!({}))
        .expect_err("nothing to reverse");
    assert_eq!(
        flat,
        ControlError::invalid_request(quantick_session::paper::NOTHING_TO_REVERSE.to_owned())
    );
    assert!(
        access.events.is_empty(),
        "a refusal before the desk records nothing"
    );

    place_order(
        &mut window,
        &mut access,
        &trader,
        &json!({ "side": "buy", "kind": "market", "quantity": "2" }),
    )
    .expect("placed");
    window.desk.on_trade(&print(1, 100));
    let long = window.desk.position_summary().expect("long");
    assert_eq!(long.quantity, Decimal::from(2));

    let reversed = reverse_position(&mut window, &mut access, &trader, &json!({}))
        .expect("the call dispatches");
    assert_eq!(reversed["accepted"], true, "{reversed}");
    window.desk.on_trade(&print(2, 100));
    let short = window.desk.position_summary().expect("short");
    assert_ne!(short.side, long.side, "the side flipped");
    assert_eq!(short.quantity, Decimal::from(2), "the size held");
    assert!(
        access
            .events
            .iter()
            .any(|event| event.kind.as_str() == REVERSE_EVENT_KIND),
        "the reversal journals"
    );
}

/// A named entry on a paused desk is refused as unavailable for now, with
/// the reading in the details and nothing sent or journaled; a named exit
/// still lands.
#[test]
fn a_paused_desk_refuses_a_named_entry_as_retryable_and_still_flattens() {
    use quantick_sources::sync::{Desync, LinkState, SyncLimits, SyncReading};
    let mut window = desk_window();
    let mut access = FakeAccess::with_budget(0);
    let trader = FakeWindow::trader();
    window.desk.set_trade_gate(Some(Desync {
        reading: SyncReading::Link,
        link: LinkState::Offline,
        latency_ms: None,
        latency_hop: None,
        tape_age_ms: Some(12_000),
        limits: SyncLimits::default(),
        notice: None,
    }));
    let refused = place_order(
        &mut window,
        &mut access,
        &trader,
        &json!({ "side": "buy", "kind": "market", "quantity": "1" }),
    )
    .expect_err("an entry waits for the feed");
    assert_eq!(
        refused.code.as_str(),
        quantick_control::error::codes::CAPABILITY_UNAVAILABLE
    );
    assert!(refused.retryable, "nothing was sent, so a retry is safe");
    assert!(
        refused.message.contains("the feed is offline"),
        "{refused:?}"
    );
    let details = refused.context.details.expect("the reading is named");
    assert_eq!(details["trade_gate"], "link");
    assert_eq!(details["link"], "offline");
    assert!(access.events.is_empty(), "nothing journaled");
    assert!(window.desk.working_orders().is_empty(), "nothing sent");

    let flattened = flatten_position(&mut window, &mut access, &trader, &json!({}))
        .expect("an exit is never paused");
    assert_eq!(flattened["accepted"], true, "{flattened}");
}

/// A ticket edit persists the one shared setting it changed — the exit
/// strategies or the risk settings — and the ruler and an order persist
/// nothing.
#[test]
fn a_ticket_edit_persists_the_setting_it_changed_and_an_order_nothing() {
    let mut window = desk_window();
    let mut access = FakeAccess::with_budget(0);
    let trader = FakeWindow::trader();
    select_strategy(&mut window, &mut access, &trader, &json!({})).expect("selected");
    set_risk(
        &mut window,
        &mut access,
        &trader,
        &json!({ "basis": "off" }),
    )
    .expect("risk");
    set_ruler(&mut window, &mut access, &trader, &json!({ "ticks": 4 })).expect("ruler");
    place_order(
        &mut window,
        &mut access,
        &trader,
        &json!({ "side": "buy", "kind": "market", "quantity": "1" }),
    )
    .expect("placed");
    assert_eq!(
        window.persisted,
        [SharedChange::OrderStrategies, SharedChange::RiskSettings]
    );
}
