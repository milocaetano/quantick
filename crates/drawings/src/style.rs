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
