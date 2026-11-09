//! C3-C7: the UI and gateway share source persistence and the reload adapter.
use super::*;
use crate::app::control_host::ScriptsPort;
use crate::indicator_worker::IndicatorCommand;
use quantick_stores::script_library::{
    ScriptEditorAction as Action, ScriptLibrary, ScriptLibraryPort,
};
use std::time::Duration;

fn edit(app: &mut QuantickApp, action: Action) {
    app.edit_script(action).unwrap();
}
fn editor_frame(app: &mut QuantickApp, elapsed_ms: u64) {
    let now = app.surfaces.script_editor.epoch + Duration::from_millis(elapsed_ms);
    crate::app::script_editor_wiring::frame(
        &mut app.surfaces,
        &mut app.runtime.indicators,
        &mut app.runtime.tabs,
        Vec::new(),
        now,
    );
}
fn compile_edit(app: &mut QuantickApp, source: &str, at: u64) {
    edit(
        app,
        Action::ReplaceSource {
            source: source.into(),
        },
    );
    editor_frame(app, at);
    assert!(
        app.surfaces.script_editor.compilation.is_none(),
        "debounce holds compilation"
    );
    editor_frame(app, at + 350);
    let started = Instant::now();
    while app.surfaces.script_editor.compilation.is_some() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "validator completes"
        );
        std::thread::yield_now();
        editor_frame(app, at + 351);
    }
    settle_indicators(app);
}
fn copy_fixture() -> (
    QuantickApp,
    crate::scratch::ScratchDir,
    mpsc::Receiver<FeedCommand>,
) {
    let (mut app, commands) = app_with_history(40);
    let directory = crate::scratch::ScratchDir::new("source-editor");
    app.runtime.indicators.script_library = ScriptLibrary::open(&directory);
    edit(
        &mut app,
        Action::Open {
            id: "ema.pine".into(),
        },
    );
    edit(
        &mut app,
        Action::Copy {
            id: "ema.pine".into(),
            new_id: "my-ema.pine".into(),
        },
    );
    let index = app
        .runtime
        .indicators
        .script_library
        .entries()
        .iter()
        .position(|entry| entry.name == "my-ema.pine")
        .unwrap();
    app.apply_toolbar_action(ToolbarAction::AddScriptIndicator(index));
    settle_indicators(&mut app);
    (app, directory, commands)
}
fn values(app: &QuantickApp) -> Vec<Vec<f64>> {
    app.active_tab().flow_pane.indicators.all()[0]
        .columns
        .clone()
}

