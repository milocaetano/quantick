//! The one call that dresses egui in the palette above.

use eframe::egui;

use super::{Palette, active_tint};
use quantick_appearance::FontRole;

/// Point egui's own widgets at the tokens, so combos, sliders and windows
/// drawn anywhere in the app match the chrome without per-call overrides.
pub fn apply(ctx: &egui::Context) {
    super::sync(ctx, &quantick_appearance::Appearance::default());
}

pub(super) fn apply_palette(ctx: &egui::Context, palette: &Palette, typography_changed: bool) {
    let mut style = (*ctx.style()).clone();
    if typography_changed {
        for (style_name, role) in [
            (egui::TextStyle::Heading, FontRole::Heading),
            (egui::TextStyle::Body, FontRole::Body),
            (egui::TextStyle::Button, FontRole::Button),
            (egui::TextStyle::Monospace, FontRole::Monospace),
            (egui::TextStyle::Small, FontRole::Small),
        ] {
            style.text_styles.insert(style_name, palette.font(role));
        }
    }
    // Tooltips wait a beat (350 ms) so a sweep across the tool rail does not
    // flash a trail of labels.
    style.interaction.tooltip_delay = 0.35;
    style.spacing.interact_size.y = palette.layout(18.0);
    let visuals = &mut style.visuals;
    *visuals = if palette.resolved.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = palette.chrome();
    visuals.window_fill = palette.chrome();
    visuals.window_stroke = egui::Stroke::new(1.0_f32, palette.border());
    visuals.extreme_bg_color = palette.inset();
    visuals.faint_bg_color = palette.inset();
    visuals.widgets.noninteractive.bg_fill = palette.chrome();
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, palette.border());
    visuals.widgets.noninteractive.fg_stroke.color = palette.text_muted();
    visuals.widgets.inactive.bg_fill = palette.control();
    visuals.widgets.inactive.weak_bg_fill = palette.control();
    visuals.widgets.inactive.fg_stroke.color = palette.text_primary();
    let dark = palette.resolved.dark;
    if let Some(edge) = quantick_appearance::widget::resting_edge(dark) {
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, *palette.get(edge));
    }
    let hover = *palette.get(quantick_appearance::widget::hover_fill(dark));
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, palette.border());
    visuals.widgets.hovered.fg_stroke.color = palette.text_primary();
    visuals.widgets.active.bg_fill = active_tint(palette.accent());
    visuals.widgets.active.weak_bg_fill = active_tint(palette.accent());
    visuals.widgets.active.fg_stroke.color = palette.text_primary();
    visuals.widgets.open.bg_fill = palette.control();
    visuals.widgets.open.weak_bg_fill = palette.control();
    visuals.widgets.open.fg_stroke.color = palette.text_primary();
    visuals.selection.bg_fill = active_tint(palette.accent());
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, palette.accent());
    visuals.hyperlink_color = palette.accent();
    visuals.code_bg_color = palette.widget_code();
    visuals.warn_fg_color = palette.widget_warn();
    visuals.error_fg_color = palette.widget_error();
    visuals.text_cursor.stroke.color = palette.widget_cursor();
    visuals.window_shadow.color = palette.widget_shadow();
    visuals.popup_shadow.color = palette.widget_shadow();
    visuals.widgets.noninteractive.weak_bg_fill = palette.widget_noninteractive();
    visuals.widgets.active.bg_stroke.color = palette.widget_active_border();
    visuals.widgets.open.bg_stroke.color = palette.widget_open_border();
    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
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
}
