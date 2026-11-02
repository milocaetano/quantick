//! The `⋮` window menu and the strip beside it, operated the way a trader does.
use super::*;

use quantick_control_schema::window_menu::KEBAB_CONTROL;

fn kebab(app: &QuantickApp) -> egui::Rect {
    app.chrome.title_bar.control_rect(KEBAB_CONTROL).unwrap()
}

fn open_menu(app: &mut QuantickApp, ctx: &egui::Context) -> egui::FullOutput {
    run_frame(app, ctx);
    click_sized(app, ctx, TEST_WINDOW, kebab(app).center());
    run_frame(app, ctx)
}

fn click_entry(app: &mut QuantickApp, ctx: &egui::Context, output: &egui::FullOutput, entry: &str) {
    let position = painted_text_center(output, entry)
        .unwrap_or_else(|| panic!("missing {entry:?} in {:?}", painted_text(output)));
    click_sized(app, ctx, TEST_WINDOW, position);
}

#[test]
fn the_five_top_level_menus_are_gone() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = run_frame(&mut app, &ctx);
    for gone in ["File", "View", "Workspace", "Tools", "Help"] {
        assert!(painted_text_center(&output, gone).is_none(), "{gone}");
    }
}

#[test]
fn the_kebab_is_36_by_30_and_opens_the_groups_of_the_brief() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = open_menu(&mut app, &ctx);
    assert_eq!(kebab(&app).size(), egui::vec2(36.0, 30.0));
    for row in [
        "Save desk",
        "Hide panels",
        "Market Replay…",
        "Workspace",
        "Panels",
        "Desk",
        "Command palette…",
        "Settings…",
        "Help",
    ] {
        assert!(painted_text_center(&output, row).is_some(), "{row}");
    }
    assert!(painted_text_center(&output, "Exit").is_none());
    assert!(
        painted_text_center(&output, "Close Replay").is_none(),
        "nothing is replaying"
    );
}

#[test]
fn f10_opens_and_closes_the_menu_and_leaves_the_tabs_alone() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let tabs = app.runtime.tabs.len();
    let press = vec![egui::Event::Key {
        key: egui::Key::F10,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }];
    let output = run_sized_frame(&mut app, &ctx, TEST_WINDOW, press.clone());
    assert!(app.chrome.window_menu.open);
    assert_eq!(app.runtime.tabs.len(), tabs);
    drop(output);
    run_sized_frame(&mut app, &ctx, TEST_WINDOW, press);
    assert!(!app.chrome.window_menu.open);
}

#[test]
fn a_submenu_opens_to_the_left_of_its_row() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = open_menu(&mut app, &ctx);
    let row = painted_text_center(&output, "Help").unwrap();
    click_entry(&mut app, &ctx, &output, "Help");
    let output = run_frame(&mut app, &ctx);
    let entry = painted_text_center(&output, "Replay file format…").expect("Help opened");
    assert!(entry.x < row.x, "{entry:?} sits left of {row:?}");
}

#[test]
fn a_row_carries_out_its_command_and_closes_the_menu() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let visible = app.dock.visible();
    let output = open_menu(&mut app, &ctx);
    let label = if visible {
        "Hide panels"
    } else {
        "Show panels"
    };
    click_entry(&mut app, &ctx, &output, label);
    assert_eq!(app.dock.visible(), !visible);
    assert!(!app.chrome.window_menu.open);
}

#[test]
fn a_click_outside_closes_the_menu_and_the_palette_row_stays_disabled() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = open_menu(&mut app, &ctx);
    click_entry(&mut app, &ctx, &output, "Command palette…");
    assert!(app.chrome.window_menu.open, "a disabled row does nothing");
    click_sized(&mut app, &ctx, TEST_WINDOW, egui::pos2(200.0, 400.0));
    assert!(!app.chrome.window_menu.open);
}

#[test]
fn settings_opens_with_the_switches_that_left_the_menus() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = open_menu(&mut app, &ctx);
    click_entry(&mut app, &ctx, &output, "Settings…");
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    for row in [
        "Appearance…",
        "Timezone",
        "Perf readings",
        "Progressive venue history",
        "Venue candles on charts cut by trades",
        "Record deals by default",
    ] {
        assert!(painted_text_center(&output, row).is_some(), "{row}");
    }
}

