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
    fn hold(reason: Option<crate::store_home::WritesRefused>) -> Self {
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
    let refused = reason.clone().expect("the stray hook refuses writes");
    assert_eq!(refused.hooks, ["QUANTICK_UI_STATE"]);
    let reason_text = refused.to_string();
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
    // And an agent reads it without the pixels: `health.summary` names it.
    assert_eq!(
        health_saves_off(&app),
        serde_json::json!(["QUANTICK_UI_STATE"])
    );
}

/// `health.summary`'s `saves_off_unread_hooks`, `Null` when absent.
fn health_saves_off(app: &QuantickApp) -> serde_json::Value {
    let scope = observer_scope("health.summary");
    let snapshot = crate::control::standard_registry()
        .unwrap()
        .capture(app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    snapshot.scopes[&scope].value["saves_off_unread_hooks"].clone()
}

/// Opening a workspace file and forgetting the saved workspace write the
/// cockpit too, so a saves-off session (DS7) does neither.
#[test]
fn a_saves_off_session_neither_opens_a_workspace_nor_forgets_one() {
    let source = crate::scratch::thread_dir("ds7-import-source");
    assert!(crate::ui_state::save(
        &source.join(crate::ui_state::UI_STATE_FILE),
        &ui_state::Workspace::default()
    ));
    let stores = crate::store_home::COCKPIT_STORES;
    let bundle =
        crate::workspace_bundle::capture("other", stores, &|store| source.join(store.file))
            .expect("capture");

    let live = crate::scratch::thread_dir("ds7-import-live");
    let live_state = live.join(crate::ui_state::UI_STATE_FILE);
    // The live file differs from the bundle, so an import would show.
    let before = format!(
        "# the trader's own
{}",
        std::fs::read_to_string(source.join(crate::ui_state::UI_STATE_FILE)).unwrap()
    );
    std::fs::write(&live_state, &before).unwrap();

    let _refusal = RefusalOnThisThread::hold(Some(crate::store_home::WritesRefused {
        hooks: vec!["QUANTICK_UI_STATE".to_owned()],
    }));
    let error = crate::workspace_bundle::apply(&bundle, stores, &|store| live.join(store.file))
        .expect_err("the import is refused");
    assert!(error.contains("saving is off"), "{error}");
    assert!(
        !crate::ui_state::forget(&live_state),
        "forgetting is refused"
    );
    assert_eq!(std::fs::read_to_string(&live_state).unwrap(), before);
    let names: Vec<_> = std::fs::read_dir(&live)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1, "no staged import file either: {names:?}");
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
    assert_eq!(health_saves_off(&app), serde_json::Value::Null);
}
