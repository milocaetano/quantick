//! A chart's ⋯ menu, its minimized strip and the remove dialog, operated the
//! way a trader does: by pressing what was painted.
use super::*;

use crate::canvas_layout::preset;
use quantick_workspace::canvas::{PaneKind, PaneMode, PresetGroup};

fn frames(app: &mut QuantickApp, ctx: &egui::Context, count: usize) -> egui::FullOutput {
    for _ in 1..count {
        run_frame(app, ctx);
    }
    run_frame(app, ctx)
}

/// Where the control the tab published as `id` was drawn.
fn drawn(app: &QuantickApp, id: &str) -> Option<egui::Rect> {
    (app.active_tab().canvas.menus.drawn.iter())
        .find(|control| control.id == id)
        .map(|control| control.rect)
}

/// Press the control published as `id`, then let any popup it opens settle.
fn press(app: &mut QuantickApp, ctx: &egui::Context, id: &str) -> egui::FullOutput {
    let rect = drawn(app, id).unwrap_or_else(|| panic!("{id} was not drawn"));
    click_sized(app, ctx, TEST_WINDOW, rect.center());
    frames(app, ctx, 2)
}

/// The slot the canvas gave the first pane of `kind`.
fn slot_of(app: &QuantickApp, kind: PaneKind) -> u32 {
    let panes = app.active_tab().canvas.state.panes();
    panes.iter().find(|(_, held, _)| *held == kind).unwrap().0
}

fn pending(app: &mut QuantickApp) -> Option<u32> {
    app.surfaces
        .remove_chart
        .scoped(app.runtime.tabs.active_id())
        .0
        .pending()
}

fn mode(app: &QuantickApp, slot: u32) -> Option<PaneMode> {
    app.active_tab().canvas.state.mode(slot)
}

/// A tab on `id`, its charts built, saved with auto save off.
fn saved_tab(id: &str) -> (QuantickApp, egui::Context) {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    app.runtime.workspace.book.book_mut().set_autosave(false);
    app.apply_layout_preset(preset(id).unwrap());
    frames(&mut app, &ctx, 6);
    let save = vec![key_press_with(egui::Key::S, egui::Modifiers::CTRL)];
    run_sized_frame(&mut app, &ctx, TEST_WINDOW, save);
    frames(&mut app, &ctx, 2);
    (app, ctx)
}

