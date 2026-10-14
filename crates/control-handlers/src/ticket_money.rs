//! `trade.set_risk` and `trade.set_instrument_money`, read into the paper
//! account's own edits.
//!
//! The window keeps the dispatch, the fan-out and the journal; what a call's
//! input means — and every refusal it earns — is decided here and in
//! `quantick_paper`, so the capability and the ticket's fields share one
//! validation.

use quantick_control::error::ControlError;
use quantick_control_schema::trade::{SetInstrumentMoneyInput, SetRiskInput};
use quantick_paper::account::{PaperAccount, RiskEdit, RiskEditText};

/// The risk change a `trade.set_risk` input asks for, refused before any
/// chart is looked at.
///
/// # Errors
///
/// `invalid_request` with the account's sentence for any refused field.
pub fn risk_edit(input: &SetRiskInput) -> Result<RiskEdit, ControlError> {
    RiskEdit::parse(RiskEditText {
        basis: &input.basis,
        amount: input.amount.as_deref(),
        percent: input.percent.as_deref(),
        capital: input.capital.as_deref(),
        currency: input.currency.as_deref(),
        lock: input.lock,
    })
    .map_err(ControlError::invalid_request)
}

/// Apply a `trade.set_instrument_money` input to `account`.
///
/// # Errors
///
/// `invalid_request` with the account's sentence; a refusal changes nothing.
pub fn declare_money(
    account: &mut PaperAccount,
    input: &SetInstrumentMoneyInput,
) -> Result<(), ControlError> {
    account
        .declare_instrument_money(
            input.symbol.as_deref(),
            input.point_value.as_deref(),
            input.size_step.as_deref(),
            input.currency.as_deref(),
        )
        .map_err(ControlError::invalid_request)
}

#[cfg(test)]
#[path = "ticket_money_tests.rs"]
mod ticket_money_tests;
