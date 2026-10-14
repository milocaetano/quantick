//! The named call's money edits: a refusal changes nothing, a change lands
//! on the account exactly as the ticket's fields would land it.

use std::path::PathBuf;

use quantick_trading::{Currency, InstrumentMoney, MoneySource};
use rust_decimal::Decimal;

use super::*;

fn account(symbol: &str) -> PaperAccount {
    let mut account = PaperAccount::with_trades_dir(PathBuf::from("no-such-trades-folder"));
    account.set_symbol(symbol);
    account
}

fn dec(text: &str) -> Decimal {
    Decimal::from_str_exact(text).expect("fixture decimal")
}

fn text(basis: &str) -> RiskEditText<'_> {
    RiskEditText {
        basis,
        ..RiskEditText::default()
    }
}

#[test]
fn a_risk_edit_refuses_what_the_ticket_refuses() {
    let refused = |text: RiskEditText<'_>| RiskEdit::parse(text).expect_err("refused");
    assert_eq!(
        refused(text("sometimes")),
        "basis must be `off`, `amount` or `percent`"
    );
    assert_eq!(
        refused(RiskEditText {
            amount: Some("ten"),
            ..text("amount")
        }),
        "amount must be a decimal"
    );
    assert_eq!(
        refused(RiskEditText {
            percent: Some("0"),
            ..text("percent")
        }),
        "percent must be above zero"
    );
    assert!(
        refused(RiskEditText {
            capital: Some("1000"),
            ..text("off")
        })
        .starts_with("capital and currency go together")
    );
    assert_eq!(
        refused(RiskEditText {
            capital: Some("1000"),
            currency: Some("  "),
            ..text("off")
        }),
        "currency must not be blank"
    );
}

#[test]
fn a_risk_edit_stamps_its_amount_and_declares_its_capital() {
    let mut account = account("BTCUSDT");
    let edit = RiskEdit::parse(RiskEditText {
        amount: Some(" 50 "),
        capital: Some("1000"),
        currency: Some("brl"),
        lock: Some(false),
        ..text("amount")
    })
    .expect("valid edit");
    account.apply_risk_edit(edit);
    let risk = account.risk_settings();
    assert_eq!(risk.basis, RiskBasis::Amount);
    assert_eq!(risk.amount, dec("50"));
    assert_eq!(risk.amount_currency, Currency::new("BRL"));
    assert!(!risk.lock);
    assert_eq!(account.capital().get("BRL"), Some(&dec("1000")));

    let clear = RiskEdit::parse(RiskEditText {
        capital: Some("0"),
        currency: Some("BRL"),
        ..text("off")
    })
    .expect("valid edit");
    account.apply_risk_edit(clear);
    assert!(account.capital().is_empty(), "a zero capital clears it");
    assert_eq!(account.risk_settings().amount, dec("50"), "kept");
}

#[test]
fn a_declaration_lands_keeps_the_size_limits_and_clears_when_half_given() {
    let mut account = account("WINZ25");
    let mut seeded = account.instrument_money().clone();
    seeded.insert(
        "WINZ25".to_owned(),
        InstrumentMoney {
            point_value: dec("1"),
            size_step: dec("1"),
            min_size: dec("2"),
            max_size: Some(dec("9")),
            currency: Currency::new("BRL").expect("code"),
            source: MoneySource::Declared,
        },
    );
    account.set_instrument_money(seeded);
    account
        .declare_instrument_money(None, Some("0.20"), Some("1"), Some("brl"))
        .expect("declared");
    let money = &account.instrument_money()["WINZ25"];
    assert_eq!(money.point_value, dec("0.20"));
    assert_eq!(money.min_size, dec("2"));
    assert_eq!(money.max_size, Some(dec("9")));

    let before = account.instrument_money().clone();
    assert_eq!(
        account.declare_instrument_money(Some("WINZ25"), Some("0"), Some("1"), Some("BRL")),
        Err("point_value and size_step must both be above zero".to_owned())
    );
    assert_eq!(
        account.instrument_money(),
        &before,
        "a refusal changes nothing"
    );

    account
        .declare_instrument_money(Some(" WINZ25 "), Some("0.20"), None, None)
        .expect("cleared");
    assert!(account.instrument_money().is_empty());
}

#[test]
fn a_declaration_needs_a_symbol() {
    let mut account = account("");
    assert_eq!(
        account.declare_instrument_money(Some(" "), None, None, None),
        Err("no chart symbol to declare money for - name one".to_owned())
    );
}
