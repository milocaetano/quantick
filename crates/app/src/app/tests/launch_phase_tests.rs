//! The launch phase applies the scenario inputs the composition root
//! captured, and reads nothing else: an app built without them opens as its
//! workspace left it, whatever the process environment holds.
use super::*;

#[test]
fn captured_scenario_inputs_reach_the_launch_phase() {
    let launch = AppLaunch {
        scenario: crate::hooks::ScenarioInputs::from_pairs(&[
            ("QUANTICK_INVERTED", "1"),
            ("QUANTICK_DOCK_TAB", "trades"),
        ]),
        ..AppLaunch::default()
    };
    let (app, _evt, _cmd, _book) = test_app_with_launch(launch);
    assert!(app.active_tab().flow_pane.price_view.is_inverted());
    assert_eq!(app.dock.tab(), Some(crate::dock::DockTab::Trades));
}

#[test]
fn an_app_without_scenario_inputs_applies_no_scenario() {
    let (app, _evt, _cmd, _book) = test_app();
    assert!(!app.active_tab().flow_pane.price_view.is_inverted());
    assert_ne!(app.dock.tab(), Some(crate::dock::DockTab::Trades));
}
