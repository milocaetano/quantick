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
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, palette.control_edge());
    visuals.widgets.hovered.bg_fill = palette.hover_fill();
    visuals.widgets.hovered.weak_bg_fill = palette.hover_fill();
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
mod tests;
