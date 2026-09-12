//! The paint frame: [`ChartPane::draw_chart`], the order the layers go down
//! in, and the [`DrawFrame`] it lends to its painters.
//!
//! What stays in this function is every step that writes the pane — the
//! footprint switch and the ladder snapshot, the live lane and the viewport
//! clamp, the projection, the fold refreshes, the footprint layer, the
//! indicator panes, the flow layers, the paper layer and the frame's cached
//! geometry — because the slices of `self.state` are borrowed for the whole
//! frame and a step that writes a field cannot be a method while they are.
//! Every step that only reads went to `layer_painters.rs` as a `&self`
//! painter taking the frame by reference. The bodies here are the ones that
//! ran in `pane.rs`, at the same indentation.

use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;

use crate::chart;
use crate::chart_layers::ChartLayer;
use crate::indicator_render::{self, PlotX};
use crate::orderflow_view::{OrderflowView, VisibleBarTimeline};
use crate::plot_area::split_time_strip;
use crate::theme;
use quantick_orderflow::reserved_span_ms;

use super::draw_frame::{AxisChips, DrawFrame};
use super::tape_switch::TAPE_SWITCH_RESERVED_PX;
use super::{
    ChartPane, DrawPass, PaneChrome, PriceAxisClaims, background_color, grid_color, lane_rungs,
};

/// How often the forming bar's footprint ladder is re-snapshotted for
/// drawing, in seconds. ~10 Hz: the eye reads the pattern, not the ticking
/// digits, and a layout that repaints per print reflows under the pointer.
const LIVE_LADDER_REFRESH_S: f64 = 0.1;

impl ChartPane {
    pub fn draw_chart(
        &mut self,
        painter: &egui::Painter,
        area: egui::Rect,
        chrome: &mut PaneChrome<'_>,
    ) {
        self.paper_hud_anchor = None;
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

        // Field borrows, not `self` borrows: the tape below needs `&mut
        // self.orderflow` while these are alive.
        let prefix = self.history_prefix.as_slice();
        let closed = self.state.bars();
        let partial = self.state.partial();
        let closed_total = prefix.len() + closed.len();
        let total = closed_total + usize::from(partial.is_some());

        // Snapshot the forming bar's ladder at ~10 Hz rather than per print;
        // between snapshots the drawn numbers hold still. Taken here, with
        // the accumulation switch, because it has two consumers now — the
        // footprint layer and the range-profile drawings — and each reading
        // the live ladder on its own cadence would show two different bars.
        if footprint_on {
            let now = painter.ctx().input(|i| i.time);
            match self.state.partial_footprint() {
                Some(partial_ladder) => {
                    let stale =
                        self.footprint
                            .live
                            .as_ref()
                            .is_none_or(|(taken, snapshot_slot, _)| {
                                *snapshot_slot != closed_total
                                    || now - *taken >= LIVE_LADDER_REFRESH_S
                            });
                    if stale {
                        self.footprint.live = Some((now, closed_total, partial_ladder.clone()));
                        self.footprint.live_version = self.footprint.live_version.wrapping_add(1);
                    }
                }
                None => {
                    if self.footprint.live.take().is_some() {
                        self.footprint.live_version = self.footprint.live_version.wrapping_add(1);
                    }
                }
            }
        }
        let areas = self.plot_areas(area, chrome.capabilities);
        // Indicator panes claimed the bottom band inside `plot_split`, so the
        // rect the candles scale to is the same one the input handler uses.
        let chart_rect = areas.chart;
        self.frame.price_gutter = Some(areas.price_gutter);
        self.frame.time_strip =
            Some(split_time_strip(areas.time_strip, self.frame.lane_divider_x).0);
        let pane_rects = areas.indicator_panes.clone();
        if total == 0 {
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                format!("connecting to {} …", chrome.symbol),
                egui::FontId::proportional(16.0),
                theme::TEXT_MUTED,
            );
            if let Some(orderflow) = self.orderflow.as_ref() {
                orderflow.draw_status_badge(painter, chart_rect, TAPE_SWITCH_RESERVED_PX);
            }
            self.draw_tape_switch(painter, chart_rect);
            return;
        }

