//! The paint frame: [`ChartPane::draw_chart`], the order the layers go down
//! in, and the stages it runs them through.
//!
//! [`ChartPane::begin_frame`] resets and snapshots; [`ChartPane::lay_out`] splits
//! and clamps. [`FrameLayout::resolve`] lends the series to each [`DrawFrame`] painter.
//!
//! Under that borrow a step that writes a field cannot be a `&mut self`
//! method, so each writing stage is an owner type in `frame_stages.rs` —
//! [`FlowFrame`] for the tape's layers, [`HistoryStage`] for the candle
//! pane's — handed only the field it writes; a stage whose only write is a
//! published value (the legend's bounds, the paper HUD's anchor, the lane's
//! reference) is a `&self` method here returning it, and the orchestrator
//! stores it. Every step that only reads is a `&self` painter here.
//!
//! The painters below the orchestrator read the frame and write nothing
//! back: the axis claims, the trade marks, the axis marks and dividers, and
//! the canvas chrome. Each takes the [`DrawFrame`] by reference and the
//! values it needs beside it, so the paint order and the per-frame work are
//! the orchestrator's. A painter that has to write a field stays in the
//! orchestrator, because the slices of the series are borrowed across the
//! frame and a `&mut self` method cannot run under them.

use crate::toolrail::ToolRailExt as _;
use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;

#[cfg(test)]
#[path = "draw_chart/tests/lane_boundary.rs"]
mod lane_boundary_tests;

use crate::bands::{Band, Bands};
use crate::chart::PriceScale;
use crate::orderflow_view::{LiveLane, OrderflowView};
use crate::plot_area::split_time_strip;
use crate::pointer_compass;
use crate::theme;
use crate::toolrail::Tool;
use quantick_chart::day_turn::DayTurn;
use quantick_chart::flow_band::trail_strip;
use quantick_chart::price_axis_fit::{beside_candles_right, tape_only_candles_px};
use quantick_layers::ChartLayer;
use quantick_orderflow::projection::TapeHorizontalGeometry;
use quantick_orderflow::{LaneView, reserved_span_ms};

use super::draw_frame::{AxisChips, DrawFrame};
use super::frame_layout::{
    CandleDress, FrameLayout, FrameStart, Series, live_ladder, nothing_in_view,
    snapshot_live_ladder,
};
use super::frame_stages::{FlowFrame, HistoryStage, PaneLane, refresh_drawing_folds};
use super::render_registry::*;
use super::{
    ChartPane, DrawPass, PaneChrome, PointerCompass, PriceAxisClaims, PriceAxisLevel,
    background_color, draw_live_chip, grid_color, lane_rungs,
};

