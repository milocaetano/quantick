//! The one call that dresses egui in the palette above.

use eframe::egui;

use super::{Palette, active_tint};
use quantick_appearance::{FontRole, Token};

/// Point egui's own widgets at the tokens, so combos, sliders and windows
/// drawn anywhere in the app match the chrome without per-call overrides.
pub fn apply(ctx: &egui::Context) {
    super::sync(ctx, &quantick_appearance::Appearance::default());
}

pub(super) fn apply_palette(ctx: &egui::Context, palette: &Palette) {
    let mut style = (*ctx.style()).clone();
    for (style_name, role) in [
        (egui::TextStyle::Heading, FontRole::Heading),
        (egui::TextStyle::Body, FontRole::Body),
        (egui::TextStyle::Button, FontRole::Button),
        (egui::TextStyle::Monospace, FontRole::Monospace),
        (egui::TextStyle::Small, FontRole::Small),
    ] {
        style.text_styles.insert(style_name, palette.font(role));
    }
    // Tooltips wait a beat (350 ms) so a sweep across the tool rail does not
    // flash a trail of labels.
    style.interaction.tooltip_delay = 0.35;
    let visuals = &mut style.visuals;
    *visuals = if palette.resolved.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = palette.color(Token::Chrome);
    visuals.window_fill = palette.color(Token::Chrome);
    visuals.window_stroke = egui::Stroke::new(1.0_f32, palette.color(Token::Border));
    visuals.extreme_bg_color = palette.color(Token::Inset);
    visuals.faint_bg_color = palette.color(Token::Inset);
    visuals.widgets.noninteractive.bg_fill = palette.color(Token::Chrome);
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0_f32, palette.color(Token::Border));
    visuals.widgets.noninteractive.fg_stroke.color = palette.color(Token::TextMuted);
    visuals.widgets.inactive.bg_fill = palette.color(Token::Control);
    visuals.widgets.inactive.weak_bg_fill = palette.color(Token::Control);
    visuals.widgets.inactive.fg_stroke.color = palette.color(Token::TextPrimary);
    visuals.widgets.hovered.bg_fill = palette.color(Token::Border);
    visuals.widgets.hovered.weak_bg_fill = palette.color(Token::Border);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, palette.color(Token::Border));
    visuals.widgets.hovered.fg_stroke.color = palette.color(Token::TextPrimary);
    visuals.widgets.active.bg_fill = active_tint(palette.color(Token::Accent));
    visuals.widgets.active.weak_bg_fill = active_tint(palette.color(Token::Accent));
    visuals.widgets.active.fg_stroke.color = palette.color(Token::TextPrimary);
    visuals.widgets.open.bg_fill = palette.color(Token::Control);
    visuals.widgets.open.weak_bg_fill = palette.color(Token::Control);
    visuals.widgets.open.fg_stroke.color = palette.color(Token::TextPrimary);
    visuals.selection.bg_fill = active_tint(palette.color(Token::Accent));
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, palette.color(Token::Accent));
    visuals.hyperlink_color = palette.color(Token::Accent);
    ctx.set_style(style);
}
