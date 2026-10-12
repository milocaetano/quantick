//! Chart drawings without a window: Model-View-Update over the marks a trader
//! places on a chart.
//!
//! The functional core lives here. [`update`] folds a [`DrawingIntent`] into
//! a pane's [`Drawings`] and returns the [`DrawingEffect`]s the shell must carry
//! out; each [`MarkTool`] turns marks into a [`MarkScene`] and answers hit
//! tests in screen pixels. The imperative shell, the window, only translates
//! pointer and keyboard input into intents and paints scenes. It never decides
//! what a mark is.
//!
//! The mark model lives here: what a mark is ([`Drawing`]), the per-pane
//! book of them with its undo history ([`Drawings`]), how one is placed and
//! re-anchored, what a new one opens with, and the preset file that
//! remembers it. So do the tools ([`DRAWING_TOOLS`]): each paints a
//! [`MarkScene`] in screen pixels ([`Px`]) and answers hit tests on the same
//! geometry, and the window's one renderer turns the scene into its own
//! primitives.

use quantick_chart::geometry as chart;

pub mod frvp;
pub mod gesture;
pub mod icons;
pub mod palette;
pub mod presets;

mod clipboard;
mod collection;
mod color;
mod context;
mod defaults;
mod family;
mod geometry;
mod object;
mod payload;
mod placement;
mod rect;
mod scene;
mod store;
mod style;
mod tool;
mod tools;
mod update;

#[cfg(test)]
mod scratch;

pub use color::{RgbaExt, TRANSPARENT, WHITE, rgba_premultiplied, rgba_unmultiplied};
pub use context::{AxisLevels, DrawContext, Handles, ValueUnit};
#[cfg(any(test, feature = "test-support"))]
pub use defaults::NullPresetHost;
pub use defaults::{
    PresetHost, has_saved_default, new_drawing_from_defaults, reset_tool_default, save_tool_default,
};
pub use family::{
    AnchorSnap, BodySnap, Constrain, IconDots, IconLetter, IconStrokes, ToolFamily, ToolShortcut,
};
pub use geometry::{distance_to_segment, level_with, off_line_by, pos2, unit_normal, vec2};
pub use object::{
    ChartPoint, DeleteOutcome, Drawing, DrawingAuthor, DrawingBand, DrawingId, DrawingScope,
    Duplicated, NewDrawing, PaneKey,
};
pub use payload::{DrawingPayload, NoPayload};
pub use placement::reanchor_points;
pub use rect::{Align, Align2, PxRange, PxRect};
pub use scene::{
    FontFamily, FontSpec, MarkScene, MarkShape, Stroke, TextMetrics, UNMEASURED, Unmeasured,
    dashed_segment,
};
pub use store::{Drawings, PlacementBatch, UNDO_HISTORY_LIMIT, UndoEntry};
pub use style::{
    CLAMPED_OPACITY, DEFAULT_DRAWING_COLOR, DEFAULT_DRAWING_FILL_ALPHA, DEFAULT_DRAWING_WIDTH_PX,
    DrawingStyle, FIB_LABEL_OFFSET_PX, FIB_LABEL_SIZE_PX, GlyphSize, MAX_DRAWING_FILL_ALPHA,
    MAX_DRAWING_WIDTH_PX, MIN_DRAWING_WIDTH_PX, SELECTED_ANCHOR_FILL, SELECTED_ANCHOR_RADIUS_PX,
    SELECTED_ANCHOR_RING_WIDTH_PX, SELECTION_HALO_COLOR, SELECTION_HALO_EXTRA_WIDTH_PX,
    drawing_fill, drawing_stroke, painted_color,
};
pub use tool::{DoubleClickHint, DrawingTool, MarkTool};
pub use tools::*;
pub use update::{DrawingEffect, DrawingIntent, update};

/// The facts the mark model needs about the tool a mark was drawn with.
///
/// A mark holds its tool by a small `Copy` handle; the model asks the handle,
/// never a central table, for what placing and naming a mark of that tool
/// takes. The window's tool registry implements it until the tools move here.
pub trait ToolCatalog: Copy + PartialEq + std::fmt::Debug {
    /// The stable id presets and saved layouts store the tool under.
    fn id(self) -> &'static str;
    /// The name a list shows for an unnamed mark of this tool.
    fn name(self) -> &'static str;
    /// Anchors a placement takes; `0` for a free-hand tool the release ends.
    fn required_points(self) -> usize;
    /// The tool-owned state a fresh mark starts with.
    fn default_payload(self) -> Box<dyn DrawingPayload>;
    /// The built-in look a fresh mark opens with.
    fn default_style(self) -> DrawingStyle;
    /// The band a mark whose first anchor landed in `band` belongs to.
    fn band_for(self, band: &DrawingBand) -> DrawingBand;
}

/// A point (or an offset) in screen pixels, `x` rightwards and `y`
/// downwards, as a renderer's coordinate system has it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Px {
    pub x: f32,
    pub y: f32,
}

/// The screen vocabulary every tool test reaches for, in one glob.
#[cfg(test)]
pub(crate) mod test_prelude {
    pub(crate) use crate::{MarkScene, MarkShape, Px, PxRect, vec2};
}
