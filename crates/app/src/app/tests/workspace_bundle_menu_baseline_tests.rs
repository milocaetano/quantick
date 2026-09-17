//! Real Workspace menu frames before file-action ownership moves.
use super::*;

fn open_bundle_menu(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
) -> egui::FullOutput {
    run_sized_frame(app, ctx, size, vec![]);
    let position = app.chrome.workspace_menu_rect.unwrap().center();
    click_sized(app, ctx, size, position);
    run_sized_frame(app, ctx, size, vec![])
}

fn click_bundle_label(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    output: &egui::FullOutput,
    label: &str,
) -> egui::FullOutput {
    let position = painted_text_center(output, label)
        .unwrap_or_else(|| panic!("missing {label:?} in {:?}", painted_text(output)));
    click_sized(app, ctx, size, position);
    run_sized_frame(app, ctx, size, vec![])
}

#[test]
fn bundle_menu_baseline_preserves_literal_order_and_partial_scene() {
    for size in [TEST_WINDOW, MIN_WINDOW] {
        let (mut app, _evt, _cmd, _book) = test_app();
        let ctx = egui::Context::default();
        run_sized_frame(&mut app, &ctx, size, vec![]);
        let before = observer_scene(&app);
        let output = open_bundle_menu(&mut app, &ctx, size);
        let texts = painted_text(&output);
        let labels = [
            "Save workspace",
            "Reset startup layout",
            "Save as…",
            "Export to file…",
            "Open from file…",
            "Open recent",
            "Show where it's saved",
            "Save on exit",
        ];
        let positions: Vec<_> = labels
            .iter()
            .map(|label| {
                texts
                    .iter()
                    .position(|text| text == label)
                    .unwrap_or_else(|| panic!("missing {label:?} at {size:?}: {texts:?}"))
            })
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let after = observer_scene(&app);
        assert_eq!(scene_control_ids(&after), scene_control_ids(&before));
        assert_eq!(after["coverage"], before["coverage"]);
        assert_eq!(
            after["coverage"]["reason"],
            "only_the_named_group_of_each_covered_region_is_enumerated"
        );
        assert!(
            scene_control_ids(&after)
                .iter()
                .any(|id| id.ends_with(".canvas"))
        );
    }
}

#[test]
fn bundle_menu_baseline_empty_recent_cannot_open_an_entry() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = open_bundle_menu(&mut app, &ctx, TEST_WINDOW);
    let after = click_bundle_label(&mut app, &ctx, TEST_WINDOW, &output, "Open recent");
    assert!(
        painted_text(&after)
            .iter()
            .any(|text| text == "Open recent")
    );
    assert!(app.workspace.recent_on_disk().is_empty());
    assert!(app.workspace.session().recent().is_empty());
    assert_eq!(app.surfaces.toast.message(), None);
    assert!(!app.workspace.picker_open());
}

#[test]
fn bundle_menu_baseline_recent_click_imports_the_real_file() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let input = crate::scratch::ScratchFile::new("bundle-menu-recent", "chosen.qws.toml");
    std::fs::write(&input, "version = 1\nname = 'chosen'\n[sections]\n").unwrap();
    app.added_symbols.add("binance", "REOPENUSDT");
    symbols_file::save(app.workspace.symbols_path(), &app.added_symbols).unwrap();
    app.added_symbols.remove("binance", "REOPENUSDT");
    app.workspace.set_recent_on_disk(vec![input.to_path_buf()]);
    let output = open_bundle_menu(&mut app, &ctx, TEST_WINDOW);
    let submenu = click_bundle_label(&mut app, &ctx, TEST_WINDOW, &output, "Open recent");
    let label = crate::workspace_bundle::recent_label(&input);
    assert_eq!(label, "chosen");
    click_bundle_label(&mut app, &ctx, TEST_WINDOW, &submenu, &label);
    assert!(app.added_symbols.contains("binance", "REOPENUSDT"));
    assert_eq!(
        app.workspace.session().recent(),
        [input.to_string_lossy().into_owned()]
    );
    assert!(
        app.surfaces
            .toast
            .message()
            .unwrap()
            .contains("0 settings groups restored")
    );
}

#[test]
fn bundle_menu_baseline_duplicate_opens_preserve_original_channel_and_intent() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let (sender, receiver) = std::sync::mpsc::channel();
    app.workspace
        .picker_mut()
        .set_pending_for_test(crate::workspace_store::WorkspacePick::Export, receiver);
    for label in ["Export to file…", "Open from file…"] {
        let output = open_bundle_menu(&mut app, &ctx, TEST_WINDOW);
        click_bundle_label(&mut app, &ctx, TEST_WINDOW, &output, label);
        assert!(app.workspace.picker_open());
    }
    let output = crate::scratch::ScratchFile::new("bundle-menu-original-picker", "export.qws.toml");
    assert!(!output.exists());
    sender
        .send(Some(output.to_path_buf()))
        .expect("original receiver is still live");
    app.poll_workspace_picker();
    assert!(!app.workspace.picker_open());
    assert!(
        output.is_file(),
        "the original Export intent, not Import, was retained"
    );
    assert!(
        app.surfaces
            .toast
            .message()
            .unwrap()
            .starts_with("Workspace exported to ")
    );
}