        // The live lane: a pane of its own, pinned to the right edge of the
        // chart, showing a fixed window of market time that always ends at
        // now. Fixed width, fixed pixels-per-ms: a print enters at the right
        // edge and slides left until it leaves into the slot of its own bar.
        //
        // It belongs to the tape rather than to the forming bar, which is what
        // keeps a bar close from emptying it — the reset that made the book
        // look like it was restarting every few seconds. And it is a pane
        // rather than a reservation inside the viewport, which is what keeps
        // every chart movement out of it: panning, zooming and dragging move
        // the candles beside the tape and never the tape itself, so the most
        // recent prints are on screen whatever the rest of the chart is doing.
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

        // The visible closed bars, plus the partial if it falls in view. With
        // a venue prefix the window can straddle both series, so it is two
        // slices — chained where they are read rather than copied into one.
        // Copying was 24-48 KB every frame for the life of the pane, including
        // the common case of following the live edge, where the seam is three
        // months off screen and the prefix half of the window is empty.
        let closed_start = start.min(closed_total);
        let closed_end = end.min(closed_total);
        let visible_prefix = &prefix[closed_start.min(prefix.len())..closed_end.min(prefix.len())];
        let visible_state = &closed[closed_start.saturating_sub(prefix.len())
            ..closed_end.saturating_sub(prefix.len()).min(closed.len())];
        let visible_closed = || visible_prefix.iter().chain(visible_state);
        let partial_visible = partial.filter(|_| closed_total >= start && closed_total < end);

        // Auto-fit the visible bars, then apply any manual price pan/zoom. A
        // window with no bars in it still gets a scale (the last one, then the
        // newest bar), because a chart that draws nothing at all is
        // indistinguishable from a hung app — which is exactly how the blank
        // frame after a rebuild read.
        let nothing_in_view =
            visible_prefix.is_empty() && visible_state.is_empty() && partial_visible.is_none();
        let Some(auto_scale) = chart::price_window(
            visible_closed(),
            partial_visible,
            self.frame.auto_range,
            partial.or_else(|| closed.last()),
            chart_rect.top(),
            chart_rect.bottom(),
        ) else {
            return;
        };
        let auto_range = auto_scale.range();
        let scale = self
            .price_view
            .scale(auto_range, chart_rect.top(), chart_rect.bottom());

        let cw = self.viewport.candle_width();
        let half = chrome.style.candles.body_half_width(cw);
        let right = history_rect.right();
        let frame = DrawFrame {
            painter,
            areas: &areas,
            chart_rect,
            history_rect,
            right,
            total,
            start,
            end,
            closed_start,
            closed_total,
            scale,
            prefix,
            closed,
            partial,
            partial_visible,
            visible_prefix,
            visible_state,
            canvas_background,
            cw,
        };

        // How the candle behaves under the footprint is the *style's* answer,
        // not this function's: a style that draws inside the candle needs its
        // interior, and one that draws in a box beside it needs the candle out
        // of the way entirely. With the layer off, candles are untouched at
        // any zoom.
        // The style that will actually draw, not the one that was asked for: a
        // style below its own zoom floor hands over, and the candle must be
        // laid out for whichever one paints. Asking the requested style put a
        // sidebar lane under a style that draws full width.
        let requested_style = self
            .footprint
            .config
            .as_ref()
            .unwrap_or(chrome.footprint)
            .style;
        let footprint_style = self.footprint.lod.effective_style(requested_style);
        let treatment = footprint_style.candle_treatment();
        // The lane a sidebar candle keeps at the left of its slot, and the
        // style the layer leaves the candle in. Both from one function, whose
        // whole point is that they answer to `footprint_paints` and never to
        // the accumulation switch — see `footprint_render::candle_dressing`.
        let (candle_lane, faded_candles) = crate::footprint_render::candle_dressing(
            footprint_paints,
            treatment,
            cw,
            chrome.style.candles,
        );
        // The half-width the footprint's content actually spans. The lane is
        // cut out of *this*, so the candle placed beside it has to be measured
        // from the same edge — measuring from the candle's own body width put
        // it inside the box the lane was reserved next to, where the opaque
        // plate then painted straight over it.
        let content_half = treatment.content_half_width(cw, half);
        let candles = faded_candles.as_ref().unwrap_or(&chrome.style.candles);

