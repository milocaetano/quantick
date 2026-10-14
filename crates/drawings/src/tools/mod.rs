//! The drawing tools: one implementation file each, registered by name.
//!
//! Each tool implements [`MarkTool`] in its own file. The registry macro is
//! the only docking point: add a module name there and the toolbox,
//! placement state, renderer and hit-testing all see the new tool. Market
//! data remains immutable and the deterministic engine never learns about
//! marks.

// Geometry shared by a family of tools. Not tools themselves, so they are not
// in the registry — a family core exists so its members stay declarations.
mod fib;
mod line_core;
mod mark_core;
mod measure_core;
mod shape_core;

// The names every tool file reaches through `super::`.
#[allow(unused_imports)]
use crate::{
    AnchorSnap, AxisLevels, BodySnap, ChartPoint, Constrain, DEFAULT_DRAWING_COLOR, DrawContext,
    DrawingBand, DrawingPayload, DrawingStyle, DrawingTool, FIB_LABEL_OFFSET_PX, FIB_LABEL_SIZE_PX,
    GlyphSize, Handles, IconDots, IconLetter, IconStrokes, MarkTool, NoPayload, PointerHint,
    PresetHost, SELECTED_ANCHOR_RADIUS_PX, SELECTED_ANCHOR_RING_WIDTH_PX, ToolFamily, ToolShortcut,
    ValueUnit, dashed_segment, distance_to_segment, drawing_fill, drawing_stroke, level_with,
    off_line_by, unit_normal,
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

pub(crate) use line_core::LINES_FAMILY;

// The tool-owned payloads, ids and limits the window's inspector tabs,
// refresh passes and strategy seat name.
pub use anchored_vwap::{AVWAP_SOURCES, AvwapPayload};
pub use fib::{
    DEFAULT_BAND_ALPHA, EXTENSION_PRESETS, Extend as FibExtend, FIB_EXTENSION_DOTS,
    FIB_EXTENSION_ICON, FIB_EXTENSION_LETTER, FIB_RETRACEMENT_DOTS, FIB_RETRACEMENT_ICON,
    FIB_RETRACEMENT_LETTER, FibKind, FibLevelSpec, FibPayload, FibPreset, LabelMode, LabelPosition,
    MAX_BAND_ALPHA, MAX_LEVEL_LABEL_LEN, RATIO_DUPLICATE_TOLERANCE, RETRACEMENT_PRESETS,
    builtin_presets, log_scale_allowed, parse_ratio,
};
pub use fixed_range_profile::{
    DEFAULT_VALUE_AREA_PCT, FrvpPayload, MAX_WIDTH_FRAC, MIN_WIDTH_FRAC,
};
pub use parallel_channel::ChannelPayload;
pub use rectangle::{RectanglePayload, TOOL_ID as RECTANGLE_TOOL_ID};
pub use text::{MAX_TEXT_PX, MIN_TEXT_PX, PLACEHOLDER as TEXT_PLACEHOLDER, TextPayload};

#[cfg(test)]
mod tests;
