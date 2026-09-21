//! The one call that dresses egui in the palette above.

use eframe::egui;

use super::{ACCENT, BORDER, CHROME, CONTROL, INSET, TEXT_MUTED, TEXT_PRIMARY, active_tint};

/// Point egui's own widgets at the tokens, so combos, sliders and windows
/// drawn anywhere in the app match the chrome without per-call overrides.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    // Tooltips wait a beat (350 ms) so a sweep across the tool rail does not
    // flash a trail of labels.
    style.interaction.tooltip_delay = 0.35;
    let visuals = &mut style.visuals;
    *visuals = egui::Visuals::dark();
    visuals.panel_fill = CHROME;
    visuals.window_fill = CHROME;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, BORDER);
    visuals.extreme_bg_color = INSET;
    visuals.faint_bg_color = INSET;
    visuals.widgets.noninteractive.bg_fill = CHROME;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke.color = TEXT_MUTED;
    visuals.widgets.inactive.bg_fill = CONTROL;
    visuals.widgets.inactive.weak_bg_fill = CONTROL;
    visuals.widgets.inactive.fg_stroke.color = TEXT_PRIMARY;
    visuals.widgets.hovered.bg_fill = BORDER;
    visuals.widgets.hovered.weak_bg_fill = BORDER;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    visuals.widgets.hovered.fg_stroke.color = TEXT_PRIMARY;
    visuals.widgets.active.bg_fill = active_tint(ACCENT);
    visuals.widgets.active.weak_bg_fill = active_tint(ACCENT);
    visuals.widgets.active.fg_stroke.color = TEXT_PRIMARY;
    visuals.widgets.open.bg_fill = CONTROL;
    visuals.widgets.open.weak_bg_fill = CONTROL;
    visuals.widgets.open.fg_stroke.color = TEXT_PRIMARY;
    visuals.selection.bg_fill = active_tint(ACCENT);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
    visuals.hyperlink_color = ACCENT;
    ctx.set_style(style);
}
