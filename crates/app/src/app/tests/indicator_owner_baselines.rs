use super::*;
use crate::app::QuantickApp;
use crate::app::tests::update_indicator_settings;
use crate::app::tests::{click_chart, pane_point, settle_indicators, split_app};
use crate::indicators::state_file::SavedInput;
use crate::toolbar::ToolbarAction;
use quantick_indicators::{InputValue, SourceId};
use quantick_ui_model::indicator_settings::{Intent, SettingsOutcome};

fn dialog_app() -> QuantickApp {
    let (mut app, _commands) = split_app(&egui::Context::default(), 200);
    app.apply_toolbar_action(ToolbarAction::AddNative("native.ema"));
    settle_indicators(&mut app);
    let target = TabSlot {
        workspace: crate::app::WORKSPACE_ID,
        pane: app.active_tab().pane(PaneSide::Time(0)).id,
        tab: app.runtime.tabs.active_id(),
        side: PaneSide::Time(0),
        slot: app.active_tab().pane(PaneSide::Time(0)).indicators.all()[0].slot,
    };
    app.indicator_wiring().apply_indicator_legend_action(
        target.tab,
        target.side,
        crate::indicator_legend::LegendAction::OpenSettings(target.slot),
    );
    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &egui::Context::default(), point);
    app
}

fn dispatch_preset(
    app: &mut QuantickApp,
    specs: &[quantick_indicators::InputSpec],
    outcome: SettingsOutcome,
) {
    let target = app.surfaces.indicator_settings.model_state.target();
    let kind = target.and_then(|target| app.runtime.indicators.registry.kind(target));
    let mut changes = Vec::new();
    app.surfaces.indicator_settings.with_updates(
        |response| changes.extend(response.model_effects),
        |_, session| {
            crate::surfaces::indicator_settings::dispatch(
                session,
                target,
                kind,
                specs,
                &mut app.runtime.indicators.indicator_presets,
                app.runtime.workspace.indicator_presets_path(),
                Intent::FinishPaint {
                    changed: false,
                    style_changed: false,
                    outcome,
                    window_closed: false,
                    escape: false,
                },
            )
        },
    );
    for effect in changes {
        if let quantick_ui_model::HostEffect::IndicatorSettings(effects) = effect {
            for change in effects.changes {
                app.indicator_wiring()
                    .apply_indicator_settings_change(change.into());
            }
        }
    }
}

#[test]
fn i1_preview_and_discard_keep_the_original_target_and_committed_values() {
    let mut app = dialog_app();
    let target = app
        .surfaces
        .indicator_settings
        .model_state
        .target()
        .unwrap();
    assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);
    let before = app
        .surfaces
        .indicator_settings
        .model_state
        .dialog()
        .unwrap()
        .committed
        .clone();
    let other = app.active_tab().flow_pane.indicators.all()[0]
        .input_values
        .clone();
    update_indicator_settings(
        &mut app,
        Intent::EditInput {
            index: 0,
            value: InputValue::Int(37),
        },
    );
    update_indicator_settings(&mut app, Intent::Preview);
    settle_indicators(&mut app);
    assert_eq!(
        app.active_tab().pane(target.side).indicators.all()[0].input_values[0],
        InputValue::Int(37)
    );
    assert_eq!(
        app.active_tab().flow_pane.indicators.all()[0].input_values,
        other
    );
    assert!(
        app.surfaces
            .indicator_settings
            .model_state
            .dialog()
            .unwrap()
            .previewed
    );
    assert_eq!(
        app.surfaces
            .indicator_settings
            .model_state
            .dialog()
            .unwrap()
            .committed,
        before
    );
    assert!(
        crate::app::tests::pane_set(&app, PaneSide::Flow)[0]
            .inputs
            .is_empty()
    );
    update_indicator_settings(&mut app, Intent::Close);
    settle_indicators(&mut app);
    assert_eq!(
        app.active_tab().pane(target.side).indicators.all()[0].input_values,
        before
    );
    assert_eq!(
        app.active_tab().flow_pane.indicators.all()[0].input_values,
        other
    );
}

