//! The canvas split: which pane is which, and how the window is shared
//! between the flow chart and the context stack.
//!
//! `PaneSide` is the address every reader of the split uses; the two
//! constants and `split_pane_header` are the geometry the divider drags against.
//! Cut out of `pane.rs` as a pure move: the pane struct never reads these, the
//! tab and the layout code do.

use eframe::egui;
use quantick_chart::flow_band::{FlowRect, flow_pane_layout_scaled};

pub use quantick_chart_interaction::pane::PaneSide;

/// Half-width of the divider's grab area, which reaches a little into both
/// panes so the handle is catchable without widening the rule itself.
pub const CANVAS_DIVIDER_HANDLE_PX: f32 = 5.0;

/// Where the divider sits when the split is first shown.
///
/// Roughly a third to the context pane, the rest to the flow pane. An even
/// split says the two charts matter equally, and in quantick they do not: the
/// heatmap is what the product is for, and the timeframe chart beside it is
/// context. The opening canvas should say so before the trader touches
/// anything — a default is an argument about what matters.
pub const DEFAULT_PANE_FRACTION: f32 = 0.35;

/// A pane's area, split into the strip its header sits in and the chart below
/// it.
pub struct PaneHeaderAreas {
    pub header: egui::Rect,
    pub chart: egui::Rect,
}

/// The flow pane's area, carved by [`quantick_chart::flow_band`].
pub struct FlowPaneAreas {
    pub band: egui::Rect,
    pub side_row: egui::Rect,
    pub header: egui::Rect,
    pub chart: egui::Rect,
}

/// Carve the flow pane's band, side headers and framed chart out of its area.
#[must_use]
pub fn split_flow_pane(area: egui::Rect, scale: f32) -> FlowPaneAreas {
    let layout =
        flow_pane_layout_scaled(area.left(), area.top(), area.right(), area.bottom(), scale);
    let rect = |r: FlowRect| {
        egui::Rect::from_min_max(egui::pos2(r.left, r.top), egui::pos2(r.right, r.bottom))
    };
    FlowPaneAreas {
        band: rect(layout.band),
        side_row: rect(layout.side_row),
        header: area.with_max_y(layout.side_row.bottom),
        chart: rect(layout.chart),
    }
}

/// One visible pane split into its chart body and its layout-tab footer.
#[derive(Clone, Copy)]
pub(crate) struct PaneAreas {
    pub body: egui::Rect,
    pub layout_strip: egui::Rect,
}

/// Hold a stored split inside the canvas.
///
/// A sanity clamp, not a floor. The floor is
/// [`canvas_layout::MIN_PANE_WIDTH_PX`], and it is applied where the canvas
/// width is known — inside the splitter, on every frame, for every pane.
/// Holding a *second* floor here as a share of the canvas is what made
/// collapse-by-drag unreachable: the share (a quarter) always bound before the
/// width (120 px) could, so a drag restarted at 400 px of a 1600 px canvas
/// every frame and could never travel far enough in one to dismiss the column.
/// One floor, one owner, and it is the one that knows how wide the canvas is.
#[must_use]
pub fn clamp_pane_fraction(fraction: f32) -> f32 {
    if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        DEFAULT_PANE_FRACTION
    }
}

/// Carve a pane's header strip off the top of its area (§11); the rest is the
/// chart. The header is a strip rather than an overlay so it is never painted
/// across market data.
#[must_use]
pub fn split_pane_header(area: egui::Rect, height: f32) -> PaneHeaderAreas {
    let split_y = (area.top() + height).min(area.bottom());
    PaneHeaderAreas {
        header: egui::Rect::from_min_max(area.min, egui::pos2(area.right(), split_y)),
        chart: egui::Rect::from_min_max(egui::pos2(area.left(), split_y), area.max),
    }
}

/// Reserve a layout-tab strip inside the bottom of one visible pane.
///
/// The footer is part of the pane rather than the canvas around it: two panes
/// beside or above one another therefore keep two stable selectors, whatever
/// focus does. Clamping makes a temporarily tiny band spend its available
/// height on chrome instead of producing an inverted chart rectangle.
#[must_use]
pub(crate) fn split_pane_layout_strip(area: egui::Rect) -> PaneAreas {
    let split_y = (area.bottom() - crate::layout_strip::STRIP_HEIGHT).max(area.top());
    PaneAreas {
        body: egui::Rect::from_min_max(area.min, egui::pos2(area.right(), split_y)),
        layout_strip: egui::Rect::from_min_max(egui::pos2(area.left(), split_y), area.max),
    }
}
