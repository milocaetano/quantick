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

fn gate(view: ScriptView<u8>) -> NumericInputGate<u8> {
    NumericInputGate {
        script_open: true,
        settings_open: false,
        diagnostics: false,
        view,
    }
}

fn loaded(stale: bool, inputs_ready: bool) -> ScriptView<u8> {
    ScriptView::Loaded {
        view: 7,
        stale,
        inputs_ready,
    }
}

#[test]
fn a_ready_numeric_input_gate_admits_its_view() {
    assert_eq!(gate(loaded(false, true)).admit(), Ok(7));
}

#[test]
fn a_numeric_input_gate_refuses_with_the_first_reason_that_holds() {
    use IndicatorRefusal as R;
    let worst = NumericInputGate {
        script_open: false,
        settings_open: true,
        diagnostics: true,
        view: ScriptView::<u8>::NotAttached,
    };
    assert_eq!(worst.clone().admit(), Err(R::ScriptNotOpen));
    let open = NumericInputGate {
        script_open: true,
        ..worst
    };
    assert_eq!(open.clone().admit(), Err(R::SettingsOpen));
    assert_eq!(gate(ScriptView::NotAttached).admit(), Err(R::NotAttached));
    assert_eq!(gate(ScriptView::NotLoaded).admit(), Err(R::NotLoaded));
    let diagnosed = NumericInputGate {
        diagnostics: true,
        ..gate(loaded(false, false))
    };
    assert_eq!(diagnosed.admit(), Err(R::SourceErrors));
    assert_eq!(
        gate(loaded(true, false)).admit(),
        Err(R::SourceErrors),
        "a stale reload is a source error too"
    );
    assert_eq!(gate(loaded(false, false)).admit(), Err(R::NotValidated));
}