        // Resting liquidity is the bottom visual layer. Projection is pure with
        // respect to candles and uses the same bar-warped viewport coordinates.
        // The projection builds a lane exactly when the layout draws one. Tied
        // to `lane_width_px` rather than restated, because the two decide the
        // same thing: with them apart, the newest prints would be clustered and
        // sized as lane prints and then squeezed into a single candle slot.
        // Only the engine's own bars carry tape, so the timeline starts at
        // the first *state* bar's global slot: when the window straddles the
        // venue seam (a time-cutting flow pane, audit S1), that is the seam
        // itself, not the window's first slot.
        let timeline = VisibleBarTimeline::new(
            self.state.timeline_revision(),
            closed_start.max(prefix.len()),
            visible_state,
            partial_visible,
        );
        // Two surfaces consume the projection without being the depth map or
        // the bubbles: the live strip draws the same clusters, and the lane's
        // marks need the frame's live edge. Stated here, every frame, from the
        // layers this pane owns — so with the bubbles hidden the pipeline stays
        // alive for the strip, and with every other flow layer off the lane is
        // still marked instead of being a reserved but empty band whose menu
        // entry claims it is on.
        let demand = self.projection_demand();
        let orderflow_frame = self.orderflow.as_mut().and_then(|orderflow| {
            orderflow.set_projection_demand(demand);
            // The tape's automatic window comes from the newest bars of the
            // series, never from the slice on screen: panning the candles is
            // not a statement about how much market time the tape shows.
            orderflow.project_visible(
                timeline,
                lane_width_px > 0.0,
                end == total,
                Some(quantick_orderflow::reserved_span_ms(self.state.bars())),
                scale.range(),
            )
        });
        if let Some(orderflow) = self.orderflow.as_mut()
            && let Some(frame) = &orderflow_frame
        {
            orderflow.draw_background(
                painter,
                chart_rect,
                &self.viewport,
                total,
                frame,
                canvas_background,
                lane_width_px,
                self.price_view.is_inverted(),
            );
        }

        // Bring the range-profile drawings' folds up to date before anything
        // paints over the map. Key-guarded inside: the common frame compares
        // one small key per profile object and folds nothing. It runs after
        // the heatmap projection on purpose — the map's left boundary is
        // where each profile's paint cuts from fill to silhouette, and the
        // O(cells) scan behind it is paid only while a profile object exists.
        let heat_first_slot = orderflow_frame
            .as_ref()
            .filter(|_| {
                self.orderflow
                    .as_ref()
                    .is_some_and(OrderflowView::depth_visible)
                    && self.wants_range_profile()
            })
            .and_then(|frame| frame.first_heat_slot());
        // Read before the drawings are borrowed mutably below.
        let partial_bucket_slot = self.partial_bucket_slot();
        let folding = crate::frvp::refresh(
            &mut self.drawings,
            &crate::frvp::RefreshInputs {
                state: &self.state,
                budget: crate::frvp::fold_budget(),
                prefix,
                partial_ladder: self.footprint.live.as_ref().map(|(_, _, ladder)| ladder),
                partial_version: self.footprint.live_version,
                blocked: footprint_blocked,
                side_inferred: chrome.side_inferred,
                heat_first_slot,
                draft_hover_bar: self.gestures.hover.map(|point| point.bar),
                partial_bucket_slot,
            },
        );
        if folding {
            // A range too long for one pass: paint what is folded and come
            // straight back for the next slice. Without this the fill would
            // stall wherever the tape happened to stop waking the window.
            painter.ctx().request_repaint();
        }
        // The anchored-VWAP objects' cached rows, same pass discipline: a key
        // comparison per object on the common frame, a replay only when the
        // tape or the config moved (see `crate::avwap`).
        crate::avwap::refresh(
            &mut self.drawings,
            &crate::avwap::RefreshInputs {
                state: &self.state,
                prefix: &self.history_prefix,
            },
        );

