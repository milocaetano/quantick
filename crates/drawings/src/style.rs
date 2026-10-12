//! What a drawn object looks like: its style, the stock values a new one
//! opens with, and the selection chrome painted around it.

use quantick_chart::style::PremultipliedRgba;

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

pub const SELECTED_ANCHOR_RING_WIDTH_PX: f32 = 1.25;

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

/// Shared honesty fade for marks whose location this series does not prove.
pub const CLAMPED_OPACITY: f32 = 0.45;

/// Handles read as hollow rings, not solid discs: the core is the chart's own
/// backdrop, so the handle marks the anchor without adding a bright blob over
/// the candles (`docs/ux/drawing-tools-2026-08.md` §D2).
pub const SELECTED_ANCHOR_FILL: PremultipliedRgba = crate::palette::CANVAS;

/// Selection never repaints the object white: it keeps the configured colour
/// and paints this soft halo underneath instead, plus ring anchor handles.
/// Premultiplied ~11% white — enough to find the object under the pointer,
/// not enough to double its visual weight.
pub const SELECTION_HALO_COLOR: PremultipliedRgba =
    crate::color::rgba_premultiplied(28, 28, 28, 28);

#[must_use]
pub fn drawing_stroke(style: DrawingStyle) -> crate::Stroke {
    crate::Stroke::new(style.width_px, style.color)
}

#[must_use]
pub fn drawing_fill(style: DrawingStyle) -> PremultipliedRgba {
    use crate::RgbaExt as _;
    let color = style.color;
    crate::color::rgba_unmultiplied(color.r(), color.g(), color.b(), style.fill_alpha)
}

/// The colour a mark paints in: its own, faded when the chart under it does
/// not back it (off this series, or drawn on another market).
#[must_use]
pub fn painted_color<T>(drawing: &crate::Drawing<T>) -> PremultipliedRgba {
    use crate::RgbaExt as _;
    if drawing.off_series || drawing.foreign_market {
        drawing.style.color.gamma_multiply(CLAMPED_OPACITY)
    } else {
        drawing.style.color
    }
}

/// `style` faded for a mark whose place in time the chart under it does not
/// prove. Only the colour fades: its prices are still exact, so a zone keeps
/// its fill and reads as the zone the trader drew rather than two stray
/// border lines.
#[must_use]
pub fn faded_style(style: DrawingStyle) -> DrawingStyle {
    use crate::RgbaExt as _;
    DrawingStyle {
        color: style.color.gamma_multiply(CLAMPED_OPACITY),
        ..style
    }
}

/// The style a mark paints in on its own pane. Off this series it fades but
/// keeps its fill ([`faded_style`]); drawn on another market its prices are
/// not levels here at all, so it gives the fill up too.
#[must_use]
pub fn painted_style<T>(drawing: &crate::Drawing<T>) -> DrawingStyle {
    if drawing.foreign_market {
        DrawingStyle {
            color: painted_color(drawing),
            fill_alpha: 0,
            ..drawing.style
        }
    } else if drawing.off_series {
        faded_style(drawing.style)
    } else {
        drawing.style
    }
}
