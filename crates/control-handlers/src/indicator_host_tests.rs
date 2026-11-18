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

fn ready_gate() -> NumericInputGate<u8> {
    NumericInputGate {
        script_open: true,
        settings_open: false,
        view: Some(Some(7)),
        source_errors: false,
        inputs_ready: true,
    }
}

#[test]
fn a_ready_numeric_input_gate_admits_its_view() {
    assert_eq!(ready_gate().admit(), Ok(7));
}

#[test]
fn a_numeric_input_gate_refuses_with_the_first_reason_that_holds() {
    use IndicatorRefusal as R;
    let blocked = NumericInputGate {
        script_open: false,
        settings_open: true,
        view: None,
        source_errors: true,
        inputs_ready: false,
    };
    assert_eq!(blocked.clone().admit(), Err(R::ScriptNotOpen));
    let blocked = NumericInputGate {
        script_open: true,
        ..blocked
    };
    assert_eq!(blocked.clone().admit(), Err(R::SettingsOpen));
    let blocked = NumericInputGate {
        settings_open: false,
        ..blocked
    };
    assert_eq!(blocked.clone().admit(), Err(R::NotAttached));
    let blocked = NumericInputGate {
        view: Some(None),
        ..blocked
    };
    assert_eq!(blocked.clone().admit(), Err(R::NotLoaded));
    let blocked = NumericInputGate {
        view: Some(Some(7)),
        ..blocked
    };
    assert_eq!(blocked.clone().admit(), Err(R::SourceErrors));
    let blocked = NumericInputGate {
        source_errors: false,
        ..blocked
    };
    assert_eq!(blocked.admit(), Err(R::NotValidated));
}
