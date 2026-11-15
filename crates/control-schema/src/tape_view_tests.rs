//! `chart.tape_view.set`'s input: what it asks the tape for, and what it
//! refuses before any pane is read.

use super::*;
use serde_json::{Value, json};

fn parse(fields: Value) -> Result<TapeViewInput, serde_json::Error> {
    let mut input = json!({"tab_id": "1", "pane_id": "2"});
    for (key, value) in fields.as_object().expect("an object") {
        input[key] = value.clone();
    }
    serde_json::from_value(input)
}

fn request(fields: Value) -> Result<(Option<TapeEnd>, Option<LaneWindow>), ControlError> {
    parse(fields).expect("the input parses").request()
}

/// An end, a window or both, each read as asked; `auto` is the default window.
#[test]
fn an_end_and_a_window_are_read_as_asked() {
    assert_eq!(
        request(json!({"end": {"kind": "live"}})).unwrap(),
        (Some(TapeEnd::Live), None)
    );
    let past = json!({
        "end": {"kind": "past", "end_unix_ms": 35_000},
        "window": {"kind": "fixed", "ms": 3_000},
    });
    assert_eq!(
        request(past).unwrap(),
        (
            Some(TapeEnd::Past { end_ms: 35_000 }),
            Some(LaneWindow::Fixed { ms: 3_000 })
        )
    );
    let auto = json!({"window": {"kind": "auto"}, "file": true});
    assert_eq!(request(auto).unwrap(), (None, Some(LaneWindow::default())));
}

/// A call that asks for nothing, files no window, or names a window of no
/// time is refused.
#[test]
fn a_call_that_names_nothing_to_move_is_refused() {
    for fields in [
        json!({}),
        json!({"end": {"kind": "live"}, "file": true}),
        json!({"window": {"kind": "fixed", "ms": 0}}),
        json!({"window": {"kind": "fixed", "ms": -5}}),
    ] {
        assert!(request(fields.clone()).is_err(), "{fields}");
    }
}

/// What the wire does not say never parses: a past end without its instant,
/// a kind it has no word for, a field it does not know.
#[test]
fn malformed_words_never_parse() {
    for fields in [
        json!({"end": {"kind": "past"}}),
        json!({"end": {"kind": "sometime"}}),
        json!({"window": {"kind": "fixed"}}),
        json!({"end": {"kind": "live"}, "zoom": 2}),
    ] {
        assert!(parse(fields.clone()).is_err(), "{fields}");
    }
}
