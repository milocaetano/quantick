//! C3-C7: the UI and gateway share source persistence and the reload adapter.
use super::*;
use crate::app::control_host::ScriptsPort;
use crate::indicator_worker::IndicatorCommand;
use quantick_stores::script_library::{
    ScriptEditorAction as Action, ScriptLibrary, ScriptLibraryPort,
};
use std::time::Duration;

fn editor(app: &QuantickApp) -> &quantick_stores::script_editor::ScriptEditor {
    app.surfaces.script_editor.model_state.editor()
}
fn edit(app: &mut QuantickApp, action: Action) {
    app.edit_script(action).unwrap();
}
pub(super) fn editor_frame(app: &mut QuantickApp, elapsed_ms: u64) {
    let now = app.surfaces.script_editor.view.epoch + Duration::from_millis(elapsed_ms);
    crate::app::script_editor_wiring::frame(
        &mut app.surfaces,
        &mut app.runtime.indicators,
        &mut app.runtime.tabs,
        app.runtime.workspace.layouts(),
        Vec::new(),
        now,
    );
}
pub(super) fn validate_open_editor(app: &mut QuantickApp, at: u64) {
    editor_frame(app, at);
    editor_frame(app, at + 350);
    finish_validation(app, at + 351);
}
fn finish_validation(app: &mut QuantickApp, at: u64) {
    let started = Instant::now();
    while app.surfaces.script_editor.view.compilation.is_some() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "validator completes"
        );
        std::thread::yield_now();
        editor_frame(app, at);
    }
    settle_indicators(app);
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
        app.surfaces.script_editor.view.compilation.is_none(),
        "debounce holds compilation"
    );
    editor_frame(app, at + 350);
    finish_validation(app, at + 351);
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
fn saved_flow_indicators(app: &QuantickApp) -> Vec<crate::indicators::state_file::SavedIndicator> {
    let layout = app
        .layout_state()
        .pane_layout(app.runtime.tabs.active_id(), PaneSide::Flow);
    app.layout_state()
        .layouts()
        .get(layout)
        .unwrap()
        .indicators
        .clone()
}
fn reopen_serialized_indicators(
    directory: &std::path::Path,
    saved: &[crate::indicators::state_file::SavedIndicator],
) -> QuantickApp {
    let encoded = toml::to_string(&saved[0]).unwrap();
    let restored = vec![toml::from_str(&encoded).unwrap()];
    let (mut reopened, _commands) = app_with_history(40);
    reopened.runtime.indicators.script_library = ScriptLibrary::open(directory);
    let tab = reopened.runtime.tabs.active_id();
    reopened
        .layout_adapter()
        .materialize_indicators_at(tab, PaneSide::Flow, &restored);
    settle_indicators(&mut reopened);
    reopened
}
fn poll_external_reload(app: &mut QuantickApp) {
    let owners: Vec<_> = app
        .runtime
        .indicators
        .registry
        .script_files()
        .iter()
        .map(|(owner, ..)| *owner)
        .collect();
    for owner in owners {
        app.runtime
            .indicators
            .registry
            .observe_script(owner, std::time::SystemTime::UNIX_EPOCH);
    }
    app.runtime.indicators.last_script_poll = Instant::now() - Duration::from_secs(2);
    crate::app::script_reload::reload_changed_scripts(
        &mut app.runtime.indicators,
        &mut app.runtime.tabs,
        app.runtime.workspace.layouts(),
    );
    settle_indicators(app);
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
    let document = editor(&app).document.as_ref().unwrap();
    assert_eq!(document.id, "legacy_signal.PINE");
    assert_eq!(document.title, "legacy signal");
    let saved = saved_flow_indicators(&app);
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
    let old_revision = editor(&app).revision;
    let document = editor(&app).document.clone().unwrap();
    edit(
        &mut app,
        Action::ReplaceSource {
            source: "plot(close * 3)\n".into(),
        },
    );
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    sender
        .send((old_revision, document, None, Vec::new()))
        .unwrap();
    app.surfaces.script_editor.view.compilation = Some((old_revision, receiver));
    editor_frame(&mut app, 800);
    settle_indicators(&mut app);
    assert_eq!(values(&app), good);
    editor_frame(&mut app, 1150);
    finish_validation(&mut app, 1151);
    assert_ne!(values(&app), good);
    assert_eq!(app.active_tab().flow_pane.indicators.all()[0].slot, slot);
}
#[test]
fn script_editor_stale_read_disconnect_preserves_newer_unsaved_edit() {
    let (mut app, _directory, _commands) = copy_fixture();
    app.surfaces
        .script_editor
        .update(quantick_ui_model::script_editor::Intent::Tick(0));
    let response = app
        .surfaces
        .script_editor
        .update(quantick_ui_model::script_editor::Intent::Tick(350));
    let quantick_ui_model::HostEffect::ScriptEditor(effects) =
        response.model_effects.into_iter().next().unwrap()
    else {
        panic!("editor effect")
    };
    let quantick_ui_model::script_editor::Effect::Validate {
        revision: old_revision,
        ..
    } = effects.into_iter().next().unwrap()
    else {
        panic!("read validation")
    };
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.surfaces.script_editor.view.compilation = Some((old_revision, receiver));
    edit(
        &mut app,
        Action::Open {
            id: "my-ema.pine".into(),
        },
    );
    let source = "plot(bar_index * 4)\n";
    edit(
        &mut app,
        Action::ReplaceSource {
            source: source.into(),
        },
    );
    drop(sender);
    editor_frame(&mut app, 800);
    assert_ne!(
        ScriptLibraryPort::read(&app.runtime.indicators.script_library, "my-ema.pine")
            .unwrap()
            .source,
        source
    );
    validate_open_editor(&mut app, 1000);
    assert_eq!(values(&app)[0][39], 39.0 * 4.0);
    assert_eq!(
        ScriptLibraryPort::read(&app.runtime.indicators.script_library, "my-ema.pine")
            .unwrap()
            .source,
        source
    );
    edit(&mut app, Action::Close);
}

