use super::*;
use crate::app::LayoutPort;
use crate::app::QuantickApp;
use crate::app::tests::{click_chart, pane_point, settle_indicators, split_app};
use crate::toolbar::ToolbarAction;
use quantick_indicators::{InputValue, SourceId};

fn dialog_app() -> QuantickApp {
    let (mut app, _commands) = split_app(&egui::Context::default(), 200);
    app.apply_toolbar_action(ToolbarAction::AddNative("native.ema"));
    settle_indicators(&mut app);
    let target = TabSlot {
        tab: app.tabs.active_id(),
        side: PaneSide::Time(0),
        slot: app.active_tab().pane(PaneSide::Time(0)).indicators.all()[0].slot,
    };
    app.apply_indicator_legend_action(
        target.tab,
        target.side,
        crate::indicator_legend::LegendAction::OpenSettings(target.slot),
    );
    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &egui::Context::default(), point);
    app
}

#[test]
fn i1_preview_and_discard_keep_the_original_target_and_committed_values() {
    let mut app = dialog_app();
    let target = app.indicators.indicator_settings_target;
    assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);
    let before = app
        .indicators
        .indicator_settings
        .as_ref()
        .unwrap()
        .committed
        .clone();
    let other = app.active_tab().flow_pane.indicators.all()[0]
        .input_values
        .clone();
    app.indicators.indicator_settings.as_mut().unwrap().draft[0] = InputValue::Int(37);
    let change = app.indicators.preview_settings();
    app.apply_indicator_settings_change(change);
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
        app.indicators
            .indicator_settings
            .as_ref()
            .unwrap()
            .previewed
    );
    assert_eq!(
        app.indicators
            .indicator_settings
            .as_ref()
            .unwrap()
            .committed,
        before
    );
    assert!(
        app.layout_state().layouts().active().indicators[0]
            .inputs
            .is_empty()
    );
    let change = app.indicators.discard_preview();
    app.apply_indicator_settings_change(change);
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
    let target = app.indicators.indicator_settings_target;
    let kind = app
        .indicators
        .slot_kinds
        .iter()
        .find(|(at, _)| *at == target)
        .unwrap()
        .1
        .clone();
    let original = app
        .indicators
        .indicator_settings
        .as_ref()
        .unwrap()
        .draft
        .clone();
    assert!(app.indicators.indicator_presets.insert(
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
    let change = app
        .indicators
        .load_preset(Some("positional".to_owned()), &specs);
    app.apply_indicator_settings_change(change);
    let dialog = app.indicators.indicator_settings.as_ref().unwrap();
    assert_eq!(dialog.draft[0], original[0]);
    assert_eq!(dialog.draft[1], InputValue::Source(SourceId::Open));
    assert_eq!(dialog.committed, original);
    assert!(dialog.previewed);
    assert_eq!(dialog.preset_label.as_deref(), Some("positional"));
    assert!(
        app.layout_state().layouts().active().indicators[0]
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
    app.indicators.script_library = ScriptLibrary::scan_dir(directory.path());
    let index = app
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
    for (_, _, seen) in &mut app.indicators.script_files {
        *seen = std::time::SystemTime::UNIX_EPOCH;
    }
    app.indicators.last_script_poll = Instant::now() - Duration::from_secs(2);
    assert!(app.indicators.poll_script_files().is_empty());
    assert!(!app.indicators.script_files.is_empty());
    assert!(
        app.indicators
            .script_files
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
    app.indicators.script_library = ScriptLibrary::scan_dir(directory.path());
    let index = app
        .indicators
        .script_library
        .entries()
        .iter()
        .position(|entry| entry.name == "phase.pine")
        .unwrap();
    let target = (app.tabs.active_id(), app.active_tab().focused_side());
    let (_, added) = app
        .indicators
        .add_library(
            app.tabs
                .runtime_mut(app.tabs.active_index())
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
    app.apply_indicator_edit(IndicatorEdit::Attached(added.attachment.unwrap()));
    assert_eq!(
        app.indicators.script_files.len(),
        1,
        "only the mirrored peer watches before the origin is registered"
    );
    app.indicators.watch_attachment(added.watch);
    assert_eq!(
        app.indicators.script_files.len(),
        2,
        "one peer and one origin registration"
    );
    assert!(
        app.indicators
            .script_files
            .iter()
            .all(|(_, _, seen)| *seen == observed_after_read)
    );
}
