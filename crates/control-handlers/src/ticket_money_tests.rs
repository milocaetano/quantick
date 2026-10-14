//! The schema's inputs reach the account's edits field for field, and a
//! refusal comes back as an `invalid_request`.

use std::path::PathBuf;

use quantick_control::error::codes;
use serde_json::json;

use super::*;

fn risk_input(value: serde_json::Value) -> SetRiskInput {
    serde_json::from_value(value).expect("fixture input")
}

#[test]
fn a_risk_input_is_read_field_for_field() {
    let edit = risk_edit(&risk_input(json!({
        "basis": "percent",
        "percent": "1.5",
        "capital": "2000",
        "currency": "usd",
        "lock": true
    })))
    .expect("valid input");
    assert_eq!(edit.percent, Some(rust_decimal::Decimal::new(15, 1)));
    assert_eq!(edit.lock, Some(true));
    assert_eq!(
        edit.capital.as_ref().map(|(_, currency)| currency.code()),
        Some("USD")
    );
}

#[test]
fn a_refused_input_is_an_invalid_request() {
    let error =
        risk_edit(&risk_input(json!({ "basis": "amount", "amount": "-1" }))).expect_err("refused");
    assert_eq!(error.code.as_str(), codes::INVALID_REQUEST);
    assert_eq!(error.message, "amount must be above zero");

    let mut account = PaperAccount::with_trades_dir(PathBuf::from("no-such-trades-folder"));
    account.set_symbol("WINZ25");
    let input: SetInstrumentMoneyInput = serde_json::from_value(json!({
        "point_value": "x", "size_step": "1", "currency": "BRL"
    }))
    .expect("fixture input");
    let error = declare_money(&mut account, &input).expect_err("refused");
    assert_eq!(error.message, "point_value must be a decimal");
    assert!(account.instrument_money().is_empty());
}
