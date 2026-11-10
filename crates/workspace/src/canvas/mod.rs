//! The workspace canvas as a headless model: which panes a tab draws, how the
//! canvas splits between them, which are folded, maximized or floating, where
//! a dragged pane would land, and what can be added.
//!
//! The window paints from this and wires input into it; it decides nothing
//! here. Every type is plain data with deterministic transitions: no clock, no
//! pixels read off a screen, `BTreeSet`/`Vec` only.
//!
//! - [`presets`]: the layout registry, every entry with its pane tree.
//! - [`tree`]: the split tree and the geometry it resolves to.
//! - [`state`]: one tab's canvas and its transitions.
//! - [`drop`]: the one rule that decides what a drop does.
//! - [`widgets`]: what the add button offers.

pub mod drop;
pub mod presets;
pub mod state;
pub mod tree;
pub mod widgets;

pub use drop::{
    Corner, DOCK_EDGE_BAND_PX, DropTarget, Edge, PARK_CORNER_PX, PARK_MARGIN_PX, drop_target,
    park_rect,
};
pub use presets::{
    LAYOUT_PRESETS, LayoutPreset, MAX_CANVAS_PANES, PaneKind, PresetGroup, matching_preset, preset,
};
pub use state::{
    CanvasRefusal, CanvasState, FloatHost, FloatingPane, MINIMIZED_STRIP_PX, PaneMode,
    remove_needs_confirm,
};
pub use tree::{CanvasPoint, CanvasRect, PaneSlot, PaneTree, SplitAxis};
pub use widgets::{WIDGETS, WidgetSpec, widget};

#[cfg(test)]
mod drop_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod tree_tests;
