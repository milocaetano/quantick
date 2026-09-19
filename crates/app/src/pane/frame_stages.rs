//! The writing stages of [`ChartPane::draw_chart`](super::ChartPane::draw_chart)
//! that need a field of the pane mutably while the frame's series stay
//! borrowed: the tape's layers ([`FlowFrame`]) and the candle pane's
//! ([`HistoryStage`]).
//!
//! Each is an owner type holding the per-frame context its passes share —
//! geometry, painter, viewport, the resolved dress — built once by the
//! orchestrator, and each method takes only the one field it writes. That is
//! what lets them run under the frame's borrows of `self.state`: a `&mut self`
//! method could not, and a free function handed every input separately would
//! take a dozen of them.

use std::sync::Arc;

use eframe::egui;
use quantick_engine::BarFootprint;
use quantick_orderflow::engine::VisibleOrderflow;
use quantick_orderflow::reserved_span_ms;

use crate::drawings::Drawings;
use crate::indicator_render::{self, PlotX};
use crate::indicators::IndicatorViews;
use crate::orderflow_view::{LiveLane, OrderflowView, VisibleBarTimeline};
use crate::style::CandleStyle;
use crate::viewport::Viewport;

use super::PaneChrome;
use super::draw_frame::DrawFrame;
use super::footprint::PaneFootprint;
use super::frame_layout::CandleDress;
use super::render_registry::{
    CandlePass, FlowPass, FootprintPass, IndicatorPanePass, LegendPass, OverlayPass,
    RenderRegistry, StripPass,
};

/// The tape's share of one frame: the geometry every flow pass reads and the
/// projection this frame got back from the book worker.
///
/// It holds no borrow of the tape itself — each pass is handed the
/// `OrderflowView` it paints — so the pane stays readable between them.
pub(super) struct FlowFrame<'a> {
    renderers: &'static RenderRegistry,
    painter: &'a egui::Painter,
    rect: egui::Rect,
    viewport: &'a Viewport,
    total: usize,
    background: egui::Color32,
    lane_width: f32,
    inverted: bool,
    projection: Option<Arc<VisibleOrderflow>>,
}

impl<'a> FlowFrame<'a> {
    pub(super) fn new(
        renderers: &'static RenderRegistry,
        frame: &DrawFrame<'a>,
        lane_width: f32,
        viewport: &'a Viewport,
        inverted: bool,
    ) -> Self {
        Self {
            renderers,
            painter: frame.painter,
            rect: frame.chart_rect,
            viewport,
            total: frame.total,
            background: frame.canvas_background,
            lane_width,
            inverted,
            projection: None,
        }
    }

    /// Ask the book worker to project the visible bars and keep the newest
    /// frame it has built.
    ///
    /// The projection builds a lane exactly when the layout draws one. Tied
    /// to the lane's width rather than restated, because the two decide the
    /// same thing: with them apart, the newest prints would be clustered and
    /// sized as lane prints and then squeezed into a single candle slot.
    /// Only the engine's own bars carry tape, so the timeline starts at the
    /// first *state* bar's global slot: when the window straddles the venue
    /// seam (a time-cutting flow pane, audit S1), that is the seam itself,
    /// not the window's first slot.
    ///
    /// `demand` says whether anything but the depth map and the bubbles
    /// consumes the projection: the live strip draws the same clusters, and
    /// the lane's marks need the frame's live edge. Stated every frame, so
    /// with the bubbles hidden the pipeline stays alive for the strip, and
    /// with every other flow layer off the lane is still marked instead of
    /// being a reserved but empty band whose menu entry claims it is on.
    pub(super) fn project(
        &mut self,
        orderflow: Option<&mut OrderflowView>,
        demand: bool,
        timeline_revision: u64,
        frame: &DrawFrame<'_>,
    ) {
        let timeline = VisibleBarTimeline::new(
            timeline_revision,
            frame.closed_start.max(frame.prefix.len()),
            frame.visible_state,
            frame.partial_visible,
        );
        self.projection = orderflow.and_then(|orderflow| {
            orderflow.set_projection_demand(demand);
            // The tape's automatic window comes from the newest bars of the
            // series, never from the slice on screen: panning the candles is
            // not a statement about how much market time the tape shows.
            orderflow.project_visible(
                timeline,
                self.lane_width > 0.0,
                frame.end == frame.total,
                Some(reserved_span_ms(frame.closed)),
                frame.scale.range(),
            )
        });
    }