/// C14: ⋯ → Minimize folds a chart to a named strip the others fill around,
/// and » restores it; ⋯ → Maximize fills the canvas, the row then reads
/// "Restore size", and choosing it or Esc brings the chart back.
#[test]
fn minimize_strip_and_maximize_restore() {
    let (mut app, ctx) = saved_tab("time+flow");
    let (time, flow) = (slot_of(&app, PaneKind::Time), slot_of(&app, PaneKind::Flow));
    for slot in [time, flow] {
        assert!(
            drawn(&app, &format!("pane_menu.{slot}")).is_some(),
            "every header wears ⋯"
        );
    }
    press(&mut app, &ctx, &format!("pane_menu.{time}"));
    let rows: Vec<_> = (app.active_tab().canvas.menus.drawn.iter())
        .filter(|control| control.id.starts_with(&format!("pane_menu.{time}.")))
        .map(|control| control.label.as_str())
        .collect();
    assert_eq!(
        rows,
        ["Minimize", "Maximize", "Open in new window", "Remove chart"]
    );
    let output = press(&mut app, &ctx, &format!("pane_menu.{time}.minimize"));
    assert_eq!(mode(&app, time), Some(PaneMode::Minimized));
    let texts = painted_text(&output);
    assert!(
        texts.iter().any(|text| text.starts_with("Time · ")),
        "the strip names the chart along itself: {texts:?}"
    );
    let canvas = app.active_tab().canvas.last_canvas;
    let strip = drawn(&app, &format!("pane_menu.{time}.strip")).expect("the strip's restore");
    assert!(strip.width() <= 26.0, "a strip, not a chart");
    let bands = &app.active_tab().canvas.drawn_bands;
    let flow_band = bands
        .iter()
        .find(|(side, _)| *side == PaneSide::Flow)
        .unwrap()
        .1;
    assert!(
        flow_band.width() > canvas.width() - 40.0,
        "the flow takes the space"
    );
    press(&mut app, &ctx, &format!("pane_menu.{time}.strip"));
    assert_eq!(mode(&app, time), Some(PaneMode::Docked), "» restores");

    press(&mut app, &ctx, &format!("pane_menu.{flow}"));
    press(&mut app, &ctx, &format!("pane_menu.{flow}.maximize"));
    assert_eq!(mode(&app, flow), Some(PaneMode::Maximized));
    press(&mut app, &ctx, &format!("pane_menu.{flow}"));
    let restore = (app.active_tab().canvas.menus.drawn.iter())
        .find(|control| control.id == format!("pane_menu.{flow}.restore"))
        .map(|control| control.label.clone());
    assert_eq!(restore.as_deref(), Some("Restore size"));
    press(&mut app, &ctx, &format!("pane_menu.{flow}.restore"));
    assert_eq!(mode(&app, flow), Some(PaneMode::Docked), "the row restores");

    press(&mut app, &ctx, &format!("pane_menu.{flow}"));
    press(&mut app, &ctx, &format!("pane_menu.{flow}.maximize"));
    assert_eq!(mode(&app, flow), Some(PaneMode::Maximized));
    run_sized_frame(
        &mut app,
        &ctx,
        TEST_WINDOW,
        vec![key_press(egui::Key::Escape)],
    );
    frames(&mut app, &ctx, 2);
    assert_eq!(mode(&app, flow), Some(PaneMode::Docked), "Esc restores");
}

/// C15: removing a chart the save holds asks first, in the parent's words,
/// and "Minimize instead" minimizes; a chart the save does not hold goes
/// without asking; removing the last flow leaves a TIMEFRAMES layout.
#[test]
fn remove_asks_only_for_saved_panes() {
    let (mut app, ctx) = saved_tab("time+flow");
    let (time, flow) = (slot_of(&app, PaneKind::Time), slot_of(&app, PaneKind::Flow));
    app.apply_layout_preset(preset("time+time+flow").unwrap());
    frames(&mut app, &ctx, 6);
    let added = 2;
    assert_eq!(mode(&app, added), Some(PaneMode::Docked));
    press(&mut app, &ctx, &format!("pane_menu.{added}"));
    press(&mut app, &ctx, &format!("pane_menu.{added}.remove"));
    assert_eq!(
        mode(&app, added),
        None,
        "a chart the save lacks goes quietly"
    );
    assert_eq!(pending(&mut app), None);

    let name = (app.runtime.workspace.book)
        .tab_row(app.runtime.tabs.active_id())
        .0;
    press(&mut app, &ctx, &format!("pane_menu.{time}"));
    let output = press(&mut app, &ctx, &format!("pane_menu.{time}.remove"));
    assert_eq!(pending(&mut app), Some(time), "it asks");
    let texts = painted_text(&output);
    let said = |line: &str| texts.iter().any(|text| text == line);
    let lines = [
        format!("Remove this chart from “{name}”?"),
        format!("“{name}” is saved with this chart (1 TF + Flow)."),
        "Removing it changes the layout to Flow.".to_owned(),
        format!("The saved “{name}” keeps the chart until you save."),
    ];
    for line in &lines {
        assert!(said(line), "{line} in {texts:?}");
    }
    press(&mut app, &ctx, "remove_chart.minimize_instead");
    assert_eq!(
        mode(&app, time),
        Some(PaneMode::Minimized),
        "minimized instead"
    );
    assert_eq!(pending(&mut app), None);

    press(&mut app, &ctx, &format!("pane_menu.{flow}"));
    press(&mut app, &ctx, &format!("pane_menu.{flow}.remove"));
    assert_eq!(pending(&mut app), Some(flow));
    press(&mut app, &ctx, "remove_chart.remove");
    let state = &app.active_tab().canvas.state;
    assert_eq!(state.mode(flow), None, "the flow is gone");
    let preset = state.preset().expect("a catalogue layout");
    assert_eq!(preset.group, PresetGroup::Timeframes);
    assert_eq!(
        mode(&app, time),
        Some(PaneMode::Docked),
        "the last chart unfolds"
    );
}