#[test]
fn script_titles_preserve_identity() {
    let (mut app, _commands) = app_with_history(8);
    let directory = crate::scratch::ScratchDir::new("script-titles");
    std::fs::write(directory.join("legacy_signal.PINE"), "plot(close)\n").unwrap();
    app.runtime.indicators.script_library = ScriptLibrary::open(&directory);
    let entries = app.runtime.indicators.script_library.entries();
    let picker = quantick_stores::indicator_picker::Picker::default();
    assert!(
        picker
            .entries(entries)
            .iter()
            .all(|entry| !entry.name.to_ascii_lowercase().ends_with(".pine"))
    );
    let index = entries
        .iter()
        .position(|entry| entry.name == "legacy_signal.PINE")
        .unwrap();
    app.apply_toolbar_action(ToolbarAction::AddScriptIndicator(index));
    settle_indicators(&mut app);
    let view = &app.active_tab().flow_pane.indicators.all()[0];
    assert_eq!(view.label(), "legacy signal");
    assert_eq!(view.kind.as_ref(), "script.legacy_signal.PINE");
    edit(
        &mut app,
        Action::Open {
            id: "legacy_signal.PINE".into(),
        },
    );
    let document = app.surfaces.script_editor.state.document.as_ref().unwrap();
    assert_eq!(document.id, "legacy_signal.PINE");
    assert_eq!(document.title, "legacy signal");
    let tab = app.runtime.tabs.active_id();
    let layout = app.layout_state().pane_layout(tab, PaneSide::Flow);
    let saved = app
        .layout_state()
        .layouts()
        .get(layout)
        .unwrap()
        .indicators
        .clone();
    let (mut reopened, _commands) = app_with_history(8);
    reopened.runtime.indicators.script_library = ScriptLibrary::open(&directory);
    let tab = reopened.runtime.tabs.active_id();
    reopened
        .layout_adapter()
        .materialize_indicators_at(tab, PaneSide::Flow, &saved);
    settle_indicators(&mut reopened);
    assert_eq!(
        reopened.active_tab().flow_pane.indicators.all()[0].label(),
        "legacy signal"
    );
    assert_eq!(
        reopened.active_tab().flow_pane.indicators.all()[0]
            .kind
            .as_ref(),
        "script.legacy_signal.PINE"
    );
}
#[test]
fn script_editor_live_update() {
    let (mut app, _directory, _commands) = copy_fixture();
    let slot = app.active_tab().flow_pane.indicators.all()[0].slot;
    compile_edit(
        &mut app,
        "indicator(\"Edited average\", overlay=true)\nplot(close * 2)\n",
        0,
    );
    assert_eq!(app.active_tab().flow_pane.indicators.all().len(), 1);
    assert_eq!(app.active_tab().flow_pane.indicators.all()[0].slot, slot);
    assert_eq!(
        app.active_tab().flow_pane.indicators.all()[0].label(),
        "Edited average"
    );
    let good = values(&app);
    // An older validation result arrives after a new edit. It must never reload.
    let old_revision = app.surfaces.script_editor.state.revision;
    let document = app.surfaces.script_editor.state.document.clone().unwrap();
    edit(
        &mut app,
        Action::ReplaceSource {
            source: "plot(close * 3)\n".into(),
        },
    );
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    sender.send((old_revision, document, None)).unwrap();
    app.surfaces.script_editor.compilation = Some(receiver);
    editor_frame(&mut app, 800);
    settle_indicators(&mut app);
    assert_eq!(values(&app), good);
    assert!(app.surfaces.script_editor.state.dirty);
    editor_frame(&mut app, 1150);
    while app.surfaces.script_editor.compilation.is_some() {
        std::thread::yield_now();
        editor_frame(&mut app, 1151);
    }
    settle_indicators(&mut app);
    assert_ne!(values(&app), good);
    assert_eq!(app.active_tab().flow_pane.indicators.all()[0].slot, slot);
}
#[test]
fn script_editor_compile_recovery() {
    let (mut app, _directory, _commands) = copy_fixture();
    compile_edit(&mut app, "plot(close * 2)\n", 0);
    let good = values(&app);
    compile_edit(&mut app, "plot(unknown_value)\n", 1000);
    assert!(
        app.surfaces
            .script_editor
            .state
            .diagnostics
            .as_ref()
            .unwrap()
            .contains("PINE_UNKNOWN_NAME")
    );
    assert_eq!(values(&app), good);
    compile_edit(&mut app, "plot(close * 4)\n", 2000);
    assert!(app.surfaces.script_editor.state.diagnostics.is_none());
    assert_ne!(values(&app), good);
}
#[test]
fn script_editor_manual_save_waits_for_validation() {
    let (mut app, _directory, _commands) = copy_fixture();
    let good = values(&app);
    edit(
        &mut app,
        Action::ReplaceSource {
            source: "plot(unknown_value)\n".into(),
        },
    );
    edit(&mut app, Action::Save);
    app.runtime.indicators.last_script_poll = Instant::now() - Duration::from_secs(2);
    assert!(app.runtime.indicators.poll_script_files().is_empty());
    assert_eq!(values(&app), good);
    editor_frame(&mut app, 0);
    editor_frame(&mut app, 350);
    while app.surfaces.script_editor.compilation.is_some() {
        std::thread::yield_now();
        editor_frame(&mut app, 351);
    }
    settle_indicators(&mut app);
    assert!(app.surfaces.script_editor.state.diagnostics.is_some());
    assert_eq!(values(&app), good);
}
#[test]
fn script_library_end_to_end() {
    let (mut app, directory, _commands) = copy_fixture();
    let original =
        ScriptLibraryPort::read(&app.runtime.indicators.script_library, "ema.pine").unwrap();
    compile_edit(
        &mut app,
        "indicator(\"My average\", overlay=true)\nlength = input.int(2)\nplot(close * length)\n",
        0,
    );
    let target = app.runtime.indicators.registry.slot_kinds()[0].0;
    let inputs = vec![quantick_indicators::InputValue::Int(4)];
    app.active_tab()
        .flow_pane
        .indicator_worker
        .send(IndicatorCommand::SetInputs {
            slot: target.slot,
            values: inputs.clone(),
        });
    app.apply_indicator_edit(crate::app::indicator_manager::IndicatorEdit::Inputs(
        target, inputs,
    ));
    settle_indicators(&mut app);
    let output = values(&app);
    edit(&mut app, Action::Save);
    edit(&mut app, Action::Close);
    let tab = app.runtime.tabs.active_id();
    let layout = app.layout_state().pane_layout(tab, PaneSide::Flow);
    let saved = app
        .layout_state()
        .layouts()
        .get(layout)
        .unwrap()
        .indicators
        .clone();
    let encoded = toml::to_string(&saved[0]).unwrap();
    let restored =
        vec![toml::from_str::<crate::indicators::state_file::SavedIndicator>(&encoded).unwrap()];
    let (mut reopened, _commands) = app_with_history(40);
    reopened.runtime.indicators.script_library = ScriptLibrary::open(&directory);
    let tab = reopened.runtime.tabs.active_id();
    reopened
        .layout_adapter()
        .materialize_indicators_at(tab, PaneSide::Flow, &restored);
    settle_indicators(&mut reopened);
    edit(
        &mut reopened,
        Action::Open {
            id: "my-ema.pine".into(),
        },
    );
    assert_eq!(
        reopened
            .surfaces
            .script_editor
            .state
            .document
            .as_ref()
            .unwrap()
            .source,
        "indicator(\"My average\", overlay=true)\nlength = input.int(2)\nplot(close * length)\n"
    );
    assert_eq!(
        reopened.active_tab().flow_pane.indicators.all()[0].label(),
        "My average"
    );
    assert_eq!(values(&reopened), output);
    assert_eq!(
        ScriptLibraryPort::read(&reopened.runtime.indicators.script_library, "ema.pine").unwrap(),
        original
    );
}