        let AxisChips {
            compass,
            price: price_claims,
            time: time_claims,
        } = self.axis_claims(&frame, chrome);
        // Gathered once, read twice: the axis stands aside for these just
        // below, and the same list is what gets painted onto the gutter
        // further down. Borrowed out of the pane so the container survives
        // the frame and the next one refills it rather than reallocating —
        // and lent to the axis as a slice, so the claims list stays the chips
        // the axis draws itself and never spills onto the heap.
        let mut levels = std::mem::take(&mut self.price_axis_levels);
        if self.layer_visible(ChartLayer::Drawings, chrome.style) {
            self.price_axis_levels(chart_rect, right, total, &scale, &mut levels);
        } else {
            levels.clear();
        }

        // Grid + price labels first, behind the candles. Labels anchor on the
        // gutter's edge, past the live strip when one is shown.
        let axis_x = areas.price_gutter.left();
        let price_claims = PriceAxisClaims {
            marks: price_claims,
            levels: &levels,
        };
        self.draw_price_axis(painter, chart_rect, axis_x, &scale, &price_claims, chrome);

        // Candles, clipped to their own pane: panning far enough into history
        // sends the newest bars off the right of it, and they scroll out of
        // sight behind the tape instead of being drawn over it.
        let clip = painter.with_clip_rect(history_rect);
        let viewport = &self.viewport;
        let mut carved = std::mem::take(&mut self.frame.bands);
        self.paint_candles(
            &frame,
            &clip,
            &mut carved,
            orderflow_frame.as_ref(),
            half,
            candle_lane,
            content_half,
            candles,
        );
        // The footprint rides directly on the candles, before everything
        // drawn over them: it is a representation of the bars themselves,
        // not an annotation. Prefix (venue) candles carry no tape and draw
        // no ladder — the layer starts where trade-built bars start.
        if footprint_paints {
            // The forming bar's ladder is the ~10 Hz snapshot taken with the
            // accumulation switch at the top of the frame, shared with the
            // range-profile drawings.
            let frame = crate::footprint_render::LayerFrame {
                painter: &clip,
                chart_rect: history_rect,
                scale: &scale,
                footprints: self.state.bar_footprints(),
                first_state_slot: prefix.len(),
                visible: (start, end),
                partial: self
                    .footprint
                    .live
                    .as_ref()
                    .map(|(_, _, ladder)| ladder)
                    .filter(|_| partial_visible.is_some()),
                partial_slot: closed_total,
                x_center: &|slot| viewport.x_center(slot, right, total),
                // The *content* half-width, which is not always the candle's.
                // A style that draws inside the candle is bounded by it; one
                // that draws in a box beside it is bounded only by the slot,
                // and charging it the candle gap as well spends a quarter of
                // the row on air twice over.
                half: content_half,
                candle_width: cw,
                side_inferred: chrome.side_inferred,
                depth_visible: self
                    .orderflow
                    .as_ref()
                    .is_some_and(OrderflowView::depth_visible),
                pixels_per_point: painter.ctx().pixels_per_point(),
                // Field access, not `self.footprint_config(..)`: the method
                // borrows all of `self` and the draw below needs
                // `self.footprint.lod` mutably. Same resolution rule.
                config: self.footprint.config.as_ref().unwrap_or(chrome.footprint),
            };
            crate::footprint_render::draw_layer(&frame, &mut self.footprint.lod);
        }
        // Overlay indicator plots ride the candles' own clip, scale and
        // x-mapping — after candles, before aggression bubbles (the same
        // paint-order slot draw objects take).
        let plot_x = PlotX {
            viewport: &self.viewport,
            right,
            total,
        };
        self.paint_overlays(&frame, &clip, &plot_x);
        // Pane indicators stack in the band carved off above, sharing the
        // candles' x-mapping so bars and their flow read as one chart. Each
        // pane records the range it auto-fitted to, so the gesture over its
        // axis zooms the very range this frame drew.
        let grid = grid_color(chrome.style);
        // The lane's window of tape time, and the closes inside it. Both are
        // the same for every pane, so they are resolved once here rather than
        // per pane — and both come from the tape's own numbers, so a pane's
        // curve lands under the prints it was computed from.
        let lane_window = self
            .frame
            .lane_divider_x
            .zip(live_lane)
            .and_then(|(divider, lane)| {
                let orderflow = self.orderflow.as_ref()?;
                let window = orderflow.live_lane_window_ms(visible_state).max(1);
                Some((divider, lane.end_ms.saturating_sub(window), lane.end_ms))
            });
        let lane_steps: Vec<(i64, usize)> =
            lane_window.map_or_else(Vec::new, |(_, start_ms, _)| {
                let first = prefix.len();
                // Walked back from the newest close and reversed in place: the
                // window holds a handful of bars, and building it front to back
                // would mean scanning every closed bar the chart has ever seen.
                let mut steps: Vec<(i64, usize)> = closed
                    .iter()
                    .enumerate()
                    .rev()
                    .take_while(|(_, bar)| bar.close_time >= start_ms)
                    .map(|(index, bar)| (bar.close_time, first + index))
                    .collect();
                steps.reverse();
                steps
            });
        for ((view, pane), gutter) in self
            .indicators
            .visible_panes_mut()
            .zip(&pane_rects)
            .zip(&areas.pane_gutters)
        {
            let auto = indicator_render::pane_auto_range(view, start, end);
            view.last_auto = auto;
            let frame = indicator_render::PaneFrame {
                rect: egui::Rect::from_min_max(
                    egui::pos2(history_rect.left(), pane.rect.top()),
                    egui::pos2(history_rect.right(), pane.rect.bottom()),
                ),
                lane: lane_window.map(|(divider, start_ms, end_ms)| indicator_render::LaneFrame {
                    rect: egui::Rect::from_min_max(
                        egui::pos2(divider, pane.rect.top()),
                        egui::pos2(chart_rect.right(), pane.rect.bottom()),
                    ),
                    start_ms,
                    end_ms,
                    steps: &lane_steps,
                }),
                gutter: *gutter,
                background: canvas_background,
                grid,
                collapsed: pane.collapsed,
            };
            indicator_render::draw_pane(
                painter,
                &frame,
                view,
                &plot_x,
                auto.map(|auto| view.scale.resolve(auto)),
                start,
                end,
                // The slot of the forming bar counts the venue prefix too: a
                // pane's partial marker has to land on the same slot the
                // candles' does, and this pane's series starts at the prefix.
                partial_visible.map(|_| closed_total),
            );
        }
        if let Some(orderflow) = self.orderflow.as_mut()
            && let Some(frame) = &orderflow_frame
        {
            orderflow.draw_aggressions(
                painter,
                chart_rect,
                &self.viewport,
                total,
                frame,
                canvas_background,
                lane_width_px,
                self.price_view.is_inverted(),
            );
        }

