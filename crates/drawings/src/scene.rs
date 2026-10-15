//! The view a tool hands a renderer: shapes in screen pixels, in painting
//! order. The tool did the price and time mapping; the renderer only turns
//! each [`MarkShape`] into its own primitive.
//!
//! [`MarkScene`]'s methods are named after the painter calls they replace, so
//! a tool reads as it did when it painted directly, and each shape maps to
//! exactly one renderer primitive — which is what lets the scene golden in
//! `app` prove the move changed no pixel.

use quantick_chart::geometry::PixelRect;
use quantick_chart::style::PremultipliedRgba;

use crate::color::TRANSPARENT;
use crate::{Align2, Px, PxRect};

/// How an outline is drawn: its width in pixels and its colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub color: PremultipliedRgba,
}

impl Stroke {
    /// No outline at all.
    pub const NONE: Self = Self {
        width: 0.0,
        color: TRANSPARENT,
    };

    #[must_use]
    pub const fn new(width: f32, color: PremultipliedRgba) -> Self {
        Self { width, color }
    }
}

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

/// One primitive a renderer paints. Every coordinate is already in screen
/// pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum MarkShape {
    /// A straight segment between two points.
    Line { from: Px, to: Px, stroke: Stroke },
    /// A chain of segments through `points`, closed back to the first when
    /// `closed`; `fill` paints the interior of a closed, convex path.
    Path {
        points: Vec<Px>,
        closed: bool,
        fill: PremultipliedRgba,
        stroke: Stroke,
    },
    /// An open chain of segments drawn as `dash_px` dashes and `gap_px` gaps.
    DashedLine {
        points: Vec<Px>,
        stroke: Stroke,
        dash_px: f32,
        gap_px: f32,
    },
    /// An axis-aligned rectangle with rounded corners.
    Rect {
        rect: PixelRect,
        rounding_px: f32,
        fill: PremultipliedRgba,
        stroke: Stroke,
    },
    /// A disc and its ring.
    Circle {
        center: Px,
        radius_px: f32,
        fill: PremultipliedRgba,
        stroke: Stroke,
    },
    /// One line of text whose `anchor` point sits at `at`.
    Text {
        at: Px,
        anchor: Align2,
        text: String,
        font: FontSpec,
        color: PremultipliedRgba,
    },
}

impl MarkShape {
    /// An open polyline.
    #[must_use]
    pub fn line(points: Vec<Px>, stroke: Stroke) -> Self {
        Self::Path {
            points,
            closed: false,
            fill: TRANSPARENT,
            stroke,
        }
    }

    /// A closed outline with no fill.
    #[must_use]
    pub fn closed_line(points: Vec<Px>, stroke: Stroke) -> Self {
        Self::Path {
            points,
            closed: true,
            fill: TRANSPARENT,
            stroke,
        }
    }

    /// A filled convex polygon with its outline.
    #[must_use]
    pub fn convex_polygon(points: Vec<Px>, fill: PremultipliedRgba, stroke: Stroke) -> Self {
        Self::Path {
            points,
            closed: true,
            fill,
            stroke,
        }
    }

    /// A dashed polyline.
    #[must_use]
    pub fn dashed_line(points: &[Px], stroke: Stroke, dash_px: f32, gap_px: f32) -> Self {
        Self::DashedLine {
            points: points.to_vec(),
            stroke,
            dash_px,
            gap_px,
        }
    }
}

/// Everything one object asks a renderer to paint, in painting order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MarkScene {
    pub shapes: Vec<MarkShape>,
}

impl MarkScene {
    pub fn add(&mut self, shape: MarkShape) {
        self.shapes.push(shape);
    }

    pub fn line_segment(&mut self, [from, to]: [Px; 2], stroke: Stroke) {
        self.add(MarkShape::Line { from, to, stroke });
    }

    pub fn rect_filled(&mut self, rect: PxRect, rounding_px: f32, fill: PremultipliedRgba) {
        self.add(MarkShape::Rect {
            rect: rect.into(),
            rounding_px,
            fill,
            stroke: Stroke::NONE,
        });
    }

