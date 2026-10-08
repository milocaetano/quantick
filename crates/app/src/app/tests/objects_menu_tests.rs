//! The chart menu's object entries: the objects submenu's rows and the
//! confirmed "clear objects…", applied through the drawing controller on the
//! chart the right-click named.

use super::*;

const SCREEN: egui::Rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 700.0));

fn place_rectangles(app: &mut QuantickApp, count: usize) {
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .expect("the rectangle tool is registered");
    let pane = &mut app.active_tab_mut().flow_pane;
    for offset in 0..count {
        let first = offset as f32 * 10.0;
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(first + 1.0, 100.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(first + 5.0, 110.0));
    }
    pane.drawings.select(None);
    assert_eq!(pane.drawings.items().len(), count);
}

fn click_at(pos: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        pointer_button(pos, true),
        pointer_button(pos, false),
    ]
}

fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(SCREEN),
        events,
        ..Default::default()
    }
}

/// One frame of the submenu's rows on their own context, the way the menu
/// draws them; the controller applies their ask on the next app frame.
fn rows_frame(app: &mut QuantickApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    with_flow_pane(app, |pane, chrome| {
        let _ = ctx.run(input(events), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let ask = pane
                    .context_menu
                    .draw_object_rows(ui, &pane.drawings, &pane.indicators);
                pane.apply_menu_intents(ask, chrome);
            });
        });
    });
}

fn click_row(app: &mut QuantickApp, menu: &egui::Context, index: usize, label: &str) {
    rows_frame(app, menu, Vec::new());
    let pos = app
        .active_tab()
        .flow_pane
        .context_menu
        .object_rects
        .iter()
        .find(|(row, entry, _)| *row == index && *entry == label)
        .unwrap_or_else(|| panic!("row {index} offers {label}"))
        .2
        .center();
    rows_frame(app, menu, click_at(pos));
}

fn menu_frame(app: &mut QuantickApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    with_flow_pane(app, |pane, chrome| {
        let _ = ctx.run(input(events), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| pane.draw_layer_menu(ui, chrome));
        });
    });
}

fn confirm_frame(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    with_flow_pane(app, |pane, chrome| {
        ctx.run(input(events), |ctx| {
            let count = pane.drawings.items().len();
            let ask = pane.context_menu.draw_clear_objects_confirm(
                &mut pane.model,
                ctx,
                SCREEN,
                count,
                pane.id,
            );
            pane.apply_menu_intents(ask, chrome);
        })
    })
}

/// Answer the confirmation with `label`, after one frame to lay it out.
fn answer_confirm(app: &mut QuantickApp, ctx: &egui::Context, label: &str) {
    confirm_frame(app, ctx, Vec::new());
    let output = confirm_frame(app, ctx, Vec::new());
    assert!(
        painted_text(&output).contains(&"Delete all 2 drawing(s), locked included?".to_owned()),
        "the question names the count and the locked objects"
    );
    let pos = painted_text_center(&output, label).unwrap_or_else(|| panic!("{label} is offered"));
    confirm_frame(app, ctx, click_at(pos));
}

fn count(app: &QuantickApp) -> usize {
    app.active_tab().flow_pane.drawings.items().len()
}

/// Each row reaches its own object and no other, through the manager's path:
/// a locked row's delete asks first instead of deleting.
#[test]
fn each_objects_row_acts_on_its_own_object_only() {
    let menu = egui::Context::default();
    let ctx = egui::Context::default();
    let (mut app, _events, _commands, _book) = test_app();
    place_rectangles(&mut app, 2);

    click_row(&mut app, &menu, 0, "Eye");
    run_frame(&mut app, &ctx);
    let items = app.active_tab().flow_pane.drawings.items();
    assert!(items[0].hidden, "the clicked row's object hides");
    assert!(!items[1].hidden, "the other object stays visible");

    let kept = items[0].id;
    click_row(&mut app, &menu, 1, "Delete");
    run_frame(&mut app, &ctx);
    let ids: Vec<_> = app
        .active_tab()
        .flow_pane
        .drawings
        .items()
        .iter()
        .map(|drawing| drawing.id)
        .collect();
    assert_eq!(ids, [kept], "only the clicked row's object is deleted");

    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_locked_at(0, true);
    click_row(&mut app, &menu, 0, "Delete");
    run_frame(&mut app, &ctx);
    assert_eq!(count(&app), 1, "a locked object never deletes on one click");
    assert!(
        app.drawings.chrome.delete_confirm(),
        "the locked delete raises the same confirmation as the manager"
    );
}

