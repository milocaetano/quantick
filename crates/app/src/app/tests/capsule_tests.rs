//! The toolbar's capsule, popover A and row, operated the way a trader does.
use super::*;

use crate::app::capsule_wiring::CapsuleState;
use crate::canvas_layout::{LAYOUT_PRESETS, PresetGroup, preset};

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
    assert_eq!(
        CapsuleState::of(&app, &egui::Context::default()).marks().1,
        false,
        "a clean tab"
    );
    assert!(control(&app, "toolbar.capsule.save").is_none());

    app.apply_layout_preset(preset("charts-4").unwrap());
    frames(&mut app, &ctx, 3);
    let (_, unsaved, layout) = CapsuleState::of(&app, &egui::Context::default()).marks();
    assert!(
        unsaved && layout,
        "the floppy and the miniature carry the dot"
    );
    assert!(
        control(&app, "toolbar.capsule.save").is_some(),
        "the floppy shows"
    );

    let save = vec![key_press_with(egui::Key::S, egui::Modifiers::CTRL)];
    run_sized_frame(&mut app, &ctx, TEST_WINDOW, save);
    frames(&mut app, &ctx, 2);
    let (_, unsaved, layout) = CapsuleState::of(&app, &egui::Context::default()).marks();
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

/// C11: popover A lists 8 + 8 cells from the registry, rings the current
/// layout, tags the saved one, and shows the footer only while they differ.
#[test]
fn popover_lists_registry_rings_current_tags_saved() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    app.runtime.workspace.book.book_mut().set_autosave(false);
    let miniature = app.chrome.capsule.miniature;
    click_sized(&mut app, &ctx, TEST_WINDOW, miniature.center());
    let output = frames(&mut app, &ctx, 2);
    let cells: Vec<_> = (app.chrome.toolbar_controls.iter())
        .filter(|control| control.id.starts_with("toolbar.layout_popover."))
        .collect();
    assert_eq!(cells.len(), LAYOUT_PRESETS.len());
    for group in [PresetGroup::WithFlow, PresetGroup::Timeframes] {
        let held = LAYOUT_PRESETS.iter().filter(|preset| preset.group == group);
        assert_eq!(held.count(), 8, "{group:?}");
    }
    for (cell, preset) in cells.iter().zip(LAYOUT_PRESETS) {
        assert_eq!(
            cell.id,
            format!("toolbar.layout_popover.{}", preset.id),
            "registry order"
        );
    }
    let current = app.active_tab().canvas.state.preset().unwrap();
    let lit: Vec<_> = cells.iter().filter(|cell| cell.selected).collect();
    assert_eq!(lit.len(), 1);
    assert_eq!(lit[0].id, format!("toolbar.layout_popover.{}", current.id));
    let tag = painted_text_center(&output, "saved").expect("the saved layout wears its tag");
    let saved_cell = cells.iter().find(|cell| cell.selected).unwrap();
    assert!(
        saved_cell.rect.expand(4.0).contains(tag),
        "the tag sits on the saved cell"
    );
    assert!(
        painted_text_center(&output, "Ctrl+S").is_none(),
        "no footer while they agree"
    );

    let cell = (cells
        .iter()
        .find(|cell| cell.id == "toolbar.layout_popover.charts-2"))
    .unwrap()
    .rect;
    click_sized(&mut app, &ctx, TEST_WINDOW, cell.center());
    frames(&mut app, &ctx, 2);
    assert_eq!(
        app.active_tab().canvas.state.preset().map(|p| p.id),
        Some("charts-2")
    );
    assert!(!app.chrome.layout_picker_open, "a pick closes the popover");
    app.chrome.layout_picker_open = true;
    let output = frames(&mut app, &ctx, 2);
    let lit = control(&app, "toolbar.layout_popover.charts-2").unwrap();
    assert!(lit.selected, "the ring follows the canvas");
    let tag = painted_text_center(&output, "saved").expect("the saved layout keeps its tag");
    let flow = control(&app, &format!("toolbar.layout_popover.{}", current.id)).unwrap();
    assert!(
        flow.rect.expand(4.0).contains(tag),
        "the tag stays on the saved layout"
    );
    assert!(
        painted_text_center(&output, "Ctrl+S").is_some(),
        "the footer says it changed"
    );
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