        // The canvas's key, in a pass of its own so the bubble switch cannot
        // take it down with them. It starts below everything already stacked
        // at this corner — the chart header, the position HUD while a
        // position is open, and one row per indicator chip — so nothing at the
        // top-left prints over anything else.
        //
        // The HUD's row counts only where the HUD paints: on the focused
        // pane — exactly the condition this pane caches its anchor under,
        // further down this same draw. The anchor is not readable yet this
        // frame (it is written after the paper layer), so the condition is
        // restated here rather than read back.
        let hud_here = chrome.paper_hud_here && chrome.paper.position_summary().is_some();
        let legend_inset = crate::orderflow_render::LEGEND_HEADER_CLEARANCE_PX
            + crate::indicator_legend::hud_offset_px(hud_here)
            + crate::indicator_legend::stack_height_px(
                self.indicators.all(),
                self.legend_collapsed,
            );
        if let Some(orderflow) = self.orderflow.as_mut()
            && let Some(frame) = &orderflow_frame
        {
            orderflow.draw_legend(
                painter,
                chart_rect,
                &self.viewport,
                total,
                frame,
                canvas_background,
                lane_width_px,
                legend_inset,
            );
        }

        // The live strip: the book right now plus the forming bar's
        // aggression histogram, beside the axis the price labels live on.
        // Its own rect, so chart layers never bleed into it. The histogram
        // follows `partial` (not its visible filter): the strip reports the
        // bar forming now even while the user pans through history.
        if let Some(orderflow) = self.orderflow.as_mut()
            && let Some(strip) = areas.live_strip
        {
            orderflow.draw_live_strip(
                painter,
                strip,
                &scale,
                canvas_background,
                partial.map(|bar| bar.open_time),
            );
        }

