// The risk-sizing half of the `paper_trading.rs` unit tests, moved out
// beside the rest of them. The `_tests` suffix is load-bearing: a module
// named `risk` here would shadow a real crate module inside every file
// that globs this one.

use super::*;

fn win_money() -> quantick_sim::InstrumentMoney {
    quantick_sim::InstrumentMoney {
        point_value: Decimal::new(20, 2),
        size_step: Decimal::ONE,
        min_size: Decimal::ONE,
        max_size: None,
        currency: quantick_sim::Currency::new("BRL").expect("BRL"),
        source: quantick_sim::MoneySource::Declared,
    }
}

fn book(money: quantick_sim::InstrumentMoney) -> crate::risk_sizing::InstrumentBook {
    [("WIN$N".to_owned(), money)].into_iter().collect()
}

/// A ticket on WIN$N with the money declared and a fixed risk set.
fn armed_ticket(price: i64) -> PaperTrading {
    let mut paper = PaperTrading::new();
    paper.set_symbol("WIN$N");
    paper.seed(&Trade {
        agg_id: 0,
        timestamp_ms: 0,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
    paper.account_mut().set_instrument_money(book(win_money()));
    paper
        .account_mut()
        .set_risk_settings(crate::risk_sizing::RiskSettings {
            basis: crate::risk_sizing::RiskBasis::Amount,
            amount: Decimal::from(100),
            amount_currency: None,
            percent: Decimal::ZERO,
            lock: true,
        });
    paper.set_ruler_step(Some(Decimal::ONE));
    paper
}

/// The wheel and a saved strategy are two ways of saying where the stop
/// is, and the size must not depend on which the trader used - that is
/// the whole reason both go through `aim_bracket`. Proven by sizing one
/// entry each way and comparing, not by reading the funnel.
#[test]
fn the_wheel_and_a_saved_strategy_size_one_entry_identically() {
    let stop_points = 10_u32;

    let mut wheel = armed_ticket(140_000);
    wheel.set_ruler_ticks(stop_points);
    let by_wheel = wheel.risk_state(Side::Buy, Decimal::from(140_000));

    let mut ladder = armed_ticket(140_000);
    ladder.account_mut().set_order_strategies(
        vec![crate::order_strategies::OrderStrategy {
            name: "one rung".to_owned(),
            rows: vec![crate::order_strategies::StrategyRow {
                share_percent: Decimal::ONE_HUNDRED,
                gain_ticks: Some(stop_points),
                loss_ticks: Some(stop_points),
            }],
        }],
        Some("one rung"),
    );
    let by_ladder = ladder.risk_state(Side::Buy, Decimal::from(140_000));

    assert_eq!(
        by_wheel.derived_quantity(),
        by_ladder.derived_quantity(),
        "wheel {by_wheel:?} vs ladder {by_ladder:?}"
    );
    assert_eq!(by_wheel.code(), by_ladder.code());
    // 10 points x 0.20 = 2.00 a contract; 100 / 2 = 50.
    assert_eq!(by_wheel.derived_quantity(), Some(Decimal::from(50)));
}

/// The lock, at the surface. With a risk per trade set there is no entry
/// that exceeds it, and the refusal names the number rather than leaving
/// a wheel that quietly stopped turning.
#[test]
fn a_stop_too_wide_for_the_budget_refuses_the_entry_and_says_why() {
    let mut paper = armed_ticket(140_000);
    // 4000 points x 0.20 = 800 for one contract, against a budget of 100.
    // Reached with a coarse step rather than more notches: the ruler
    // itself stops at `RULER_MAX_NOTCHES`.
    paper.set_ruler_step(Some(Decimal::from(20)));
    paper.set_ruler_ticks(200);
    let (state, blocks) = paper.risk_report();
    assert_eq!(state.code(), "clamped_at_minimum", "{state:?}");
    assert!(blocks, "the lock stands");
    let sentence = state.sentence();
    assert!(sentence.contains("800 BRL"), "{sentence}");
    assert!(sentence.contains("raise the risk"), "{sentence}");

    // And it is the *placement* that is refused, not merely the label.
    paper.market(Side::Buy);
    assert!(
        paper.is_flat(),
        "the lock refused the order rather than only colouring the ticket"
    );
}

/// The ceiling holds on the named path too. A lock the ticket enforces
/// and `place_intent` does not would be a ceiling that stands only while
/// a human is clicking - and `CLAUDE.md` makes the other operator
/// first-class.
#[test]
fn the_lock_refuses_a_named_order_the_same_as_a_clicked_one() {
    let mut paper = armed_ticket(140_000);
    let intent = quantick_sim::OrderIntent::market(Side::Buy, Decimal::from(10))
        .with_bracket(Bracket::whole(Some(Decimal::from(136_000)), None));
    // 4000 points x 0.20 x 10 = 8,000 BRL against a budget of 100.
    let refusal = paper
        .account
        .risk_refusal_for(&intent)
        .expect("the ceiling names it");
    assert!(refusal.contains("8000 BRL"), "{refusal}");
    assert!(refusal.contains("turn the lock off"), "{refusal}");
    assert!(
        paper.account.place_intent(intent).is_empty(),
        "a named call must not slip past the ceiling the ticket enforces"
    );
    assert!(paper.is_flat(), "nothing was placed");
}

/// With the lock down the same named order goes out - the refusal is the
/// trader's setting, not a rule the platform invented.
#[test]
fn a_named_order_over_budget_goes_out_once_the_lock_is_down() {
    let mut paper = armed_ticket(140_000);
    let mut risk = paper.account().risk_settings().clone();
    risk.lock = false;
    paper.account_mut().set_risk_settings(risk);
    let intent = quantick_sim::OrderIntent::market(Side::Buy, Decimal::from(10))
        .with_bracket(Bracket::whole(Some(Decimal::from(136_000)), None));
    assert!(paper.account.risk_refusal_for(&intent).is_none());
}

/// Taking the lock down is how a trader takes that entry anyway - a
/// deliberate act, never a silent one.
#[test]
fn taking_the_lock_down_lets_the_over_budget_entry_through() {
    let mut paper = armed_ticket(140_000);
    paper.set_ruler_step(Some(Decimal::from(20)));
    paper.set_ruler_ticks(200);
    let mut risk = paper.account().risk_settings().clone();
    risk.lock = false;
    paper.account_mut().set_risk_settings(risk);
    let (state, blocks) = paper.risk_report();
    assert_eq!(state.code(), "clamped_at_minimum", "still over budget");
    assert!(!blocks, "but no longer refused");
}

/// An instrument with no declared money never guesses a size. In
/// particular it must not borrow this file's `tick_size`, which is
/// derived from the decimal places the tape has printed and says 1 for
/// WIN$N where the real step is 5.
#[test]
fn an_undeclared_instrument_sizes_nothing_and_says_so() {
    let mut paper = armed_ticket(140_000);
    paper
        .account_mut()
        .set_instrument_money(crate::risk_sizing::InstrumentBook::new());
    paper.set_ruler_ticks(10);
    let (state, blocks) = paper.risk_report();
    assert_eq!(state.code(), "instrument_unknown");
    assert_eq!(state.derived_quantity(), None);
    assert!(!blocks, "an unknown instrument is not an over-budget one");
    assert!(state.sentence().contains("WIN$N"), "{}", state.sentence());
}

/// The steppers walk the instrument's own size step. A hard-coded 1 is
/// already wrong on any fractional lot - it moved a crypto size by a
/// hundred thousand steps.
#[test]
fn the_quantity_steppers_walk_the_instruments_own_size_step() {
    let mut paper = armed_ticket(140_000);
    paper
        .account_mut()
        .set_instrument_money(book(quantick_sim::InstrumentMoney {
            size_step: Decimal::new(1, 5),
            min_size: Decimal::new(1, 5),
            ..win_money()
        }));
    paper.qty_text = "0.00002".to_owned();
    paper.step_quantity(Decimal::ONE);
    assert_eq!(paper.qty_text, "0.00003");
    paper.step_quantity(-Decimal::ONE);
    assert_eq!(paper.qty_text, "0.00002");
}
