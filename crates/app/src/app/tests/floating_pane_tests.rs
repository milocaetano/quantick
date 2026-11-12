//! The toolbar's + and the charts it adds: floating over the canvas, docked
//! by a drop on an edge, parked in a corner, popped out to a window of their
//! own and brought back, and restored with the workspace.
use super::*;

use crate::canvas_layout::preset;
use quantick_workspace::canvas::{
    CanvasRect, Corner, FloatHost, PARK_MARGIN_PX, PaneKind, PaneMode, WIDGETS, park_rect,
};

fn frames(app: &mut QuantickApp, ctx: &egui::Context, count: usize) {
    for _ in 0..count {
        run_frame_at(app, ctx, TEST_WINDOW);
    }
}

/// Where the toolbar or the tab published the control `id`.
fn drawn(app: &QuantickApp, id: &str) -> Option<egui::Rect> {
    let toolbar = app.chrome.toolbar_controls.iter();
    (toolbar.chain(&app.active_tab().canvas.menus.drawn))
        .find(|control| control.id == id)
        .map(|control| control.rect)
}

/// Press what was painted as `id`, and let what it opens settle.
fn press(app: &mut QuantickApp, ctx: &egui::Context, id: &str) {
    let rect = drawn(app, id).unwrap_or_else(|| panic!("{id} was not drawn"));
    click_sized(app, ctx, TEST_WINDOW, rect.center());
    frames(app, ctx, 3);
}

/// A tab on `id`, its charts built, saved with auto save off.
fn saved_tab(id: &str) -> (QuantickApp, egui::Context) {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    app.runtime.workspace.book.book_mut().set_autosave(false);
    app.apply_layout_preset(preset(id).unwrap());
    frames(&mut app, &ctx, 6);
    save(&mut app, &ctx);
    (app, ctx)
}

fn save(app: &mut QuantickApp, ctx: &egui::Context) {
    let keys = vec![key_press_with(egui::Key::S, egui::Modifiers::CTRL)];
    run_sized_frame(app, ctx, TEST_WINDOW, keys);
    frames(app, ctx, 2);
}

/// Put the tab back on its workspace as saved, as opening it does.
fn reload(app: &mut QuantickApp, ctx: &egui::Context) {
    let id = app.runtime.tabs.active_id();
    let book = &app.runtime.workspace.book;
    let saved = book.book().recipe(&book.tab_row(id).0).cloned();
    let recipe = saved.expect("the tab's workspace is saved");
    let mut host = crate::app::tab_workspaces::BookTabs(app);
    quantick_workspace::book_upkeep::show_recipe(&mut host, id, &recipe);
    frames(app, ctx, 6);
}

/// Add a timeframe chart from the + menu, as a trader does; its slot.
fn add_chart(app: &mut QuantickApp, ctx: &egui::Context) -> u32 {
    let before = app.active_tab().canvas.state.panes();
    press(app, ctx, "toolbar.add_widget");
    press(app, ctx, "add_widget.timeframe-chart");
    frames(app, ctx, 6);
    let after = app.active_tab().canvas.state.panes();
    let added = after.iter().find(|pane| !before.contains(pane));
    added.expect("the + menu added a chart").0
}

fn host(app: &QuantickApp, slot: u32) -> Option<FloatHost> {
    app.active_tab().canvas.state.host(slot).cloned()
}

fn grip(app: &QuantickApp, slot: u32) -> egui::Pos2 {
    let Some(FloatHost::Canvas { rect }) = host(app, slot) else {
        panic!("chart {slot} floats over the canvas");
    };
    egui::pos2(rect.x + crate::floating_panes::GRIP_PX / 2.0, rect.y + 8.0)
}

