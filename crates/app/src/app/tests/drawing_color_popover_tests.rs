// The colour popover on the drawing context bar, driven through the app:
// the additive swatches, the inline custom picker and the fill slider must
// change the object without the popover closing under the pointer.

use super::bare_canvas::app_with_history;
use super::*;
use crate::drawings::StyleColor32;

/// A selected rectangle with its colour popover open and laid out.
fn rectangle_with_open_palette() -> (QuantickApp, egui::Context) {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail.arm(Tool::Drawing(drawing_tool("rectangle")));
    click_chart(&mut app, &ctx, egui::pos2(500.0, 250.0));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 350.0));
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
    let slot = app
        .drawings
        .chrome
        .context_bar()
        .color_rect()
        .expect("the bar renders its colour slot");
    click_chart(&mut app, &ctx, slot.center());
    // A second frame lets the popover settle the size it opens with.
    run_frame(&mut app, &ctx);
    assert!(app.drawings.chrome.context_bar().popover_open());
    (app, ctx)
}

fn rectangle_style(app: &QuantickApp) -> drawings::DrawingStyle {
    app.active_tab().flow_pane.drawings.items()[0].style
}

#[test]
fn the_palette_offers_pure_red_green_blue_and_their_mixes() {
    let (mut app, ctx) = rectangle_with_open_palette();
    // The theme row holds eight; the additive row follows it.
    let red = app
        .drawings
        .chrome
        .context_bar()
        .swatch_rect(8)
        .expect("the additive row is rendered");
    let white = app
        .drawings
        .chrome
        .context_bar()
        .swatch_rect(14)
        .expect("white closes the additive row");
    assert!(app.drawings.chrome.context_bar().swatch_rect(15).is_none());
    click_chart(&mut app, &ctx, red.center());
    run_frame(&mut app, &ctx);
    assert_eq!(
        rectangle_style(&app).color32(),
        egui::Color32::from_rgb(255, 0, 0)
    );
    click_chart(&mut app, &ctx, white.center());
    run_frame(&mut app, &ctx);
    assert_eq!(rectangle_style(&app).color32(), egui::Color32::WHITE);
}

#[test]
fn dragging_the_fill_slider_changes_the_fill_and_keeps_the_popover() {
    let (mut app, ctx) = rectangle_with_open_palette();
    let before = rectangle_style(&app).fill_alpha;
    let slider = app
        .drawings
        .chrome
        .context_bar()
        .fill_slider_rect()
        .expect("a rectangle has an interior, so the fill row is shown");
    let start = egui::pos2(slider.left() + 4.0, slider.center().y);
    let end = egui::pos2(slider.right() - 4.0, slider.center().y);
    drag_chart(&mut app, &ctx, start, end);
    run_frame(&mut app, &ctx);
    assert_ne!(rectangle_style(&app).fill_alpha, before, "the fill moved");
    assert!(
        app.drawings.chrome.context_bar().popover_open(),
        "a drag on the slider is not a drag of the chart"
    );
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
}

#[test]
fn the_custom_picker_opens_inline_and_recolours_without_closing() {
    let (mut app, ctx) = rectangle_with_open_palette();
    let before = rectangle_style(&app).color32();
    let toggle = app
        .drawings
        .chrome
        .context_bar()
        .custom_toggle_rect()
        .expect("the custom row is rendered");
    click_chart(&mut app, &ctx, toggle.center());
    run_frame(&mut app, &ctx);
    // The taller popover is re-measured and may flip above the bar.
    run_frame(&mut app, &ctx);
    let picker = app
        .drawings
        .chrome
        .context_bar()
        .custom_picker_rect()
        .expect("custom expands the picker inside the popover");
    // The saturation/value square fills the middle of egui's picker, under
    // its numeric row and preview and above the hue strip.
    let start = picker.center();
    let end = picker.center() + egui::vec2(-30.0, 15.0);
    drag_chart(&mut app, &ctx, start, end);
    run_frame(&mut app, &ctx);
    assert_ne!(rectangle_style(&app).color32(), before, "the colour moved");
    assert!(app.drawings.chrome.context_bar().popover_open());
    assert!(
        app.drawings
            .chrome
            .context_bar()
            .custom_picker_rect()
            .is_some(),
        "the picker stays open under the pointer"
    );
    // A plain click inside the picker must not dismiss it either.
    click_chart(&mut app, &ctx, end);
    run_frame(&mut app, &ctx);
    assert!(app.drawings.chrome.context_bar().popover_open());
}
