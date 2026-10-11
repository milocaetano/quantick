//! Modular user-authored chart drawings.
//!
//! Each drawing tool implements [`DrawingToolImpl`] in its own file. The
//! registry macro is the only docking point: add a module name there and the
//! toolbox, placement state, renderer and hit-testing all see the new tool.
//! Market data remains immutable and the deterministic engine never learns
//! about UI marks.

pub mod action_bar;
pub mod context_bar;
pub mod fib;
pub mod presets;

// Geometry shared by a family of tools. Not tools themselves, so they are not
// in the registry — a family core exists so its members stay declarations.
mod line_core;
mod mark_core;
mod measure_core;
mod shape_core;
mod tool;

// The subsystem's other owner here: a tool's port and handle live in `tool`.
// The mark model — what a mark is, the per-pane book with its undo history,
// placement, defaults and the preset file — is headless, in
// `quantick_drawings`; the window keeps what a tool is handed to paint
// (`context`), the egui side of a look (`style`), the shortcut key
// (`family`) and the screen geometry in egui's coordinates (`geometry`).
// The leaves are private: the re-exports below are the subsystem's address,
// so no call site outside it moves when an item changes leaf.
mod context;
mod family;
mod geometry;
mod style;

// The test modules reach egui through `use super::*`.
#[cfg(test)]
use eframe::egui;

use tool::DrawingToolImpl;
/// The handle every caller outside this module holds a tool by; the port it
/// implements stays inside the subsystem.
pub use tool::{DoubleClickHint, DrawingTool};

pub use context::{AxisLevels, DrawContext, Handles, ValueUnit};
pub use family::ToolShortcut;
pub(super) use geometry::{
    dashed_segment, distance_to_segment, level_with, off_line_by, unit_normal,
};
#[cfg(test)]
pub use quantick_drawings::NullPresetHost;
#[cfg(test)]
pub(crate) use quantick_drawings::UNDO_HISTORY_LIMIT;
pub use quantick_drawings::{
    AnchorSnap, BodySnap, ChartPoint, Constrain, DEFAULT_DRAWING_COLOR, DeleteOutcome,
    DrawingAuthor, DrawingBand, DrawingId, DrawingPayload, DrawingScope, DrawingStyle, Duplicated,
    GlyphSize, IconDots, IconLetter, IconStrokes, MAX_DRAWING_FILL_ALPHA, MAX_DRAWING_WIDTH_PX,
    MIN_DRAWING_WIDTH_PX, NewDrawing, NoPayload, PaneKey, PresetHost, ToolFamily,
    has_saved_default, new_drawing_from_defaults, reset_tool_default, save_tool_default,
};
pub(crate) use quantick_drawings::{CLAMPED_OPACITY, reanchor_points};
pub(super) use quantick_drawings::{
    FIB_LABEL_OFFSET_PX, FIB_LABEL_SIZE_PX, SELECTED_ANCHOR_RADIUS_PX,
    SELECTED_ANCHOR_RING_WIDTH_PX, SELECTION_HALO_EXTRA_WIDTH_PX,
};
pub use style::StyleColor32;
pub(super) use style::{SELECTED_ANCHOR_FILL, SELECTION_HALO_COLOR, drawing_fill, drawing_stroke};
pub(crate) use style::{from_color32, painted_color, to_color32};

/// One drawn mark, holding the registry tool that paints it.
pub type Drawing = quantick_drawings::Drawing<DrawingTool>;
/// The marks of one pane and their undo history.
pub type Drawings = quantick_drawings::Drawings<DrawingTool>;

macro_rules! register_drawing_tools {
    ($($module:ident),+ $(,)?) => {
        $(mod $module;)+
        pub const DRAWING_TOOLS: [DrawingTool; [$(stringify!($module)),+].len()] = [
            $(DrawingTool(&$module::TOOL)),+
        ];
    };
}

// The extension port: a new tool is one implementation file plus one name
// here. Order is rail order, and consecutive entries declaring the same
// family fold into one rail slot — so the grouping below is the grouping the
// trader sees, and adding a tool cannot silently reorder the rail.
register_drawing_tools!(
    // Lines
    trend_line,
    ray,
    extended_line,
    horizontal_line,
    horizontal_ray,
    vertical_line,
    arrow,
    // Channels
    parallel_channel,
    // Marks
    arrow_mark_up,
    arrow_mark_down,
    // Freehand
    brush,
    // Shapes
    rectangle,
    ellipse,
    triangle,
    // Fib
    fib_retracement,
    fib_extension,
    // Measure
    measure,
    price_range,
    date_range,
    fixed_range_profile,
    // Series
    anchored_vwap,
    // Annotation
    text,
);

// The rectangle's registry id, re-exported for the strategy seat — the one
// gate that names a specific shape (two anchors honestly bound a price
// region), in the `frvp::TOOL_ID` idiom.
pub use rectangle::TOOL_ID as RECTANGLE_TOOL_ID;

// The rectangle's payload, re-exported for the strategy seat too: whether
// the drawn band extends right decides whether an armed region ever
// expires off its right anchor.
pub use rectangle::RectanglePayload;

// The profile drawing's payload types, re-exported for `crate::frvp` — the
// refresh pass that folds engine ladders into the cache the paint reads.
pub use fixed_range_profile::FrvpPayload;

// The anchored VWAP's payload types, re-exported for `crate::avwap` — the
// refresh pass that replays the indicators-crate kernel into the cache.
pub use anchored_vwap::AvwapPayload;

#[cfg(test)]
mod tests;
