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

use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;

use crate::plot_area::split_time_strip;
use crate::pointer_compass;
use crate::theme;
use crate::toolrail::Tool;
use quantick_layers::ChartLayer;

use super::draw_frame::{AxisChips, DrawFrame};
use super::{
    ChartPane, PaneChrome, PointerCompass, PriceAxisLevel, draw_live_chip, live_chip_rect,
};

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
                    x: &|slot| self.viewport.x_center(slot, right, total),
                });
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
            self.layer_renderers
                .status(&mut super::render_registry::StatusPass {
                    owner: orderflow,
                    painter,
                    rect: chart_rect,
                });
        }
        self.draw_canvas_contributions(painter, chart_rect, chrome.capabilities);
    }
}
