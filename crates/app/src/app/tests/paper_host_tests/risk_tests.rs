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
    paper.desk.ticket.qty_text = "0.00002".to_owned();
    paper.step_quantity(Decimal::ONE);
    assert_eq!(paper.desk.ticket.qty_text, "0.00003");
    paper.step_quantity(-Decimal::ONE);
    assert_eq!(paper.desk.ticket.qty_text, "0.00002");
}
