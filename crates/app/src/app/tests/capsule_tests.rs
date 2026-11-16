//! The toolbar's capsule, popover A and row, operated the way a trader does.
use super::*;

use crate::app::chrome::capsule::snapshot;
use crate::canvas_layout::{LAYOUT_PRESETS, preset};

fn frames(app: &mut QuantickApp, ctx: &egui::Context, count: usize) -> egui::FullOutput {
    for _ in 1..count {
        run_frame(app, ctx);
    }
    run_frame(app, ctx)
}

/// The centre of the first painted text starting with `wanted`.
fn painted_starting(output: &egui::FullOutput, wanted: &str) -> Option<egui::Pos2> {
    fn walk(shape: &egui::Shape, wanted: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.text().starts_with(wanted) => {
                Some(text.visual_bounding_rect().center())
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| walk(shape, wanted)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|clipped| walk(&clipped.shape, wanted))
}

fn control<'a>(app: &'a QuantickApp, id: &str) -> Option<&'a crate::toolbar::DrawnControl> {
    app.chrome
        .toolbar_controls
        .iter()
        .find(|control| control.id == id)
}

/// C10: with auto save off a layout change shows the floppy, its dot and the
/// miniature's dot; Ctrl+S saves and clears all three.
#[test]
fn layout_change_shows_unsaved_marks_until_ctrl_s() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    app.runtime.workspace.book.book_mut().set_autosave(false);
    frames(&mut app, &ctx, 2);
    assert!(
        !snapshot(&app, &egui::Context::default()).marks().1,
        "a clean tab"
    );
    assert!(control(&app, "toolbar.capsule.save").is_none());

    app.apply_layout_preset(preset("charts-4").unwrap());
    // A frame builds each new chart; the tab settles once all four stand.
    frames(&mut app, &ctx, 6);
    let (_, unsaved, layout) = snapshot(&app, &egui::Context::default()).marks();
    assert!(
        unsaved && layout,
        "the floppy ({unsaved}) and the miniature ({layout}) carry the dot"
    );
    assert!(
        control(&app, "toolbar.capsule.save").is_some(),
        "the floppy shows"
    );

    let save = vec![key_press_with(egui::Key::S, egui::Modifiers::CTRL)];
    run_sized_frame(&mut app, &ctx, TEST_WINDOW, save);
    frames(&mut app, &ctx, 2);
    let (_, unsaved, layout) = snapshot(&app, &egui::Context::default()).marks();
    assert!(!unsaved && !layout, "Ctrl+S cleared both dots");
    assert!(
        control(&app, "toolbar.capsule.save").is_none(),
        "the floppy went"
    );
    let saved = app
        .runtime
        .workspace
        .book
        .tab_row(app.runtime.tabs.active_id());
    assert!(!saved.1);
}

fn picker_rect(ctx: &egui::Context) -> egui::Rect {
    ctx.memory(|memory| memory.area_rect(egui::Id::new("layout_popover")))
        .expect("the picker was laid out")
}

fn open_picker(app: &mut QuantickApp, ctx: &egui::Context, size: egui::Vec2) {
    let miniature = app.chrome.capsule.miniature;
    click_sized(app, ctx, size, miniature.center());
    for _ in 0..2 {
        run_sized_frame(app, ctx, size, Vec::new());
    }
    let open = app.surfaces.canvas_popovers.model_state.projection();
    assert!(open.layout_open, "the miniature opens the picker");
}

/// The picker's frame is its content's size in every state — saved, unsaved
/// with autosave off, after several opens — and hangs below its button,
/// never over the toolbar; a window too short scrolls it instead of lifting it.
#[test]
fn the_picker_is_its_contents_size_and_hangs_below_its_button() {
    use quantick_ui_model::layout_picker::{DROP_PT, OUTER};
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    for autosave in [true, false] {
        app.runtime.workspace.book.book_mut().set_autosave(autosave);
        if !autosave {
            app.apply_layout_preset(preset("charts-2").unwrap());
            frames(&mut app, &ctx, 4);
        }
        for _ in 0..3 {
            open_picker(&mut app, &ctx, TEST_WINDOW);
            let (frame, button) = (picker_rect(&ctx), app.chrome.capsule.miniature);
            assert_eq!(
                frame.size(),
                egui::vec2(OUTER[0], OUTER[1]),
                "autosave {autosave}"
            );
            let capsule = button.union(app.chrome.capsule.name);
            assert_eq!(frame.top(), (capsule.bottom() + DROP_PT).round());
            assert!(
                (frame.right() - capsule.right()).abs() <= 1.0,
                "right edges flush: {frame:?} {capsule:?}"
            );
            let escape = vec![key_press(egui::Key::Escape)];
            run_sized_frame(&mut app, &ctx, TEST_WINDOW, escape);
            let open = app.surfaces.canvas_popovers.model_state.projection();
            assert!(!open.layout_open, "Esc closes at once");
        }
    }
    let short = egui::vec2(1100.0, 380.0);
    frames(&mut app, &ctx, 1);
    run_sized_frame(&mut app, &ctx, short, Vec::new());
    open_picker(&mut app, &ctx, short);
    let (frame, button) = (picker_rect(&ctx), app.chrome.capsule.miniature);
    assert_eq!(
        frame.top(),
        (button.bottom() + DROP_PT).round(),
        "never lifted"
    );
    assert!(
        frame.height() < OUTER[1] && frame.bottom() <= short.y,
        "{frame:?}"
    );
}

