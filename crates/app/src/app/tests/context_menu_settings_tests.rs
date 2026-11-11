//! The chart menu's settings doors: right under "chart layers", the footprint's
//! window, and on the flow pane the gear's panel opened on its L2 or bubbles
//! section, by a real click on the drawn menu.

use super::*;
use crate::surfaces::chart_settings::ChartSettingsTab;
use crate::ui_state::PanelSection;

const SCREEN: egui::Rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 700.0));

fn menu_frame(app: &mut QuantickApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    with_flow_pane(app, |pane, chrome| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(SCREEN),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| pane.draw_layer_menu(ui, chrome));
            },
        );
    });
}

fn offered(app: &QuantickApp) -> Vec<&'static str> {
    let menu = &app.active_tab().flow_pane.context_menu;
    menu.menu_rects.iter().map(|(label, _)| *label).collect()
}

/// Draw the menu, click `label` in it, and let the window drain the asks.
fn click(app: &mut QuantickApp, menu: &egui::Context, label: &str) {
    menu_frame(app, menu, Vec::new());
    let pos = app
        .active_tab()
        .flow_pane
        .context_menu
        .menu_rects
        .iter()
        .find(|(entry, _)| *entry == label)
        .unwrap_or_else(|| panic!("{label} is offered"))
        .1
        .center();
    menu_frame(
        app,
        menu,
        vec![
            egui::Event::PointerMoved(pos),
            pointer_button(pos, true),
            pointer_button(pos, false),
        ],
    );
    app.layer_wiring().apply_actions();
}

#[test]
fn the_flow_menu_groups_its_settings_doors() {
    let menu = egui::Context::default();
    let (mut app, _events, _commands, _book) = test_app();
    menu_frame(&mut app, &menu, Vec::new());
    let labels = offered(&app);
    let at = labels
        .iter()
        .position(|label| *label == "configure footprint")
        .expect("the footprint door is in the main menu");
    assert_eq!(
        labels[at..at + 3],
        ["configure footprint", "configure L2", "configure bubbles"],
        "the three settings doors sit together"
    );
}

#[test]
fn the_l2_and_bubbles_entries_open_the_gears_panel_on_their_section() {
    let menu = egui::Context::default();
    for (label, section) in [
        ("configure L2", PanelSection::L2),
        ("configure bubbles", PanelSection::Bubbles),
    ] {
        let (mut app, _events, _commands, _book) = test_app();
        assert!(!app.active_tab().tape().settings_panel.is_open());
        click(&mut app, &menu, label);
        let symbol = app.active_tab().symbol.clone();
        let mut folds = crate::ui_state::AssetFolds::default();
        let panel = &mut app.active_tab_mut().tape_mut().settings_panel;
        assert!(panel.is_open(), "{label} opens the panel");
        assert_eq!(
            panel.take_request(&symbol, &mut folds),
            Some(section),
            "{label} lands on its own section"
        );
    }
}

#[test]
fn the_footprint_entry_opens_the_footprint_window() {
    let menu = egui::Context::default();
    let (mut app, _events, _commands, _book) = test_app();
    click(&mut app, &menu, "configure footprint");
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Footprint)
    );
}

/// A context chart's menu offers its footprint door and no flow doors, and
/// the window it opens edits that chart: the menu's chart takes focus.
#[test]
fn a_context_charts_footprint_door_edits_that_chart() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 40);
    let side = PaneSide::Time(0);
    app.active_tab_mut().focus = PaneSide::Flow;
    run_frame(&mut app, &ctx);
    let target = app
        .active_tab()
        .pane(side)
        .frame
        .chart_area
        .expect("the split laid out the context chart")
        .center();
    let secondary = |pressed| egui::Event::PointerButton {
        pos: target,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(target), secondary(true)],
    );
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(target), secondary(false)],
    );
    let open = run_frame(&mut app, &ctx);
    let texts = painted_text(&open);
    assert!(
        !texts.iter().any(|text| text == "configure L2\u{2026}"),
        "the gear's panel belongs to the flow pane: {texts:?}"
    );
    let door = painted_text_center(&open, "configure footprint\u{2026}")
        .unwrap_or_else(|| panic!("the footprint door is in the main menu: {texts:?}"));
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(door),
            pointer_button(door, true),
            pointer_button(door, false),
        ],
    );
    run_frame(&mut app, &ctx);
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Footprint)
    );
    assert_eq!(
        app.active_tab().focused_side(),
        side,
        "the footprint window edits the chart whose menu asked"
    );
}