/// Cancellation clears the host's pending prompt without changing its canvas.
#[test]
fn remove_cancel_and_escape_clear_pending_without_mutation() {
    use quantick_workspace::canvas::RemoveAnswer;
    let (mut app, ctx) = saved_tab("time+flow");
    let time = slot_of(&app, PaneKind::Time);
    let before = app.active_tab().canvas.state.clone();
    for escape in [false, true] {
        press(&mut app, &ctx, &format!("pane_menu.{time}"));
        press(&mut app, &ctx, &format!("pane_menu.{time}.remove"));
        assert_eq!(pending(&mut app), Some(time));
        for choice in RemoveAnswer::ALL {
            let id = format!("remove_chart.{}", choice.id());
            let control = app
                .active_tab()
                .canvas
                .menus
                .drawn
                .iter()
                .find(|control| control.id == id)
                .unwrap();
            assert_eq!(control.label, choice.label());
            assert_eq!(
                control.capability,
                quantick_control_handlers::canvas::answer_capability(choice)
            );
        }
        if escape {
            run_sized_frame(
                &mut app,
                &ctx,
                TEST_WINDOW,
                vec![key_press(egui::Key::Escape)],
            );
            frames(&mut app, &ctx, 2);
        } else {
            press(&mut app, &ctx, "remove_chart.cancel");
        }
        assert_eq!(pending(&mut app), None);
        assert_eq!(app.active_tab().canvas.state, before);
        assert!(drawn(&app, "remove_chart.cancel").is_none());
    }
}

#[test]
fn remove_clicked_answer_wins_over_simultaneous_escape() {
    let (mut app, ctx) = saved_tab("time+flow");
    let time = slot_of(&app, PaneKind::Time);
    press(&mut app, &ctx, &format!("pane_menu.{time}"));
    press(&mut app, &ctx, &format!("pane_menu.{time}.remove"));
    let position = drawn(&app, "remove_chart.minimize_instead")
        .unwrap()
        .center();
    run_sized_frame(
        &mut app,
        &ctx,
        TEST_WINDOW,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
        ],
    );
    run_sized_frame(
        &mut app,
        &ctx,
        TEST_WINDOW,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, false),
            key_press(egui::Key::Escape),
        ],
    );
    frames(&mut app, &ctx, 2);
    assert_eq!(mode(&app, time), Some(PaneMode::Minimized));
    assert_eq!(pending(&mut app), None);
}