/// A pick commits on release, the header says "Changed to" with the unsaved
/// state, the toolbar's miniature follows the canvas, Ctrl+Z restores the
/// previous layout, and the confirmation closes the picker on its own.
#[test]
fn a_pick_confirms_and_ctrl_z_restores_the_previous_layout() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    app.runtime.workspace.book.book_mut().set_autosave(false);
    frames(&mut app, &ctx, 2);
    let before = app.active_tab().canvas.state.preset().unwrap();
    open_picker(&mut app, &ctx, TEST_WINDOW);
    let cell = control(&app, "toolbar.layout_popover.charts-2")
        .unwrap()
        .rect;
    click_sized(&mut app, &ctx, TEST_WINDOW, cell.center());
    let output = frames(&mut app, &ctx, 3);
    let picked = preset("charts-2").unwrap();
    assert_eq!(app.active_tab().canvas.state.preset(), Some(picked));
    let miniature = quantick_workspace::canvas::matching_preset(&snapshot(&app, &ctx).tree);
    assert_eq!(miniature, Some(picked), "the miniature follows");
    let header = control(&app, "toolbar.layout_popover.header").expect("still open");
    assert!(
        header.label.contains("Changed to 2 charts"),
        "{}",
        header.label
    );
    assert!(header.label.contains("Not saved"), "{}", header.label);
    let painted = painted_text(&output);
    assert!(
        painted
            .iter()
            .any(|text| text.ends_with("Changed to 2 charts")),
        "{painted:?}"
    );
    let undo = control(&app, "toolbar.layout_popover.undo").expect("the was words");
    assert!(undo.label.contains(before.label), "{}", undo.label);
    let shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
    run_sized_frame(
        &mut app,
        &ctx,
        TEST_WINDOW,
        vec![key_press_with(egui::Key::Z, shift)],
    );
    frames(&mut app, &ctx, 2);
    assert_eq!(
        app.active_tab().canvas.state.preset(),
        Some(picked),
        "Ctrl+Shift+Z is not the picker's undo"
    );
    let ctrl_z = vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)];
    run_sized_frame(&mut app, &ctx, TEST_WINDOW, ctrl_z);
    frames(&mut app, &ctx, 3);
    assert_eq!(
        app.active_tab().canvas.state.preset(),
        Some(before),
        "Ctrl+Z"
    );
    frames(&mut app, &ctx, 90);
    let open = app.surfaces.canvas_popovers.model_state.projection();
    assert!(!open.layout_open, "the confirmation closes the picker");
}

/// One display name per layout: a cell's scene label, the header after its
/// pick and the `canvas.layout.picker` read all say the registry's label.
#[test]
fn one_display_name_per_layout_in_cell_header_scene_and_read() {
    use quantick_control_handlers::canvas::CanvasHost as _;
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    open_picker(&mut app, &ctx, TEST_WINDOW);
    let cells: Vec<_> = (app.chrome.toolbar_controls.iter())
        .filter(|control| control.id.starts_with("toolbar.layout_popover."))
        .filter(|control| !control.id.ends_with(".header"))
        .cloned()
        .collect();
    assert_eq!(cells.len(), LAYOUT_PRESETS.len());
    let read = app.layout_picker().unwrap();
    assert!(read.open);
    for (cell, preset) in cells.iter().zip(LAYOUT_PRESETS) {
        assert_eq!(cell.id, format!("toolbar.layout_popover.{}", preset.id));
        assert_eq!(cell.label, preset.label);
        let row = read
            .cells
            .iter()
            .find(|row| row.preset_id == preset.id)
            .unwrap();
        assert_eq!(row.name, preset.label);
    }
    let current = app.active_tab().canvas.state.preset().unwrap();
    let lit: Vec<_> = cells.iter().filter(|cell| cell.selected).collect();
    assert_eq!(lit.len(), 1);
    assert_eq!(lit[0].id, format!("toolbar.layout_popover.{}", current.id));
    let target = preset("flow-over-2time").unwrap();
    let cell = control(&app, "toolbar.layout_popover.flow-over-2time")
        .unwrap()
        .rect;
    click_sized(&mut app, &ctx, TEST_WINDOW, cell.center());
    frames(&mut app, &ctx, 2);
    let header = control(&app, "toolbar.layout_popover.header").unwrap();
    assert!(
        header
            .label
            .contains(&format!("Changed to {}", target.label))
    );
    let read = app.layout_picker().unwrap();
    assert_eq!(read.changed, Some(format!("Changed to {}", target.label)));
    assert_eq!(read.current_id.as_deref(), Some(target.id));
    assert_eq!(read.undo_id.as_deref(), Some(current.id));
}

