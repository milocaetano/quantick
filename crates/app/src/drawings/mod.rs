//! The window's side of chart drawings.
//!
//! The drawings themselves — the mark model, every tool, the scenes they
//! paint and the hit tests they answer — are headless, in
//! `quantick_drawings`. This module is their address in the window and the
//! egui half they need: the colour crossing (`style`), the screen
//! coordinates in egui's types (`geometry`), the tools' inspector tabs
//! (`tool_tabs`, `fib`), the context and action bars, and where the preset
//! file lives. The leaves are private: the re-exports below are the
//! subsystem's address, so no call site outside it moves when an item
//! changes leaf.

pub mod action_bar;
pub mod context_bar;
pub mod presets;

mod fib;
mod geometry;
mod style;
mod tool_tabs;

#[cfg(test)]
mod headless_pin_tests;

pub(crate) use geometry::{distance_to_segment, egui_rect, pos, px, px_rect};
#[cfg(any(feature = "drawing-harness", test))]
pub(crate) use quantick_drawings::FrvpPayload;
#[cfg(test)]
pub(crate) use quantick_drawings::UNDO_HISTORY_LIMIT;
#[cfg(test)]
pub(crate) use quantick_drawings::ValueUnit;
pub use quantick_drawings::{
    ChartPoint, Constrain, DEFAULT_DRAWING_COLOR, DRAWING_TOOLS, DeleteOutcome, DrawContext,
    DrawingAuthor, DrawingBand, DrawingId, DrawingPayload, DrawingScope, DrawingStyle, DrawingTool,
    Duplicated, GlyphSize, IconDots, IconLetter, IconStrokes, MAX_DRAWING_FILL_ALPHA,
    MAX_DRAWING_WIDTH_PX, MIN_DRAWING_WIDTH_PX, MarkScene, NewDrawing, PaneKey, PresetHost,
    RECTANGLE_TOOL_ID, RectanglePayload, ToolFamily, ToolShortcut, has_saved_default,
    new_drawing_from_defaults, reset_tool_default, save_tool_default,
};
pub use style::{StyleColor32, shortcut_key};
pub(crate) use style::{from_color32, painted_color, to_color32};
pub use tool_tabs::draw_extra_tab;

/// One drawn mark, holding the registry tool that paints it.
pub type Drawing = quantick_drawings::Drawing<DrawingTool>;
/// The marks of one pane and their undo history.
pub type Drawings = quantick_drawings::Drawings<DrawingTool>;