    /// Whether this frame has a projection to paint.
    pub(super) fn projected(&self) -> bool {
        self.projection.is_some()
    }

    /// The oldest slot the depth map covers, asked only when `wanted` says a
    /// range profile is there to cut its paint at it.
    pub(super) fn first_heat_slot(&self, wanted: impl FnOnce() -> bool) -> Option<usize> {
        self.projection
            .as_ref()
            .filter(|_| wanted())
            .and_then(|frame| frame.first_heat_slot())
    }

    fn pass<'s>(
        &'s self,
        owner: &'s OrderflowView,
        projection: &'s VisibleOrderflow,
    ) -> FlowPass<'s> {
        FlowPass {
            owner,
            painter: self.painter,
            rect: self.rect,
            viewport: self.viewport,
            total: self.total,
            projection,
            background: self.background,
            lane_width: self.lane_width,
            inverted: self.inverted,
        }
    }

    /// Resting liquidity: the bottom visual layer.
    pub(super) fn heatmap(&self, owner: Option<&OrderflowView>) {
        if let Some(owner) = owner
            && let Some(projection) = self.projection.as_deref()
        {
            self.renderers.heatmap(&mut self.pass(owner, projection));
        }
    }

    /// The aggression bubbles, over the candles and the indicator panes.
    pub(super) fn aggressions(&self, owner: Option<&OrderflowView>) {
        if let Some(owner) = owner
            && let Some(projection) = self.projection.as_deref()
        {
            self.renderers
                .aggressions(&mut self.pass(owner, projection));
        }
    }

    /// The canvas's key, in a pass of its own so the bubble switch cannot
    /// take it down with them. Returns where it landed, for the chrome.
    pub(super) fn legend(
        &self,
        owner: Option<&OrderflowView>,
        legend_inset: f32,
    ) -> Option<egui::Rect> {
        let mut bounds = None;
        if let Some(owner) = owner
            && let Some(projection) = self.projection.as_deref()
        {
            self.renderers.legend(&mut LegendPass {
                owner,
                painter: self.painter,
                rect: self.rect,
                viewport: self.viewport,
                total: self.total,
                projection,
                background: self.background,
                lane_width: self.lane_width,
                legend_inset,
                bounds: &mut bounds,
            });
        }
        bounds
    }

    /// The live strip: the book right now plus the forming bar's aggression
    /// histogram, beside the axis the price labels live on. Its own rect, so
    /// chart layers never bleed into it. The histogram follows `partial`
    /// (not its visible filter): the strip reports the bar forming now even
    /// while the user pans through history.
    pub(super) fn strip(&self, owner: Option<&mut OrderflowView>, frame: &DrawFrame<'_>) {
        if let Some(owner) = owner
            && let Some(strip) = frame.areas.live_strip
        {
            self.renderers.strip(&mut StripPass {
                owner,
                painter: self.painter,
                rect: strip,
                scale: &frame.scale,
                background: self.background,
                partial_time: frame.partial.map(|bar| bar.open_time),
            });
        }
    }
}

/// Bring the drawings' cached folds up to date before anything paints over
/// the depth map: the range profiles (`crate::frvp`) and the anchored VWAPs
/// (`crate::avwap`), both from the same series.
///
/// Key-guarded inside: the common frame compares one small key per object
/// and folds nothing. It runs after the heatmap projection on purpose — the
/// map's left boundary is where each profile's paint cuts from fill to
/// silhouette, and the O(cells) scan behind it is paid only while a profile
/// object exists.
pub(super) fn refresh_drawing_folds(
    drawings: &mut Drawings,
    inputs: &crate::frvp::RefreshInputs<'_>,
    ctx: &egui::Context,
) {
    if crate::frvp::refresh(drawings, inputs) {
        // A range too long for one pass: paint what is folded and come
        // straight back for the next slice. Without this the fill would
        // stall wherever the tape happened to stop waking the window.
        ctx.request_repaint();
    }
    // The anchored-VWAP objects' cached rows, same pass discipline: a key
    // comparison per object on the common frame, a replay only when the
    // tape or the config moved.
    crate::avwap::refresh(
        drawings,
        &crate::avwap::RefreshInputs {
            state: inputs.state,
            prefix: inputs.prefix,
        },
    );
}

