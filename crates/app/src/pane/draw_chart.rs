//! The paint frame: [`ChartPane::draw_chart`], the order the layers go down
//! in, and the stages it runs them through.
//!
//! The frame is a staged pipeline. Two opening stages write the pane before
//! anything borrows its series: [`ChartPane::begin_frame`] (the published
//! resets, the footprint switch and the ladder snapshot) and
//! [`ChartPane::lay_out`] (the plot split, the live lane and the viewport
//! clamp), which returns the frame's geometry as one [`FrameLayout`] value.
//! [`FrameLayout::resolve`] then borrows the series into the [`DrawFrame`]
//! every painter reads, and from there on `self.state` stays borrowed for
//! the whole frame.
//!
//! Under that borrow a step that writes a field cannot be a `&mut self`
//! method, so each writing stage is an owner type in `frame_stages.rs` —
//! [`FlowFrame`] for the tape's layers, [`HistoryStage`] for the candle
//! pane's — handed only the field it writes; a stage whose only write is a
//! published value (the legend's bounds, the paper HUD's anchor, the lane's
//! reference) is a `&self` method here returning it, and the orchestrator
//! stores it. Every step that only reads is a `&self` painter here or in
//! `layer_painters.rs`.

use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;

use crate::bands::{Band, Bands};
use crate::chart::PriceScale;
use crate::orderflow_view::{LiveLane, OrderflowView};
use crate::plot_area::split_time_strip;
use crate::theme;
use quantick_layers::ChartLayer;
use quantick_orderflow::reserved_span_ms;

use super::draw_frame::{AxisChips, DrawFrame};
use super::frame_layout::{
    CandleDress, FrameLayout, FrameStart, Series, live_ladder, nothing_in_view,
    snapshot_live_ladder,
};
use super::frame_stages::{FlowFrame, HistoryStage, PaneLane, refresh_drawing_folds};
use super::render_registry::*;
use super::{
    ChartPane, DrawPass, PaneChrome, PriceAxisClaims, PriceAxisLevel, background_color, grid_color,
    lane_rungs,
};

impl ChartPane {
    pub fn draw_chart(
        &mut self,
        painter: &egui::Painter,
        area: egui::Rect,
        chrome: &mut PaneChrome<'_>,
    ) {
        let renderers = self.layer_renderers;
        let start = self.begin_frame(painter, area, chrome);
        let Some(layout) = self.lay_out(painter, area, chrome) else {
            return;
        };
        // Field borrows, not `self` borrows: the tape below needs `&mut
        // self.orderflow` while these are alive.
        let series = Series {
            prefix: self.history_prefix.as_slice(),
            closed: self.state.bars(),
            partial: self.state.partial(),
        };
        let Some((frame, auto_range)) = layout.resolve(
            painter,
            series,
            self.frame.auto_range,
            &self.price_view,
            start.canvas_background,
        ) else {
            return;
        };
        let chart_rect = frame.chart_rect;

        // Resting liquidity is the bottom visual layer. Projection is pure with
        // respect to candles and uses the same bar-warped viewport coordinates.
        let mut flow = FlowFrame::new(
            renderers,
            &frame,
            layout.lane_width_px(),
            &self.viewport,
            self.price_view.is_inverted(),
        );
        let demand = self.projection_demand();
        let timeline_revision = self.state.timeline_revision();
        flow.project(self.orderflow.as_mut(), demand, timeline_revision, &frame);
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
                partial_ladder: live_ladder(&self.footprint),
                partial_version: self.footprint.live_version,
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
            time: time_claims,
        } = self.axis_claims(&frame, chrome);
        // Gathered once, read twice: the axis stands aside for these just
        // below, and the same list is what gets painted onto the gutter
        // further down. Borrowed out of the pane so the container survives
        // the frame and the next one refills it rather than reallocating.
        let mut levels = std::mem::take(&mut self.price_axis_levels);
        self.fill_price_axis_levels(&frame, chrome, &mut levels);

