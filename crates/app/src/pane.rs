//! One chart pane: everything that answers "what is on this canvas".
//!
//! A pane owns the bar series it aggregates, the viewport and price scale it is
//! read through, the drawings anchored to its bar indices and the indicator
//! slots computed over it. What it deliberately does *not* own is the market
//! feeding it — feed channels, connection state and notices belong to the tab
//! around it — or the window chrome (menus, toolbar, dock, status bar), which
//! belongs to the application around that.
//!
//! That split is what lets one tab hold two panes over the same trades: a flow
//! pane and a time-frame pane, the split view of `docs/ux/ui-design-model.md`
//! §11. Every egui interaction id a pane registers is derived from
//! [`ChartPane::id`] for the same reason — two panes registering one id would
//! share a drag.

// The tests in `pane/tests/` reach these through `use super::*`; the production
// code that read them moved to the siblings, so only the tests still need
// them here.
#[cfg(test)]
use crate::bands::BandLabel;
#[cfg(test)]
use crate::drawings;
#[cfg(test)]
use crate::drawings::{ChartPoint, DrawingBand};
#[cfg(test)]
use crate::indicator_render;
#[cfg(test)]
use crate::plot_area::split_time_strip;
#[cfg(test)]
use crate::theme;
#[cfg(test)]
use pointer_hit::PLOT_PICK_TOLERANCE_PX;
#[cfg(test)]
use quantick_drawings::gesture::MAGNET_REACH_PX;
#[cfg(test)]
use strategies::strategy_badge_text;

mod axes_and_panes;
mod bubble_scale_controls;
pub(crate) use bubble_scale_controls::show as bubble_scale_controls;
mod canvas_gestures;
mod constants;
// `pub(crate)`, like `app::launch_hooks`: `split_time_pane` returns
// `TimePaneAreas`, which nothing outside names yet, so a `pub use` of it is an
// unused import under the workspace's deny-warnings policy while a public
// module keeps it nameable as `pane::canvas_split::TimePaneAreas`.
pub(crate) mod canvas_split;
mod context_menu;
mod draw_chart;
mod draw_frame;
mod footprint;
mod frame;
mod frame_layout;
mod frame_stages;
mod gesture_input;
mod history_worker;
mod lane_rail;
mod layer_painters;
mod layers;
mod render_registry;
pub(crate) fn registered_layers() -> quantick_layers::LayerRegistry {
    render_registry::standard().layers()
}
mod menu_renderer;
mod menus;
mod objects_menu;
mod pointer_hit;
mod primary_button;
mod quick_range;
mod series;
pub(crate) mod strategies;
mod tape_switch;

/// Every sub-struct of a pane is `Pane*`, without exception: prefixing only
/// where the bare noun clashes is how one ends up beside a `PaneFrame`.
pub use context_menu::PaneContextMenu;
pub use footprint::PaneFootprint;
pub use frame::PaneFrame;
pub use quantick_drawings::gesture::PaneGestures;
pub use strategies::PaneStrategies;
pub use tape_switch::TapeSwitch;

pub(crate) use canvas_split::split_pane_layout_strip;
/// The canvas split and the shared-mark contract keep their public paths
/// here: the tab, the layouts and the control plane name them as `pane::`.
pub use canvas_split::{
    CANVAS_DIVIDER_HANDLE_PX, DEFAULT_PANE_FRACTION, PaneSide, clamp_pane_fraction, split_time_pane,
};
pub(crate) use pointer_hit::PaneHitTest;
pub(crate) use pointer_hit::{ControlDrawingHit, ControlPointerHit};
use quantick_drawings::gesture::{self, SharedPointer};
pub use quantick_drawings::gesture::{PaneIndex, SharedEdit, SharedInteraction, SharedPick};
pub(crate) use quantick_drawings::gesture::{SharedMarksMut, SharedSource};
pub(crate) use tape_switch::tape_switch_rect;

// The pane's own vocabulary, one leaf per question: the pane itself in
// `chart_pane`, what surrounds its plot area in `chrome`, the pointer's grip
// on a drawing in `drag`, the placement plate in `hint`, the pane's
// decoration in `painting` and the axis claims in `price_axis`. The leaves
// are private, so the names below stay the only address for all of them.
mod chart_pane;
mod chrome;
mod drag;
mod hint;
mod painting;
mod price_axis;

pub use chart_pane::ChartPane;
use chart_pane::DrawPass;
pub use chrome::PaneChrome;
use drag::region_pause;
use hint::paint_placement_hint;
#[cfg(test)]
pub(crate) use painting::live_chip_rect as test_live_chip_rect;
use painting::{
    LANE_AXIS_FONT_PX, LANE_AXIS_GAP_PX, LANE_HANDLE_HALF_WIDTH_PX, LAST_PRICE_CHIP_TEXT,
    LAST_PRICE_DASH_PX, LAST_PRICE_GAP_PX, LAST_PRICE_LINE_ALPHA, SCROLL_ZOOM_PX, SEAM_DASH_PX,
    SEAM_GAP_PX, SEAM_LABEL_INSET_PX, SEAM_LABEL_PT, draw_dashed_vertical, draw_live_chip,
    lane_rungs, live_chip_rect,
};
pub use painting::{background_color, grid_color};
use price_axis::PriceAxisClaims;
pub(crate) use price_axis::PriceAxisLevel;
#[cfg(any(feature = "drawing-harness", test))]
pub use quantick_drawings::gesture::ParkedHand;
pub use quantick_drawings::gesture::{DRAWING_ANCHOR_RADIUS_PX, DrawingDrag};

// The compass is painted by several of the leaves and by the axes registry,
// which all reach it as `pane::PointerCompass`.
use crate::pointer_compass::PointerCompass;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "pane/tests/lane_transport_tests.rs"]
mod lane_transport_tests;
#[cfg(test)]
#[path = "pane/tests/renko_tests.rs"]
mod renko_tests;