/// The live lane's window of tape time as the indicator panes draw it:
/// where the lane starts on screen and the tape instants at its two edges.
#[derive(Clone, Copy)]
pub(super) struct PaneLane {
    divider: f32,
    start_ms: i64,
    end_ms: i64,
}

impl PaneLane {
    /// Resolved once per frame rather than per pane — it is the same for
    /// every pane — and from the tape's own numbers, so a pane's curve lands
    /// under the prints it was computed from.
    pub(super) fn resolve(
        divider: Option<f32>,
        live_lane: Option<LiveLane>,
        orderflow: Option<&OrderflowView>,
        frame: &DrawFrame<'_>,
    ) -> Option<Self> {
        let (divider, lane) = divider.zip(live_lane)?;
        let window = orderflow?.live_lane_window_ms(frame.visible_state).max(1);
        Some(Self {
            divider,
            start_ms: lane.end_ms.saturating_sub(window),
            end_ms: lane.end_ms,
        })
    }
}

/// The candle pane's share of one frame: the clipped painter, the viewport
/// and the candles' dress, for the passes that paint the bars themselves and
/// what rides on them.
pub(super) struct HistoryStage<'a, 'f> {
    pub(super) renderers: &'static RenderRegistry,
    pub(super) frame: &'a DrawFrame<'f>,
    /// Clipped to the candles' own pane: panning far enough into history
    /// sends the newest bars off the right of it, and they scroll out of
    /// sight behind the tape instead of being drawn over it.
    pub(super) clip: egui::Painter,
    pub(super) viewport: &'a Viewport,
    pub(super) dress: CandleDress,
}

