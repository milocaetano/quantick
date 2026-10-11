//! Chart drawings without a window: Model-View-Update over the marks a trader
//! places on a chart.
//!
//! The functional core lives here. [`update`] folds a [`DrawingIntent`] into
//! the [`MarkStore`] and returns the [`DrawingEffect`]s the shell must carry
//! out; each [`MarkTool`] turns marks into a [`MarkScene`] and answers hit
//! tests in screen pixels. The imperative shell, the window, only translates
//! pointer and keyboard input into intents and paints scenes. It never decides
//! what a mark is.
//!
//! This is the skeleton: the port and the shared vocabulary, with no
//! behaviour. Intents, effects and the store's contents arrive with the code
//! that moves out of the window.

use quantick_chart::geometry::PixelRect;
use quantick_chart::style::PremultipliedRgba;

/// A point in screen pixels, `x` rightwards and `y` downwards, as a
/// renderer's coordinate system has it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Px {
    pub x: f32,
    pub y: f32,
}

/// How an outline is drawn: its width in pixels and its colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub width_px: f32,
    pub color: PremultipliedRgba,
}

/// One primitive a renderer paints. Every coordinate is already in screen
/// pixels: the tool did the price and time mapping, the renderer does none.
#[derive(Debug, Clone, PartialEq)]
pub enum MarkShape {
    /// A straight segment between two points.
    Line { from: Px, to: Px, stroke: Stroke },
    /// An open chain of segments through its points, in order.
    Polyline { points: Vec<Px>, stroke: Stroke },
    /// A closed, filled area with an optional outline.
    Polygon {
        points: Vec<Px>,
        fill: PremultipliedRgba,
        stroke: Option<Stroke>,
    },
    /// An axis-aligned rectangle, filled and optionally outlined.
    Rect {
        rect: PixelRect,
        fill: PremultipliedRgba,
        stroke: Option<Stroke>,
    },
    /// The ellipse inscribed in `bounds`.
    Ellipse {
        bounds: PixelRect,
        fill: PremultipliedRgba,
        stroke: Option<Stroke>,
    },
    /// A label whose top-left corner sits at `at`.
    Text {
        at: Px,
        text: String,
        size_px: f32,
        color: PremultipliedRgba,
    },
    /// A free-hand stroke, closed back to its first point when `closed`.
    Path {
        points: Vec<Px>,
        closed: bool,
        stroke: Stroke,
    },
}

/// Everything one tool asks a renderer to paint, in painting order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MarkScene {
    pub shapes: Vec<MarkShape>,
}

/// What a tool is told about the chart it draws on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawContext {
    /// The plot area in screen pixels.
    pub plot: PixelRect,
}

/// The answer to a hit test that found a mark under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// How far the pointer is from the mark, in pixels; the nearest wins.
    pub distance_px: f32,
}

/// The port every drawing tool implements: it paints from, and is hit-tested
/// against, the same context, so what a trader sees is what a click reaches.
pub trait MarkTool {
    /// The shapes this tool's marks paint into `ctx`.
    fn scene(&self, ctx: &DrawContext) -> MarkScene;
    /// The mark under `at`, if any.
    fn hit(&self, at: Px, ctx: &DrawContext) -> Option<Hit>;
}

/// The model: every mark on a chart.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MarkStore {}

/// Something the window asks the drawings to do: a pointer gesture or a
/// command, already translated out of the window's input vocabulary.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawingIntent {}

/// Something [`update`] asks the shell to carry out, such as saving.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawingEffect {}

/// Fold one intent into the store, returning the effects the shell runs.
pub fn update(_store: &mut MarkStore, intent: DrawingIntent) -> Vec<DrawingEffect> {
    match intent {}
}
