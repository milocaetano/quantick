//! `orderflow.tape.view.set` through a fake pane: from any view, and a refused
//! call keeps nothing.

use super::*;
use serde_json::json;

struct Pane {
    refuse: Option<ControlError>,
    view: TapeView,
    sets: usize,
}

impl TapeViewPort for Pane {
    fn set_tape_view(
        &mut self,
        _tab: u64,
        _pane: u64,
        view: TapeView,
    ) -> Result<bool, ControlError> {
        if let Some(error) = self.refuse.clone() {
            return Err(error);
        }
        self.sets += 1;
        let changed = self.view != view;
        self.view = view;
        Ok(changed)
    }
}

fn pane(view: TapeView) -> Pane {
    Pane {
        refuse: None,
        view,
        sets: 0,
    }
}

fn call(pane: &mut Pane, view: Value) -> Result<Value, ControlError> {
    let actor = crate::test_support::FakeWindow::assistant();
    let input = json!({"tab_id": "1", "pane_id": "0", "view": view});
    set(pane, &mut (), &actor, &input)
}

const VIEWS: [(&str, TapeView); 3] = [
    ("beside", TapeView::Beside),
    ("candles", TapeView::Candles),
    ("tape", TapeView::Tape),
];

/// Every view is reachable from every view, and repeating one changes nothing.
#[test]
fn a_view_is_allowed_from_any_view_and_idempotent() {
    for (_, from) in VIEWS {
        for (word, to) in VIEWS {
            let mut on = pane(from);
            let answer = call(&mut on, json!(word)).unwrap();
            assert_eq!(answer["view"], word);
            assert_eq!(answer["changed"], from != to, "{from:?} to {to:?}");
            assert_eq!(on.view, to);
            let again = call(&mut on, json!(word)).unwrap();
            assert_eq!(again["changed"], false);
        }
    }
    assert!(descriptor().preconditions.is_empty());
}

/// Malformed input and a pane without a view are refused and keep nothing.
#[test]
fn what_cannot_be_applied_is_refused() {
    let mut on = pane(TapeView::Beside);
    for bad in [json!("left"), json!("open"), json!(1), Value::Null] {
        assert!(call(&mut on, bad).is_err());
    }
    assert_eq!(on.sets, 0);
    on.refuse = Some(ControlError::invalid_request("no order-flow view"));
    assert!(call(&mut on, json!("tape")).is_err());
    assert_eq!(on.sets, 0, "every refusal came before any set");
}
