//! The named trade calls, decided where the tab decides them: what each
//! input states, and what the desk answers. The control plane only decodes
//! and journals (`quantick_control_handlers::trade`).

use std::path::PathBuf;

use quantick_engine::{Side, Trade};
use quantick_paper::PaperAccount;
use quantick_sim::{Bracket, OrderId, OrderIntent};
use rust_decimal::Decimal;
use serde_json::{Value, json};

use super::*;
use crate::paper::{NOTHING_TO_REVERSE, TabPaper, TradeRefusal};

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// A desk marked at 100 that journals nowhere.
fn marked() -> TabPaper {
    let mut paper = TabPaper::new(PaperAccount::with_trades_dir(PathBuf::new()));
    paper.seed(&print(0, 100));
    paper
}

fn command<C: TradeCall>(input: Value) -> Result<TradeCommand, String> {
    serde_json::from_value::<C>(input)
        .expect("the input decodes")
        .command()
}

/// Run one call on `paper` and answer as the wire does.
fn call<C: TradeCall>(paper: &mut TabPaper, input: Value) -> TradeResult {
    let outcome = paper.execute(command::<C>(input).expect("states an order"));
    assert_eq!(outcome.refusal, None, "the tab refused it");
    TradeResult::of(&paper.account, paper.ruler_ticks(), &outcome.events)
}

/// C1: a named call states the very command a click sends, so the two run
/// one decision in `TabPaper::execute`.
#[test]
fn each_call_states_the_command_a_click_sends() {
    let limit = OrderIntent::limit(Side::Buy, Decimal::from(2), Decimal::from(99))
        .with_bracket(Bracket::whole(Some(Decimal::from(95)), None));
    assert_eq!(
        command::<PlaceInput>(
            json!({ "side": "buy", "kind": "limit", "quantity": "2", "price": " 99 ", "stop_loss": "95" })
        ),
        Ok(TradeCommand::Place {
            intent: Box::new(limit)
        })
    );
    assert_eq!(
        command::<PlaceInput>(json!({ "side": "sell", "kind": "market", "quantity": "1" })),
        Ok(TradeCommand::Place {
            intent: Box::new(OrderIntent::market(Side::Sell, Decimal::ONE))
        })
    );
    assert_eq!(
        command::<BracketInput>(json!({ "order_id": 3, "take_profit": "110" })),
        Ok(TradeCommand::Amend {
            id: OrderId(3),
            bracket: Box::new(Bracket::whole(None, Some(Decimal::from(110)))),
        })
    );
    assert_eq!(
        command::<CancelInput>(json!({ "order_id": 3 })),
        Ok(TradeCommand::Cancel { id: OrderId(3) })
    );
    assert_eq!(
        command::<FlattenInput>(json!({})),
        Ok(TradeCommand::Flatten)
    );
    assert_eq!(
        command::<ReverseInput>(json!({})),
        Ok(TradeCommand::Reverse)
    );
}

/// An input that states no order says why, naming the field.
#[test]
fn an_input_that_states_no_order_is_refused_with_the_reason() {
    let market_with_price =
        json!({ "side": "buy", "kind": "market", "quantity": "1", "price": "99" });
    assert!(
        command::<PlaceInput>(market_with_price)
            .is_err_and(|reason| reason.contains("a market order has no price"))
    );
    let resting_without_price = json!({ "side": "buy", "kind": "stop", "quantity": "1" });
    assert_eq!(
        command::<PlaceInput>(resting_without_price),
        Err("a limit or stop needs the price it rests at".to_owned())
    );
    let bad_quantity = json!({ "side": "buy", "kind": "market", "quantity": "two" });
    assert!(
        command::<PlaceInput>(bad_quantity).is_err_and(|reason| reason.starts_with("quantity: "))
    );
    let bad_leg = json!({ "order_id": 1, "stop_loss": "x" });
    assert!(
        command::<BracketInput>(bad_leg).is_err_and(|reason| reason.starts_with("stop_loss: "))
    );
    assert!(
        serde_json::from_value::<FlattenInput>(json!({ "size": "1" })).is_err(),
        "a position call takes nothing"
    );
}

/// The two position inputs are one wire shape under the published name.
#[test]
fn both_position_calls_publish_one_input_shape() {
    let flatten = serde_json::to_value(schemars::schema_for!(FlattenInput)).unwrap();
    let reverse = serde_json::to_value(schemars::schema_for!(ReverseInput)).unwrap();
    assert_eq!(flatten, reverse);
    assert_eq!(flatten["title"], "PositionInput");
}

