//! What a drawn object looks like: its style, the stock values a new one
//! opens with, and the selection chrome painted around it.

use eframe::egui;
use quantick_chart::style::PremultipliedRgba;

use crate::theme;

use super::Drawing;

pub const DEFAULT_DRAWING_COLOR: PremultipliedRgba = PremultipliedRgba::from_rgb(138, 180, 248);

/// A drawing is an annotation *on* the chart, never a second series: the
/// stock stroke is a hairline, thinner than the candle bodies it sits over
/// (`docs/ux/drawing-tools-2026-08.md` §D2). The width slider keeps its full
/// range — this is the default, not the ceiling.
pub const DEFAULT_DRAWING_WIDTH_PX: f32 = 1.0;

pub const DEFAULT_DRAWING_FILL_ALPHA: u8 = 14;

pub const MIN_DRAWING_WIDTH_PX: f32 = 0.5;

pub const MAX_DRAWING_WIDTH_PX: f32 = 6.0;

pub const MAX_DRAWING_FILL_ALPHA: u8 = 160;

pub const SELECTED_ANCHOR_RADIUS_PX: f32 = 3.5;

/// Handles read as hollow rings, not solid discs: the core is the chart's own
/// backdrop, so the handle marks the anchor without adding a bright blob over
/// the candles (`docs/ux/drawing-tools-2026-08.md` §D2).
pub const SELECTED_ANCHOR_FILL: egui::Color32 = theme::CANVAS;

pub const SELECTED_ANCHOR_RING_WIDTH_PX: f32 = 1.25;

/// Selection never repaints the object white: it keeps the configured colour
/// and paints this soft halo underneath instead, plus ring anchor handles.
/// Premultiplied ~11% white — enough to find the object under the pointer,
/// not enough to double its visual weight.
pub const SELECTION_HALO_COLOR: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(28, 28, 28, 28);

/// How much wider than the object's own stroke the halo pass paints.
pub const SELECTION_HALO_EXTRA_WIDTH_PX: f32 = 2.5;

pub const FIB_LABEL_OFFSET_PX: f32 = 3.0;

pub const FIB_LABEL_SIZE_PX: f32 = 10.0;

/// A glyph tool's own type size, and the range it accepts. The range travels
/// with the value so a host offering sizes never has to know which tool it
/// is talking to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphSize {
    pub px: f32,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawingStyle {
    pub color: PremultipliedRgba,
    pub width_px: f32,
    pub fill_alpha: u8,
}

impl Default for DrawingStyle {
    fn default() -> Self {
        Self {
            color: DEFAULT_DRAWING_COLOR,
            width_px: DEFAULT_DRAWING_WIDTH_PX,
            fill_alpha: DEFAULT_DRAWING_FILL_ALPHA,
        }
    }
}

impl DrawingStyle {
    /// The colour as the painter takes it; the only crossing into egui.
    #[must_use]
    pub fn color32(&self) -> egui::Color32 {
        to_color32(self.color)
    }

    pub fn set_color32(&mut self, color: egui::Color32) {
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

/// Shared honesty fade for marks whose location this series does not prove.
pub const CLAMPED_OPACITY: f32 = 0.45;

pub fn painted_color(drawing: &Drawing) -> egui::Color32 {
    if drawing.off_series || drawing.foreign_market {
        drawing.style.color32().gamma_multiply(CLAMPED_OPACITY)
    } else {
        drawing.style.color32()
    }
}
