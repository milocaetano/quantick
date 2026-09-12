//! The painters [`ChartPane::draw_chart`] calls that read the frame and write
//! nothing back: the axis claims, the candles, the overlays, the trade marks,
//! the axis marks and dividers, and the canvas chrome.
//!
//! Each is a `&self` method taking the [`DrawFrame`] by reference and the
//! values it needs beside it; each body is the block that ran inline in
//! `draw_chart`, at the same indentation, so the paint order and the per-frame
//! work are exactly what they were. A painter that has to write a field — the
//! footprint's level of detail, the indicator panes' auto range, the paper
//! HUD's anchor — stayed in the orchestrator, because the slices of the series
//! are borrowed across the frame and a `&mut self` method cannot run under
//! them.

use std::sync::Arc;

use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;

use crate::bands::Bands;
use crate::candle_view::draw_candle;
use crate::chart;
use crate::chart_layers::ChartLayer;
use crate::indicator_render::{self, PlotX};
use crate::orderflow_view::OrderflowView;
use crate::plot_area::split_time_strip;
use crate::pointer_compass;
use crate::style::CandleStyle;
use crate::theme;
use crate::toolrail::Tool;
use quantick_orderflow::engine::VisibleOrderflow;

use super::draw_chart::DrawFrame;
use super::tape_switch::TAPE_SWITCH_RESERVED_PX;
use super::{
    ChartPane, DrawPass, PaneChrome, PointerCompass, PriceAxisLevel, draw_live_chip, live_chip_rect,
};

/// Font size, in points, of the "nothing in view" line drawn where the candles
/// would be. Matches the "connecting…" line: same voice, same weight.
const EMPTY_VIEW_FONT_SIZE: f32 = 16.0;

/// How much of a sidebar candle's lane its *body* takes, as a fraction of the
/// half-lane.
///
/// Seven tenths, so the body reads as a body and the wick still shows either
/// side of it. Derived from the lane rather than fixed, so widening the lane
/// widens the candle instead of leaving a wider gap around the same sliver.
const SIDEBAR_BODY_FRAC: f32 = 0.35;

