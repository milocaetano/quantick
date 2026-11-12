//! Anchored note layout and persisted appearance, independent of the renderer.
use std::cell::Cell;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::Px;
use crate::geometry::{pos2, vec2};
use crate::rect::PxRect;

/// The two typefaces a mark's label is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamily {
    Proportional,
    Monospace,
}

/// A label's face and size in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontSpec {
    pub size_px: f32,
    pub family: FontFamily,
}

impl FontSpec {
    pub fn themed(
        theme: &quantick_appearance::ResolvedTheme,
        role: quantick_appearance::FontRole,
        size: f32,
    ) -> Self {
        let font = theme.typography.at_size(role, size);
        Self {
            size_px: font.size,
            family: match font.family {
                quantick_appearance::FontFamily::Proportional => FontFamily::Proportional,
                quantick_appearance::FontFamily::Monospace => FontFamily::Monospace,
            },
        }
    }

    #[must_use]
    pub const fn proportional(size_px: f32) -> Self {
        Self {
            size_px,
            family: FontFamily::Proportional,
        }
    }

    #[must_use]
    pub const fn monospace(size_px: f32) -> Self {
        Self {
            size_px,
            family: FontFamily::Monospace,
        }
    }
}

/// How big a label sets, asked of the renderer that owns the fonts: a tool
/// that sizes a plate around its words needs the words' real extent.
pub trait TextMetrics {
    /// The width and height `text` occupies on one line in `font`.
    fn size(&self, text: &str, font: FontSpec) -> Px;
    fn note_size(&self, text: &str, format: TextFormat) -> Px {
        self.size(text, format.font)
    }
}

/// The metrics of a context that sets no label — a hit test or a gesture
/// paints nothing, so every size it could ask for is zero.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unmeasured;

impl TextMetrics for Unmeasured {
    fn size(&self, _text: &str, _font: FontSpec) -> Px {
        Px::ZERO
    }
}

/// See [`Unmeasured`].
pub const UNMEASURED: Unmeasured = Unmeasured;

pub const MIN_WRAP_PX: f32 = 40.0;
pub const MAX_WRAP_PX: f32 = 1200.0;
pub const DEFAULT_WRAP_PX: f32 = 240.0;
pub const TEXT_LIFT_PX: f32 = 4.0;
pub const TEXT_PADDING_PX: f32 = 4.0;

/// Optional properties default to the legacy note's unadorned appearance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TextAppearance {
    pub bold: bool,
    pub italic: bool,
    /// Straight (not premultiplied) RGBA bytes, including optional transparency.
    pub background: Option<[u8; 4]>,
    pub border: Option<[u8; 4]>,
    /// None keeps explicit line breaks only. Width is in screen pixels.
    pub wrap_width_px: Option<f32>,
}

impl TextAppearance {
    pub fn valid(self) -> bool {
        self.wrap_width_px
            .is_none_or(|width| width.is_finite() && (MIN_WRAP_PX..=MAX_WRAP_PX).contains(&width))
    }

    pub fn padding(self) -> f32 {
        if self.background.is_some() || self.border.is_some() {
            TEXT_PADDING_PX
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextFormat {
    pub font: FontSpec,
    pub appearance: TextAppearance,
}

/// A renderer measurement is derived state, excluded from persistence and undo.
/// The signature prevents a stale extent after changing words or appearance.
#[derive(Debug, Clone, Default)]
pub struct TextExtent(Cell<Option<(u64, Px)>>);

impl PartialEq for TextExtent {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl TextExtent {
    pub fn measure(&self, text: &str, format: TextFormat, metrics: &dyn TextMetrics) -> Px {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hash);
        format.font.size_px.to_bits().hash(&mut hash);
        (format.font.family == FontFamily::Monospace).hash(&mut hash);
        format.appearance.bold.hash(&mut hash);
        format.appearance.italic.hash(&mut hash);
        format
            .appearance
            .wrap_width_px
            .map(f32::to_bits)
            .hash(&mut hash);
        let key = hash.finish();
        // Measured contexts also refresh after DPI/font changes; the font engine
        // caches the galley. Headless picks reuse the last real measurement.
        let measured = metrics.note_size(text, format);
        if measured.x > 0.0 && measured.y > 0.0 {
            self.0.set(Some((key, measured)));
            return measured;
        }
        if let Some((cached, size)) = self.0.get()
            && key == cached
        {
            return size;
        }
        let width = text
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(1) as f32
            * format.font.size_px
            * 0.55;
        let wrap = format.appearance.wrap_width_px.unwrap_or(width.max(1.0));
        vec2(
            width.min(wrap),
            text.lines()
                .map(|line| {
                    ((line.chars().count() as f32 * format.font.size_px * 0.55) / wrap)
                        .ceil()
                        .max(1.0)
                })
                .sum::<f32>()
                .max(1.0)
                * format.font.size_px
                * 1.25,
        )
    }
}

/// The note's lower left corner stays at its price/time anchor at every zoom.
pub fn text_bounds(anchor: Px, size: Px, appearance: TextAppearance) -> PxRect {
    let pad = appearance.padding();
    PxRect::from_min_size(
        pos2(anchor.x - pad, anchor.y - TEXT_LIFT_PX - size.y - pad),
        size + vec2(pad * 2.0, pad * 2.0),
    )
}

/// Reverse the shared note bounds for an editor using the same projection.
pub fn anchor_from_bounds(bounds: PxRect, appearance: TextAppearance) -> Px {
    pos2(
        bounds.min.x + appearance.padding(),
        bounds.max.y + TEXT_LIFT_PX - appearance.padding(),
    )
}
