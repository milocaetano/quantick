//! A drawn object's look as egui paints it: the colour crossing, the stroke
//! and fill, and the selection chrome. The style itself is headless, in
//! `quantick_drawings`.

use eframe::egui;
use quantick_chart::style::PremultipliedRgba;

use crate::theme;

use super::{CLAMPED_OPACITY, Drawing, DrawingStyle};

/// Handles read as hollow rings, not solid discs: the core is the chart's own
/// backdrop, so the handle marks the anchor without adding a bright blob over
/// the candles (`docs/ux/drawing-tools-2026-08.md` §D2).
pub const SELECTED_ANCHOR_FILL: egui::Color32 = theme::CANVAS;

/// Selection never repaints the object white: it keeps the configured colour
/// and paints this soft halo underneath instead, plus ring anchor handles.
/// Premultiplied ~11% white — enough to find the object under the pointer,
/// not enough to double its visual weight.
pub const SELECTION_HALO_COLOR: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(28, 28, 28, 28);

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
pub fn to_color32(color: PremultipliedRgba) -> egui::Color32 {
    let [r, g, b, a] = color.to_array();
    egui::Color32::from_rgba_premultiplied(r, g, b, a)
}

#[must_use]
pub fn from_color32(color: egui::Color32) -> PremultipliedRgba {
    PremultipliedRgba::from_premultiplied(color.to_array())
}

pub fn drawing_stroke(style: DrawingStyle) -> egui::Stroke {
    egui::Stroke::new(style.width_px, style.color32())
}

pub fn drawing_fill(style: DrawingStyle) -> egui::Color32 {
    let color = style.color32();
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), style.fill_alpha)
}

pub fn painted_color(drawing: &Drawing) -> egui::Color32 {
    if drawing.off_series || drawing.foreign_market {
        drawing.style.color32().gamma_multiply(CLAMPED_OPACITY)
    } else {
        drawing.style.color32()
    }
}