impl ChartPane {
    pub fn draw_chart(
        &mut self,
        painter: &egui::Painter,
        area: egui::Rect,
        chrome: &mut PaneChrome<'_>,
    ) {
        let flow = |_| theme::scope(painter.ctx(), theme::surface(painter.ctx(), self.surface()));
        let _surface = self.orderflow.as_ref().map(flow);
        let renderers = self.layer_renderers;
        // A return to live asks this frame, laid out or not, and never a later one.
        let centre_live = self.model.take_live_centre();
        let mut start = self.begin_frame(painter, area, chrome);
        let flow_execution = self
            .orderflow
            .as_ref()
            .is_some_and(OrderflowView::flow_execution_active)
            && self.state.tick_membership().is_some();
        if flow_execution {
            start.candle_aggression = false;
        }
        let surface = theme::get(painter.ctx());
        self.frame.axis_metrics = surface.resolved.axis_metrics();
        self.indicators.paint_on(&surface.resolved);
        let layout = self.lay_out(area, chrome);
        let axes = theme::axes(painter.ctx());
        let light_flow = self.orderflow.is_some() && !axes.resolved.dark;
        let fill = |rect, color| painter.rect_filled(rect, egui::Rounding::ZERO, color);
        let background = if light_flow {
            *axes.get(quantick_appearance::Token::ChartCanvas)
        } else {
            start.canvas_background
        };
        fill(area, background);
        if let (Some(gutter), Some(strip)) = (self.frame.price_gutter, self.frame.time_strip) {
            if light_flow {
                let plot = area.with_max_x(gutter.left()).with_max_y(strip.top());
                fill(plot, start.canvas_background);
            }
            if self.orderflow.is_some() && layout.is_some() && !light_flow {
                fill(area.with_min_y(strip.top()), axes.flow_surface());
            }
        }
        let Some(layout) = layout else { return };
        let tape_range = self
            .orderflow
            .as_ref()
            .and_then(OrderflowView::tape_price_range);
        // Field borrows, not `self` borrows: the tape below needs `&mut
        // self.orderflow` while these are alive.
        let series = Series {
            prefix: self.history_prefix.as_slice(),
            closed: self.state.bars(),
            partial: self.state.partial(),
        };
        let Some((mut frame, auto_range, flip_span, newest)) = layout.resolve(
            painter,
            series,
            tape_range,
            self.frame.auto_range,
            &self.price_view,
            start.canvas_background,
        ) else {
            return;
        };
        let chart_rect = frame.chart_rect;
        // A return to live centres the last price on the live view's fit.
        if centre_live && self.price_view.centre_on_close(newest, auto_range) {
            frame.scale = self
                .price_view
                .scale(auto_range, chart_rect.top(), chart_rect.bottom());
        }

        // Resting liquidity is the bottom visual layer. Projection is pure with
        // respect to candles and uses the same bar-warped viewport coordinates.
        let mut flow = FlowFrame::new(
            renderers,
            &frame,
            layout.lane_width_px(),
            &frame.view,
            self.price_view.is_inverted(),
        );
        let demand = self.layer_read().projection_demand();
        flow.project(self.orderflow.as_mut(), demand, &self.state, &frame);
        flow.heatmap(self.orderflow.as_ref());
        let depth_visible = self
            .orderflow
            .as_ref()
            .is_some_and(OrderflowView::depth_visible);

        let heat_first_slot = flow.first_heat_slot(|| depth_visible && self.wants_range_profile());
        // Read before the drawings are borrowed mutably below.
        let partial_bucket_slot = self.partial_bucket_slot();
        refresh_drawing_folds(
            &mut self.drawings,
            &crate::frvp::RefreshInputs {
                state: &self.state,
                budget: crate::frvp::fold_budget(),
                prefix: frame.prefix,
                partial_ladder: live_ladder(&self.presentation.footprint),
                partial_version: self.presentation.footprint.live_version,
                blocked: start.footprint_blocked,
                side_inferred: chrome.side_inferred,
                heat_first_slot,
                draft_hover_bar: self.gestures.hover.map(|point| point.bar),
                partial_bucket_slot,
            },
            painter.ctx(),
        );

        let AxisChips {
            compass,
            price: price_claims,
            time: time_chip,
        } = self.axis_claims(&frame, chrome);
        // Share the levels between grid claims and gutter marks; retain the allocation.
        let mut levels = std::mem::take(&mut self.price_axis_levels);
        self.fill_price_axis_levels(&frame, chrome, &mut levels);

        // Grid + price labels first, behind the candles. Labels anchor on the
        // gutter's edge, past the live strip when one is shown.
        let axis_x = layout.areas.price_gutter.left();
        // The vertical grid stands where the time labels do; tape only has none.
        let time_marks = self.time_marks(&frame, chrome);
        let time_grid_xs = match chrome.style.canvas.time_grid_rgba() {
            Some(_) if !layout.tape_only => time_marks.1.iter().map(|mark| mark.x).collect(),
            _ => Vec::new(),
        };
        renderers.grid(&mut GridPass {
            painter,
            chart_rect,
            axis_x,
            scale: &frame.scale,
            claims: &PriceAxisClaims {
                marks: price_claims,
                levels: &levels,
            },
            style: chrome.style,
            snap_horizontal: !axes.resolved.dark
                && self.orderflow.is_none()
                && chrome.style.ownership.grid == quantick_appearance::Inheritance::Theme,
            time_grid_xs: &time_grid_xs,
        });
        // The day rule over the grid, under the candles; none in tape-only.
        let day_turns = self.day_turns(&frame, chrome);
        if let Some(turns) = day_turns.as_deref().filter(|_| !layout.tape_only) {
            renderers.day_rule(&mut crate::pane::render_registry::DayRulePass {
                painter: &painter.with_clip_rect(frame.history_rect),
                history: frame.history_rect,
                total: frame.total,
                candle_width: frame.cw,
                viewport: &frame.view,
                turns,
            });
        }

        let history = HistoryStage {
            renderers,
            frame: &frame,
            clip: painter.with_clip_rect(layout.history_rect),
            viewport: &frame.view,
            dress: CandleDress::resolve(
                &self.presentation.footprint,
                chrome,
                start.footprint_paints,
                frame.cw,
                crate::theme::get(frame.painter.ctx())
                    .footprint_text
                    .width_scale(),
            ),
        };
        let mut carved = std::mem::take(&mut self.frame.bands);
        let clear_depth = flow.projected() && depth_visible;
        if !layout.tape_only {
            // The footprint dresses only the bars it has a ladder for.
            let ladders = self.presentation.footprint.ladder_slots(
                frame.prefix.len(),
                self.state.bar_footprints(),
                frame.partial_visible.map(|_| frame.closed_total),
            );
            let base = &chrome.style.candles;
            let mut candle_pass = history.candle_pass(&self.indicators, clear_depth, base, ladders);
            candle_pass.pixel = quantick_chart::candle_pixels::pixel_bodies(
                self.orderflow.is_some(),
                history.dress.faded.is_some() || history.dress.candle_lane > 0.0,
            );
            renderers.candle_clear(&mut candle_pass);
            // Only price-band background drawings may precede candle/indicator scales.
            self.carve_bands(&layout, &mut carved);
            let price_band = carved.get(..1).unwrap_or_default();
            self.paint_drawing_bands(&frame, price_band, DrawPass::UnderCandles);
            // Candle fills, outlines and wicks share one pass beneath bubbles.
            renderers.candles(&mut candle_pass);
            if flow_execution {
                flow.regional_context(
                    self.orderflow.as_ref(),
                    start.footprint_paints.then_some(frame.canvas_background),
                );
            }
            if start.footprint_paints || start.candle_aggression {
                history.footprint(
                    &mut self.presentation.footprint,
                    &self.state,
                    chrome,
                    depth_visible,
                    &start,
                );
            }
            if flow_execution {
                flow.regional_aggressions();
                flow.regional_perimeters();
            }
            history.overlay(&self.indicators);
        }
        let lane = self.pane_lane(&layout, &frame);
        let grid = grid_color(chrome.style);
        history.indicator_panes(&mut self.indicators, lane, layout.indicator_guide_x, grid);
        flow.aggressions(self.orderflow.as_ref());
        self.frame.flow_legend = flow.legend(self.orderflow.as_ref(), self.legend_inset(chrome));
        flow.strip(
            self.orderflow.as_mut(),
            &frame,
            self.state.partial_footprint(),
        );

        if layout.tape_only {
            carved.clear();
        } else {
            self.paint_drawings_over(&frame, &layout, &mut carved, chrome);
            self.paint_band_hint(painter);
            self.paint_trade_marks(&frame, chrome);
        }
        self.frame.bands = carved;
        super::lane_rail::paint_lane_rail(self, painter, chart_rect);
        self.paper_hud_anchor = self.paint_paper(&frame, axis_x, chrome);
        self.paint_axis_marks(&frame, axis_x, &levels, time_chip, chrome, &time_marks);
        let clock = layout
            .native_tape
            .then(|| layout.live_lane.map(|lane| (lane.end_ms, chrome.tz)))
            .flatten();
        if let Some(reference_ms) = self.paint_lane_time_axis(&frame, clock) {
            self.frame.lane_reference_ms = Some(reference_ms);
        }
        let nothing_in_view = nothing_in_view(&frame);
        self.paint_canvas_chrome(&frame, axis_x, nothing_in_view, compass.as_ref(), chrome);

        self.price_axis_levels = levels;

        // Cache the auto range + height for next frame's input handler, which
        // runs before the draw and needs them for pixel↔price conversion.
        self.frame.auto_range = Some(auto_range);
        self.frame.flip_span = Some(flip_span);
        self.frame.chart_height = chart_rect.height();
        self.frame.chart_top = chart_rect.top();
    }

