//! The time strip resets through the fake window: the gesture each target
//! takes, and what the pane cannot show.
use super::*;
use crate::test_support::{FakePane, FakeTab, FakeWindow};
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