#[test]
fn remove_pending_follows_tabs_and_prunes_only_closed_owners() {
    let (mut app, ctx) = saved_tab("time+flow");
    let first = app.runtime.tabs.active_id();
    let workspace = app.runtime.workspace.book.tab_row(first).0;
    let slot = slot_of(&app, PaneKind::Time);
    press(&mut app, &ctx, &format!("pane_menu.{slot}"));
    press(&mut app, &ctx, &format!("pane_menu.{slot}.remove"));
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "OTHERUSDT".to_owned(), None);
    let second = app.runtime.tabs.active_id();
    frames(&mut app, &ctx, 6);
    quantick_control_handlers::workspace_book::set_tab_workspace(&mut app, second, &workspace)
        .unwrap();
    frames(&mut app, &ctx, 6);
    assert_eq!(slot_of(&app, PaneKind::Time), slot);
    let book = &app.runtime.workspace.book;
    let saved = book
        .book()
        .recipe(&book.tab_row(second).0)
        .expect("saved second tab");
    assert!(crate::app::canvas_wiring::recipe_state(saved).holds(slot));
    assert_eq!(pending(&mut app), None);
    press(&mut app, &ctx, &format!("pane_menu.{slot}"));
    press(&mut app, &ctx, &format!("pane_menu.{slot}.remove"));
    assert_eq!(pending(&mut app), Some(slot));
    app.runtime.tabs.select(0);
    frames(&mut app, &ctx, 2);
    assert_eq!(pending(&mut app), Some(slot));
    assert!(drawn(&app, "remove_chart.cancel").is_some());
    press(&mut app, &ctx, "remove_chart.cancel");
    assert_eq!(pending(&mut app), None);
    app.runtime.tabs.select(1);
    frames(&mut app, &ctx, 2);
    assert_eq!(
        pending(&mut app),
        Some(slot),
        "the other tab keeps its dialog"
    );
    app.runtime.tabs.select(0);
    frames(&mut app, &ctx, 2);
    press(&mut app, &ctx, &format!("pane_menu.{slot}"));
    press(&mut app, &ctx, &format!("pane_menu.{slot}.remove"));
    app.arrangement_adapter().close_tab(1).unwrap();
    frames(&mut app, &ctx, 2);
    assert_eq!(app.runtime.tabs.active_id(), first);
    assert_eq!(pending(&mut app), Some(slot));
    assert_eq!(
        app.surfaces.remove_chart.scoped(second).0.pending(),
        None,
        "closed scope was pruned"
    );
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "THIRDUSDT".to_owned(), None);
    frames(&mut app, &ctx, 2);
    assert!(
        app.runtime.tabs.active_id() > second,
        "tab IDs are not recycled"
    );
    assert_eq!(pending(&mut app), None);
}

#[test]
fn refused_remove_still_consumes_the_pending_answer() {
    use quantick_control_handlers::canvas::{CanvasAct, CanvasHost};
    let (mut app, ctx) = saved_tab("time+flow");
    let (time, flow) = (slot_of(&app, PaneKind::Time), slot_of(&app, PaneKind::Flow));
    press(&mut app, &ctx, &format!("pane_menu.{time}"));
    press(&mut app, &ctx, &format!("pane_menu.{time}.remove"));
    app.act_on_canvas(CanvasAct::Remove {
        slot: flow,
        confirm: true,
    })
    .unwrap();
    frames(&mut app, &ctx, 2);
    assert_eq!(pending(&mut app), Some(time));
    press(&mut app, &ctx, "remove_chart.remove");
    assert_eq!(
        mode(&app, time),
        Some(PaneMode::Docked),
        "the last chart cannot be removed"
    );
    assert_eq!(
        pending(&mut app),
        None,
        "refusal does not reopen the dialog"
    );
    assert!(drawn(&app, "remove_chart.remove").is_none());
}

#[test]
fn pane_menu_row_settles_once_after_canvas() {
    let (mut app, ctx) = saved_tab("time+flow");
    let slot = slot_of(&app, PaneKind::Time);
    press(&mut app, &ctx, &format!("pane_menu.{slot}"));
    press(&mut app, &ctx, &format!("pane_menu.{slot}.minimize"));
    let tab = app.runtime.tabs.active_id();
    assert_eq!(mode(&app, slot), Some(PaneMode::Minimized));
    assert!(!app.surfaces.remove_chart.scoped(tab).0.has_pick());
    let after = app.active_tab().canvas.state.clone();
    super::super::canvas_wiring::settle_canvas(&mut app, &ctx);
    assert_eq!(app.active_tab().canvas.state, after);
    assert!(!app.surfaces.remove_chart.scoped(tab).0.has_pick());
    press(&mut app, &ctx, &format!("pane_menu.{slot}.strip"));
    assert_eq!(mode(&app, slot), Some(PaneMode::Docked));
    assert!(!app.surfaces.remove_chart.scoped(tab).0.has_pick());
}
