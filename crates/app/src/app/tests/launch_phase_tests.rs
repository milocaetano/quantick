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

/// Holds a per-thread write refusal for one test and lifts it on drop.
struct RefusalOnThisThread;

impl RefusalOnThisThread {
    fn hold(reason: Option<String>) -> Self {
        crate::store_home::refuse_writes_on_this_thread(reason);
        Self
    }
}

impl Drop for RefusalOnThisThread {
    fn drop(&mut self) {
        crate::store_home::refuse_writes_on_this_thread(None);
    }
}

/// Decision DS7. A capture run's store-isolation hook set against a default
/// build (which registers configuration only) would otherwise save the run
/// over the trader's cockpit: the session writes no store, and the status
/// line says so for as long as it runs.
#[test]
fn a_stray_store_hook_in_a_default_build_turns_saving_off_and_says_so() {
    let environment = ["QUANTICK_CONFIG", "QUANTICK_UI_STATE", "PATH"];
    let reason = crate::launch::persistence_refusal(
        environment.into_iter(),
        &crate::hooks::configuration_names(),
    );
    let reason_text = reason.clone().expect("the stray hook refuses writes");
    assert!(reason_text.contains("QUANTICK_UI_STATE"), "{reason_text}");
    assert!(!reason_text.contains("QUANTICK_CONFIG"), "{reason_text}");
    let _refusal = RefusalOnThisThread::hold(reason);

    let dir = crate::scratch::thread_dir("ds7-refused");
    let workspace = dir.join("ui-state.toml");
    assert!(!crate::ui_state::save(
        &workspace,
        &ui_state::Workspace::default()
    ));
    crate::layouts::save(
        &dir.join("layouts.toml"),
        &crate::layouts::LayoutBook::default(),
    );
    crate::chart_layers::save(&dir.join("chart-layers.toml"), &Default::default());
    assert_eq!(
        std::fs::read_dir(&dir).map_or(0, |entries| entries.count()),
        0,
        "no store file, and no partial one, is written"
    );

    let (app, _evt, _cmd, _book) = test_app();
    let notice = app.status_model().saves_off;
    assert!(
        notice
            .as_deref()
            .is_some_and(|text| text.contains("QUANTICK_UI_STATE")),
        "{notice:?}"
    );
}

/// The same scan over configuration alone refuses nothing, and saving works.
#[test]
fn configuration_alone_keeps_saving_on() {
    let environment = ["QUANTICK_CONFIG", "QUANTICK_BUBBLES", "QUANTICK_GIT_COMMIT"];
    let reason = crate::launch::persistence_refusal(
        environment.into_iter(),
        &crate::hooks::configuration_names(),
    );
    assert_eq!(reason, None);
    let _refusal = RefusalOnThisThread::hold(reason);

    let dir = crate::scratch::thread_dir("ds7-allowed");
    let workspace = dir.join("ui-state.toml");
    assert!(crate::ui_state::save(
        &workspace,
        &ui_state::Workspace::default()
    ));
    assert!(workspace.is_file());
    let (app, _evt, _cmd, _book) = test_app();
    assert_eq!(app.status_model().saves_off, None);
}