impl<'a, 'f> HistoryStage<'a, 'f> {
    /// The candle pass, for both of its stages (the clear under the depth
    /// map and the candles themselves).
    pub(super) fn candle_pass<'s>(
        &'s self,
        indicators: &'s IndicatorViews,
        clear_depth: bool,
        base: &'s CandleStyle,
    ) -> CandlePass<'s, 'f> {
        CandlePass {
            frame: self.frame,
            painter: &self.clip,
            viewport: self.viewport,
            indicators,
            clear_depth,
            half: self.dress.half,
            candle_lane: self.dress.candle_lane,
            content_half: self.dress.content_half,
            style: self.dress.style(base),
        }
    }

    fn plot_x(&self) -> PlotX<'a> {
        PlotX {
            viewport: self.viewport,
            right: self.frame.right,
            total: self.frame.total,
        }
    }

    /// The footprint rides directly on the candles, before everything drawn
    /// over them: it is a representation of the bars themselves, not an
    /// annotation. Prefix (venue) candles carry no tape and draw no ladder —
    /// the layer starts where trade-built bars start.
    pub(super) fn footprint(
        &self,
        footprint: &mut PaneFootprint,
        footprints: &[BarFootprint],
        chrome: &PaneChrome<'_>,
        depth_visible: bool,
    ) {
        let frame = self.frame;
        let (viewport, right, total) = (self.viewport, frame.right, frame.total);
        // The forming bar's ladder is the ~10 Hz snapshot taken with the
        // accumulation switch at the top of the frame, shared with the
        // range-profile drawings.
        let layer = crate::footprint_render::LayerFrame {
            painter: &self.clip,
            chart_rect: frame.history_rect,
            scale: &frame.scale,
            footprints,
            first_state_slot: frame.prefix.len(),
            visible: (frame.start, frame.end),
            // Field access, not `live_ladder`: the draw below needs the
            // level of detail mutably while this borrow is alive.
            partial: footprint
                .live
                .as_ref()
                .map(|(_, _, ladder)| ladder)
                .filter(|_| frame.partial_visible.is_some()),
            partial_slot: frame.closed_total,
            x_center: &|slot| viewport.x_center(slot, right, total),
            // The *content* half-width, which is not always the candle's.
            // A style that draws inside the candle is bounded by it; one
            // that draws in a box beside it is bounded only by the slot,
            // and charging it the candle gap as well spends a quarter of
            // the row on air twice over.
            half: self.dress.content_half,
            candle_width: frame.cw,
            side_inferred: chrome.side_inferred,
            depth_visible,
            pixels_per_point: frame.painter.ctx().pixels_per_point(),
            // Field access, not `ChartPane::footprint_config`: the method
            // borrows the whole pane and the draw below needs the level of
            // detail mutably. Same resolution rule.
            config: footprint.config.as_ref().unwrap_or(chrome.footprint),
        };
        self.renderers.footprint(&mut FootprintPass {
            frame: &layer,
            lod: &mut footprint.lod,
        });
    }

    /// Overlay indicator plots ride the candles' own clip, scale and
    /// x-mapping — after candles, before aggression bubbles (the same
    /// paint-order slot draw objects take).
    pub(super) fn overlay(&self, indicators: &IndicatorViews) {
        self.renderers.overlay(&mut OverlayPass {
            frame: self.frame,
            painter: &self.clip,
            plot_x: &self.plot_x(),
            indicators,
        });
    }

    /// Pane indicators stack in the band carved off the chart, sharing the
    /// candles' x-mapping so bars and their flow read as one chart. Each
    /// pane records the range it auto-fitted to, so the gesture over its
    /// axis zooms the very range this frame drew.
    pub(super) fn indicator_panes(
        &self,
        indicators: &mut IndicatorViews,
        lane: Option<PaneLane>,
        guide_x: Option<f32>,
        grid: egui::Color32,
    ) {
        let frame = self.frame;
        let plot_x = self.plot_x();
        let lane_steps: Vec<(i64, usize)> = lane.map_or_else(Vec::new, |lane| {
            let first = frame.prefix.len();
            // Walked back from the newest close and reversed in place: the
            // window holds a handful of bars, and building it front to back
            // would mean scanning every closed bar the chart has ever seen.
            let mut steps: Vec<(i64, usize)> = frame
                .closed
                .iter()
                .enumerate()
                .rev()
                .take_while(|(_, bar)| bar.close_time >= lane.start_ms)
                .map(|(index, bar)| (bar.close_time, first + index))
                .collect();
            steps.reverse();
            steps
        });
        for ((view, pane), gutter) in indicators
            .visible_panes_mut()
            .zip(&frame.areas.indicator_panes)
            .zip(&frame.areas.pane_gutters)
        {
            let pane_frame = indicator_render::PaneFrame {
                rect: egui::Rect::from_min_max(
                    egui::pos2(frame.history_rect.left(), pane.rect.top()),
                    egui::pos2(frame.history_rect.right(), pane.rect.bottom()),
                ),
                lane: lane.map(|lane| indicator_render::LaneFrame {
                    rect: egui::Rect::from_min_max(
                        egui::pos2(lane.divider, pane.rect.top()),
                        egui::pos2(frame.chart_rect.right(), pane.rect.bottom()),
                    ),
                    start_ms: lane.start_ms,
                    end_ms: lane.end_ms,
                    steps: &lane_steps,
                }),
                gutter: *gutter,
                background: frame.canvas_background,
                grid,
                collapsed: pane.collapsed,
            };
            self.renderers.indicator_pane(&mut IndicatorPanePass {
                painter: frame.painter,
                frame: &pane_frame,
                view,
                plot_x: &plot_x,
                start: frame.start,
                end: frame.end,
                partial_slot: frame.partial_visible.map(|_| frame.closed_total),
            });
            if view.mouse_vertical_line
                && !pane.collapsed
                && let Some(x) = guide_x
            {
                crate::indicator_guide::paint(frame.painter, pane_frame.rect, x);
            }
        }
    }
}