    /// Reset the frame's published values and snapshot the footprint ladders.
    fn begin_frame(
        &mut self,
        painter: &egui::Painter,
        area: egui::Rect,
        chrome: &PaneChrome<'_>,
    ) -> FrameStart {
        self.paper_hud_anchor = None;
        self.frame.flow_legend = None;
        // Published before anything can return early, so an empty pane still
        // says where it is.
        self.frame.area = Some(area);
        let canvas_background = background_color(chrome.style);

        // The ladders accumulate only while something consumes them: the
        // footprint layer, or a fixed-range-profile drawing (placed or being
        // placed) folding those same ladders over its bar span. Off, ingestion
        // pays nothing and holds nothing; the first frame after a switch-on
        // refolds the retained trades (declared cost, once). Then adopt the
        // book engine's capture bucket as the row grid (the instrument's
        // price_step where the feed declares one). Both before the frame
        // borrows the bar slices; both no-ops every frame but the one where
        // something changed.
        let footprint_blocked = self
            .layer_blocked(ChartLayer::Footprint, chrome.capabilities)
            .is_some();
        let candle_aggression = quantick_layers::LayerState::candle_summary(
            self.layer_facts(Some(chrome.capabilities)),
            self.layer_switched_on(ChartLayer::Bubbles, chrome.style),
            self.layers.requested(ChartLayer::CandleAggression),
        );
        let footprint_on = ((self.presentation.footprint.visible || self.wants_range_profile())
            && !footprint_blocked)
            || candle_aggression;
        self.state.set_footprint_enabled(footprint_on);
        // Accumulating is not painting, and the candles answer to the second.
        // A range profile turns the ladders *on* without ever asking for the
        // layer, so the switch above cannot be what dresses a candle: doing
        // that put every bar into the footprint's sidebar lane, or faded its
        // body down to an outline, for a layer that then drew nothing.
        // Computed here beside the switch it is so easily confused with, and
        // before the frame borrows fields out of `self`.
        let footprint_paints =
            self.layer_visible(ChartLayer::Footprint, chrome.style) && !footprint_blocked;
        if footprint_on
            && let Some(base) = self
                .orderflow
                .as_mut()
                .map(OrderflowView::capture_grouping_now)
                .or_else(|| {
                    candle_aggression
                        .then(|| self.state.tape_price_step())
                        .flatten()
                })
        {
            self.state.set_footprint_group(base);
        }
        if footprint_on {
            let now = painter.ctx().input(|i| i.time);
            let closed_total = self.history_prefix.len() + self.state.bars().len();
            snapshot_live_ladder(
                &mut self.presentation.footprint,
                self.state.partial_footprint(),
                closed_total,
                now,
            );
        }
        FrameStart {
            canvas_background,
            footprint_blocked,
            footprint_paints,
            candle_aggression,
        }
    }

