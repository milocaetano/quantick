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
        [
            "Minimize",
            "Maximize",
            "Float",
            "Open in new window",
            "Remove chart"
        ]
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
    assert_eq!(app.active_tab().canvas.menus.removing, None);

    let name = (app.runtime.workspace.book)
        .tab_row(app.runtime.tabs.active_id())
        .0;
    press(&mut app, &ctx, &format!("pane_menu.{time}"));
    let output = press(&mut app, &ctx, &format!("pane_menu.{time}.remove"));
    assert_eq!(
        app.active_tab().canvas.menus.removing,
        Some(time),
        "it asks"
    );
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
    assert_eq!(app.active_tab().canvas.menus.removing, None);

    press(&mut app, &ctx, &format!("pane_menu.{flow}"));
    press(&mut app, &ctx, &format!("pane_menu.{flow}.remove"));
    assert_eq!(app.active_tab().canvas.menus.removing, Some(flow));
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
