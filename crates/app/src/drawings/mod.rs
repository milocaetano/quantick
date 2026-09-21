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
mod clipboard;
mod line_core;
mod mark_core;
mod measure_core;
mod shape_core;

// The subsystem's other owners. This module keeps only the registry; a
// tool's port and handle live in `tool`, the collection's behaviour in
// `collection`, and the rules that draft or re-anchor an object in
// `placement`.
mod collection;
mod placement;
mod tool;

// The envelope every tool and every feature outside the subsystem speaks,
// one leaf per question: what a mark is (`object`), what it looks like
// (`style`), what a tool declares to the rail (`family`), what a tool is
// handed to paint (`context`), what a tool's own state travels in
// (`payload`), what a new object opens with (`defaults`), where the marks of
// one pane live (`store`), and the screen geometry they share (`geometry`).
// The leaves are private: the re-exports below are the subsystem's address,
// so no call site outside it moves when an item changes leaf.
mod context;
mod defaults;
mod family;
mod geometry;
mod object;
mod payload;
mod store;
mod style;

// The name the whole subsystem reaches through `super::`.
use eframe::egui;

/// The handle every caller outside this module holds a tool by; the port it
/// implements stays inside the subsystem.
pub use tool::DrawingTool;
use tool::DrawingToolImpl;

pub use context::{AxisLevels, DrawContext, Handles, ValueUnit};
#[cfg(test)]
pub use defaults::NullPresetHost;
pub use defaults::{
    PresetHost, has_saved_default, new_drawing_from_defaults, reset_tool_default, save_tool_default,
};
pub(super) use family::level_with;
pub use family::{
    AnchorSnap, Constrain, IconDots, IconLetter, IconStrokes, ToolFamily, ToolShortcut,
};
pub(super) use geometry::{distance_to_segment, off_line_by, unit_normal};
pub use object::{
    ChartPoint, DeleteOutcome, Drawing, DrawingAuthor, DrawingBand, DrawingId, DrawingScope,
    Duplicated, NewDrawing, PaneKey,
};
pub use payload::{DrawingPayload, NoPayload};
pub use store::Drawings;
use store::{UNDO_HISTORY_LIMIT, UndoEntry};
pub(crate) use style::{CLAMPED_OPACITY, painted_color};
pub use style::{
    DEFAULT_DRAWING_COLOR, DrawingStyle, GlyphSize, MAX_DRAWING_FILL_ALPHA, MAX_DRAWING_WIDTH_PX,
    MIN_DRAWING_WIDTH_PX,
};
pub(super) use style::{
    FIB_LABEL_OFFSET_PX, FIB_LABEL_SIZE_PX, SELECTED_ANCHOR_FILL, SELECTED_ANCHOR_RADIUS_PX,
    SELECTED_ANCHOR_RING_WIDTH_PX, SELECTION_HALO_COLOR, SELECTION_HALO_EXTRA_WIDTH_PX,
    drawing_fill, drawing_stroke,
};

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