/// "clear objects…" only asks; Keep deletes nothing; Delete all takes every
/// object, locked included, with the manager's Undo toast, and one Ctrl+Z
/// brings them all back to this chart.
#[test]
fn clear_objects_asks_first_and_one_ctrl_z_brings_everything_back() {
    let menu = egui::Context::default();
    let ctx = egui::Context::default();
    let (mut app, _events, _commands, _book) = test_app();
    place_rectangles(&mut app, 2);
    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_locked_at(0, true);

    menu_frame(&mut app, &menu, Vec::new());
    let clear = app
        .active_tab()
        .flow_pane
        .context_menu
        .clear_objects_rect
        .expect("clear objects is painted")
        .center();
    menu_frame(&mut app, &menu, click_at(clear));
    assert!(app.active_tab().flow_pane.model.menu.confirm_clear);
    run_frame(&mut app, &ctx);
    assert_eq!(count(&app), 2, "the click only asks");

    answer_confirm(&mut app, &menu, "Keep");
    run_frame(&mut app, &ctx);
    assert!(!app.active_tab().flow_pane.model.menu.confirm_clear);
    assert_eq!(count(&app), 2, "Keep deletes nothing");

    app.active_tab_mut().flow_pane.model.menu.confirm_clear = true;
    answer_confirm(&mut app, &menu, "Delete all");
    run_frame(&mut app, &ctx);
    assert!(!app.active_tab().flow_pane.model.menu.confirm_clear);
    assert_eq!(count(&app), 0, "every object goes, locked too");
    assert_eq!(
        app.surfaces.toast.message(),
        Some("All drawings deleted."),
        "the manager's own note, with its Undo"
    );

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );
    assert_eq!(count(&app), 2, "one undo brings them all back");
}

/// An ask tagged with a chart the drawing chrome is not speaking for is
/// dropped, never applied to the chart that happens to be focused.
#[test]
fn a_menu_ask_for_another_chart_is_dropped() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands, _book) = test_app();
    place_rectangles(&mut app, 2);
    let other = app.active_tab().flow_pane.id + 1;
    app.drawings.chrome.ask_from_menu(
        other,
        crate::surfaces::drawing_chrome::DrawingChromeAsk {
            delete_all: true,
            ..Default::default()
        },
    );
    run_frame(&mut app, &ctx);
    assert_eq!(count(&app), 2);
}

/// The menu's ask lands on the chart it was opened on even while another
/// chart holds the focus and a selection, and the Ctrl+Z after it does too.
#[test]
fn a_menu_ask_applies_on_its_own_chart_and_so_does_the_undo() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .expect("the rectangle tool is registered");
    let time = app.active_tab_mut().pane_mut(PaneSide::Time(0));
    for first in [1.0, 11.0] {
        time.drawings
            .place(rectangle, drawings::ChartPoint::at(first, 100.0));
        time.drawings
            .place(rectangle, drawings::ChartPoint::at(first + 4.0, 110.0));
    }
    time.drawings.select(None);
    let time_id = time.id;
    place_rectangles(&mut app, 1);
    app.active_tab_mut().flow_pane.drawings.select(Some(0));
    app.active_tab_mut().focus = PaneSide::Flow;

    app.drawings.chrome.ask_from_menu(
        time_id,
        crate::surfaces::drawing_chrome::DrawingChromeAsk {
            delete_all: true,
            ..Default::default()
        },
    );
    run_frame(&mut app, &ctx);
    let tab = app.active_tab();
    assert!(tab.pane(PaneSide::Time(0)).drawings.items().is_empty());
    assert_eq!(count(&app), 1, "the focused chart keeps its object");

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );
    let tab = app.active_tab();
    assert_eq!(tab.pane(PaneSide::Time(0)).drawings.items().len(), 2);
    assert_eq!(count(&app), 1);
}

/// On an empty chart both entries are painted but disabled.
#[test]
fn an_empty_chart_offers_a_disabled_clear() {
    let menu = egui::Context::default();
    let (mut app, _events, _commands, _book) = test_app();
    menu_frame(&mut app, &menu, Vec::new());
    let clear = app
        .active_tab()
        .flow_pane
        .context_menu
        .clear_objects_rect
        .expect("clear objects is painted, disabled")
        .center();
    menu_frame(&mut app, &menu, click_at(clear));
    assert!(!app.active_tab().flow_pane.model.menu.confirm_clear);
}