    /// The frame's geometry: the plot split, the live lane, the history
    /// pane beside it and the clamped viewport's visible slots. `None` for an
    /// empty series: the connection overlay speaks for an empty pane.
    fn lay_out(&mut self, area: egui::Rect, chrome: &PaneChrome<'_>) -> Option<FrameLayout> {
        self.sync_price_axis_mode();
        let closed_total = self.history_prefix.len() + self.state.bars().len();
        let total = closed_total + usize::from(self.state.partial().is_some());
        let mut areas = self.plot_areas(area, chrome.capabilities);
        // Indicator panes claimed the bottom band inside `plot_split`, so the
        // rect the candles scale to is the same one the input handler uses.
        let chart_rect = areas.chart;
        self.frame.price_gutter = Some(areas.price_gutter);
        let (history_strip, lane_strip) =
            split_time_strip(areas.time_strip, self.frame.lane_divider_x);
        self.frame.time_strip = Some(history_strip);
        self.frame.lane_time_strip = lane_strip;
        let indicator_guide_x = self
            .hover_pos
            .filter(|position| chart_rect.contains(*position))
            .map(|position| position.x);
        // Tape only: the tape is the whole canvas and the candles get none of
        // it, even before the tape has a live edge to run to. The native tape
        // beside the candles keeps its share and fits the shared price axis.
        let (tape_only, native_tape) = self.tape_modes();
        let live_lane = self.lay_out_lane(chart_rect);
        // Beside the tape the strip is the trail's: every candle edge stops at it.
        let width = self.live_strip_width(chrome.capabilities);
        let divider = self.frame.lane_divider_x;
        let trail = trail_strip(self.plot_lane_view(), chart_rect.left(), divider, width);
        if let Some((left, right)) = trail {
            let rows = chart_rect.top()..=areas.time_strip.top();
            areas.live_strip = Some(egui::Rect::from_x_y_ranges(left..=right, rows));
            let candles_right = beside_candles_right(chart_rect.left(), right, width);
            self.frame.candles_right = Some(candles_right);
        }
        if total == 0 {
            return None;
        }
        let history_right = self.frame.candles_right.unwrap_or(chart_rect.right());
        let history_rect = egui::Rect::from_min_max(
            chart_rect.min,
            egui::pos2(history_right, chart_rect.bottom()),
        );

        // The projection margin is enforced here, against the rect the candles
        // are actually drawn in, rather than in the input handler: panning
        // leaves the future end open and zooming knows nothing about the
        // window, and the window itself moves without any gesture at all (the
        // app resizing, the lane divider dragged, a pane collapsed). Painting
        // is the one place that sees all of it, so it is the one place the
        // rule holds — pushed fully left, the newest bar stops at the left
        // edge and the rest of the window is empty canvas to project into.
        //
        // Hidden by tape only, the candles keep the view the trader left for
        // their return, unclamped, and the frame reads the live edge instead: the
        // price axis fits the tape, not a stale window.
        let width = history_rect.width();
        if !tape_only {
            self.model.viewport.clamp_to_window(width, total);
        }
        let view = self.shown_viewport();
        let (start, end) = view.visible_range(width, total);
        let strip = self.beside_strip_width(chrome.capabilities);
        let fit_width = match tape_only {
            true => tape_only_candles_px(chart_rect.width(), strip),
            false => width,
        };
        Some(FrameLayout {
            fit_slots: view.visible_range(fit_width, total),
            areas,
            chart_rect,
            history_rect,
            live_lane,
            total,
            closed_total,
            start,
            end,
            view,
            cw: view.candle_width(),
            indicator_guide_x,
            tape_only,
            native_tape,
            // The lane setting's band, on screen yet or not.
            tape_padding_px: self.orderflow.as_ref().map_or(0.0, |view| {
                let (width, height) = (chart_rect.width(), chart_rect.height());
                TapeHorizontalGeometry::native_price_inset_px(view.cached_config(), width, height)
            }),
        })
    }

    /// The live lane: a pane of its own, pinned to the right edge of the
    /// chart, showing a fixed window of market time that always ends at now.
    /// Fixed width, fixed pixels-per-ms: a print enters at the right edge and
    /// slides left until it leaves into the slot of its own bar.
    ///
    /// It belongs to the tape rather than to the forming bar, which is what
    /// keeps a bar close from emptying it — the reset that made the book look
    /// like it was restarting every few seconds. And it is a pane rather than
    /// a reservation inside the viewport, which is what keeps every chart
    /// movement out of it: panning, zooming and dragging move the candles
    /// beside the tape and never the tape itself, so the most recent prints
    /// are on screen whatever the rest of the chart is doing.
    fn lay_out_lane(&mut self, chart_rect: egui::Rect) -> Option<LiveLane> {
        // Band and live edge in one look at the published book: the panes need
        // the instant the band's right edge stands for, and reading it again
        // further down would put a second worker-mutex wait on the render
        // thread for a number already in hand.
        let live_lane = self
            .orderflow
            .as_mut()
            .and_then(|orderflow| orderflow.live_lane(chart_rect.width()));
        // Tape only included: the band is the lane's own.
        let lane_px = self
            .orderflow
            .as_ref()
            .map_or(0.0, |view| view.lane_width_px(chart_rect.width()));
        // Everything left of the divider is the candles' pane. They pan and zoom
        // inside it exactly as they did when it was the whole chart. A tape switched
        // off has no divider, only a rail no candle may stand under.
        let clear_px = self.orderflow.as_ref().map_or(0.0, |view| {
            let lane = &view.cached_config().live_lane;
            lane.candles_clear_px(chart_rect.width())
        });
        self.frame.lane_divider_x = crate::orderflow_render::lane_divider_x(chart_rect, lane_px);
        self.frame.candles_right = crate::orderflow_render::lane_divider_x(chart_rect, clear_px);
        self.frame.chart_rect = Some(chart_rect);
        let rungs = lane_rungs(lane_px);
        if self.lane.set_rungs(rungs) {
            let command = self
                .lane
                .command(self.state.partial().cloned(), self.state.trades());
            self.indicator_worker.send(command);
        }
        live_lane
    }

    /// The live lane's window as the indicator panes draw it, `None`
    /// without a lane or a tape.
    fn pane_lane(&self, layout: &FrameLayout, frame: &DrawFrame<'_>) -> Option<PaneLane> {
        let divider = self.frame.lane_divider_x;
        PaneLane::resolve(divider, layout.live_lane, self.orderflow.as_ref(), frame)
    }