#[test]
fn script_editor_compile_recovery() {
    let (mut app, _directory, _commands) = copy_fixture();
    compile_edit(&mut app, "plot(close * 2)\n", 0);
    let good = values(&app);
    compile_edit(&mut app, "plot(unknown_value)\n", 1000);
    assert!(
        editor(&app)
            .diagnostics
            .as_ref()
            .unwrap()
            .contains("PINE_UNKNOWN_NAME")
    );
    assert_eq!(values(&app), good);
    compile_edit(&mut app, "plot(close * 4)\n", 2000);
    assert_ne!(values(&app), good);
}
#[test]
fn script_editor_manual_save_waits_for_validation() {
    let (mut app, _directory, _commands) = copy_fixture();
    compile_edit(&mut app, "plot(close * 2)\n", 0);
    let good = values(&app);
    edit(
        &mut app,
        Action::ReplaceSource {
            source: "plot(unknown_value)\n".into(),
        },
    );
    edit(&mut app, Action::Save);
    edit(
        &mut app,
        Action::ReplaceSource {
            source: "plot(other_unknown_value)\n".into(),
        },
    );
    app.runtime.indicators.last_script_poll = Instant::now() - Duration::from_secs(2);
    assert!(app.runtime.indicators.poll_script_files().is_empty());
    assert_eq!(values(&app), good);
    editor_frame(&mut app, 1000);
    editor_frame(&mut app, 1350);
    finish_validation(&mut app, 1351);
    assert!(editor(&app).diagnostics.is_some());
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
    app.indicator_wiring().apply_indicator_edit(
        crate::app::indicator_manager::IndicatorEdit::Inputs(target, inputs),
    );
    settle_indicators(&mut app);
    let output = values(&app);
    edit(&mut app, Action::Save);
    edit(&mut app, Action::Close);
    let saved = saved_flow_indicators(&app);
    let mut reopened = reopen_serialized_indicators(&directory, &saved);
    edit(
        &mut reopened,
        Action::Open {
            id: "my-ema.pine".into(),
        },
    );
    assert_eq!(
        editor(&reopened).document.as_ref().unwrap().source,
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

#[test]
fn script_editor_defaults_and_explicit_numeric_settings_match_restart() {
    use quantick_control_schema::script_editor::NumericValue;
    for explicit in [false, true] {
        let (mut app, directory, _commands) = copy_fixture();
        compile_edit(
            &mut app,
            "length = input.int(9, \"Length\", minval=1, maxval=30)\nplot(bar_index * length)\n",
            0,
        );
        assert_eq!(
            app.script_library_state(0, 64).numeric_inputs[0].value,
            NumericValue::Int(9)
        );
        if explicit {
            app.set_script_numeric_input("my-ema.pine", "length", NumericValue::Int(4))
                .unwrap();
            // Immediate retry before worker publication reads the durable setting.
            app.set_script_numeric_input("my-ema.pine", "length", NumericValue::Int(4))
                .unwrap();
            assert_eq!(
                app.script_library_state(0, 64).numeric_inputs[0].value,
                NumericValue::Int(4)
            );
        }
        compile_edit(
            &mut app,
            "length = input.int(17, \"Length\", minval=1, maxval=30)\nplot(bar_index * length + 1)\n",
            1000,
        );
        let expected = if explicit { 4 } else { 17 };
        assert_eq!(
            app.script_library_state(0, 64).numeric_inputs[0].value,
            NumericValue::Int(expected)
        );
        assert_eq!(values(&app)[0][39], (39 * expected + 1) as f64);
        let saved = saved_flow_indicators(&app);
        assert_eq!(
            saved[0].inputs.is_empty(),
            !explicit,
            "only committed settings are durable overrides"
        );
        let reopened = reopen_serialized_indicators(&directory, &saved);
        assert_eq!(values(&reopened), values(&app));
        assert_eq!(
            reopened.active_tab().flow_pane.indicators.all()[0].input_values[0],
            quantick_indicators::InputValue::Int(expected)
        );
    }
}

#[test]
fn script_editor_numeric_edits_wait_for_saved_validation_and_published_specs() {
    use quantick_control_schema::script_editor::NumericValue;
    let base = "length = input.int(9, \"Length\", minval=1, maxval=30)\nfactor = input.int(2, \"Factor\", minval=1, maxval=30)\nplot(bar_index * length * factor)\n";
    for declaration in [
        "length = input.int(17, \"Period\", minval=1, maxval=30)",
        "length = input.float(17.0, \"Length\", minval=1.0, maxval=30.0)",
        "length = input.int(17, \"Length\", minval=1, maxval=20)",
    ] {
        let (mut app, _directory, _commands) = copy_fixture();
        compile_edit(&mut app, base, 0);
        let source = format!(
            "{declaration}\nfactor = input.int(5, \"Factor\", minval=1, maxval=30)\nplot(bar_index * length * factor)\n"
        );
        edit(
            &mut app,
            Action::ReplaceSource {
                source: source.clone(),
            },
        );
        edit(&mut app, Action::Save);
        assert!(
            app.set_script_numeric_input("my-ema.pine", "factor", NumericValue::Int(4))
                .is_err()
        );
        assert!(app.script_library_state(0, 64).numeric_inputs.is_empty());
        let document = editor(&app).document.clone().unwrap();
        let revision = editor(&app).revision;
        let inputs = quantick_pine::compile(&source, &document.title)
            .unwrap()
            .inputs;
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        sender.send((revision, document, None, inputs)).unwrap();
        app.surfaces.script_editor.view.compilation = Some((revision, receiver));
        editor_frame(&mut app, 500);
        // The reload may have finished on the worker, but publication has not
        // been drained: its old declaration cannot authorize any input edit.
        assert!(
            app.set_script_numeric_input("my-ema.pine", "factor", NumericValue::Int(4))
                .is_err()
        );
        assert!(app.script_library_state(0, 64).numeric_inputs.is_empty());
        settle_indicators(&mut app);
        app.set_script_numeric_input("my-ema.pine", "factor", NumericValue::Int(4))
            .unwrap();
        settle_indicators(&mut app);
        assert_eq!(
            values(&app)[0][39],
            39.0 * 17.0 * 4.0,
            "the untouched first input adopts its new default"
        );
        assert_eq!(
            app.script_library_state(0, 64).numeric_inputs[1].value,
            NumericValue::Int(4)
        );
    }
}

#[test]
fn script_editor_initial_and_reopened_source_require_current_validation() {
    use quantick_control_schema::script_editor::NumericValue;
    let (mut app, _directory, _commands) = copy_fixture();
    assert!(
        app.set_script_numeric_input("my-ema.pine", "length", NumericValue::Int(4))
            .is_err()
    );
    validate_open_editor(&mut app, 0);
    app.set_script_numeric_input("my-ema.pine", "length", NumericValue::Int(4))
        .unwrap();
    settle_indicators(&mut app);
    let previous_output: Vec<_> = values(&app)
        .into_iter()
        .flatten()
        .map(f64::to_bits)
        .collect();
    let mut external = editor(&app).document.clone().unwrap();
    external.source =
        "factor = input.int(5, \"Factor\", minval=1, maxval=10)\nplot(bar_index * factor)\n".into();
    app.runtime
        .indicators
        .script_library
        .save(&external)
        .unwrap();
    edit(&mut app, Action::Open { id: external.id });
    assert!(app.script_library_state(0, 64).numeric_inputs.is_empty());
    assert!(
        app.set_script_numeric_input("my-ema.pine", "length", NumericValue::Int(3))
            .is_err()
    );
    validate_open_editor(&mut app, 1000);
    assert_eq!(
        values(&app)
            .into_iter()
            .flatten()
            .map(f64::to_bits)
            .collect::<Vec<_>>(),
        previous_output,
        "reading source never publishes a replacement"
    );
    assert!(
        app.set_script_numeric_input("my-ema.pine", "length", NumericValue::Int(3))
            .is_err()
    );
    poll_external_reload(&mut app);
    app.set_script_numeric_input("my-ema.pine", "factor", NumericValue::Int(3))
        .unwrap();
    assert_eq!(
        app.script_library_state(0, 64).numeric_inputs[0].name,
        "factor"
    );
}

#[test]
fn script_editor_external_default_changes_match_live_and_restart() {
    use quantick_control_schema::script_editor::NumericValue;
    for explicit in [false, true] {
        let (mut app, directory, _commands) = copy_fixture();
        compile_edit(
            &mut app,
            "length = input.int(9, \"Length\")\nplot(bar_index * length)\n",
            0,
        );
        if explicit {
            app.set_script_numeric_input("my-ema.pine", "length", NumericValue::Int(4))
                .unwrap();
            settle_indicators(&mut app);
        }
        let mut external = editor(&app).document.clone().unwrap();
        external.source =
            "length = input.int(17, \"Length\")\nplot(bar_index * length + 1)\n".into();
        app.runtime
            .indicators
            .script_library
            .save(&external)
            .unwrap();
        poll_external_reload(&mut app);
        let expected = if explicit { 4 } else { 17 };
        assert_eq!(
            app.active_tab().flow_pane.indicators.all()[0].input_values[0],
            quantick_indicators::InputValue::Int(expected)
        );
        assert_eq!(values(&app)[0][39], (39 * expected + 1) as f64);
        let saved = saved_flow_indicators(&app);
        assert_eq!(saved[0].inputs.is_empty(), !explicit);
        let reopened = reopen_serialized_indicators(&directory, &saved);
        assert_eq!(values(&reopened), values(&app));
    }
}

#[test]
fn script_editor_read_validation_never_republishes_an_old_open_snapshot() {
    use quantick_control_schema::script_editor::NumericValue;
    let (mut app, _directory, _commands) = copy_fixture();
    compile_edit(
        &mut app,
        "length = input.int(9, \"Length\")\nplot(bar_index * length)\n",
        0,
    );
    let mut external = editor(&app).document.clone().unwrap();
    external.source = "length = input.int(17, \"Length\")\nplot(bar_index * length)\n".into();
    app.runtime
        .indicators
        .script_library
        .save(&external)
        .unwrap();
    edit(
        &mut app,
        Action::Open {
            id: external.id.clone(),
        },
    );
    let revision = editor(&app).revision;
    let opened = editor(&app).document.clone().unwrap();
    external.source = "length = input.int(23, \"Length\")\nplot(bar_index * length)\n".into();
    app.runtime
        .indicators
        .script_library
        .save(&external)
        .unwrap();
    poll_external_reload(&mut app);
    let current = values(&app);
    assert_eq!(current[0][39], 39.0 * 23.0);
    // A clean Save must neither overwrite the newer file with this older Open
    // buffer nor suppress the file watcher's observation of that source.
    edit(&mut app, Action::Save);
    assert_eq!(
        ScriptLibraryPort::read(&app.runtime.indicators.script_library, &external.id)
            .unwrap()
            .source,
        external.source
    );
    let inputs = quantick_pine::compile(&opened.source, &opened.title)
        .unwrap()
        .inputs;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    sender.send((revision, opened, None, inputs)).unwrap();
    app.surfaces.script_editor.view.compilation = Some((revision, receiver));
    editor_frame(&mut app, 500);
    settle_indicators(&mut app);
    assert_eq!(values(&app), current);
    assert!(app.script_library_state(0, 64).numeric_inputs.is_empty());
    assert!(
        app.set_script_numeric_input("my-ema.pine", "length", NumericValue::Int(4))
            .is_err()
    );
    edit(&mut app, Action::Open { id: external.id });
    validate_open_editor(&mut app, 1000);
    assert_eq!(
        app.script_library_state(0, 64).numeric_inputs[0].value,
        NumericValue::Int(23)
    );
}

/// C9: the merged window stages still route editor keys and reloads before
/// the chart consumes them, while preserving its layout-owned slot.
#[test]
fn script_editor_survives_workspace_layout_frame_stages() {
    let (mut app, _directory, _commands) = copy_fixture();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let tab = app.runtime.tabs.active_id();
    let pane = app.active_tab().flow_pane.id;
    let slot = app.active_tab().flow_pane.indicators.all()[0].slot;
    let layout = app.layout_state().pane_layout(tab, PaneSide::Flow);
    let output = run_sized_frame(&mut app, &ctx, TEST_WINDOW, vec![key_press(egui::Key::F10)]);
    assert!(!app.chrome.window_menu.open, "editor owns menu shortcuts");
    assert!(painted_text_center(&output, "Editable copy · Saves automatically").is_some());
    edit(
        &mut app,
        Action::ReplaceSource {
            source: "indicator(\"Frame reload\", overlay=true)\nplot(close * 3)\n".into(),
        },
    );
    let now = Instant::now();
    let run = |app: &mut QuantickApp, now| {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            app.draw_frame_stage_test_order(
                ctx,
                now,
                &mut quantick_feed::spawn_live,
                [super::frame::FrameStage::Surfaces],
            );
        });
    };
    run(&mut app, now);
    run(&mut app, now + Duration::from_millis(350));
    let started = Instant::now();
    while app.surfaces.script_editor.view.compilation.is_some() {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
        run(&mut app, now + Duration::from_millis(351));
    }
    settle_indicators(&mut app);
    assert_eq!(app.runtime.tabs.active_id(), tab);
    assert_eq!(app.active_tab().flow_pane.id, pane);
    assert_eq!(app.layout_state().pane_layout(tab, PaneSide::Flow), layout);
    let view = &app.active_tab().flow_pane.indicators.all()[0];
    assert_eq!(view.slot, slot);
    assert_eq!(view.label(), "Frame reload");
    assert_eq!(values(&app)[0][39], 300.0);
}

