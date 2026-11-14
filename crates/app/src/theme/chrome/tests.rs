use super::*;
#[test]
fn builtins_preserve_main_default_typography_and_spacing() {
    let historical = egui::Style::default();
    for builtin in quantick_appearance::Builtin::ALL {
        let ctx = egui::Context::default();
        let mut state = quantick_appearance::Appearance::default();
        state.apply(builtin.source()).unwrap();
        super::super::sync(&ctx, &state);
        assert_eq!(ctx.style().text_styles, historical.text_styles);
        assert_eq!(ctx.style().spacing, historical.spacing);
    }
}
#[test]
fn dark_widget_colors_match_historical_toolkit_defaults_and_custom_colors_apply() {
    let ctx = egui::Context::default();
    apply(&ctx);
    let historical = egui::Visuals::dark();
    let current = ctx.style().visuals.clone();
    assert_eq!(current.code_bg_color, historical.code_bg_color);
    assert_eq!(current.warn_fg_color, historical.warn_fg_color);
    assert_eq!(current.error_fg_color, historical.error_fg_color);
    assert_eq!(current.text_cursor.stroke, historical.text_cursor.stroke);
    assert_eq!(current.window_shadow, historical.window_shadow);
    assert_eq!(current.popup_shadow, historical.popup_shadow);
    assert_eq!(
        current.widgets.active.bg_stroke,
        historical.widgets.active.bg_stroke
    );
    assert_eq!(
        current.widgets.open.bg_stroke,
        historical.widgets.open.bg_stroke
    );
    assert_eq!(
        current.widgets.noninteractive.weak_bg_fill,
        historical.widgets.noninteractive.weak_bg_fill
    );
    let mut appearance = quantick_appearance::Appearance::default();
    appearance.apply("version = 1\nname = 'Custom'\n[colors]\nwidget_cursor = '#12345680'\nwidget_shadow = '#789ABC40'\n[typography.body]\nsize = 25").unwrap();
    crate::theme::sync(&ctx, &appearance);
    let palette = crate::theme::get(&ctx);
    assert_eq!(
        ctx.style().visuals.text_cursor.stroke.color,
        palette.widget_cursor()
    );
    assert_eq!(
        ctx.style().visuals.window_shadow.color,
        palette.widget_shadow()
    );
    assert_eq!(ctx.style().spacing.interact_size.y, 36.0);
}