    /// Drawings sit above market layers and remain anchored to chart space,
    /// not the screen, while the viewport moves beneath them, and the quick
    /// range rides with them.
    ///
    /// Re-carved, not reused: the pass under the candles ran before the
    /// indicator panes drew, and every band's scale is written *by* that
    /// draw — which is the invariant this whole feature rests on. Into the
    /// pane's own buffer: same geometry as the input pass computed, no
    /// container allocated, and what the tab's shared projection reads
    /// afterwards.
    fn paint_drawings_over(
        &self,
        frame: &DrawFrame<'_>,
        layout: &FrameLayout,
        carved: &mut Bands,
        chrome: &mut PaneChrome<'_>,
    ) {
        self.carve_bands(layout, carved);
        self.paint_drawing_bands(frame, carved, DrawPass::OverCandles);
        self.quick_range_view(chrome.tab, chrome.side).draw(
            frame.painter,
            carved,
            frame.right,
            frame.total,
            chrome,
        );
    }

    /// Carve the drawing bands — the price band and one per indicator pane —
    /// into `out`, against this frame's scales.
    fn carve_bands(&self, layout: &FrameLayout, out: &mut Bands) {
        crate::bands::BandGeometry {
            auto_range: self.frame.auto_range,
            price_view: &self.price_view,
            candles_right: self.frame.candles_right,
            indicators: &self.indicators,
            price_label: &self.price_band_label,
        }
        .carve(&layout.areas, out);
    }

    /// One drawing pass over `bands`, each band numbered by its place in the
    /// carve.
    fn paint_drawing_bands(&self, frame: &DrawFrame<'_>, bands: &[Band], pass: DrawPass) {
        for (index, band) in bands.iter().enumerate() {
            DrawingPass {
                painter: frame.painter,
                band,
                band_index: index,
                drawings: &self.drawings,
                viewport: &self.model.viewport,
                history_right: frame.right,
                total: frame.total,
                pass,
                content_editing: self.gestures.content_editing,
                hover: self.gestures.hover,
            }
            .paint(
                self.layer_renderers,
                &self.strategies.anchors,
                &self.drawing_projection(),
                self.closed_slots(),
            );
        }
    }

    /// The drawings' price-axis levels, refilled into the pane's retained
    /// container — lent to the axis as a slice, so the claims list stays the
    /// chips the axis draws itself and never spills onto the heap.
    fn fill_price_axis_levels(
        &self,
        frame: &DrawFrame<'_>,
        chrome: &PaneChrome<'_>,
        levels: &mut Vec<PriceAxisLevel>,
    ) {
        if self.layer_read().effective(
            ChartLayer::Drawings,
            self.layer_switched_on(ChartLayer::Drawings, chrome.style),
            chrome.capabilities,
        ) {
            super::price_axis::price_axis_levels(
                &self.drawing_projection(),
                &self.drawings,
                frame.chart_rect,
                frame.right,
                frame.total,
                &frame.scale,
                levels,
            );
        } else {
            levels.clear();
        }
    }

    /// Where the flow legend starts: below everything already stacked at
    /// the canvas's top-left corner — the chart header, the position HUD
    /// while a position is open, and one row per indicator chip — so nothing
    /// there prints over anything else.
    ///
    /// The HUD's row counts only where the HUD paints: on the focused pane —
    /// exactly the condition this pane caches its anchor under, later in the
    /// same draw. The anchor is not readable yet this frame (it is written
    /// after the paper layer), so the condition is restated here rather than
    /// read back.
    fn legend_inset(&self, chrome: &PaneChrome<'_>) -> f32 {
        let hud_here = chrome.paper_hud_here && chrome.paper.position_summary().is_some();
        crate::orderflow_render::LEGEND_HEADER_CLEARANCE_PX
            + crate::indicator_legend::hud_offset_px(hud_here)
            + crate::indicator_legend::stack_height_px(self.indicators.all(), self.legend_collapsed)
    }

    /// Which band the next anchor lands in, said the way the split view
    /// already says which pane has focus: one accent hairline on the top
    /// edge. Painted after the drawings so a dense band cannot bury it.
    fn paint_band_hint(&self, painter: &egui::Painter) {
        let palette = theme::get(painter.ctx());
        if let Some(hint) = self.gestures.band_hint {
            painter.line_segment(
                [
                    egui::pos2(hint.left(), hint.top() + 0.5),
                    egui::pos2(hint.right(), hint.top() + 0.5),
                ],
                egui::Stroke::new(1.0_f32, palette.accent()),
            );
        }
    }