/// C16: the + menu lists the registry; picking "Timeframe chart" opens a
/// floating chart centred over the canvas, drawn on its own layer.
#[test]
fn add_widget_opens_a_centred_floating_chart() {
    let (mut app, ctx) = saved_tab("time+flow");
    press(&mut app, &ctx, "toolbar.add_widget");
    assert!(app.chrome.add_menu.open, "the + opens ADD TO CANVAS");
    let rows: Vec<_> = (app.chrome.add_menu.drawn.iter())
        .map(|control| control.id.trim_start_matches("add_widget.").to_owned())
        .collect();
    let registry: Vec<_> = WIDGETS.iter().map(|widget| widget.id.to_owned()).collect();
    assert_eq!(rows, registry, "one row per WIDGETS entry");
    press(&mut app, &ctx, "add_widget.timeframe-chart");
    assert!(!app.chrome.add_menu.open, "a pick closes the menu");
    frames(&mut app, &ctx, 6);
    let tab = app.active_tab();
    let floating = tab.canvas.state.floating();
    assert_eq!(floating.len(), 1);
    assert_eq!(floating[0].kind, PaneKind::Time);
    let FloatHost::Canvas { rect } = floating[0].host else {
        panic!("over the canvas");
    };
    let canvas = tab.canvas.last_canvas;
    assert!(
        (rect.x + rect.w / 2.0 - canvas.center().x).abs() < 1.0,
        "centred across"
    );
    assert!(
        (rect.y + rect.h / 2.0 - canvas.center().y).abs() < 1.0,
        "centred down"
    );
    let bands = &tab.canvas.drawn_bands;
    let band = bands
        .iter()
        .find(|(_, band)| band.min.x > rect.x - 1.0 && band.max.x < rect.x + rect.w + 1.0);
    assert!(band.is_some(), "the floating chart is drawn: {bands:?}");
    let slot = floating[0].slot;
    assert!(
        drawn(&app, &format!("pane_menu.{slot}")).is_some(),
        "it wears its ⋯"
    );
}

/// C17: on 1 TF + Flow, a floating chart dropped on the left edge docks and
/// the layout becomes 2 TF + Flow, which the miniature rings; a corner drop
/// parks one with the 12 px margin; save and reload put the floating charts
/// back in place.
#[test]
fn left_edge_docks_corner_parks_reload_restores() {
    let (mut app, ctx) = saved_tab("time+flow");
    let docked = add_chart(&mut app, &ctx);
    let canvas = app.active_tab().canvas.last_canvas;
    let left = egui::pos2(canvas.left() + 8.0, canvas.center().y);
    let from = grip(&app, docked);
    drag_sized(&mut app, &ctx, TEST_WINDOW, from, left);
    frames(&mut app, &ctx, 4);
    let state = &app.active_tab().canvas.state;
    assert_eq!(state.mode(docked), Some(PaneMode::Docked));
    assert_eq!(
        state.preset().map(|preset| preset.id),
        Some("time+time+flow")
    );
    let ring =
        (app.chrome.toolbar_controls.iter()).find(|control| control.id == "toolbar.capsule.layout");
    assert_eq!(
        ring.map(|control| control.label.as_str()),
        Some("2 TF + Flow")
    );

    let parked = add_chart(&mut app, &ctx);
    let Some(FloatHost::Canvas { rect: before }) = host(&app, parked) else {
        panic!("floating");
    };
    let corner = egui::pos2(canvas.right() - 6.0, canvas.bottom() - 6.0);
    let from = grip(&app, parked);
    drag_sized(&mut app, &ctx, TEST_WINDOW, from, corner);
    frames(&mut app, &ctx, 4);
    let area = crate::floating_panes::model_rect(canvas);
    let expected = park_rect(area, Corner::BottomRight, (before.w, before.h));
    assert_eq!(
        host(&app, parked),
        Some(FloatHost::Canvas { rect: expected })
    );
    assert_eq!(area.right() - expected.right(), PARK_MARGIN_PX);

    let floating = add_chart(&mut app, &ctx);
    let placed = host(&app, floating);
    save(&mut app, &ctx);
    let moved = CanvasRect::new(100.0, 140.0, 400.0, 300.0);
    let tab = app.active_tab_mut();
    tab.canvas.state.float(floating, moved).unwrap();
    tab.canvas.state.float(parked, moved).unwrap();
    reload(&mut app, &ctx);
    assert_eq!(host(&app, floating), placed, "restored in place");
    assert_eq!(
        host(&app, parked),
        Some(FloatHost::Canvas { rect: expected })
    );
    assert_eq!(
        app.active_tab().canvas.state.mode(docked),
        Some(PaneMode::Docked)
    );
}