#[test]
fn i1_preset_keeps_an_invalid_cell_position_before_a_valid_neighbor() {
    let mut app = dialog_app();
    let target = app
        .surfaces
        .indicator_settings
        .model_state
        .target()
        .unwrap();
    let kind = app
        .runtime
        .indicators
        .registry
        .slot_kinds()
        .iter()
        .find(|(at, _)| *at == target)
        .unwrap()
        .1
        .clone();
    let original = app
        .surfaces
        .indicator_settings
        .model_state
        .dialog()
        .unwrap()
        .draft
        .clone();
    assert!(app.runtime.indicators.indicator_presets.insert(
        &kind,
        "positional",
        vec![
            SavedInput::Source("not-a-source".to_owned()),
            SavedInput::Source("open".to_owned()),
        ]
    ));
    let specs = app.active_tab().pane(target.side).indicators.all()[0]
        .descriptor
        .inputs
        .clone();
    dispatch_preset(
        &mut app,
        &specs,
        SettingsOutcome::LoadPreset(Some("positional".to_owned())),
    );
    let dialog = app
        .surfaces
        .indicator_settings
        .model_state
        .dialog()
        .unwrap();
    assert_eq!(dialog.draft[0], original[0]);
    assert_eq!(dialog.draft[1], InputValue::Source(SourceId::Open));
    assert_eq!(dialog.committed, original);
    assert!(dialog.previewed);
    assert_eq!(dialog.preset_label.as_deref(), Some("positional"));
    assert!(
        crate::app::tests::pane_set(&app, PaneSide::Flow)[0]
            .inputs
            .is_empty()
    );
}

#[test]
fn i1_failed_file_reread_advances_seen_mtime_without_changing_live_slots() {
    let (mut app, _commands) = split_app(&egui::Context::default(), 200);
    let directory = crate::scratch::ScratchDir::new("i1-reload");
    let file = directory.path().join("reload.pine");
    std::fs::write(&file, "//@version=5\nindicator(\"reload\")\nplot(close)\n").unwrap();
    app.runtime.indicators.script_library = ScriptLibrary::scan_dir(directory.path());
    let index = app
        .runtime
        .indicators
        .script_library
        .entries()
        .iter()
        .position(|e| e.name == "reload.pine")
        .unwrap();
    app.apply_toolbar_action(ToolbarAction::AddScriptIndicator(index));
    settle_indicators(&mut app);
    let before: Vec<_> = app
        .active_tab()
        .panes()
        .map(|(pane, _)| {
            pane.indicators
                .all()
                .iter()
                .map(|v| (v.slot, v.label().to_owned()))
                .collect::<Vec<_>>()
        })
        .collect();
    std::fs::write(&file, [0xff, 0xfe]).unwrap();
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    for (owner, _, _) in app.runtime.indicators.registry.script_files().to_vec() {
        app.runtime
            .indicators
            .registry
            .observe_script(owner, std::time::SystemTime::UNIX_EPOCH);
    }
    app.runtime.indicators.last_script_poll = Instant::now() - Duration::from_secs(2);
    assert!(app.runtime.indicators.poll_script_files().is_empty());
    assert!(!app.runtime.indicators.registry.script_files().is_empty());
    assert!(
        app.runtime
            .indicators
            .registry
            .script_files()
            .iter()
            .all(|(_, _, seen)| *seen == modified)
    );
    settle_indicators(&mut app);
    let after: Vec<_> = app
        .active_tab()
        .panes()
        .map(|(pane, _)| {
            pane.indicators
                .all()
                .iter()
                .map(|v| (v.slot, v.label().to_owned()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(after, before);
}

#[test]
fn i1_origin_watch_observes_file_time_only_after_layout_mirroring() {
    let (mut app, _commands) = split_app(&egui::Context::default(), 200);
    let directory = crate::scratch::ScratchDir::new("i1-watch-phase");
    let file = directory.path().join("phase.pine");
    std::fs::write(&file, "//@version=5\nindicator(\"phase\")\nplot(close)\n").unwrap();
    app.runtime.indicators.script_library = ScriptLibrary::scan_dir(directory.path());
    let index = app
        .runtime
        .indicators
        .script_library
        .entries()
        .iter()
        .position(|entry| entry.name == "phase.pine")
        .unwrap();
    let target = (
        app.runtime.tabs.active_id(),
        app.active_tab().focused_side(),
    );
    let (_, added) = app
        .runtime
        .indicators
        .add_library(
            app.runtime
                .tabs
                .runtime_mut(app.runtime.tabs.active_index())
                .pane_mut(target.1),
            target,
            index,
        )
        .unwrap();
    let observed_after_read =
        std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(observed_after_read))
        .unwrap();
    app.indicator_wiring()
        .apply_indicator_edit(IndicatorEdit::Attached(added.attachment.unwrap()));
    assert_eq!(
        app.runtime.indicators.registry.script_files().len(),
        1,
        "only the mirrored peer watches before the origin is registered"
    );
    app.runtime.indicators.watch_attachment(added.watch);
    assert_eq!(
        app.runtime.indicators.registry.script_files().len(),
        2,
        "one peer and one origin registration"
    );
    assert!(
        app.runtime
            .indicators
            .registry
            .script_files()
            .iter()
            .all(|(_, _, seen)| *seen == observed_after_read)
    );
}
