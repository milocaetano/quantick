//! The time strip resets through the fake window: the gesture each target
//! takes, and what the pane cannot show.
use super::*;
use crate::test_support::{FakePane, FakeTab, FakeTape, FakeWindow};
use quantick_control::error::codes;
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
fn a_time_reset_takes_the_gesture_path_and_refuses_what_the_pane_cannot_show() {
    let (mut window, actor) = (window(), FakeWindow::assistant());
    let reset =
        |axis: &str, to: &str| json!({"tab_id": "1", "pane_id": "2", "axis": axis, "to": to});
    let centre = reset_time(
        &mut window,
        &mut (),
        &actor,
        &reset("candles", "live_centre"),
    );
    assert_eq!(centre.unwrap()["price_centre_pending"], true);
    reset_time(&mut window, &mut (), &actor, &reset("candles", "opening")).unwrap();
    let error = reset_time(&mut window, &mut (), &actor, &reset("tape", "opening")).unwrap_err();
    assert!(
        format!("{error:?}").contains("no tape time strip"),
        "{error:?}"
    );
    let tape_centre = reset_time(&mut window, &mut (), &actor, &reset("tape", "live_centre"));
    assert!(tape_centre.is_err());
    window.tabs[0].panes[0].shows_live = false;
    let overview = reset("candles", "live_overview");
    assert!(reset_time(&mut window, &mut (), &actor, &overview).is_err());
    assert_eq!(pane(&window).time_acts, vec!["centre", "opening"]);
}

/// Each candles target takes its own gesture and answers whether a price
/// centring follows; the tape has only its opening; an axis the schema does
/// not name is refused before any act.
#[test]
fn each_target_takes_its_gesture_and_the_tape_has_only_its_opening() {
    let (mut window, actor) = (window(), FakeWindow::assistant());
    window.tabs[0].panes[0].tape = Some(FakeTape::default());
    let reset =
        |axis: &str, to: &str| json!({"tab_id": "1", "pane_id": "2", "axis": axis, "to": to});
    for (to, act, centres) in [
        ("opening", "opening", false),
        ("live_overview", "overview", false),
        ("live_centre", "centre", true),
        ("live_centre_zoom_in", "centre_zoom_in", true),
    ] {
        let answer = reset_time(&mut window, &mut (), &actor, &reset("candles", to)).unwrap();
        assert_eq!(answer["price_centre_pending"], centres, "{to}");
        assert_eq!(answer["pane_id"], "2", "{to}");
        assert_eq!(pane(&window).time_acts.last(), Some(&act), "{to}");
    }
    let omitted = json!({"tab_id": "1", "pane_id": "2", "axis": "tape"});
    let tape = reset_time(&mut window, &mut (), &actor, &omitted).unwrap();
    assert_eq!(
        tape["price_centre_pending"], false,
        "no target is the opening"
    );
    assert_eq!(pane(&window).time_acts.last(), Some(&"tape_opening"));
    let acts = pane(&window).time_acts.len();
    for to in ["live_overview", "live_centre", "live_centre_zoom_in"] {
        let error = reset_time(&mut window, &mut (), &actor, &reset("tape", to)).unwrap_err();
        assert_eq!(error.code.as_str(), codes::INVALID_REQUEST, "{to}");
    }
    let unknown = reset_time(&mut window, &mut (), &actor, &reset("price", "opening"));
    assert_eq!(
        unknown.unwrap_err().code.as_str(),
        codes::INVALID_REQUEST,
        "an axis the schema does not name"
    );
    assert_eq!(
        pane(&window).time_acts.len(),
        acts,
        "a refusal takes no act"
    );
}

/// A pane that shows no candles (tape only) refuses the live centre and
/// says why, and nothing moves.
#[test]
fn a_pane_without_candles_refuses_the_live_centre_and_says_why() {
    let (mut window, actor) = (window(), FakeWindow::assistant());
    window.tabs[0].panes[0].shows_live = false;
    for to in ["live_centre", "live_centre_zoom_in"] {
        let input = json!({"tab_id": "1", "pane_id": "2", "axis": "candles", "to": to});
        let error = reset_time(&mut window, &mut (), &actor, &input).unwrap_err();
        assert_eq!(error.code.as_str(), codes::INVALID_REQUEST, "{to}");
        assert!(
            error.message.contains("tape-only"),
            "{to}: {}",
            error.message
        );
    }
    assert!(pane(&window).time_acts.is_empty(), "nothing moved");
}