/// C18: ⋯ → "Open in new window" puts a chart in its own window, whose ⋯
/// offers "Return to canvas", which brings it back; a save and reload keeps
/// a window on a monitor that is there and brings one on a monitor that is
/// gone back over the canvas.
#[test]
fn pop_out_and_return_round_trip() {
    let (mut app, ctx) = saved_tab("time+flow");
    let slot = add_chart(&mut app, &ctx);
    press(&mut app, &ctx, &format!("pane_menu.{slot}"));
    press(
        &mut app,
        &ctx,
        &format!("pane_menu.{slot}.open_in_new_window"),
    );
    let state = &app.active_tab().canvas.state;
    assert_eq!(state.mode(slot), Some(PaneMode::OsWindow));
    frames(&mut app, &ctx, 4);
    press(&mut app, &ctx, &format!("pane_menu.{slot}"));
    let rows: Vec<_> = (app.active_tab().canvas.menus.drawn.iter())
        .filter(|control| control.id.starts_with(&format!("pane_menu.{slot}.")))
        .map(|control| control.label.clone())
        .collect();
    assert_eq!(rows, ["Return to canvas", "Remove chart"]);
    press(
        &mut app,
        &ctx,
        &format!("pane_menu.{slot}.return_to_canvas"),
    );
    assert_eq!(
        app.active_tab().canvas.state.mode(slot),
        Some(PaneMode::Floating)
    );

    let gone = add_chart(&mut app, &ctx);
    let out = CanvasRect::new(3000.0, 200.0, 640.0, 420.0);
    let tab = app.active_tab_mut();
    let primary = quantick_control_handlers::canvas::PRIMARY_MONITOR;
    tab.canvas.state.pop_out(slot, primary, out).unwrap();
    tab.canvas
        .state
        .pop_out(gone, "a monitor long unplugged", out)
        .unwrap();
    frames(&mut app, &ctx, 2);
    save(&mut app, &ctx);
    let moved = CanvasRect::new(40.0, 60.0, 500.0, 300.0);
    let tab = app.active_tab_mut();
    tab.canvas.state.pop_out(slot, primary, moved).unwrap();
    reload(&mut app, &ctx);
    let state = &app.active_tab().canvas.state;
    let (monitor, rect) = (primary.to_owned(), out);
    let saved = FloatHost::OsWindow { monitor, rect };
    assert_eq!(state.host(slot), Some(&saved), "its monitor is there");
    assert_eq!(
        state.mode(gone),
        Some(PaneMode::Floating),
        "back on the canvas"
    );
    let Some(FloatHost::Canvas { rect }) = state.host(gone) else {
        panic!("over the canvas");
    };
    assert_eq!((rect.w, rect.h), (out.w, out.h), "its size kept");
}

/// The capture hooks: the acts run through their capabilities once the
/// canvas drew, the + menu opens, and a floating chart is held for the dock
/// preview without being dropped.
#[test]
fn canvas_hooks_add_hold_and_open_the_menu() {
    let hooks = [
        ("QUANTICK_CANVAS_ACTS", "add:timeframe-chart"),
        ("QUANTICK_ADD_MENU", "1"),
        ("QUANTICK_CANVAS_HOLD", "2:0.03,0.5"),
    ];
    quantick_feed::hooks::captured::set_on_this_thread(Some(&hooks));
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    app.apply_layout_preset(preset("time+flow").unwrap());
    frames(&mut app, &ctx, 6);
    quantick_feed::hooks::captured::set_on_this_thread(None);
    let tab = app.active_tab();
    let floating = tab.canvas.state.floating();
    assert_eq!(floating.len(), 1, "{:?}", tab.canvas.state.panes());
    assert_eq!(floating[0].slot, 2, "the slot the hook holds");
    assert!(app.chrome.add_menu.open, "the + menu is up");
    assert!(tab.canvas.floats.held.is_some(), "the chart is held");
    assert!(
        drawn(&app, "canvas.dock.left").is_some(),
        "the drop targets"
    );
}

