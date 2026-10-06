//! The chart menu's object entries: the objects submenu's rows and the
//! confirmed "clear objects…", both acting on the right-clicked pane's store.

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
    assert_eq!(pane.drawings.items().len(), count);
}

fn click_at(pos: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        },
    ]
}

fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(SCREEN),
        events,
        ..Default::default()
    }
}

fn rows_frame(app: &mut QuantickApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    with_flow_pane(app, |pane, _chrome| {
        let _ = ctx.run(input(events), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| pane.draw_object_rows(ui));
        });
    });
}

fn row_button(app: &QuantickApp, index: usize, label: &str) -> egui::Pos2 {
    app.active_tab()
        .flow_pane
        .context_menu
        .object_rects
        .iter()
        .find(|(row, entry, _)| *row == index && *entry == label)
        .unwrap_or_else(|| panic!("row {index} offers {label}"))
        .2
        .center()
}

/// Each row's buttons reach its own object and no other; a locked row's
/// delete is offered disabled, as in the menu's drawing section.
#[test]
fn each_objects_row_acts_on_its_own_object_only() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands, _book) = test_app();
    place_rectangles(&mut app, 2);

    rows_frame(&mut app, &ctx, Vec::new());
    let eye = row_button(&app, 0, "Eye");
    rows_frame(&mut app, &ctx, click_at(eye));
    let items = app.active_tab().flow_pane.drawings.items();
    assert!(items[0].hidden, "the clicked row's object hides");
    assert!(!items[1].hidden, "the other object stays visible");

    let kept = items[0].id;
    rows_frame(&mut app, &ctx, Vec::new());
    let delete = row_button(&app, 1, "Delete");
    rows_frame(&mut app, &ctx, click_at(delete));
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
    rows_frame(&mut app, &ctx, Vec::new());
    let delete = row_button(&app, 0, "Delete");
    rows_frame(&mut app, &ctx, click_at(delete));
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "a locked object never deletes from the menu"
    );
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
    with_flow_pane(app, |pane, _chrome| {
        ctx.run(input(events), |ctx| {
            pane.draw_clear_objects_confirm(ctx, SCREEN)
        })
    })
}

/// The confirmation's button, after one frame to lay the window out.
fn confirm_button(app: &mut QuantickApp, ctx: &egui::Context, label: &str) -> egui::Pos2 {
    confirm_frame(app, ctx, Vec::new());
    let output = confirm_frame(app, ctx, Vec::new());
    assert!(
        painted_text(&output).contains(&"Delete all 2 drawing(s), locked included?".to_owned()),
        "the question names the count and the locked objects"
    );
    painted_text_center(&output, label).unwrap_or_else(|| panic!("{label} is offered"))
}

/// "clear objects…" only asks; Keep deletes nothing; Delete all takes every
/// object, locked included, as one step Ctrl+Z gives back.
#[test]
fn clear_objects_asks_first_and_takes_everything_in_one_undo() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands, _book) = test_app();
    place_rectangles(&mut app, 2);
    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_locked_at(0, true);

    menu_frame(&mut app, &ctx, Vec::new());
    let clear = app
        .active_tab()
        .flow_pane
        .context_menu
        .clear_objects_rect
        .expect("clear objects is offered on a chart with objects")
        .center();
    menu_frame(&mut app, &ctx, click_at(clear));
    let pane = &app.active_tab().flow_pane;
    assert!(
        pane.context_menu.confirm_clear,
        "the click raises the question"
    );
    assert_eq!(pane.drawings.items().len(), 2, "and deletes nothing yet");

    let keep = confirm_button(&mut app, &ctx, "Keep");
    confirm_frame(&mut app, &ctx, click_at(keep));
    let pane = &app.active_tab().flow_pane;
    assert!(
        !pane.context_menu.confirm_clear,
        "Keep answers the question"
    );
    assert_eq!(pane.drawings.items().len(), 2, "Keep deletes nothing");

    app.active_tab_mut().flow_pane.context_menu.confirm_clear = true;
    let delete = confirm_button(&mut app, &ctx, "Delete all");
    confirm_frame(&mut app, &ctx, click_at(delete));
    let pane = &mut app.active_tab_mut().flow_pane;
    assert!(!pane.context_menu.confirm_clear);
    assert!(
        pane.drawings.items().is_empty(),
        "every object goes, locked too"
    );

    pane.drawings.undo();
    assert_eq!(
        pane.drawings.items().len(),
        2,
        "one undo brings them all back"
    );
}

/// On an empty chart both entries are there but disabled.
#[test]
fn an_empty_chart_offers_no_clear() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands, _book) = test_app();
    menu_frame(&mut app, &ctx, Vec::new());
    let menu = &app.active_tab().flow_pane.context_menu;
    assert!(menu.clear_objects_rect.is_none());
    assert!(menu.objects_rect.is_none());
}
