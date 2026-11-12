//! The `⋮` window menu and the strip beside it, operated the way a trader does.
use super::*;

use quantick_control_schema::settings_sections::SettingsSection;
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
    assert_eq!(
        app.chrome.window_menu.settings_shown(),
        Some(SettingsSection::Display),
        "Ctrl+, and the menu row open on Display"
    );
    // Below the strip, where `settings.open` puts it, never over the tabs.
    let window = ctx.memory(|memory| memory.area_rect(egui::Id::new("window_settings")));
    let below = egui::pos2(16.0, crate::app::title_bar::HEIGHT + 16.0);
    assert_eq!(window.map(|window| window.min), Some(below));
    let mut output = output;
    for section in SettingsSection::ALL {
        click_entry(&mut app, &ctx, &output, section.title());
        run_frame(&mut app, &ctx);
        output = run_frame(&mut app, &ctx);
        assert_eq!(app.chrome.window_menu.settings_section, section);
        for row in section.rows() {
            let label = row.label();
            assert!(painted_text_center(&output, label).is_some(), "{label}");
        }
    }
}

#[test]
fn a_switch_row_flips_the_setting_it_names() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let before = app.health.show_perf;
    app.chrome
        .window_menu
        .open_settings(ctx.viewport_id(), SettingsSection::Display);
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    let row = painted_text_center(&output, "Perf readings").unwrap();
    let window = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("window_settings")))
        .expect("the window is up");
    let switch = egui::pos2(window.right() - 22.0, row.y);
    click_sized(&mut app, &ctx, TEST_WINDOW, switch);
    run_frame(&mut app, &ctx);
    assert_eq!(app.health.show_perf, !before);
}

#[test]
fn the_status_bar_clock_opens_settings_on_display() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = run_frame(&mut app, &ctx);
    app.chrome.window_menu.settings_section = SettingsSection::Data;
    let clock = format!(
        "{} {}",
        app.runtime.tz.label(),
        egui_phosphor::regular::CLOCK
    );
    let clock = painted_text_center(&output, &clock).expect("the clock is on the status bar");
    click_sized(&mut app, &ctx, TEST_WINDOW, clock);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.chrome.window_menu.settings_shown(),
        Some(SettingsSection::Display)
    );
}

#[test]
fn the_kebab_reads_pressed_while_held_and_while_its_menu_is_open() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    assert!(!app.chrome.title_bar.kebab_pressed);
    let at = kebab(&app).center();
    let held = vec![egui::Event::PointerMoved(at), pointer_button(at, true)];
    run_sized_frame(&mut app, &ctx, TEST_WINDOW, held);
    assert!(
        app.chrome.title_bar.kebab_pressed,
        "held, before any release"
    );
    run_sized_frame(&mut app, &ctx, TEST_WINDOW, vec![pointer_button(at, false)]);
    let output = run_frame(&mut app, &ctx);
    assert!(app.chrome.title_bar.kebab_pressed);
    let rect = kebab(&app);
    let filled = output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Rect(shape)
            if shape.rect == rect && shape.fill == crate::theme::CONTROL)
    });
    assert!(filled, "the pressed fill is on the button");
    let scene = observer_scene(&app);
    assert_eq!(scene_control(&scene, "tab_strip.kebab")["selected"], true);
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
            .any(|text| text.starts_with("Deleting a workspace moves"))
    );
}

#[test]
fn the_agent_pill_shows_only_while_access_is_on() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = run_frame(&mut app, &ctx);
    assert!(painted_text_center(&output, "Agent on").is_none());
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
fn the_toolbar_market_button_switches_the_symbol_inside_the_same_tab() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let tab = app.runtime.tabs.active_id();
    let output = run_frame(&mut app, &ctx);
    assert!(
        painted_text_center(&output, "this tab").is_none(),
        "no eyebrow"
    );
    let caret = egui_phosphor::regular::CARET_DOWN;
    click_entry(
        &mut app,
        &ctx,
        &output,
        &format!("TESTUSDT \u{b7} Binance{caret}"),
    );
    run_frame(&mut app, &ctx);
    let palette = app.surfaces.market_palette.model_state.open.as_ref();
    assert!(
        palette
            .and_then(|palette| palette.switch_target())
            .is_some(),
        "the button opens the palette in switch mode"
    );
    let output = run_frame(&mut app, &ctx);
    click_entry(&mut app, &ctx, &output, "ETHUSDT");
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().symbol, "ETHUSDT");
    assert_eq!(app.runtime.tabs.active_id(), tab, "the same tab");
    assert_eq!(app.runtime.tabs.len(), 1, "no second tab");
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