/// C9: ≡ | symbol … History | BUY 1 | SELL 1 on the left; the capsule and the
/// tray on the right.
#[test]
fn toolbar_row_order_matches_the_mock() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let output = frames(&mut app, &ctx, 2);
    let at = |text: &str| {
        painted_starting(&output, text)
            .unwrap_or_else(|| panic!("{text} is painted: {:?}", painted_text(&output)))
            .x
    };
    let list = app.chrome.window_menu.toolbar_door.unwrap().center().x;
    let symbol = at(&format!("{} \u{b7}", app.active_tab().symbol));
    let history = at(&format!(
        "{} History",
        egui_phosphor::regular::CLOCK_COUNTER_CLOCKWISE
    ));
    let (buy, sell) = (at("BUY 1"), at("SELL 1"));
    let capsule = app.chrome.capsule;
    let tray = at(egui_phosphor::regular::TRAY);
    let row = [
        list,
        symbol,
        history,
        buy,
        sell,
        capsule.miniature.center().x,
        capsule.name.center().x,
        tray,
    ];
    assert!(
        row.windows(2).all(|pair| pair[0] < pair[1]),
        "left to right: {row:?}"
    );
    assert!(
        sell < TEST_WINDOW.x / 2.0,
        "BUY and SELL sit left, after History: {row:?}"
    );
    assert!(
        capsule.name.right() < tray,
        "the capsule stands before the tray"
    );
}

#[test]
fn canvas_popover_projection_is_consumed_by_one_paint() {
    use crate::surfaces::View as _;
    use quantick_ui_model::canvas_popovers::Intent;
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    app.surfaces
        .canvas_popovers
        .update(Intent::SetAddOpen(true));
    frames(&mut app, &ctx, 2);
    let scope = app.runtime.tabs.windows().presented().0;
    let (state, view) = app.surfaces.chrome_toolbar.scoped(scope);
    assert!(
        view.popovers.is_none(),
        "the registered paint consumed its borrowed projection"
    );
    let capsule = view.capsule;
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        assert!(
            view.paint(
                state,
                &mut crate::surfaces::ViewContext { ctx, ui: None },
                None
            )
            .is_none()
        );
    });
    assert!(
        output.shapes.is_empty(),
        "a second paint without input is inert"
    );
    assert_eq!(view.capsule.miniature, capsule.miniature);
    assert!(
        app.surfaces
            .canvas_popovers
            .model_state
            .projection()
            .add_open
    );
}

/// The picker belongs to its tab: after a pick on one tab, another tab's
/// frame closes it and Ctrl+Z there leaves that tab's layout alone.
#[test]
fn a_tab_change_closes_the_picker_and_ctrl_z_spares_the_other_tab() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    frames(&mut app, &ctx, 3);
    open_picker(&mut app, &ctx, TEST_WINDOW);
    let cell = control(&app, "toolbar.layout_popover.charts-2")
        .unwrap()
        .rect;
    click_sized(&mut app, &ctx, TEST_WINDOW, cell.center());
    frames(&mut app, &ctx, 2);
    assert_eq!(app.active_tab().canvas.state.preset(), preset("charts-2"));
    app.apply_tab_action(TabAction::Activate(0));
    frames(&mut app, &ctx, 2);
    let other = app.active_tab().canvas.state.preset();
    let ctrl_z = vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)];
    run_sized_frame(&mut app, &ctx, TEST_WINDOW, ctrl_z);
    frames(&mut app, &ctx, 2);
    assert_eq!(
        app.active_tab().canvas.state.preset(),
        other,
        "the other tab"
    );
    let open = app.surfaces.canvas_popovers.model_state.projection();
    assert!(!open.layout_open, "the picker closed with its tab");
    assert!(
        app.surfaces
            .canvas_popovers
            .model_state
            .picker()
            .changed
            .is_none()
    );
}