        // Grid + price labels first, behind the candles. Labels anchor on the
        // gutter's edge, past the live strip when one is shown.
        let axis_x = layout.areas.price_gutter.left();
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
        });

        let history = HistoryStage {
            renderers,
            frame: &frame,
            clip: painter.with_clip_rect(layout.history_rect),
            viewport: &self.viewport,
            dress: CandleDress::resolve(&self.footprint, chrome, start.footprint_paints, frame.cw),
        };
        let mut carved = std::mem::take(&mut self.frame.bands);
        let clear_depth = flow.projected() && depth_visible;
        let mut candle_pass =
            history.candle_pass(&self.indicators, clear_depth, &chrome.style.candles);
        renderers.candle_clear(&mut candle_pass);
        // Only price-band background drawings may precede candle/indicator scales.
        self.carve_bands(&layout, &mut carved);
        let price_band = carved.get(..1).unwrap_or_default();
        self.paint_drawing_bands(&frame, price_band, DrawPass::UnderCandles);
        renderers.candles(&mut candle_pass);
        if start.footprint_paints {
            history.footprint(
                &mut self.footprint,
                self.state.bar_footprints(),
                chrome,
                depth_visible,
            );
        }
        history.overlay(&self.indicators);
        let lane = self.pane_lane(&layout, &frame);
        let grid = grid_color(chrome.style);
        history.indicator_panes(&mut self.indicators, lane, layout.indicator_guide_x, grid);
        flow.aggressions(self.orderflow.as_ref());
        self.frame.flow_legend = flow.legend(self.orderflow.as_ref(), self.legend_inset(chrome));
        flow.strip(self.orderflow.as_mut(), &frame);

        self.paint_drawings_over(&frame, &layout, &mut carved, chrome);
        self.frame.bands = carved;
        self.paint_band_hint(painter);
        self.paint_trade_marks(&frame, chrome);
        self.paper_hud_anchor = self.paint_paper(&frame, axis_x, chrome);
        self.paint_axis_marks(&frame, axis_x, &levels, &time_claims, chrome);
        if let Some(reference_ms) = self.paint_lane_time_axis(&frame) {
            self.frame.lane_reference_ms = Some(reference_ms);
        }
        let nothing_in_view = nothing_in_view(&frame);
        self.paint_canvas_chrome(&frame, axis_x, nothing_in_view, compass.as_ref(), chrome);

        // The levels' container, back on the pane for the next frame to
        // refill rather than reallocate.
        self.price_axis_levels = levels;

        // Cache the auto range + height for next frame's input handler, which
        // runs before the draw and needs them for pixel↔price conversion.
        self.frame.auto_range = Some(auto_range);
        self.frame.chart_height = chart_rect.height();
        self.frame.chart_top = chart_rect.top();
    }

    /// The frame's opening writes: the values published per frame reset, the
    /// canvas cleared, and the footprint ladders switched and snapshotted.
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
        painter.rect_filled(area, egui::Rounding::ZERO, canvas_background);

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
        let footprint_on =
            (self.footprint.visible || self.wants_range_profile()) && !footprint_blocked;
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
        {
            self.state.set_footprint_group(base);
        }
        if footprint_on {
            let now = painter.ctx().input(|i| i.time);
            let closed_total = self.history_prefix.len() + self.state.bars().len();
            snapshot_live_ladder(
                &mut self.footprint,
                self.state.partial_footprint(),
                closed_total,
                now,
            );
        }
        FrameStart {
            canvas_background,
            footprint_blocked,
            footprint_paints,
        }
    }

    /// The frame's geometry: the plot split, the live lane, the history
    /// pane beside it and the clamped viewport's visible slots. `None` for an
    /// empty series, after painting what an empty pane says instead.
    fn lay_out(
        &mut self,
        painter: &egui::Painter,
        area: egui::Rect,
        chrome: &PaneChrome<'_>,
    ) -> Option<FrameLayout> {
        let closed_total = self.history_prefix.len() + self.state.bars().len();
        let total = closed_total + usize::from(self.state.partial().is_some());
        let areas = self.plot_areas(area, chrome.capabilities);
        // Indicator panes claimed the bottom band inside `plot_split`, so the
        // rect the candles scale to is the same one the input handler uses.
        let chart_rect = areas.chart;
        self.frame.price_gutter = Some(areas.price_gutter);
        self.frame.time_strip =
            Some(split_time_strip(areas.time_strip, self.frame.lane_divider_x).0);
        let indicator_guide_x = self
            .hover_pos
            .filter(|position| chart_rect.contains(*position))
            .map(|position| position.x);
        if total == 0 {
            self.paint_empty_pane(painter, area, chart_rect, chrome);
            return None;
        }

        let live_lane = self.lay_out_lane(chart_rect);
        let history_rect = egui::Rect::from_min_max(
            chart_rect.min,
            egui::pos2(
                self.frame
                    .lane_divider_x
                    .unwrap_or_else(|| chart_rect.right()),
                chart_rect.bottom(),
            ),
        );

        // The projection margin is enforced here, against the rect the candles
        // are actually drawn in, rather than in the input handler: panning
        // leaves the future end open and zooming knows nothing about the
        // window, and the window itself moves without any gesture at all (the
        // app resizing, the lane divider dragged, a pane collapsed). Painting
        // is the one place that sees all of it, so it is the one place the
        // rule holds — pushed fully left, the newest bar stops at the left
        // edge and the rest of the window is empty canvas to project into.
        self.viewport.clamp_to_window(history_rect.width(), total);
        let (start, end) = self.viewport.visible_range(history_rect.width(), total);
        Some(FrameLayout {
            areas,
            chart_rect,
            history_rect,
            live_lane,
            total,
            closed_total,
            start,
            end,
            cw: self.viewport.candle_width(),
            indicator_guide_x,
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
        let lane_width_px = live_lane.map_or(0.0, |lane| lane.width_px);
        // Everything left of the divider is the candles' pane. They pan and
        // zoom inside it exactly as they did when it was the whole chart.
        self.frame.lane_divider_x =
            crate::orderflow_render::lane_divider_x(chart_rect, lane_width_px);
        self.frame.chart_rect = Some(chart_rect);
        let rungs = lane_rungs(
            self.frame
                .lane_divider_x
                .map_or(0.0, |divider| chart_rect.right() - divider),
        );
        if self.lane.set_rungs(rungs) {
            let command = self
                .lane
                .command(self.state.partial().cloned(), self.state.trades());
            self.indicator_worker.send(command);
        }
        live_lane
    }

    /// What a pane with no bars yet shows: the symbol it is waiting for, the
    /// tape's status and the canvas layers that need no bars.
    fn paint_empty_pane(
        &self,
        painter: &egui::Painter,
        area: egui::Rect,
        chart_rect: egui::Rect,
        chrome: &PaneChrome<'_>,
    ) {
        painter.text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            format!("connecting to {} …", chrome.symbol),
            egui::FontId::proportional(16.0),
            theme::TEXT_MUTED,
        );
        if let Some(orderflow) = self.orderflow.as_ref() {
            self.layer_renderers.status(&mut StatusPass {
                owner: orderflow,
                painter,
                rect: chart_rect,
            });
        }
        self.layer_renderers.canvas(&mut CanvasPass {
            painter,
            rect: chart_rect,
            tape_on: self.orderflow.as_ref().map(|tape| tape.lane_enabled()),
            tape_hovered: self.tape_switch_hovered,
            state: &self.layers,
            facts: self.layer_facts(Some(chrome.capabilities)),
        });
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
        self.draw_quick_range(frame.painter, carved, frame.right, frame.total, chrome);
    }

    /// Carve the drawing bands — the price band and one per indicator pane —
    /// into `out`, against this frame's scales.
    fn carve_bands(&self, layout: &FrameLayout, out: &mut Bands) {
        crate::bands::BandGeometry {
            auto_range: self.frame.auto_range,
            price_view: &self.price_view,
            lane_divider_x: self.frame.lane_divider_x,
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
                viewport: &self.viewport,
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
        if self.layer_visible(ChartLayer::Drawings, chrome.style) {
            self.drawing_projection().price_axis_levels(
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
        if let Some(hint) = self.gestures.band_hint {
            painter.line_segment(
                [
                    egui::pos2(hint.left(), hint.top() + 0.5),
                    egui::pos2(hint.right(), hint.top() + 0.5),
                ],
                egui::Stroke::new(1.0_f32, theme::ACCENT),
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
        // Tags anchor inside the interactive plot (left of the live lane when
        // one is up) — the same right edge the input pass hands to
        // `handle_chart_input`, so a painted ✕ and its press agree about
        // where it is.
        let tag_right = self.frame.lane_divider_x.unwrap_or(chart_rect.right());
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
    /// disagree. `None` without a tape.
    fn paint_lane_time_axis(&self, frame: &DrawFrame<'_>) -> Option<i64> {
        let orderflow = self.orderflow.as_ref()?;
        LaneTimeAxisPass {
            painter: frame.painter,
            lane_strip: split_time_strip(frame.areas.time_strip, self.frame.lane_divider_x).1,
            window_ms: orderflow.live_lane_window_ms(frame.closed),
            tape_age: orderflow.tape_age(),
        }
        .paint();
        Some(reserved_span_ms(frame.closed))
    }
}
