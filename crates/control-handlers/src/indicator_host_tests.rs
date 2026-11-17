//! The family's refusals reach the wire with their codes and words.
use super::*;

#[test]
fn the_traders_own_slot_is_a_permission_refusal() {
    let error = ControlError::from(IndicatorRefusal::TradersOwn);
    assert_eq!(error.code.as_str(), codes::PERMISSION_DENIED);
    assert!(!error.retryable);
}

#[test]
fn a_lower_layers_refusal_keeps_its_words() {
    let error = ControlError::from(IndicatorRefusal::Rejected("limit must be in 1..=64".into()));
    assert_eq!(error.code.as_str(), codes::INVALID_REQUEST);
    assert_eq!(error.message, "limit must be in 1..=64");
}