#[test]
fn script_editor_rejected_paste_restores_visible_buffer_and_external_open_refreshes_it() {
    let (mut app, _directory, _commands) = copy_fixture();
    compile_edit(&mut app, "plot(close)", 0);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    let position = painted_text_center(&output, "plot(close)").expect("editable source paints");
    click_sized(&mut app, &ctx, TEST_WINDOW, position);
    let revision = editor(&app).revision;
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            key_press_with(egui::Key::A, egui::Modifiers::COMMAND),
            egui::Event::Paste("x".repeat(quantick_stores::script_library::MAX_SOURCE_BYTES + 1)),
        ],
    );
    assert_eq!(editor(&app).revision, revision);
    assert_eq!(
        app.surfaces.toast.message(),
        Some("source exceeds 256 KiB"),
        "source paste must reach the host refusal; focus: {:?}",
        ctx.memory(|memory| memory.focused())
    );
    // The new toast Area needs a layout pass before its text paints.
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    assert!(
        painted_text_center(&output, "plot(close)").is_some(),
        "rejected edit reverts even without a revision change"
    );
    assert!(
        painted_text(&output)
            .iter()
            .any(|text| text.contains("source exceeds 256 KiB")),
        "real ReplaceSource refusal reached the toast"
    );
    let mut document = editor(&app).document.clone().unwrap();
    document.source = "plot(high)".into();
    app.runtime
        .indicators
        .script_library
        .save(&document)
        .unwrap();
    edit(&mut app, Action::Open { id: document.id });
    let output = run_frame(&mut app, &ctx);
    assert!(painted_text_center(&output, "plot(high)").is_some());
    assert!(painted_text_center(&output, "plot(close)").is_none());
}

#[test]
fn script_editor_action_batch_settles_success_and_refusal_before_next_request() {
    let (mut app, _directory, _commands) = copy_fixture();
    let now = app.surfaces.script_editor.view.epoch;
    crate::app::script_editor_wiring::frame(
        &mut app.surfaces,
        &mut app.runtime.indicators,
        &mut app.runtime.tabs,
        app.runtime.workspace.layouts(),
        vec![
            Action::Open {
                id: "ema.pine".into(),
            },
            Action::ReplaceSource {
                source: "plot(low)".into(),
            },
            Action::Copy {
                id: "ema.pine".into(),
                new_id: "batch.pine".into(),
            },
            Action::ReplaceSource {
                source: "plot(high)".into(),
            },
            Action::Save,
        ],
        now,
    );
    let readback = app.edit_script(Action::Inspect).unwrap().unwrap();
    assert_eq!(readback.id, "batch.pine");
    assert_eq!(readback.source, "plot(high)");
    assert_eq!(
        ScriptLibraryPort::read(&app.runtime.indicators.script_library, "batch.pine").unwrap(),
        readback
    );
    assert!(app.edit_script(Action::Close).is_err());
    assert_eq!(app.edit_script(Action::Inspect).unwrap(), Some(readback));
}
