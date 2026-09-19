//! The File menu's tab entries, operated the way a trader clicks them.
use super::*;

/// Open the File menu, then click one of its entries by its painted label.
fn click_file_entry(app: &mut QuantickApp, ctx: &egui::Context, entry: &str) {
    let output = run_frame(app, ctx);
    let file = painted_text_center(&output, "File").expect("the menu bar paints File");
    click_sized(app, ctx, TEST_WINDOW, file);
    let output = run_frame(app, ctx);
    let position = painted_text_center(&output, entry)
        .unwrap_or_else(|| panic!("missing {entry:?} in {:?}", painted_text(&output)));
    click_sized(app, ctx, TEST_WINDOW, position);
}

/// The tab strip draws after the File menu in the same bar; its answer —
/// usually "nothing" — must not erase the entry the trader just chose.
#[test]
fn file_new_tab_opens_the_source_picker() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    assert!(!app.surfaces.source_picker.is_open());
    click_file_entry(&mut app, &ctx, "New Tab…");
    assert!(
        app.surfaces.source_picker.is_open(),
        "File > New Tab… opens the source picker, like Ctrl+T and the strip's +"
    );
}

/// See [`file_new_tab_opens_the_source_picker`].
#[test]
fn file_close_tab_closes_the_active_tab() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    assert_eq!(app.tabs.len(), 2);
    click_file_entry(&mut app, &ctx, "Close Tab");
    assert_eq!(
        app.tabs.len(),
        1,
        "File > Close Tab closes the active tab, like Ctrl+W and the chip's ×"
    );
}