        // Drawings sit above market layers and remain anchored to chart space,
        // not the screen, while the viewport moves beneath them.
        //
        // Carved *here*, after the panes drew: each band's scale is then the
        // one its own curve was just drawn with, which is the invariant this
        // whole feature rests on.
        // Re-carved, not reused: the pass above ran before the indicator panes
        // drew, and every band's scale is written *by* that draw. Into the
        // pane's own buffer: same geometry as the input pass computed, no
        // container allocated, and what the tab's shared projection reads
        // afterwards.
        self.carve_bands(&areas, &mut carved);
        for (index, band) in carved.iter().enumerate() {
            self.draw_drawings(painter, band, index, right, total, DrawPass::OverCandles);
        }
        self.frame.bands = carved;
        // Which band the next anchor lands in, said the way the split view
        // already says which pane has focus: one accent hairline on the top
        // edge. Painted after the drawings so a dense band cannot bury it.
        if let Some(hint) = self.gestures.band_hint {
            painter.line_segment(
                [
                    egui::pos2(hint.left(), hint.top() + 0.5),
                    egui::pos2(hint.right(), hint.top() + 0.5),
                ],
                egui::Stroke::new(1.0_f32, theme::ACCENT),
            );
        }

        self.paint_trade_marks(&frame, chrome);

        // Simulated orders and the position sit above the drawings: they are
        // operational state, read against the last price painted next. The
        // unclipped painter carries their chips into the gutter. Both panes
        // paint them — one market, one set of price levels, and a level is as
        // true on the 5-minute context as it is on the flow chart. Prices out
        // of a pane's visible range simply do not draw.
        //
        // Switched off, they are only unpainted: the orders keep working and
        // the dock keeps listing them (see the layer's hint).
        if self.layer_visible(ChartLayer::PaperTrading, chrome.style) {
            // The last-price chip's row, computed up front so the paper
            // chips can dodge it: at the instant a market order fills the
            // entry *is* the last price, and two chips on one pixel mangle
            // the only persistent position statement.
            let reserved_chip_y = if self.layer_visible(ChartLayer::LastPrice, chrome.style) {
                partial
                    .or_else(|| closed.last())
                    .and_then(|bar| bar.close.to_f64())
                    .map(|price| scale.y(price))
                    .filter(|y| *y >= chart_rect.top() && *y <= chart_rect.bottom())
            } else {
                None
            };
            // Tags anchor inside the interactive plot (left of the live
            // lane when one is up) — the same right edge the input pass
            // hands to `handle_chart_input`, so a painted ✕ and its press
            // agree about where it is.
            let tag_right = self.frame.lane_divider_x.unwrap_or(chart_rect.right());
            // Hover affordances paint only on the pane whose pointer feeds
            // the paper input; the others keep display-only tags. Every pane
            // still paints the lines themselves — an order is a fact about
            // the account, true on whichever chart you are looking at.
            let paper_pointer = if chrome.paper_takes_input {
                self.hover_pos.or_else(|| {
                    chrome
                        .paper
                        .forced_hover_pointer(chart_rect, tag_right, &scale)
                })
            } else {
                None
            };
            chrome.paper.draw_layer(
                painter,
                chart_rect,
                tag_right,
                axis_x,
                &scale,
                reserved_chip_y,
                paper_pointer,
            );
            if chrome.paper_hud_here {
                self.paper_hud_anchor = Some((chart_rect, scale));
            }
        }

        self.paint_axis_marks(&frame, axis_x, &levels, &time_claims, chrome);
        if let Some(orderflow) = self.orderflow.as_ref() {
            self.draw_lane_time_axis(
                painter,
                split_time_strip(areas.time_strip, self.frame.lane_divider_x).1,
                orderflow.live_lane_window_ms(closed),
                orderflow.tape_age(),
            );
            // The automatic reference this frame, kept for the tape's menu:
            // the entry that says "follows the bars" has to be able to say
            // what that works out to, and the menu is drawn without the bars
            // in reach. Recorded from the same bars the axis was just drawn
            // from, so the label and the axis can never disagree.
            self.frame.lane_reference_ms = Some(reserved_span_ms(closed));
        }
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
}