    /// Simulated orders and the position sit above the drawings: they are
    /// operational state, read against the last price painted next. The
    /// unclipped painter carries their chips into the gutter. Both panes
    /// paint them — one market, one set of price levels, and a level is as
    /// true on the 5-minute context as it is on the flow chart. Prices out
    /// of a pane's visible range simply do not draw.
    ///
    /// Switched off, they are only unpainted: the orders keep working and
    /// the dock keeps listing them (see the layer's hint). Returns where the
    /// position HUD anchored, for the pane to publish.
    fn paint_paper(
        &self,
        frame: &DrawFrame<'_>,
        axis_x: f32,
        chrome: &mut PaneChrome<'_>,
    ) -> Option<(egui::Rect, PriceScale)> {
        let mut hud_anchor = None;
        if !self.layer_visible(ChartLayer::PaperTrading, chrome.style) {
            return hud_anchor;
        }
        let chart_rect = frame.chart_rect;
        // The last-price chip's row, computed up front so the paper chips can
        // dodge it: at the instant a market order fills the entry *is* the
        // last price, and two chips on one pixel mangle the only persistent
        // position statement.
        let reserved_chip_y = if self.layer_visible(ChartLayer::LastPrice, chrome.style) {
            frame
                .partial
                .or_else(|| frame.closed.last())
                .and_then(|bar| bar.close.to_f64())
                .map(|price| frame.scale.y(price))
                .filter(|y| *y >= chart_rect.top() && *y <= chart_rect.bottom())
        } else {
            None
        };
        // Tags anchor in the band the input pass tests their ✕ against, so a
        // painted ✕ and its press agree about where it is.
        let candles = crate::bands::drawing_area(chart_rect, self.frame.candles_right);
        let tag_right = crate::paper_trading::tag_band(candles, chart_rect).right();
        // Hover affordances paint only on the pane whose pointer feeds the
        // paper input; the others keep display-only tags. Every pane still
        // paints the lines themselves — an order is a fact about the account,
        // true on whichever chart you are looking at.
        self.layer_renderers.paper(&mut PaperPass {
            paper: chrome.paper,
            painter: frame.painter,
            rect: chart_rect,
            tag_right,
            axis_x,
            scale: frame.scale,
            reserved_chip_y,
            pointer: self.hover_pos,
            takes_input: chrome.paper_takes_input,
            hud_here: chrome.paper_hud_here,
            hud_anchor: &mut hud_anchor,
        });
        hud_anchor
    }

    /// The lane's own time axis, under the tape. Returns the automatic
    /// reference this frame, kept for the tape's menu: the entry that says
    /// "follows the bars" has to be able to say what that works out to, and
    /// the menu is drawn without the bars in reach. Recorded from the same
    /// bars the axis was just drawn from, so the label and the axis can never
    /// disagree. `None` without a tape. `clock`, the live edge and the
    /// zone, labels the tape's own clock under the native tape.
    fn paint_lane_time_axis(
        &self,
        frame: &DrawFrame<'_>,
        clock: Option<(i64, crate::timezone::TzOffset)>,
    ) -> Option<i64> {
        let orderflow = self.orderflow.as_ref()?;
        // A closed tape leaves its strip to the rail: no labels.
        let closed = orderflow.cached_config().live_lane.shown_view() == LaneView::CandlesOnly;
        let lane_strip = split_time_strip(frame.areas.time_strip, self.frame.lane_divider_x).1;
        let lane_strip = lane_strip.filter(|_| !closed);
        let clock = clock.map(|(end_ms, tz)| {
            let geometry = quantick_orderflow::projection::TapeHorizontalGeometry::resolve(
                lane_strip.map_or(0.0, |strip| strip.width()),
                frame.chart_rect.height(),
                &orderflow.cached_config().bubbles,
            );
            (end_ms, tz, geometry)
        });
        LaneTimeAxisPass {
            painter: frame.painter,
            lane_strip,
            window_ms: orderflow.live_lane_window_ms(frame.closed),
            tape_age: orderflow.tape_age(),
            clock,
        }
        .paint();
        Some(reserved_span_ms(frame.closed))
    }
}

/// Font size, in points, of the "nothing in view" line drawn where the candles
/// would be. Matches the "connecting…" line: same voice, same weight.
const EMPTY_VIEW_FONT_SIZE: f32 = 16.0;

impl ChartPane {
    /// What the axes stand aside for this frame: the pointer compass, the
    /// price chips (compass, armed crosshair, last price) and the time tag.
    ///
    /// Decided once, before either axis labels itself, so the two surfaces
    /// cannot disagree about where a chip lands.
    pub(super) fn axis_claims(&self, frame: &DrawFrame<'_>, chrome: &PaneChrome<'_>) -> AxisChips {
        let &DrawFrame {
            painter,
            areas,
            chart_rect,
            right,
            total,
            scale,
            closed,
            partial,
            ..
        } = frame;
        // What the compass will say, decided before either axis labels itself:
        // both axes stand aside where a chip is going to land, and a decision
        // made twice is a decision two surfaces can disagree about.
        let compass = {
            let price_on =
                self.layer_visible(quantick_layers::ChartLayer::PointerPrice, chrome.style);
            let time_on =
                self.layer_visible(quantick_layers::ChartLayer::PointerTime, chrome.style);
            let bar = if price_on || time_on {
                self.hover_pos.and_then(|pointer| {
                    self.series_read()
                        .pointer_bar(&frame.view, pointer.x, right, total)
                })
            } else {
                None
            };
            crate::pointer_compass::PointerCompass::resolve(
                self.hover_pos,
                chart_rect,
                &scale,
                bar,
                price_on,
                time_on,
                chrome.toolrail.tool() == crate::toolrail::Tool::Crosshair || chrome.paper.aiming(),
            )
        };
        // The candles' own segment of the time axis: past the lane divider the
        // strip is the tape's rolling window, which labels itself.
        let (history_strip, _) = split_time_strip(areas.time_strip, self.frame.lane_divider_x);
        // Every claim below is a height a chip will *really* occupy. A claim
        // for a chip that is not drawn is a round number silently missing from
        // the axis — the mirror of the defect this mechanism exists for, and
        // the reason each one repeats its painter's own gate rather than
        // assuming it.
        let on_axis = |y: f32| (chart_rect.y_range().contains(y)).then_some(y);
        let mut price_claims = pointer_compass::AxisClaims::new();
        let mut time_chip = None;
        if let Some(compass) = compass.as_ref() {
            if compass.price {
                price_claims.extend(on_axis(compass.readout.position.y));
            }
            if compass.time {
                time_chip =
                    pointer_compass::time_tag(painter, history_strip, &compass.readout, chrome.tz)
                        .map(|(chip, _)| chip);
            }
        }
        // The armed crosshair writes its own price tag on this axis, on the
        // same geometry and with no compass involved. It is a chip like any
        // other and the axis stands aside for it too.
        if chrome.toolrail.tool() == Tool::Crosshair
            && self.layer_visible(ChartLayer::Crosshair, chrome.style)
            && let Some(pointer) = self.hover_pos.filter(|pos| chart_rect.contains(*pos))
        {
            price_claims.extend(on_axis(pointer.y));
        }
        // The market's own chip. `draw_last_price` refuses to draw one off the
        // pane, and `PriceScale::y` extrapolates rather than clamping, so the
        // claim has to be bounded the same way or panning the last price out
        // of view would leave a hole at the top of the axis. A chip carrying
        // the forming bar's progress grows down a row, and claims it.
        if self.layer_visible(ChartLayer::LastPrice, chrome.style)
            && let Some(bar) = partial.or_else(|| closed.last())
            && let Some(price) = bar.close.to_f64()
            && let Some(y) = on_axis(scale.y(price))
        {
            let progress = chrome
                .price_tag
                .reading(|at| self.state.bar_progress_at(at));
            let second = progress.map(|_| pointer_compass::second_line_height(painter));
            price_claims.extend(pointer_compass::price_tag_claims(y, second));
        }
        AxisChips {
            compass,
            price: price_claims,
            time: time_chip,
        }
    }

