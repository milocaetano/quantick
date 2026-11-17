//! The price and tape calls through the fake window: what the pane is asked
//! to do, and what each call refuses before anything moves.
use super::*;
use crate::test_support::{FakePane, FakeTab, FakeTape, FakeWindow};
use quantick_control::error::codes;
use quantick_control_schema::tape_view::TapeViewSnapshot;
use quantick_orderflow::LaneWindow;
use quantick_orderflow::tape_view::TapeEnd;
use serde_json::json;

/// One tab (id 1) whose pane (id 2) has no native tape.
fn window() -> FakeWindow {
    let mut tab = FakeTab::new(1, "BTCUSDT");
    tab.panes.push(FakePane::with_trades(2, 50, []));
    FakeWindow::new(vec![tab])
}

fn pane(window: &FakeWindow) -> &FakePane {
    &window.tabs[0].panes[0]
}

#[test]
fn a_price_call_frames_or_fits_the_addressed_pane_and_answers_its_framing() {
    let (mut window, actor) = (window(), FakeWindow::assistant());
    window.tabs[0].panes.push(FakePane::with_trades(3, 50, []));
    let manual = json!({"tab_id": "1", "pane_id": "2",
        "mode": {"kind": "manual", "low": "10", "high": "20"}});
    let answer = set_price(&mut window, &mut (), &actor, &manual).unwrap();
    assert_eq!(answer["pane_id"], "2");
    assert!(answer["viewport"].is_object(), "{answer}");
    let auto = json!({"tab_id": "1", "pane_id": "2", "mode": {"kind": "auto"}});
    set_price(&mut window, &mut (), &actor, &auto).unwrap();
    assert_eq!(pane(&window).framed, vec![Some((10.0, 20.0)), None]);
    assert!(
        window.tabs[0].panes[1].framed.is_empty(),
        "the tab's other pane keeps its framing"
    );
}

#[test]
fn a_price_call_refuses_bad_bounds_and_unknown_panes_before_framing() {
    let (mut window, actor) = (window(), FakeWindow::assistant());
    let inverted = json!({"tab_id": "1", "pane_id": "2",
        "mode": {"kind": "manual", "low": "20", "high": "10"}});
    let error = set_price(&mut window, &mut (), &actor, &inverted).unwrap_err();
    assert!(format!("{error:?}").contains("low below high"), "{error:?}");
    for (tab, pane) in [("1", "9"), ("9", "2")] {
        let elsewhere = json!({"tab_id": tab, "pane_id": pane, "mode": {"kind": "auto"}});
        let error = set_price(&mut window, &mut (), &actor, &elsewhere).unwrap_err();
        assert_eq!(error.code.as_str(), codes::INVALID_REQUEST, "{tab}/{pane}");
    }
    for mode in [
        json!({"kind": "manual", "low": "100", "high": "100"}),
        json!({"kind": "manual", "low": "NaN", "high": "110"}),
        json!({"kind": "manual", "low": "90"}),
        json!({"kind": "unknown"}),
    ] {
        let input = json!({"tab_id": "1", "pane_id": "2", "mode": mode});
        let error = set_price(&mut window, &mut (), &actor, &input).unwrap_err();
        assert_eq!(error.code.as_str(), codes::INVALID_REQUEST, "{mode}");
    }
    assert!(pane(&window).framed.is_empty());
}

#[test]
fn a_tape_call_moves_the_window_before_the_end_and_needs_a_native_tape() {
    let (mut window, actor) = (window(), FakeWindow::assistant());
    let input = json!({"tab_id": "1", "pane_id": "2",
        "end": {"kind": "past", "end_unix_ms": 5_000},
        "window": {"kind": "fixed", "ms": 60_000}, "file": true});
    let error = set_tape(&mut window, &mut (), &actor, &input).unwrap_err();
    assert!(format!("{error:?}").contains(NO_NATIVE_TAPE), "{error:?}");
    let frame = TapeViewSnapshot::new(
        TapeEnd::Past { end_ms: 5_000 },
        LaneWindow::Fixed { ms: 60_000 },
        60_000,
        None,
    );
    window.tabs[0].panes[0].tape = Some(FakeTape {
        frame: Some(frame.clone()),
        ..FakeTape::default()
    });
    let answer = set_tape(&mut window, &mut (), &actor, &input).unwrap();
    assert_eq!(answer["tape"], serde_json::to_value(&frame).unwrap());
    let tape = pane(&window).tape.as_ref().unwrap();
    assert_eq!(tape.window, Some((LaneWindow::Fixed { ms: 60_000 }, true)));
    assert_eq!(tape.end, Some(TapeEnd::Past { end_ms: 5_000 }));
}