#[test]
fn about_workspaces_says_what_is_not_built_yet() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = open_menu(&mut app, &ctx);
    click_entry(&mut app, &ctx, &output, "Help");
    let output = run_frame(&mut app, &ctx);
    click_entry(&mut app, &ctx, &output, "About workspaces…");
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    assert!(
        painted_text(&output)
            .iter()
            .any(|text| text.starts_with("Not built yet"))
    );
}

#[test]
fn the_agent_pill_shows_only_while_access_is_on() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = run_frame(&mut app, &ctx);
    assert!(painted_text_center(&output, "agent").is_none());
    assert!(app.chrome.title_bar.control_rect("agent").is_none());
}

#[test]
fn the_scene_lists_the_kebab_beside_drag_and_the_window_buttons() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let ids = scene_control_ids(&observer_scene(&app));
    for id in [
        "tab_strip.drag",
        "tab_strip.kebab",
        "tab_strip.minimize",
        "tab_strip.maximize",
        "tab_strip.close",
    ] {
        assert!(ids.iter().any(|known| known == id), "{id} in {ids:?}");
    }
}

#[test]
fn every_menu_row_names_a_behaviour_of_the_table() {
    use quantick_control_schema::window_menu::ROWS;
    use quantick_operability::registry::UI_BEHAVIOURS;
    for row in ROWS {
        assert!(
            UI_BEHAVIOURS.iter().any(|known| known.id == row.behaviour),
            "{} names {}",
            row.label,
            row.behaviour
        );
    }
}

#[test]
fn the_strip_is_36_tall_with_30_tall_tabs_and_46_by_36_window_buttons() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let title = &app.chrome.title_bar;
    for (_, rect) in &title.strip.tabs {
        assert_eq!(rect.height(), 30.0);
        assert_eq!(rect.bottom(), 36.0, "tabs sit on the row's floor");
    }
    for id in ["minimize", "maximize", "close"] {
        let rect = title.control_rect(id).unwrap();
        assert_eq!(rect.size(), egui::vec2(46.0, 36.0), "{id}");
        assert_eq!(rect.top(), 0.0, "{id} starts at the top edge");
    }
    let kebab = title.control_rect(KEBAB_CONTROL).unwrap();
    let minimize = title.control_rect("minimize").unwrap();
    assert!(kebab.right() <= minimize.left(), "kebab before the buttons");
}

#[test]
fn the_toolbar_labels_the_market_instead_of_offering_dropdowns() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = run_frame(&mut app, &ctx);
    assert!(painted_text_center(&output, "this tab").is_some());
    assert!(painted_text_center(&output, "symbol").is_none());
    assert!(painted_text_center(&output, "feed").is_none());
}

#[test]
fn the_kebab_on_another_window_opens_the_menu_there_and_not_on_the_first() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let (first, second) = (
        egui::ViewportId::ROOT,
        egui::ViewportId::from_hash_of("second"),
    );
    let menu = &mut app.chrome.window_menu;
    menu.toggle(first, false);
    menu.toggle(second, false);
    assert!(menu.is_open_on(second), "one press is enough");
    assert!(!menu.is_open_on(first));
    menu.toggle(second, false);
    assert!(!menu.open);
}

#[test]
fn the_top_right_corner_of_a_restored_window_closes_rather_than_resizes() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let close = app.chrome.title_bar.control_rect("close").unwrap();
    let corner = close.right_top() + egui::vec2(-1.0, 1.0);
    run_sized_frame(
        &mut app,
        &ctx,
        TEST_WINDOW,
        vec![
            egui::Event::PointerMoved(corner),
            pointer_button(corner, true),
        ],
    );
    let output = run_sized_frame(
        &mut app,
        &ctx,
        TEST_WINDOW,
        vec![pointer_button(corner, false)],
    );
    let commands: Vec<_> = output
        .viewport_output
        .values()
        .flat_map(|viewport| &viewport.commands)
        .collect();
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, egui::ViewportCommand::Close)),
        "{commands:?}"
    );
}