    /// The session's closed-trade marks, between the drawings and the live
    /// paper lines, on the bars this pane's tape actually reaches.
    pub(super) fn paint_trade_marks(&self, frame: &DrawFrame<'_>, chrome: &PaneChrome<'_>) {
        let &DrawFrame {
            painter,
            chart_rect,
            right,
            total,
            scale,
            canvas_background,
            ..
        } = frame;
        // Closed-trade marks sit between the drawings and the live paper
        // lines: history under the orders that are still working. Only the
        // session's trades paint — the tape on screen proves their fills;
        // rows loaded from earlier sessions stay in the ledger. And only
        // where this pane's own bars reach the fill's instant, which is why
        // the mapping handed over is `covering_slot_at_time` and not the
        // clamping `slot_at_time`: a trade the tape has not got to yet has
        // no bar to stand on, and standing it on the edge one is the pile-up
        // a replay seek used to draw.
        if self.layer_visible(ChartLayer::TradePaint, chrome.style) {
            let frame = crate::trade_paint::TradePaintFrame {
                painter,
                chart_rect,
                scale: &scale,
                background: canvas_background,
                pointer: self.hover_pos,
                tz: chrome.tz,
                pnl: chrome.paper.account().symbol_pnl(),
            };
            // The window once, not once per fill: `draw` asks about every
            // closed round trip of the session, twice each, every frame.
            let covered = self.covered_window();
            self.layer_renderers
                .trades(&mut super::render_registry::TradesPass {
                    frame: &frame,
                    trades: chrome.paper.session_trades(),
                    selected: chrome.paper.account().selected_trade_index(),
                    slot: &|ms| {
                        covered
                            .filter(|(oldest, newest)| ms >= *oldest && ms <= *newest)
                            .and_then(|_| self.slot_at_time(ms))
                    },
                    x: &|slot| self.model.viewport.x_center(slot, right, total),
                });
        }
    }

    /// This frame's time labels, laid out once for the strip and the
    /// vertical grid that stands at their places.
    pub(super) fn time_marks(
        &self,
        frame: &DrawFrame<'_>,
        chrome: &PaneChrome<'_>,
    ) -> crate::pane::render_registry::StripMarks {
        self.time_strip(frame, chrome, None).marks()
    }

