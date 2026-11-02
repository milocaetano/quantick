//! `orderflow.tape.fold.set` through a fake pane: every word is allowed, a repeat
//! says so, and what cannot be read keeps nothing.

use super::*;
use serde_json::json;

struct Pane {
    refuses: bool,
    fold: TapeFold,
    sets: usize,
}

impl TapeFoldPort for Pane {
    fn set_tape_fold(
        &mut self,
        _tab: u64,
        _pane: u64,
        fold: TapeFold,
    ) -> Result<bool, ControlError> {
        if self.refuses {
            return Err(ControlError::invalid_request("no order-flow view"));
        }
        self.sets += 1;
        let changed = self.fold != fold;
        self.fold = fold;
        Ok(changed)
    }
    fn set_tape_market_fold(&mut self, _: u64, _: u64, _: bool) -> Result<bool, ControlError> {
        Ok(false)
    }
}

fn pane() -> Pane {
    Pane {
        refuses: false,
        fold: TapeFold::Open,
        sets: 0,
    }
}

fn call(pane: &mut Pane, fold: Value) -> Result<Value, ControlError> {
    let actor = crate::test_support::FakeWindow::assistant();
    let input = json!({"tab_id": "1", "pane_id": "0", "fold": fold});
    set(pane, &mut (), &actor, &input)
}

/// Every word goes through from every view, and repeating one changes nothing.
#[test]
fn every_word_is_allowed_and_a_repeat_says_so() {
    let mut pane = pane();
    for word in ["candles", "tape", "open"] {
        let first = call(&mut pane, json!(word)).unwrap();
        assert_eq!(first["fold"], word);
        assert_eq!(first["changed"], true, "{word}");
        assert_eq!(call(&mut pane, json!(word)).unwrap()["changed"], false);
    }
    assert!(descriptor().preconditions.is_empty(), "the state is one");
}

/// Malformed input reaches no pane, and a missing pane's refusal is passed on.
#[test]
fn what_is_refused_keeps_nothing() {
    let mut good = pane();
    for bad in [json!("left"), json!(1), Value::Null] {
        assert!(call(&mut good, bad).is_err());
    }
    assert_eq!(good.sets, 0);
    let mut gone = Pane {
        refuses: true,
        ..pane()
    };
    assert!(call(&mut gone, json!("open")).is_err());
    assert_eq!(gone.sets, 0);
}
