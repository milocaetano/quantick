//! `settings.open` and `desk.*` over the control plane, and the retired name.
use super::*;

use quantick_control::error::{ControlError, codes};
use quantick_control_schema::desk::{
    AGENT_ACCESS_REFUSED_CODE, DESK_FORGOTTEN_EVENT_KIND, DESK_REOPEN_EVENT_KIND,
    DESK_SAVED_EVENT_KIND,
};
use quantick_control_schema::settings_sections::SettingsSection;

const COCKPIT_SCOPES: &[&str] = &["cockpit", "cockpit.layout"];
const DESK_SCOPES: &[&str] = &["cockpit", "cockpit.layout", "cockpit.desk"];

fn failure(response: &quantick_control::wire::ResponseEnvelope) -> &ControlError {
    match &response.outcome {
        ResponseOutcome::Failure { error } => error,
        other => panic!("expected a failure, got {other:?}"),
    }
}

/// A window with a scratch workspace file and a live gateway granting
/// everything the desk calls need.
fn granted(name: &str) -> (QuantickApp, egui::Context, std::path::PathBuf) {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    app.runtime
        .workspace
        .set_ui_state_path(scratch_ui_state(name));
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory(name);
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout,cockpit.desk");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    (app, ctx, directory)
}

fn journal_kinds(app: &QuantickApp) -> Vec<String> {
    app.control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 256, 1 << 20)
        .events
        .iter()
        .map(|event| event.kind.as_str().to_owned())
        .collect()
}

