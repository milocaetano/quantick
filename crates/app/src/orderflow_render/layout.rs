//! The order-flow renderer's per-frame input, and the one place the pane
//! geometry ([`quantick_chart::flow_geometry`]) meets egui's boxes.
//!
//! Every layer painter in the sibling modules is handed a [`RenderContext`]
//! and asks its [`ProjectedLayout`] the same questions — `x`, `y`, `band`,
//! `pane`, `layer_clip` — so the map, the bubbles, the legend and the
//! on-demand pointer resolver cannot grow two pixel mappings.

use eframe::egui;
use quantick_chart::flow_geometry::FlowRect;
pub(crate) use quantick_chart::flow_geometry::{ProjectedLayout, lane_divider_x};
use quantick_orderflow::projection::{PastTape, PastTapeMemory};
use quantick_orderflow::{AggressionPrimitive, HeatmapProjection, PriceWindow};
use std::sync::Arc;

use super::OrderflowRenderStyle;
pub(super) use quantick_orderflow::projection::EventBand;

/// A geometry box as egui's: the painters' side of the crossing.
pub(crate) trait EguiRect {
    fn egui(self) -> egui::Rect;
}

impl EguiRect for FlowRect {
    fn egui(self) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(self.left, self.top),
            egui::pos2(self.right, self.bottom),
        )
    }
}

/// An egui box as the geometry's: the window's side of the crossing.
#[must_use]
pub(crate) fn flow_rect(rect: egui::Rect) -> FlowRect {
    FlowRect {
        left: rect.left(),
        top: rect.top(),
        right: rect.right(),
        bottom: rect.bottom(),
    }
}

/// Complete input shared by the independently callable rendering layers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RenderContext<'a> {
    pub(crate) projection: &'a HeatmapProjection,
    pub(crate) layout: ProjectedLayout<'a>,
    pub(crate) style: &'a OrderflowRenderStyle,
    pub(super) tape_time: Option<(quantick_orderflow::LiveEdge, i64)>,
    pub(super) tape_prices: Option<PriceWindow>,
    pub(super) tape_memory:
        Option<&'a std::cell::RefCell<quantick_orderflow::projection::TapeDotMemory>>,
    pub(super) tape_rebuilds: Option<(
        &'a std::cell::RefCell<super::PaneTapeRebuilds>,
        &'a Arc<HeatmapProjection>,
    )>,
    pub(super) past_tape: Option<(&'a std::cell::RefCell<PastTapeMemory>, &'a PastTape)>,
    pub(super) tape_overlay: Option<&'a Arc<quantick_orderflow::projection::TapeOverlay>>,
    /// The frame's verdict on its current book (`VisibleOrderflow::current_book_live`).
    pub(super) current_book_live: bool,
}

impl<'a> RenderContext<'a> {
    #[must_use]
    pub(crate) fn new(
        projection: &'a HeatmapProjection,
        layout: ProjectedLayout<'a>,
        style: &'a OrderflowRenderStyle,
    ) -> Self {
        Self {
            projection,
            layout: ProjectedLayout {
                tape_geometry: quantick_orderflow::projection::TapeHorizontalGeometry::from_style(
                    style,
                    layout.lane_width_px,
                    layout.chart_rect.height(),
                ),
                ..layout
            },
            style,
            tape_time: None,
            tape_prices: None,
            tape_memory: None,
            tape_rebuilds: None,
            past_tape: None,
            tape_overlay: None,
            current_book_live: true,
        }
    }

    /// Draw the tape held in the past from its frozen blocks instead.
    pub(crate) fn with_past_tape(
        mut self,
        past: Option<(&'a std::cell::RefCell<PastTapeMemory>, &'a PastTape)>,
    ) -> Self {
        self.past_tape = past;
        self
    }

    /// Preview the frame's current book only while the pane holds it current.
    pub(crate) fn with_current_book_live(mut self, live: bool) -> Self {
        self.current_book_live = live;
        self
    }

    /// Reproject factual tape prices against the axis being painted now,
    /// before visibility and overlap decisions use their screen positions.
    pub(crate) fn with_tape_price_range(mut self, range: (f64, f64)) -> Self {
        self.tape_prices = PriceWindow::from_f64_range(range);
        self
    }

    /// Reposition a cached tape against this frame's supplied market clock.
    pub(crate) fn with_tape_time(
        mut self,
        edge: quantick_orderflow::LiveEdge,
        dot_window_ms: i64,
    ) -> Self {
        self.tape_time = Some((edge, dot_window_ms));
        self
    }

    /// Keep closed tape membership in the pane that owns its source lifetime,
    /// with the accepted prints the frame carries beside its tape.
    pub(crate) fn with_tape_memory(
        mut self,
        memory: &'a std::cell::RefCell<quantick_orderflow::projection::TapeDotMemory>,
        overlay: Option<&'a Arc<quantick_orderflow::projection::TapeOverlay>>,
    ) -> Self {
        self.tape_memory = Some(memory);
        self.tape_overlay = overlay;
        self
    }

    /// Run the tape's reconciliations too large for a frame beside it,
    /// drawing the retained groups until each lands; `projection` is the
    /// frame's own, shared with them.
    pub(crate) fn with_tape_rebuilds(
        mut self,
        rebuilds: Option<(
            &'a std::cell::RefCell<super::PaneTapeRebuilds>,
            &'a Arc<HeatmapProjection>,
        )>,
    ) -> Self {
        self.tape_rebuilds = rebuilds;
        self
    }

    /// The aggressions this canvas draws as bubbles, in projection order:
    /// the display switches live in the painter, never in the projection
    /// ([`quantick_orderflow::projection::draws_bubble`]). Reads the raw style
    /// rather than the sanitized copy on purpose: `sanitized` never touches a
    /// display flag, so the filter costs no second clone per frame.
    pub(crate) fn bubbles(&self) -> impl Iterator<Item = &'a AggressionPrimitive> {
        let style = self.style;
        self.projection
            .aggressions
            .iter()
            .filter(move |mark| quantick_orderflow::projection::draws_bubble(style, mark))
    }
}
