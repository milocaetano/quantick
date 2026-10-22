//! The fold rule and `orderflow.tape.fold.set` through a fake pane: the rule
//! decides before anything is kept, and a refused call keeps nothing.

use super::*;
use serde_json::json;

struct Pane {
    facts: Result<TapeFoldPane, ControlError>,
    fold: TapeFold,
    sets: usize,
}

impl TapeFoldPort for Pane {
    fn tape_fold_pane(&mut self, _tab: u64, _pane: u64) -> Result<TapeFoldPane, ControlError> {
        self.facts.clone()
    }
    fn set_tape_fold(
        &mut self,
        _tab: u64,
        _pane: u64,
        fold: TapeFold,
    ) -> Result<bool, ControlError> {
        self.sets += 1;
        let changed = self.fold != fold;
        self.fold = fold;
        Ok(changed)
    }
}

const BESIDE: TapeFoldPane = TapeFoldPane {
    tape_on: true,
    tape_only: false,
};

fn pane(facts: TapeFoldPane) -> Pane {
    Pane {
        facts: Ok(facts),
        fold: TapeFold::Open,
        sets: 0,
    }
}

fn call(pane: &mut Pane, fold: Value) -> Result<Value, ControlError> {
    let actor = crate::test_support::FakeWindow::assistant();
    let input = json!({"tab_id": "1", "pane_id": "0", "fold": fold});
    set(pane, &mut (), &actor, &input)
}

/// Either side folds beside the candles; repeating a fold changes nothing
/// more, and open always returns the lane.
#[test]
fn a_side_folds_beside_the_candles_and_open_always_unfolds() {
    let mut beside = pane(BESIDE);
    let folded = call(&mut beside, json!("candles")).unwrap();
    assert_eq!(folded["fold"], "candles");
    assert_eq!(folded["changed"], true);
    assert_eq!(
        call(&mut beside, json!("candles")).unwrap()["changed"],
        false
    );
    assert_eq!(call(&mut beside, json!("tape")).unwrap()["changed"], true);
    assert_eq!(beside.fold, TapeFold::Tape);

    for facts in [
        TapeFoldPane {
            tape_on: false,
            ..BESIDE
        },
        TapeFoldPane {
            tape_only: true,
            ..BESIDE
        },
    ] {
        beside.facts = Ok(facts);
        let opened = call(&mut beside, json!("open")).unwrap();
        assert_eq!(opened["fold"], "open", "{facts:?}");
        beside.fold = TapeFold::Tape;
    }
}

/// A fold without a divider to fold is refused by name, before the pane
/// keeps anything; malformed input and a missing pane are refused too.
#[test]
fn folding_without_a_lane_beside_the_candles_is_refused_by_name() {
    for facts in [
        TapeFoldPane {
            tape_on: false,
            ..BESIDE
        },
        TapeFoldPane {
            tape_only: true,
            ..BESIDE
        },
    ] {
        let mut refused = pane(facts);
        for fold in ["candles", "tape"] {
            let error = call(&mut refused, json!(fold)).unwrap_err();
            let named = error.context.violated_precondition.as_ref();
            assert_eq!(
                named.map(|id| id.as_str()),
                Some(BESIDE_CANDLES_PRECONDITION)
            );
            assert!(!error.retryable);
            assert_eq!(error.context.next_steps.len(), 1);
        }
        assert_eq!(refused.sets, 0, "{facts:?}");
    }
    let mut beside = pane(BESIDE);
    for bad in [json!("left"), json!(1), Value::Null] {
        assert!(call(&mut beside, bad).is_err());
    }
    beside.facts = Err(ControlError::invalid_request("no order-flow view"));
    assert!(call(&mut beside, json!("open")).is_err());
    assert_eq!(beside.sets, 0, "every refusal came before any set");

    let declared: Vec<String> = descriptor()
        .preconditions
        .iter()
        .map(|declared| declared.id.as_str().to_owned())
        .collect();
    assert_eq!(declared, [BESIDE_CANDLES_PRECONDITION]);
}