    /// The time strip this frame, standing aside for the pointer's `chip`.
    fn time_strip<'s>(
        &'s self,
        frame: &'s DrawFrame<'_>,
        chrome: &'s PaneChrome<'_>,
        chip: Option<pointer_compass::TimeChip>,
    ) -> crate::pane::render_registry::TimeStripPass<'s> {
        crate::pane::render_registry::TimeStripPass {
            painter: frame.painter,
            strip: frame.areas.time_strip,
            start: frame.start,
            end: frame.end,
            total: frame.total,
            chip,
            style: chrome.style,
            tz: chrome.tz,
            candles_right: self.frame.candles_right,
            viewport: &frame.view,
            series: self.series_read(),
        }
    }

    /// The axis marks and the candles' own dividers: the trader's levels and
    /// the last price on the gutter, the venue seam, the backfill boundary,
    /// the feed gaps, and the time strip.
    pub(super) fn paint_axis_marks(
        &self,
        frame: &DrawFrame<'_>,
        axis_x: f32,
        levels: &[PriceAxisLevel],
        time_chip: Option<pointer_compass::TimeChip>,
        chrome: &PaneChrome<'_>,
        time_marks: &crate::pane::render_registry::StripMarks,
    ) {
        let &DrawFrame {
            painter,
            chart_rect,
            history_rect,
            total,
            scale,
            closed,
            partial,
            cw,
            ..
        } = frame;
        // Above the flow layers: everything else on the canvas is read against
        // it. Drawn on the unclipped painter so the chip reaches the gutter.
        // The trader's own levels on the axis, and then the market's price
        // over them. That order and not the other way round: a level is a
        // static annotation whose value the trader already knows, the last
        // price is live market data, and the moment the two coincide — price
        // arriving at the level — is exactly the moment the live number must
        // not be the one that gets covered.
        crate::pane::render_registry::AxisMarksPass {
            painter,
            chart_rect,
            axis_x,
            scale: &scale,
            levels,
            newest: partial.or_else(|| closed.last()),
            last_price_visible: self
                .layer_visible(quantick_layers::ChartLayer::LastPrice, chrome.style),
            progress: chrome
                .price_tag
                .reading(|at| self.state.bar_progress_at(at)),
            style: chrome.style,
        }
        .paint(self.layer_renderers);
        // The candles' own marks, so they are placed and clipped in their
        // pane: where venue candles give way to bars built from prints, and
        // where backfilled prints give way to live ones.
        let divider = |gaps| DividerPass {
            painter,
            pane: history_rect,
            total,
            candle_width: cw,
            viewport: &frame.view,
            seam: self.seam_slot(),
            boundary: self.state.backfill_boundary(),
            bars: self.state.bars(),
            gaps,
        };
        if self.layer_visible(ChartLayer::SeamDivider, chrome.style) {
            self.layer_renderers.seam(&mut divider(&[]));
        }
        if self.layer_visible(ChartLayer::BackfillDivider, chrome.style) {
            self.layer_renderers.backfill(&mut divider(&[]));
        }
        // Under the same switch as the venue seam: both answer "what is the
        // provenance of the bars either side of this line?", and a trader who
        // turned that class of mark off meant this one too.
        if self.layer_visible(ChartLayer::SeamDivider, chrome.style) {
            self.layer_renderers
                .feed_gaps(&mut divider(chrome.feed_gaps));
        }
        self.time_strip(frame, chrome, time_chip).paint(time_marks);
    }

    /// Where the display day turns in view, or `None` with the separator
    /// off. The bar before the first visible one seeds the comparison, so a
    /// day that opens on the leftmost bar is still marked. The forming bar
    /// counts: a day is marked the moment its first bar opens. Not on a chart
    /// cut at a day or longer: every bar opens a new day there, and the time
    /// labels already write the dates.
    pub(super) fn day_turns(
        &self,
        frame: &DrawFrame<'_>,
        chrome: &PaneChrome<'_>,
    ) -> Option<Vec<DayTurn>> {
        let days_on = self.layer_visible(ChartLayer::DaySeparator, chrome.style)
            && self
                .spec
                .spec()
                .time_interval_ms()
                .is_none_or(|ms| ms < quantick_engine::time_bucket::DAY_MS);
        if !days_on {
            return None;
        }
        let series = self.series_read();
        let slots = frame.start.saturating_sub(1)..frame.end;
        let starts = quantick_civil::day_starts(
            slots.filter_map(|slot| Some((slot, series.slot_open_time(slot)?))),
            chrome.tz,
        );
        let turns = starts
            .into_iter()
            .map(|(slot, date)| DayTurn { slot, date });
        Some(turns.collect())
    }

    /// The last marks on the canvas: the jump-to-live chip, the empty-view
    /// notice, the crosshair, the pointer compass and the status badge, in that
    /// order so each sits on top of the ones before it.
    pub(super) fn paint_canvas_chrome(
        &self,
        frame: &DrawFrame<'_>,
        axis_x: f32,
        nothing_in_view: bool,
        compass: Option<&PointerCompass>,
        chrome: &PaneChrome<'_>,
    ) {
        let palette = theme::get(frame.painter.ctx());
        let &DrawFrame {
            painter,
            areas,
            chart_rect,
            history_rect,
            scale,
            ..
        } = frame;
        // The way back from history (audit F6), painted over the strip's
        // labels on the same geometry the input path registered.
        if let Some(chip) = self.shown_live_chip(areas.time_strip) {
            draw_live_chip(painter, chip);
        }
        // Panned off the data (or a rebuild re-cut the series under the
        // window): the chart is whole — axis, tape, badges — but there is
        // nothing in the candles' pane, so say so and say the way back.
        if nothing_in_view {
            painter.text(
                history_rect.center(),
                egui::Align2::CENTER_CENTER,
                "no bars in view — double-click to return to the live edge",
                palette.label_font(EMPTY_VIEW_FONT_SIZE),
                palette.text_muted(),
            );
        }
        if self.layer_visible(ChartLayer::Crosshair, chrome.style) {
            self.layer_renderers
                .crosshair(&mut crate::pane::render_registry::CrosshairPass {
                    painter,
                    chart_rect,
                    axis_x,
                    scale: &scale,
                    pointer: self.hover_pos,
                    armed: chrome.toolrail.tool() == crate::toolrail::Tool::Crosshair,
                });
        }
        // Last of the canvas marks, so the answer the trader is asking for by
        // holding the mouse where they are holding it is on top of the ones
        // the chart volunteers. Decided in `axis_claims`, where the axes read it too.
        if let Some(compass) = compass.as_ref() {
            self.layer_renderers
                .pointer(&mut crate::pane::render_registry::PointerPass {
                    painter,
                    compass,
                    axis_x,
                    time_strip: areas.time_strip,
                    divider_x: self.frame.lane_divider_x,
                    tz: chrome.tz,
                });
        }
    }
}