/// Where the canvas model lays every docked pane of the active tab now.
fn model_rects(app: &QuantickApp) -> Vec<(u32, CanvasRect)> {
    let tab = &app.active_tab().canvas;
    let area = crate::floating_panes::model_rect(tab.last_canvas);
    tab.state.rects(area)
}

/// Each pane with the built chart its slot shows.
fn seats(app: &mut QuantickApp) -> Vec<(u32, Option<Option<usize>>)> {
    let canvas = &mut app.active_tab_mut().canvas;
    let slots: Vec<u32> = canvas.state.panes().iter().map(|pane| pane.0).collect();
    let mut seat =
        |slot| quantick_workspace::canvas::slot_seat(&canvas.state, &mut canvas.seats, slot);
    slots.into_iter().map(|slot| (slot, seat(slot))).collect()
}

/// C15: on 2 TF + Flow a chart is added, docked below chart 1 by a drop on
/// that chart's bottom zone, minimized, restored, and a seam dragged: every
/// pane keeps its seat, the restore puts every pane back where it stood, and
/// the seam moves only the panes beside it.
#[test]
fn canvas_dock_minimize_restore_resize_round_trip() {
    let (mut app, ctx) = saved_tab("time+time+flow");
    let added = add_chart(&mut app, &ctx);
    let first = model_rects(&app);
    let one = (first.iter()).find(|(slot, _)| *slot == 1);
    let one = one.expect("chart 1 is docked").1;
    let below = egui::pos2(
        one.x + one.w / 2.0,
        one.bottom() - quantick_workspace::canvas::DOCK_EDGE_BAND_PX - 8.0,
    );
    let from = grip(&app, added);
    drag_sized(&mut app, &ctx, TEST_WINDOW, from, below);
    frames(&mut app, &ctx, 4);
    let docked = model_rects(&app);
    let at =
        |rects: &[(u32, CanvasRect)], held| rects.iter().find(|(slot, _)| *slot == held).unwrap().1;
    let (top, under) = (at(&docked, 1), at(&docked, added));
    assert_eq!(
        app.active_tab().canvas.state.mode(added),
        Some(PaneMode::Docked)
    );
    assert_eq!((under.x, under.w), (top.x, top.w), "below chart 1");
    assert_eq!(top.bottom(), under.y, "chart 1 split top over bottom");
    assert_eq!(at(&docked, 0), at(&first, 0), "chart 0 is unchanged");
    let seated = seats(&mut app);

    press(&mut app, &ctx, &format!("pane_menu.{added}"));
    press(&mut app, &ctx, &format!("pane_menu.{added}.minimize"));
    assert_eq!(
        app.active_tab().canvas.state.mode(added),
        Some(PaneMode::Minimized)
    );
    press(&mut app, &ctx, &format!("pane_menu.{added}.strip"));
    assert_eq!(
        app.active_tab().canvas.state.mode(added),
        Some(PaneMode::Docked)
    );
    assert_eq!(
        model_rects(&app),
        docked,
        "the restore puts every pane back"
    );
    assert_eq!(seats(&mut app), seated, "every pane keeps its seat");

    let area = crate::floating_panes::model_rect(app.active_tab().canvas.last_canvas);
    let state = &app.active_tab().canvas.state;
    let seams = quantick_workspace::canvas::tree_seams(state, area, CANVAS_DIVIDER_PX);
    let seam = (seams.iter())
        .find(|seam| seam.axis == quantick_workspace::canvas::SplitAxis::Row && seam.resizable)
        .expect("a seam between the charts");
    let grab = egui::pos2(
        seam.rect.x + seam.rect.w / 2.0,
        seam.rect.y + seam.rect.h / 2.0,
    );
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        grab,
        grab + egui::vec2(30.0, 0.0),
    );
    frames(&mut app, &ctx, 2);
    let moved = model_rects(&app);
    assert_ne!(moved, docked, "the seam moved");
    assert_eq!(seats(&mut app), seated, "a seam drag keeps every seat");
    let flow = seated
        .iter()
        .find(|(_, seat)| *seat == Some(None))
        .unwrap()
        .0;
    assert_eq!(
        at(&moved, flow),
        at(&docked, flow),
        "the flow beyond the seam stays"
    );
    let (top, under) = (at(&moved, 1), at(&moved, added));
    assert_eq!((under.x, under.w), (top.x, top.w), "still below chart 1");
    assert!((top.h - at(&docked, 1).h).abs() < 0.5, "its split kept");
}

