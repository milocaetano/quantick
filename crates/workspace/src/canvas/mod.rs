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
//! - [`bands`]: what the window paints a canvas from and compares it by.
//! - [`menu`]: a chart's ⋯ menu, its folded strip and the remove dialog.
//! - [`seams`]: the seams between docked panes and the resize they share.
//! - [`columns`]: the column painter's folds, focus on a switch and its reach.

pub mod bands;
pub mod columns;
pub mod drag;
pub mod drop;
pub mod menu;
pub mod presets;
pub mod seams;
pub mod state;
pub mod tree;
pub mod widgets;

pub use bands::{
    LayoutMarks, Seats, Signal, TreeBand, inset_seams, same_layout, seat_drawn, seat_mode,
    seat_slot, signal, slot_seat, tree_bands,
};
pub use columns::{fold_column, revealed};
pub use drag::{
    DOCK_HINT, DOCK_PILL, DropOutcome, FloatGesture, LIFT_PX, MIN_FLOAT_SIZE, RESIZE_GRIP_PX,
    ResizeEdges, open_panes,
};
pub use drop::{
    Corner, DOCK_EDGE_BAND_PX, DockTarget, DropTarget, PANE_CENTRE_PX, PANE_SIDE_PX,
    PARK_CORNER_PX, PARK_MARGIN_PX, drop_target, drop_zones, kept_inside, park_rect,
};
pub use menu::{
    AddRow, PANE_MENU_BUTTON_PX, PaneMenuItem, RemoveAnswer, RemovePrompt, SURFACE_UNSERVED,
    add_menu, menu_hook, pane_menu, remove_prompt, strip_label,
};
pub use presets::{
    LAYOUT_PRESETS, LayoutPreset, MAX_CANVAS_PANES, PaneKind, PresetGroup, matching_preset, preset,
};
pub use seams::{MIN_TREE_PANE_PX, SeamAddress, TreeSeam, tree_seams};
pub use state::{
    CanvasRefusal, CanvasState, FloatHost, FloatingPane, MINIMIZED_STRIP_PX, PaneMode,
    remove_needs_confirm,
};
pub use tree::{CanvasPoint, CanvasRect, Edge, PaneSlot, PaneTree, SplitAxis};
pub use widgets::{WIDGETS, WidgetSpec, widget};

#[cfg(test)]
mod bands_tests;
#[cfg(test)]
mod columns_tests;
#[cfg(test)]
mod drag_tests;
#[cfg(test)]
mod drop_tests;
#[cfg(test)]
mod menu_tests;
#[cfg(test)]
mod seams_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod tree_tests;
