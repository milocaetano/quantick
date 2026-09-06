use super::*;

use crate::indicators::state_file::SavedKind;
use crate::workspace_store::LayoutSave;

fn assert_one_indicator_save(app: &mut QuantickApp) {
    let store = app.workspace.layouts_mut();
    // Worker completion may outlast the debounce; save intent has no deadline.
    assert!(store.is_dirty(), "the edit leaves a pending save");
    assert_eq!(store.take_flush(), LayoutSave::Write, "one save is pending");
    assert!(!store.is_dirty(), "flushing clears the pending save");
    assert_eq!(
        store.take_flush(),
        LayoutSave::Wait,
        "the save intent is consumed exactly once"
    );
}

#[test]
fn human_script_operations_mirror_two_panes_and_save_once() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 40);
    let tab = app.active_tab().id;
    let layout = app.pane_layout(tab, PaneSide::Flow);
    assert_eq!(layout, app.pane_layout(tab, PaneSide::Time(0)));
    app.workspace.layouts_mut().take_flush();
    let index = app
        .indicators
        .script_library
        .entries()
        .iter()
        .position(|entry| entry.name == "ema.pine")
        .unwrap();
    app.apply_toolbar_action(ToolbarAction::AddScriptIndicator(index));
    settle_indicators(&mut app);
    assert_one_indicator_save(&mut app);
    assert_eq!(app.layouts().get(layout).unwrap().indicators.len(), 1);
    let targets: Vec<_> = app
        .indicators
        .slot_kinds
        .iter()
        .map(|(target, _)| *target)
        .collect();
    assert_eq!(targets.len(), 2);
    assert_eq!(
        targets[0].slot, targets[1].slot,
        "pane-local slot numbers collide"
    );
    for side in [PaneSide::Flow, PaneSide::Time(0)] {
        let views = app.active_tab().pane(side).indicators.all();
        assert_eq!(views.len(), 1);
        assert!(views[0].error.is_none());
        assert!(views[0].label().contains("EMA"));
    }
    // Remove from the other pane, with focus left on the original pane.
    // The legend captures this address; mirroring must not reinterpret focus.
    let origin = targets
        .iter()
        .find(|target| target.side != app.active_tab().focused_side())
        .copied()
        .unwrap();
    assert_ne!(app.active_tab().focused_side(), origin.side);
    app.remove_indicator_at(origin);
    settle_indicators(&mut app);
    assert_one_indicator_save(&mut app);
    assert!(app.layouts().get(layout).unwrap().indicators.is_empty());
    assert!(app.indicators.slot_kinds.is_empty());
    for side in [PaneSide::Flow, PaneSide::Time(0)] {
        assert!(app.active_tab().pane(side).indicators.all().is_empty());
    }
}

#[test]
fn operator_script_operations_preserve_human_mirrors_and_saved_content() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 40);
    app.apply_toolbar_action(ToolbarAction::AddScriptIndicator(0));
    settle_indicators(&mut app);
    let saved = app.layouts().clone();
    let human_targets = app.indicators.slot_kinds.clone();
    app.workspace.layouts_mut().take_flush();
    let attached = app
        .run_agent_action(
            "indicator.script.attach",
            serde_json::json!({
                "name": "overlay", "source": "//@version=5\nindicator(\"overlay\")\nplot(close)\n"
            }),
        )
        .unwrap();
    settle_indicators(&mut app);
    assert_one_indicator_save(&mut app);
    assert_eq!(
        *app.layouts(),
        saved,
        "operator source never enters saved layouts"
    );
    assert_eq!(app.indicators.slot_kinds.len(), human_targets.len() + 1);
    let slot = attached["slot_id"].as_str().unwrap();
    let detached = app
        .run_agent_action(
            "indicator.script.detach",
            serde_json::json!({"slot_id": slot}),
        )
        .unwrap();
    assert_eq!(detached["detached"], true);
    settle_indicators(&mut app);
    assert_one_indicator_save(&mut app);
    assert_eq!(app.indicators.slot_kinds, human_targets);
    assert_eq!(*app.layouts(), saved);
    let refused = app
        .run_agent_action(
            "indicator.script.detach",
            serde_json::json!({
                "slot_id": human_targets[0].0.slot.0.to_string()
            }),
        )
        .unwrap_err();
    assert_eq!(
        refused.code.as_str(),
        quantick_control::error::codes::PERMISSION_DENIED
    );
    assert_eq!(app.workspace.layouts_mut().take_flush(), LayoutSave::Wait);
    assert_eq!(app.indicators.slot_kinds, human_targets);
    assert_eq!(*app.layouts(), saved);
}

#[test]
fn invalid_control_script_leaves_both_panes_layout_and_save_intent_unchanged() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 40);
    app.apply_toolbar_action(ToolbarAction::AddScriptIndicator(0));
    settle_indicators(&mut app);
    let before = app.layouts().clone();
    let slots = app.indicators.slot_kinds.clone();
    let owners = app.indicators.operator_slots.clone();
    app.workspace.layouts_mut().take_flush();
    let input = serde_json::json!({
        "name": "broken", "source": "//@version=5\nindicator(\"broken\")\nplot(\n"
    });
    for human in [true, false] {
        let result = if human {
            app.control_action(
                "indicator.script.attach",
                1,
                crate::control::ActionOrigin::Human,
                input.clone(),
            )
        } else {
            app.run_agent_action("indicator.script.attach", input.clone())
        };
        assert_eq!(
            result.unwrap_err().code.as_str(),
            quantick_control::error::codes::INVALID_REQUEST
        );
        settle_indicators(&mut app);
        assert_eq!(*app.layouts(), before);
        assert_eq!(app.indicators.slot_kinds, slots);
        assert_eq!(app.indicators.operator_slots, owners);
        assert_eq!(app.workspace.layouts_mut().take_flush(), LayoutSave::Wait);
        for side in [PaneSide::Flow, PaneSide::Time(0)] {
            assert_eq!(app.active_tab().pane(side).indicators.all().len(), 1);
        }
    }
}

#[test]
fn invalid_library_script_retains_mirrored_error_slots_for_repair() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 40);
    let directory = crate::scratch::ScratchDir::new("f1-broken-script");
    std::fs::write(
        directory.path().join("broken.pine"),
        "//@version=5\nindicator(\"broken\")\nplot(\n",
    )
    .unwrap();
    app.indicators.script_library =
        crate::indicators::library::ScriptLibrary::scan_dir(directory.path());
    let index = app
        .indicators
        .script_library
        .entries()
        .iter()
        .position(|entry| entry.name == "broken.pine")
        .unwrap();
    app.workspace.layouts_mut().take_flush();
    app.apply_toolbar_action(ToolbarAction::AddScriptIndicator(index));
    settle_indicators(&mut app);
    assert_one_indicator_save(&mut app);
    assert_eq!(
        app.indicators.script_files.len(),
        2,
        "both slots still follow the source file"
    );
    assert_eq!(app.indicators.slot_kinds.len(), 2);
    let layout = app.focused_pane_layout();
    assert_eq!(
        app.layouts().get(layout).unwrap().indicators[0].kind,
        SavedKind::Script {
            name: "broken.pine".into()
        }
    );
    for side in [PaneSide::Flow, PaneSide::Time(0)] {
        let views = app.active_tab().pane(side).indicators.all();
        assert_eq!(views.len(), 1);
        assert!(
            views[0].error.is_some(),
            "the invalid source remains visible for repair"
        );
    }
}