    pub fn rect_stroke(&mut self, rect: PxRect, rounding_px: f32, stroke: Stroke) {
        self.add(MarkShape::Rect {
            rect: rect.into(),
            rounding_px,
            fill: TRANSPARENT,
            stroke,
        });
    }

    pub fn circle_filled(&mut self, center: Px, radius_px: f32, fill: PremultipliedRgba) {
        self.add(MarkShape::Circle {
            center,
            radius_px,
            fill,
            stroke: Stroke::NONE,
        });
    }

    pub fn circle_stroke(&mut self, center: Px, radius_px: f32, stroke: Stroke) {
        self.add(MarkShape::Circle {
            center,
            radius_px,
            fill: TRANSPARENT,
            stroke,
        });
    }

    pub fn text(
        &mut self,
        at: Px,
        anchor: Align2,
        text: impl ToString,
        font: FontSpec,
        color: PremultipliedRgba,
    ) {
        self.add(MarkShape::Text {
            at,
            anchor,
            text: text.to_string(),
            font,
            color,
        });
    }
}

/// Paint `from`–`to` as `dash_px` dashes separated by `gap_px` gaps, starting
/// on a dash. A drawing repaints every frame, so this walks the segment
/// itself and emits plain line segments.
pub fn dashed_segment(
    scene: &mut MarkScene,
    from: Px,
    to: Px,
    stroke: Stroke,
    dash_px: f32,
    gap_px: f32,
) {
    let span = to - from;
    let length = span.length();
    let step = dash_px + gap_px;
    if !(length > f32::EPSILON && length.is_finite() && step > 0.0 && step.is_finite()) {
        return;
    }
    let direction = span / length;
    let mut travelled = 0.0_f32;
    while travelled < length {
        let dash_end = (travelled + dash_px).min(length);
        scene.line_segment(
            [from + direction * travelled, from + direction * dash_end],
            stroke,
        );
        travelled += step;
    }
}

/// [`dashed_segment`] for the part of `from`–`to` that can reach `clip`,
/// in the phase the whole segment has: a line that runs far past the chart
/// costs the dashes across the chart, not the dashes along its length, and
/// the ones on screen land where the whole walk would put them.
pub fn dashed_segment_within(
    scene: &mut MarkScene,
    clip: PxRect,
    from: Px,
    to: Px,
    stroke: Stroke,
    dash_px: f32,
    gap_px: f32,
) {
    let span = to - from;
    let length = span.length();
    let step = dash_px + gap_px;
    if !(length > f32::EPSILON && length.is_finite() && step > 0.0 && step.is_finite()) {
        return;
    }
    // A dash reaching the clip may start a dash's length outside it.
    let Some((near, far)) = span_within(clip.expand(dash_px.max(0.0)), from, span) else {
        return;
    };
    let direction = span / length;
    // Whole dashes only: the one holding each end of the visible span
    // is the whole walk's dash, never a dash cut at the clip.
    let first = (near * length / step).floor() * step;
    let last = ((far * length / step).ceil() * step).min(length);
    if first < last {
        dashed_segment(
            scene,
            from + direction * first,
            from + direction * last,
            stroke,
            dash_px,
            gap_px,
        );
    }
}

/// The parameter range of `from + t * span`, `t` in `0..=1`, inside
/// `clip` (Liang–Barsky), or `None` when the segment misses it.
fn span_within(clip: PxRect, from: Px, span: Px) -> Option<(f32, f32)> {
    let (mut enter, mut leave) = (0.0_f32, 1.0_f32);
    for (delta, start, low, high) in [
        (span.x, from.x, clip.min.x, clip.max.x),
        (span.y, from.y, clip.min.y, clip.max.y),
    ] {
        for (p, q) in [(-delta, start - low), (delta, high - start)] {
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    enter = enter.max(t);
                } else {
                    leave = leave.min(t);
                }
            }
        }
    }
    (enter <= leave).then_some((enter, leave))
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod scene_tests;