/// A docked chart's ⋯ is its grip: dragged, the chart lifts out of the
/// layout where it stood; dropped on the flow's centre it takes the flow's
/// place, and the flow floats where the chart stood.
#[test]
fn a_dragged_menu_button_lifts_and_swaps_a_docked_chart() {
    let (mut app, ctx) = saved_tab("time+time+flow");
    let panes = app.active_tab().canvas.state.panes();
    let kind = |want| panes.iter().find(|pane| pane.1 == want).unwrap().0;
    let (chart, flow) = (kind(PaneKind::Time), kind(PaneKind::Flow));
    let stood = (model_rects(&app).into_iter()).find(|(slot, _)| *slot == chart);
    let stood = stood.unwrap().1;
    let button = drawn(&app, &format!("pane_menu.{chart}")).expect("the chart wears its ⋯");
    let frame = |app: &mut QuantickApp, events| {
        run_frame_sized(app, &ctx, TEST_WINDOW, events, egui::Modifiers::NONE);
    };
    let at = button.center();
    frame(
        &mut app,
        vec![egui::Event::PointerMoved(at), pointer_button(at, true)],
    );
    frame(
        &mut app,
        vec![egui::Event::PointerMoved(at + egui::vec2(0.0, 60.0))],
    );
    let state = &app.active_tab().canvas.state;
    assert_eq!(state.mode(chart), Some(PaneMode::Floating), "lifted");
    assert_eq!(state.host(chart), Some(&FloatHost::Canvas { rect: stood }));
    assert!(app.active_tab().canvas.floats.held.is_some(), "and held");
    let place = (model_rects(&app).into_iter()).find(|(slot, _)| *slot == flow);
    let place = place.unwrap().1;
    let centre = egui::pos2(place.x + place.w / 2.0, place.y + place.h / 2.0);
    frame(&mut app, vec![egui::Event::PointerMoved(centre)]);
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(centre),
            pointer_button(centre, false),
        ],
    );
    frames(&mut app, &ctx, 3);
    let state = &app.active_tab().canvas.state;
    assert_eq!(
        state.mode(chart),
        Some(PaneMode::Docked),
        "{:?}",
        state.panes()
    );
    assert_eq!(state.mode(flow), Some(PaneMode::Floating));
    let now = (model_rects(&app).into_iter()).find(|(slot, _)| *slot == chart);
    assert_eq!(now.unwrap().1, place, "the chart took the flow's place");
    assert_eq!(
        app.active_tab().canvas.state.host(flow),
        Some(&FloatHost::Canvas { rect: stood }),
        "the flow floats where the chart stood"
    );
    let menus = &app.active_tab().canvas.menus;
    assert!(menus.open.is_none(), "a drag opens no menu");
}