/// L2 and Bubbles open their section of the order-flow panel, which rides on the
/// flow pane: with none on screen the rows grey out, as the panel's key does nothing.
#[test]
fn the_panel_rows_open_the_order_flow_panel_only_on_a_shown_flow_pane() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let click_l2 = |app: &mut QuantickApp| {
        let output = open_menu(app, &ctx);
        click_entry(app, &ctx, &output, "Panels");
        let output = run_frame(app, &ctx);
        click_entry(app, &ctx, &output, "L2");
        run_frame(app, &ctx);
    };
    assert!(crate::orderflow_panel::flow_pane_shown(app.active_tab()));
    app.active_tab_mut().canvas.set_flow_collapsed(true);
    click_l2(&mut app);
    assert!(app.chrome.window_menu.open, "a greyed row does nothing");
    assert!(!app.active_tab().tape().settings_panel.is_open());

    app.chrome.window_menu.toggle(egui::ViewportId::ROOT, false);
    app.active_tab_mut().canvas.set_flow_collapsed(false);
    click_l2(&mut app);
    assert!(!app.chrome.window_menu.open);
    assert!(app.active_tab().tape().settings_panel.is_open());
}

/// C8: `≡` hangs the window menu below itself, its submenus open rightwards
/// and it reads pressed; `⋮` still opens it at the top right, and F10 at `⋮`.
#[test]
fn list_door_opens_window_menu_below_itself() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let list = app
        .chrome
        .window_menu
        .toolbar_door
        .expect("the toolbar drew its list button");
    assert!(
        list.right() < kebab(&app).left(),
        "the list sits at the toolbar's left end"
    );
    click_sized(&mut app, &ctx, TEST_WINDOW, list.center());
    // The first frame on a new door sizes the pane; the second paints it.
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    let row = painted_text_center(&output, "Save desk")
        .unwrap_or_else(|| panic!("the menu is up: {:?}", painted_text(&output)));
    assert!(
        row.x > list.left(),
        "the menu hangs rightwards from the list"
    );
    assert!(row.y > list.bottom(), "the menu drops below the toolbar");
    let door = app
        .chrome
        .toolbar_controls
        .iter()
        .find(|c| c.id == "toolbar.window_menu");
    assert!(door.is_some_and(|door| door.selected), "≡ reads pressed");
    assert!(
        !app.chrome.title_bar.kebab_pressed,
        "⋮ is not the one that opened it"
    );
    let help = painted_text_center(&output, "Help").unwrap();
    click_entry(&mut app, &ctx, &output, "Help");
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    let entry = painted_text_center(&output, "Replay file format…").expect("Help opened");
    assert!(
        entry.x > help.x,
        "the submenu opens rightwards: {entry:?} beside {help:?}"
    );
    click_sized(&mut app, &ctx, TEST_WINDOW, list.center());
    let output = run_frame(&mut app, &ctx);
    assert!(
        painted_text_center(&output, "Save desk").is_none(),
        "a second click closes it"
    );

    open_menu(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    let row = painted_text_center(&output, "Save desk").expect("⋮ opens the menu");
    assert!(
        row.x < kebab(&app).right() && row.x > list.right(),
        "it hangs from ⋮"
    );
    assert!(app.chrome.title_bar.kebab_pressed);
    let press = vec![key_press(egui::Key::F10)];
    run_sized_frame(&mut app, &ctx, TEST_WINDOW, press.clone());
    assert!(!app.chrome.window_menu.open, "F10 closes it");
    run_sized_frame(&mut app, &ctx, TEST_WINDOW, press);
    run_frame(&mut app, &ctx);
    assert!(app.chrome.title_bar.kebab_pressed, "F10 opens it at ⋮");
}