/// What one frame's axes stand aside for, as `axis_claims` decides it.
///
/// Three fields with names rather than a tuple: the two claim lists are the
/// same type, and a tuple would let the price axis's chips and the time
/// strip's be swapped by a `let` that still compiles.
pub(super) struct AxisChips {
    pub(super) compass: Option<PointerCompass>,
    pub(super) price: pointer_compass::AxisClaims,
    pub(super) time: pointer_compass::AxisClaims,
}

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
        let compass = self.pointer_compass(chart_rect, right, total, &scale, chrome);
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
        let mut time_claims = pointer_compass::AxisClaims::new();
        if let Some(compass) = compass.as_ref() {
            if compass.price {
                price_claims.extend(on_axis(compass.readout.position.y));
            }
            if compass.time {
                time_claims.extend(
                    pointer_compass::time_tag(painter, history_strip, &compass.readout)
                        .map(|(centre, _)| centre),
                );
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
        // of view would leave a hole at the top of the axis.
        if self.layer_visible(ChartLayer::LastPrice, chrome.style)
            && let Some(bar) = partial.or_else(|| closed.last())
            && let Some(price) = bar.close.to_f64()
        {
            price_claims.extend(on_axis(scale.y(price)));
        }
        AxisChips {
            compass,
            price: price_claims,
            time: time_claims,
        }
    }

    /// The candles' own pass: the heat cleared behind each body, the
    /// under-candles drawings on the price band, then one candle per visible
    /// bar, dressed for the footprint style that will paint over them.
    ///
    /// `carved` is the pane's band buffer, taken by the caller and carved
    /// here on the price band only — see the comment inside for why that is a
    /// correctness bound and not an optimisation.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_candles(
        &self,
        frame: &DrawFrame<'_>,
        clip: &egui::Painter,
        carved: &mut Bands,
        orderflow_frame: Option<&Arc<VisibleOrderflow>>,
        half: f32,
        candle_lane: f32,
        content_half: f32,
        candles: &CandleStyle,
    ) {
        let &DrawFrame {
            painter,
            areas,
            right,
            total,
            scale,
            closed_start,
            closed_total,
            partial_visible,
            visible_prefix,
            visible_state,
            canvas_background,
            ..
        } = frame;
        let viewport = &self.viewport;
        let visible_closed = || visible_prefix.iter().chain(visible_state);
        // Every candle this frame draws, in order: its bar index, the bar
        // itself, and whether it is still forming. One bar, one candle — the
        // law `Viewport::candle_width` states — so this is simply the visible
        // bars, borrowed.
        let visible_candles = |paint: &mut dyn FnMut(usize, &quantick_engine::Bar, bool)| {
            for (offset, bar) in visible_closed().enumerate() {
                paint(closed_start + offset, bar, false);
            }
            if let Some(partial) = partial_visible {
                paint(closed_total, partial, true);
            }
        };
        // Clear the heat behind each candle's high–low span so a translucent
        // candle stays a clean divider — no liquidity band shows through it.
        // Where the price swept, the wall reads as consumed; bands survive only
        // in the gaps between candles and above/below each bar.
        if orderflow_frame.is_some()
            && self
                .orderflow
                .as_ref()
                .is_some_and(OrderflowView::depth_visible)
        {
            let clear_bar = |xc: f32, bar: &quantick_engine::Bar| {
                let (top, bottom) = scale.band(
                    bar.high.to_f64().unwrap_or(0.0),
                    bar.low.to_f64().unwrap_or(0.0),
                );
                clip.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(xc - half, top),
                        egui::pos2(xc + half, bottom),
                    ),
                    egui::Rounding::ZERO,
                    canvas_background,
                );
            };
            visible_candles(&mut |index, bar, _forming| {
                clear_bar(viewport.x_center(index, right, total), bar);
            });
        }
        // Objects that are *context* rather than annotation go down here,
        // between the liquidity map and the candles: a volume profile is read
        // the way the heatmap is, and drawn over the price it tints every body
        // it covers.
        //
        // The **price band only**, and that is a correctness bound rather than
        // an optimisation. An indicator band's scale is written when its own
        // curve draws, further down this function, so a band carved here would
        // be a frame behind the plot it belongs to — which is exactly the
        // invariant the over-candles carve says it exists to keep. The price
        // band has no such dependency, so it is the one band that can be
        // carved this early and still be right. A tool wanting a background
        // pass on an indicator band would need its own carve after that pane
        // draws; there is none, and inventing a stale one for it would be
        // worse than not offering it.
        self.carve_bands(areas, carved);
        if let Some(price_band) = carved.iter().next() {
            self.draw_drawings(painter, price_band, 0, right, total, DrawPass::UnderCandles);
        }
        // Asked once for the whole frame: on a chart where no indicator paints
        // — every chart until a script calls `barcolor` — the per-bar lookup
        // below never runs at all.
        let painted = self.indicators.paints_any();
        visible_candles(&mut |index, bar, forming| {
            let xc = viewport.x_center(index, right, total);
            // Plot rows map 1:1 onto bars (see `PlotX`), and one bar is one
            // candle at every zoom, so a drawn candle covers exactly its own
            // row.
            let paint = painted
                .then(|| self.indicators.slot_paint(index..index + 1, forming))
                .flatten();
            // A sidebar candle moves into the lane the footprint left it at
            // the slot's left edge; every other case draws where it always
            // did, at full body width. One call, two geometries — never a
            // second candle path, which would drift from this one.
            let slot = if candle_lane > 0.0 {
                // A third of the lane each side, so the body is a body and the
                // wick still has room to show either side of it.
                let sliver = (candle_lane * SIDEBAR_BODY_FRAC).max(1.0);
                crate::candle_view::BarSlot {
                    xc: xc - content_half + sliver + 1.0,
                    half_width: sliver,
                }
            } else {
                crate::candle_view::BarSlot {
                    xc,
                    half_width: half,
                }
            };
            draw_candle(clip, slot, &scale, bar, forming, candles, paint);
        });
    }

    /// Overlay indicator plots and their draw objects, on the candles' own
    /// clip, scale and x-mapping — after the candles, before the bubbles.
    pub(super) fn paint_overlays(
        &self,
        frame: &DrawFrame<'_>,
        clip: &egui::Painter,
        plot_x: &PlotX<'_>,
    ) {
        let &DrawFrame {
            scale,
            start,
            end,
            closed_total,
            prefix,
            closed,
            partial,
            partial_visible,
            ..
        } = frame;
        // Slot -> (high_y, low_y) in pixels, for above/below-bar markers.
        let bar_extents = |slot: usize| -> Option<(f32, f32)> {
            let bar = if slot < prefix.len() {
                prefix.get(slot)
            } else if slot < closed_total {
                closed.get(slot - prefix.len())
            } else if slot == closed_total {
                partial
            } else {
                None
            }?;
            Some((
                scale.y(chart::to_f64(bar.high)),
                scale.y(chart::to_f64(bar.low)),
            ))
        };
        indicator_render::draw_overlays(
            clip,
            self.indicators.visible_overlays(),
            plot_x,
            &scale,
            start,
            end,
            partial_visible.map(|_| closed_total),
            &bar_extents,
        );
        // Draw objects (lines/boxes/labels) share the overlays' paint slot:
        // after candles, before aggression bubbles.
        for view in self.indicators.visible_overlays() {
            indicator_render::draw_objects(
                clip,
                view.render_objects(),
                plot_x,
                |v| scale.y(v),
                start,
                end,
            );
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
            };
            // The window once, not once per fill: `draw` asks about every
            // closed round trip of the session, twice each, every frame.
            let covered = self.covered_window();
            crate::trade_paint::draw(
                &frame,
                chrome.paper.session_trades(),
                chrome.paper.account().selected_trade_index(),
                |ms| {
                    covered
                        .filter(|(oldest, newest)| ms >= *oldest && ms <= *newest)
                        .and_then(|_| self.slot_at_time(ms))
                },
                |slot| self.viewport.x_center(slot, right, total),
            );
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
        time_claims: &pointer_compass::AxisClaims,
        chrome: &PaneChrome<'_>,
    ) {
        let &DrawFrame {
            painter,
            areas,
            chart_rect,
            history_rect,
            total,
            start,
            end,
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
        self.draw_axis_marks(
            painter,
            chart_rect,
            axis_x,
            &scale,
            levels,
            partial.or_else(|| closed.last()),
            chrome,
        );
        // The candles' own marks, so they are placed and clipped in their
        // pane: where venue candles give way to bars built from prints, and
        // where backfilled prints give way to live ones.
        if self.layer_visible(ChartLayer::SeamDivider, chrome.style) {
            self.draw_seam_divider(painter, history_rect, total, cw);
        }
        if self.layer_visible(ChartLayer::BackfillDivider, chrome.style) {
            self.draw_backfill_divider(painter, history_rect, total, cw);
        }
        // Under the same switch as the venue seam: both answer "what is the
        // provenance of the bars either side of this line?", and a trader who
        // turned that class of mark off meant this one too.
        if self.layer_visible(ChartLayer::SeamDivider, chrome.style) {
            self.draw_feed_gaps(painter, history_rect, total, cw, chrome.feed_gaps);
        }
        self.draw_time_strip(
            painter,
            areas.time_strip,
            start,
            end,
            total,
            time_claims,
            chrome,
        );
    }

    /// The last marks on the canvas: the jump-to-live chip, the empty-view
    /// notice, the crosshair, the pointer compass, the status badge and the
    /// tape switch, in that order so each sits on top of the ones before it.
    pub(super) fn paint_canvas_chrome(
        &self,
        frame: &DrawFrame<'_>,
        axis_x: f32,
        nothing_in_view: bool,
        compass: Option<&PointerCompass>,
        chrome: &PaneChrome<'_>,
    ) {
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
        if !self.viewport.follows_live() {
            let (history_strip, _) = split_time_strip(areas.time_strip, self.frame.lane_divider_x);
            draw_live_chip(painter, live_chip_rect(history_strip));
        }
        // Panned off the data (or a rebuild re-cut the series under the
        // window): the chart is whole — axis, tape, badges — but there is
        // nothing in the candles' pane, so say so and say the way back.
        if nothing_in_view {
            painter.text(
                history_rect.center(),
                egui::Align2::CENTER_CENTER,
                "no bars in view — double-click to return to the live edge",
                egui::FontId::proportional(EMPTY_VIEW_FONT_SIZE),
                theme::TEXT_MUTED,
            );
        }
        if self.layer_visible(ChartLayer::Crosshair, chrome.style) {
            self.draw_crosshair(painter, chart_rect, axis_x, &scale, chrome);
        }
        // Last of the canvas marks, so the answer the trader is asking for by
        // holding the mouse where they are holding it is on top of the ones
        // the chart volunteers. Decided in `axis_claims`, where the axes read it too.
        if let Some(compass) = compass.as_ref() {
            self.draw_pointer_compass(painter, compass, axis_x, areas.time_strip, chrome);
        }
        // The status badge is not a layer: it reports whether the source is
        // healthy, and a chart with every layer off must still say that. It
        // shares the top-right corner with the tape switch, which is drawn last
        // and holds the corner itself.
        if let Some(orderflow) = self.orderflow.as_ref() {
            orderflow.draw_status_badge(painter, chart_rect, TAPE_SWITCH_RESERVED_PX);
        }
        self.draw_tape_switch(painter, chart_rect);
    }
}
