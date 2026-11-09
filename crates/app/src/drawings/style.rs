//! A drawn object's look crossing into egui: the colour conversion and the
//! key a tool's shortcut names. The style itself is headless, in
//! `quantick_drawings`.

use eframe::egui;
use quantick_chart::style::PremultipliedRgba;

use super::{Drawing, DrawingStyle, ToolShortcut};

/// The colour of a [`DrawingStyle`] as the painter takes it; the only
/// crossing into egui.
pub trait StyleColor32 {
    fn color32(&self) -> egui::Color32;
    fn set_color32(&mut self, color: egui::Color32);
}

impl StyleColor32 for DrawingStyle {
    fn color32(&self) -> egui::Color32 {
        to_color32(self.color)
    }

    fn set_color32(&mut self, color: egui::Color32) {
        self.color = from_color32(color);
    }
}

/// Lossless: both sides hold the same premultiplied bytes.
#[must_use]
pub const fn to_color32(color: PremultipliedRgba) -> egui::Color32 {
    let [r, g, b, a] = color.to_array();
    egui::Color32::from_rgba_premultiplied(r, g, b, a)
}

#[must_use]
fn from_color32(color: egui::Color32) -> PremultipliedRgba {
    PremultipliedRgba::from_premultiplied(color.to_array())
}

/// The colour a mark paints in, faded when the chart under it does not back
/// it — see [`quantick_drawings::painted_color`].
pub fn painted_color(drawing: &Drawing) -> egui::Color32 {
    to_color32(quantick_drawings::painted_color(drawing))
}

/// The egui key a tool's shortcut letter names. Every shortcut a tool
/// declares is a letter key; a test holds the registry to that. The rail
/// asks for every tool on every frame, so the name is read off the stack.
#[must_use]
pub fn shortcut_key(shortcut: ToolShortcut) -> Option<egui::Key> {
    egui::Key::from_name(shortcut.key.encode_utf8(&mut [0; 4]))
}

/// A text-only backing on Light surfaces; strokes and authored ink stay intact.
/// The current palette is scoped to the actual canvas (including Bookmap).
pub(crate) fn text_backing(
    palette: &crate::theme::Palette,
    color: egui::Color32,
    background: egui::Color32,
) -> Option<egui::Color32> {
    use quantick_appearance::contrast;
    const TEXT_CONTRAST: f32 = 4.5;
    if palette.resolved.dark || color.a() == 0 {
        return None;
    }
    let rgba = |c: egui::Color32| PremultipliedRgba::from_premultiplied(c.to_array());
    let ratio =
        |back| contrast::contrast_ratio(contrast::over(rgba(color), rgba(back)), rgba(back));
    if ratio(background) >= TEXT_CONTRAST {
        return None;
    }
    Some(
        if ratio(egui::Color32::BLACK) >= ratio(egui::Color32::WHITE) {
            egui::Color32::BLACK
        } else {
            egui::Color32::WHITE
        },
    )
}