/// An order is placed, bracketed, refused and cancelled by name, and every
/// answer reads the desk back; a refusal is the venue's own words and
/// changes nothing.
#[test]
fn the_answer_reads_the_desk_back_after_every_call() {
    let mut paper = marked();
    let placed = call::<PlaceInput>(
        &mut paper,
        json!({ "side": "buy", "kind": "limit", "quantity": "2", "price": "99" }),
    );
    assert!(placed.accepted && placed.simulated);
    assert_eq!(placed.mark_price.as_deref(), Some("100"));
    assert_eq!(placed.working_orders[0].kind, "limit");
    assert_eq!(placed.working_orders[0].side, "buy");
    assert_eq!(placed.working_orders[0].quantity, "2");
    let id = placed.order_id.expect("the venue named it");

    let bracketed = call::<BracketInput>(
        &mut paper,
        json!({ "order_id": id, "stop_loss": "98", "take_profit": "101" }),
    );
    assert!(bracketed.accepted);
    assert_eq!(bracketed.working_orders[0].stop_loss.as_deref(), Some("98"));
    assert_eq!(
        bracketed.working_orders[0].take_profit.as_deref(),
        Some("101")
    );

    let refused = call::<BracketInput>(&mut paper, json!({ "order_id": id, "stop_loss": "100" }));
    assert!(!refused.accepted);
    assert!(
        refused
            .rejected_because
            .as_deref()
            .is_some_and(|reason| reason.contains("stop loss")),
        "{refused:?}"
    );
    assert_eq!(
        refused.working_orders[0].stop_loss.as_deref(),
        Some("98"),
        "a refusal changed nothing"
    );

    let through = call::<PlaceInput>(
        &mut paper,
        json!({ "side": "buy", "kind": "limit", "quantity": "1", "price": "101" }),
    );
    assert!(
        through
            .rejected_because
            .as_deref()
            .is_some_and(|reason| reason.contains("fill immediately")),
        "{through:?}"
    );

    let cancelled = call::<CancelInput>(&mut paper, json!({ "order_id": id }));
    assert!(cancelled.accepted);
    assert_eq!(cancelled.order_id, Some(id));
    assert!(cancelled.working_orders.is_empty());
}

/// C2: flatten takes a position and a resting order to nothing in one call;
/// reverse flips a long of 2 into a short of 2, and is refused while flat.
#[test]
fn the_position_calls_go_flat_and_flip() {
    let mut paper = marked();
    let refused = paper.execute(command::<ReverseInput>(json!({})).unwrap());
    assert_eq!(
        refused.refusal,
        Some(TradeRefusal::Ticket(NOTHING_TO_REVERSE.to_owned()))
    );

    call::<PlaceInput>(
        &mut paper,
        json!({ "side": "buy", "kind": "market", "quantity": "2" }),
    );
    paper.on_trade(&print(1, 100));
    let long = paper.position_summary().expect("long");
    let reversed = call::<ReverseInput>(&mut paper, json!({}));
    assert!(reversed.accepted);
    paper.on_trade(&print(2, 100));
    let short = paper.position_summary().expect("short");
    assert_ne!(short.side, long.side);
    assert_eq!(short.quantity, Decimal::from(2));

    call::<PlaceInput>(
        &mut paper,
        json!({ "side": "buy", "kind": "limit", "quantity": "1", "price": "95" }),
    );
    let flat = call::<FlattenInput>(&mut paper, json!({}));
    assert!(flat.accepted);
    assert!(flat.working_orders.is_empty());
    paper.on_trade(&print(3, 100));
    assert!(paper.is_flat(), "the exit filled at the next print");
}

/// The lock holds on the named path too: an order whose own stop risks more
/// than the budget is refused with the ticket's sentence before the venue.
#[test]
fn the_lock_refuses_a_named_order_over_the_budget() {
    let mut account = PaperAccount::with_trades_dir(PathBuf::new());
    account.set_symbol("WIN$N");
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
    let mut paper = TabPaper::new(account);
    paper.seed(&print(0, 140_000));
    let named = command::<PlaceInput>(json!({
        "side": "buy", "kind": "limit", "quantity": "10",
        "price": "139000", "stop_loss": "135000",
    }))
    .unwrap();
    // 4000 points x 0.20 x 10 = 8,000 BRL against a budget of 100.
    let TradeCommand::Place { intent } = named.clone() else {
        unreachable!("a placement states a place command")
    };
    let expected = paper.account.risk_refusal_for(&intent);
    let outcome = paper.execute(named);
    let Some(TradeRefusal::Ticket(sentence)) = outcome.refusal else {
        panic!("the lock refuses: {outcome:?}");
    };
    assert!(sentence.contains("8000 BRL"), "{sentence}");
    assert!(sentence.contains("turn the lock off"), "{sentence}");
    assert_eq!(Some(sentence), expected, "the ticket's own sentence");
    assert!(outcome.events.is_empty());
    assert!(paper.working_orders().is_empty() && paper.is_flat());
}