#[test]
fn settings_open_needs_a_grant_shows_each_section_and_the_scene_says_which() {
    const CAPABILITY: &str = "settings.open";
    let (mut app, ctx, directory) = granted("settings-open-control");
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut client = connect(&directory, &options("cockpit", COCKPIT_SCOPES));

    let (denied, _) = unkeyed_call(&mut app, &mut observer, CAPABILITY, json!({}));
    assert_eq!(failure(&denied).code.as_str(), codes::PERMISSION_DENIED);
    assert!(app.surfaces.settings_window.model_state.shown().is_none());

    let (shown, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, json!({}));
    assert_eq!(success_result(&shown)["section"], "display");
    for (token, section) in [
        ("display", SettingsSection::Display),
        ("data", SettingsSection::Data),
        ("recording", SettingsSection::Recording),
    ] {
        let input = json!({ "section": token });
        let (shown, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, input);
        assert_eq!(success_result(&shown)["section"], token);
        assert_eq!(
            app.surfaces.settings_window.model_state.shown(),
            Some(section)
        );
        let scene = observer_scene(&app);
        assert_eq!(
            scene_control(&scene, section.control_id())["selected"],
            true
        );
    }
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn a_connection_cannot_bring_up_the_agent_access_section() {
    let (mut app, ctx, directory) = granted("settings-agent-refused");
    let mut client = connect(&directory, &options("cockpit", COCKPIT_SCOPES));
    let input = json!({ "section": "agent_access" });
    let (refused, _) = unkeyed_call(&mut app, &mut client, "settings.open", input);
    let error = failure(&refused);
    assert_eq!(error.code.as_str(), codes::PERMISSION_DENIED);
    assert_eq!(
        error.context.details.as_ref().unwrap()["code"],
        AGENT_ACCESS_REFUSED_CODE
    );
    assert!(
        app.surfaces.settings_window.model_state.shown().is_none(),
        "nothing came up"
    );
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn workspace_save_is_refused_by_a_code_naming_both_successors() {
    let (mut app, ctx, directory) = granted("workspace-save-retired");
    let saved = app.runtime.workspace.session().saved();
    for profile in [options("observer", &[]), options("cockpit", COCKPIT_SCOPES)] {
        let mut client = connect(&directory, &profile);
        let (refused, _) = unkeyed_call(&mut app, &mut client, "workspace.save", json!({}));
        let error = failure(&refused);
        assert_eq!(error.code.as_str(), codes::CAPABILITY_RETIRED);
        assert_eq!(
            error.context.details.as_ref().unwrap()["successors"],
            json!(["desk.save", "workspace.save_to"])
        );
        assert_eq!(
            error.context.details.as_ref().unwrap()["planned"],
            json!([])
        );
        assert!(error.message.contains("desk.save"));
        assert!(error.message.contains("workspace.save_to"));
        assert!(!error.message.contains("not built"));
    }
    assert_eq!(app.runtime.workspace.session().saved(), saved);
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn desk_save_writes_the_file_and_forget_removes_it() {
    let (mut app, ctx, directory) = granted("desk-save-forget");
    let mut observer = connect(&directory, &options("observer", &[]));
    let (denied, _) = unkeyed_call(&mut app, &mut observer, "desk.save", json!({}));
    assert_eq!(failure(&denied).code.as_str(), codes::PERMISSION_DENIED);
    assert!(!app.runtime.workspace.session().saved());

    let mut layout = connect(&directory, &options("cockpit", COCKPIT_SCOPES));
    for capability in ["desk.save", "desk.forget", "desk.export", "desk.import"] {
        let (denied, _) = unkeyed_call(&mut app, &mut layout, capability, json!({}));
        assert_eq!(
            failure(&denied).code.as_str(),
            codes::PERMISSION_DENIED,
            "{capability} takes the desk's own grant"
        );
    }
    assert!(!app.runtime.workspace.picker_mut().is_open());

    let mut client = connect(&directory, &options("cockpit", DESK_SCOPES));
    let (saved, _) = unkeyed_call(&mut app, &mut client, "desk.save", json!({}));
    assert_eq!(success_result(&saved)["written"], true);
    assert!(app.runtime.workspace.session().saved());

    let (forgotten, _) = unkeyed_call(&mut app, &mut client, "desk.forget", json!({}));
    assert_eq!(success_result(&forgotten)["written"], true);
    assert!(!app.runtime.workspace.session().saved());
    let (again, _) = unkeyed_call(&mut app, &mut client, "desk.forget", json!({}));
    assert_eq!(
        success_result(&again)["written"],
        false,
        "nothing left to forget"
    );

    let kinds = journal_kinds(&app);
    assert!(kinds.iter().any(|kind| kind == DESK_SAVED_EVENT_KIND));
    assert!(kinds.iter().any(|kind| kind == DESK_FORGOTTEN_EVENT_KIND));
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn reopen_on_launch_moves_the_choice_writes_it_and_a_repeat_changes_nothing() {
    const CAPABILITY: &str = "desk.reopen_on_launch.set";
    let (mut app, ctx, directory) = granted("desk-reopen");
    let mut client = connect(&directory, &options("cockpit", COCKPIT_SCOPES));
    let before = app.runtime.workspace.session().save_on_exit();
    let input = json!({ "reopen": !before });
    let (moved, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, input.clone());
    let answer = success_result(&moved);
    assert_eq!(answer["changed"], true);
    assert_eq!(answer["written"], true);
    assert_eq!(app.runtime.workspace.session().save_on_exit(), !before);

    let (repeat, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, input);
    assert_eq!(success_result(&repeat)["changed"], false);
    let moved_events = journal_kinds(&app)
        .iter()
        .filter(|kind| *kind == DESK_REOPEN_EVENT_KIND)
        .count();
    assert_eq!(moved_events, 1, "the repeat journals nothing");
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn the_reopen_choice_leaves_the_saved_desk_alone() {
    let (mut app, ctx, directory) = granted("desk-reopen-standing");
    let mut client = connect(&directory, &options("cockpit", COCKPIT_SCOPES));
    let before = app.runtime.workspace.session().save_on_exit();
    let input = json!({ "reopen": !before });
    let (moved, _) = unkeyed_call(&mut app, &mut client, "desk.reopen_on_launch.set", input);
    assert_eq!(success_result(&moved)["written"], true);
    let file = crate::ui_state::load(app.runtime.workspace.ui_state_path());
    assert_eq!(file.save_on_exit, !before);
    assert!(
        file.tabs.is_empty(),
        "writing the choice did not make what is on screen the startup desk"
    );
    assert!(
        !app.runtime.workspace.session().saved(),
        "a choice is not a desk"
    );
    let mut desk = connect(&directory, &options("cockpit", DESK_SCOPES));
    let (forget, _) = unkeyed_call(&mut app, &mut desk, "desk.forget", json!({}));
    assert_eq!(success_result(&forget)["written"], false);
    let file = crate::ui_state::load(app.runtime.workspace.ui_state_path());
    assert_eq!(file.save_on_exit, !before, "the choice survives");
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn forgetting_with_nothing_saved_says_nothing_changed() {
    let (mut app, ctx, directory) = granted("desk-forget-nothing");
    let mut client = connect(&directory, &options("cockpit", DESK_SCOPES));
    let (answer, _) = unkeyed_call(&mut app, &mut client, "desk.forget", json!({}));
    assert_eq!(success_result(&answer)["written"], false);
    let forgotten = journal_kinds(&app)
        .iter()
        .filter(|kind| *kind == DESK_FORGOTTEN_EVENT_KIND)
        .count();
    assert_eq!(forgotten, 0);
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn export_and_import_refuse_a_path_before_any_dialog() {
    let (mut app, ctx, directory) = granted("desk-no-path");
    let mut client = connect(&directory, &options("cockpit", DESK_SCOPES));
    for capability in ["desk.export", "desk.import"] {
        let input = json!({ "path": "C:/elsewhere.toml" });
        let (refused, _) = unkeyed_call(&mut app, &mut client, capability, input);
        assert_eq!(failure(&refused).code.as_str(), codes::INVALID_REQUEST);
    }
    assert!(!app.runtime.workspace.picker_mut().is_open());
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn the_agent_on_pill_is_amber_and_opens_agent_access() {
    let (mut app, ctx, _directory) = granted("agent-pill");
    let output = run_frame(&mut app, &ctx);
    let label = quantick_control_schema::window_menu::AGENT_PILL_LABEL;
    assert_eq!(label, "Agent on");
    assert!(painted_text_center(&output, label).is_some());
    assert!(painted_text_center(&output, "agent").is_none());
    let amber = output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Text(text)
            if text.galley.text() == label
                && text.galley.job.sections[0].format.color == crate::theme::AMBER)
    });
    assert!(amber, "the pill's words wear amber");
    let pill = app.chrome.title_bar.control_rect("agent").unwrap();
    click_sized(&mut app, &ctx, TEST_WINDOW, pill.center());
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.surfaces.settings_window.model_state.shown(),
        Some(SettingsSection::AgentAccess)
    );
    let output = run_frame(&mut app, &ctx);
    for row in ["Local agent access", "Manage clients…"] {
        assert!(painted_text_center(&output, row).is_some(), "{row}");
    }
    disable_test_gateway(&mut app, &ctx);
}

/// The named call lands on the owners the rows' clicks reach, and what it
/// answers is what the workspace summary reads back.
#[test]
fn settings_set_moves_the_rows_owners_and_the_summary_agrees() {
    let (mut app, ctx, directory) = granted("settings-set");
    let mut client = connect(&directory, &options("cockpit", COCKPIT_SCOPES));
    let (perf, progressive) = (
        !app.health.show_perf,
        !app.runtime.history.progressive_history,
    );
    let lead_in = !app.runtime.history.venue_lead_in;
    let input = json!({ "timezone_offset_minutes": 60, "perf_readings": perf,
        "progressive_history": progressive, "venue_lead_in": lead_in });
    let (set, _) = unkeyed_call(&mut app, &mut client, "settings.set", input);
    let answer = success_result(&set);
    assert_eq!(app.runtime.tz, crate::timezone::TzOffset::new(60));
    assert_eq!(app.health.show_perf, perf);
    assert_eq!(app.runtime.history.progressive_history, progressive);
    assert_eq!(app.runtime.history.venue_lead_in, lead_in);
    let mut registry = crate::control::standard_registry().unwrap();
    let scope = observer_scope("workspace.summary");
    let capture = (registry.capture(&app, &observer_instance(), std::slice::from_ref(&scope)))
        .unwrap()
        .into_serialized()
        .unwrap();
    let summary = &capture.scopes[&scope].value;
    for (field, read) in [
        ("timezone_offset_minutes", "timezone_offset_minutes"),
        ("perf_readings", "performance_readings_visible"),
        ("progressive_history", "progressive_venue_history"),
        ("venue_lead_in", "venue_lead_in"),
    ] {
        assert_eq!(answer[field], summary[read], "{field}");
    }
    disable_test_gateway(&mut app, &ctx);
}

/// The rows no named call sets reach their owners through the same door.
#[test]
fn the_desk_door_settles_the_record_and_agent_rows_on_the_window() {
    use quantick_control_handlers::desk::DeskHost as _;
    use quantick_control_handlers::settings::SettingsAct;
    let (mut app, _evt, _cmd, _book) = test_app();
    run_frame(&mut app, &egui::Context::default());
    let settle = |app: &mut QuantickApp, act| app.desk().unwrap().settle(act).unwrap();
    let record = !app.desk().unwrap().readings().record_deals;
    settle(&mut app, SettingsAct::RecordDeals(record));
    assert_eq!(app.chrome.record_deals, Some(record));
    assert_eq!(app.desk().unwrap().readings().record_deals, record);
    app.control.control_access = None;
    settle(&mut app, SettingsAct::OpenAgentAccess);
    assert!(
        !app.surfaces.access_panel.model_state.open,
        "no gateway, no panel"
    );
    app.control.control_access = Some(crate::control::ControlAccess::new());
    settle(&mut app, SettingsAct::OpenAgentAccess);
    assert!(app.surfaces.access_panel.model_state.open);
}
