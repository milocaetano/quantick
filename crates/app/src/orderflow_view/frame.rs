//! The per-frame path of the order-flow view: the projection request the
//! pane sends every frame, and the passes that paint the answer — the
//! depth map behind the candles, the bubbles over them, the key, the
//! book's status badge and the live strip beside the price axis.
//!
//! Each pass builds one [`ProjectedLayout`] and one [`RenderContext`] on
//! the stack from borrowed inputs and hands them to the renderer; nothing
//! here is retained between frames.

use std::sync::Arc;

use eframe::egui;
use quantick_orderbook::BookLevel;
use quantick_orderflow::engine::{CaptureStatus, ProjectionRequest, VisibleOrderflow};
use quantick_orderflow::projection::normalized_area_size;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use crate::chart::PriceScale;
use crate::live_strip;
use crate::orderflow_render::{
    OrderflowRenderStyle, ProjectedLayout, RenderContext, draw_aggression_bubbles,
    draw_compact_legend, draw_heatmap_background, draw_liquidity_events, draw_live_lane_marks,
    theme_bubble_rgb,
};
use crate::orderflow_worker::BookCommand;
use crate::viewport::Viewport;

use super::{OrderflowView, VisibleBarTimeline};

pub(super) fn status_color(status: &CaptureStatus) -> egui::Color32 {
    match status {
        CaptureStatus::Live { .. } => crate::theme::BUY,
        CaptureStatus::Connecting | CaptureStatus::Buffering | CaptureStatus::SnapshotFetching => {
            crate::theme::AMBER
        }
        CaptureStatus::Disabled => crate::theme::TEXT_MUTED,
        CaptureStatus::Resyncing { .. }
        | CaptureStatus::Disconnected { .. }
        | CaptureStatus::Error => crate::theme::WARN,
    }
}

impl OrderflowView {
    /// Request projection of the visible bar slice and return the newest
    /// already-built frame. Never blocks: a heavy projection only delays the
    /// next frame swap, not the UI.
    pub fn project_visible(
        &mut self,
        timeline: VisibleBarTimeline<'_>,
        lane: bool,
        on_newest_bar: bool,
        lane_reference_ms: Option<i64>,
        price_range: (f64, f64),
    ) -> Option<Arc<VisibleOrderflow>> {
        if !self.config.any_layer_enabled() {
            return None;
        }
        self.sync_published();
        let request = ProjectionRequest {
            timeline_revision: timeline.revision,
            first_bar_index: timeline.first_bar_index,
            closed: timeline.closed.to_vec(),
            partial: timeline.partial.cloned(),
            lane,
            on_newest_bar,
            lane_reference_ms,
            price_range,
        };
        // Every frame, with no gate of its own. The worker coalesces requests
        // latest-wins and decides for itself what is worth rebuilding, so the
        // only thing a gate here could add is a bar snapshot older than the
        // prints it is supposed to place — which is how a fresh print ends up
        // outside the timeline and drawn nowhere.
        self.worker.send(BookCommand::Project(request));
        self.published.frame.clone()
    }

    /// Draw resting liquidity, coverage gaps and factual liquidity changes
    /// behind the candle layer. `inverted` is the candles' own orientation,
    /// so the map turns over with the bars it sits behind.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_background(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        viewport: &Viewport,
        total_bars: usize,
        frame: &VisibleOrderflow,
        canvas_background: egui::Color32,
        lane_width_px: f32,
        inverted: bool,
    ) {
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            lane_width_px,
        )
        .with_inverted(inverted);
        let style = OrderflowRenderStyle::from_config(&self.config, canvas_background);
        let context = RenderContext::new(&frame.projection, layout, &style);
        draw_heatmap_background(painter, &context);
        draw_live_lane_marks(painter, &context);
        draw_liquidity_events(painter, &context);
    }

    /// Draw factual aggressive prints over the candles. The canvas's key is
    /// not part of this pass — see [`draw_legend`](Self::draw_legend).
    /// `inverted` is the candles' own orientation, as in
    /// [`draw_background`](Self::draw_background).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_aggressions(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        viewport: &Viewport,
        total_bars: usize,
        frame: &VisibleOrderflow,
        canvas_background: egui::Color32,
        lane_width_px: f32,
        inverted: bool,
    ) {
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            lane_width_px,
        )
        .with_inverted(inverted);
        let style = OrderflowRenderStyle::from_config(&self.config, canvas_background);
        let context = RenderContext::new(&frame.projection, layout, &style);
        draw_aggression_bubbles(painter, &context);
    }

    /// Draw the canvas's compact visual key.
    ///
    /// Its own pass, not a tail of the bubbles: the legend is chrome about
    /// what the canvas is showing — it names the depth layers too — so hiding
    /// the bubbles must not take it down, and hiding it must not take the
    /// bubbles down. The trader switches it from the canvas's right-click
    /// menu (`ChartLayer::FlowLegend`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_legend(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        viewport: &Viewport,
        total_bars: usize,
        frame: &VisibleOrderflow,
        canvas_background: egui::Color32,
        lane_width_px: f32,
        top_inset_px: f32,
    ) {
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            lane_width_px,
        );
        let mut style = OrderflowRenderStyle::from_config(&self.config, canvas_background);
        style.legend_top_inset = top_inset_px;
        let context = RenderContext::new(&frame.projection, layout, &style);
        draw_compact_legend(painter, &context);
    }

    /// `right_inset_px` is the room another piece of chrome has already claimed
    /// in this corner — the tape switch, which sits on the corner itself. The
    /// badge steps left of it rather than under it: two labels on one pixel is
    /// how a status message becomes unreadable exactly when it matters.
    pub fn draw_status_badge(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        right_inset_px: f32,
    ) {
        // Tied to the map, not to the recorder: a badge reporting on a book
        // nobody asked to see is just chrome. The trader can silence it on its
        // own too, from the canvas's right-click menu — but only while the
        // book is working. A failure re-asserts the badge: it is the one
        // real-time statement that the depth on screen has stopped being the
        // book, and hiding chrome may never hide *that* (data honesty).
        let failing = self.published.status.is_failure();
        if !self.config.depth_visible() || (!self.config.show_status_badge && !failing) {
            return;
        }
        let text = self.published.status.label();
        let color = status_color(&self.published.status);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), color);
        let pos = egui::pos2(
            chart_rect.right() - galley.size().x - 10.0 - right_inset_px.max(0.0),
            chart_rect.top() + 4.0,
        );
        let rect = egui::Rect::from_min_size(
            pos - egui::vec2(5.0, 3.0),
            galley.size() + egui::vec2(10.0, 6.0),
        );
        painter.rect_filled(
            rect,
            egui::Rounding::same(3.0),
            egui::Color32::from_black_alpha(165),
        );
        painter.galley(pos, galley, color);
    }

    /// Draw the live strip: the forming bar's aggression histogram, buys
    /// growing rightward from the centre and sells leftward, on the bubbles'
    /// square-root area rule normalized by the bar itself — it resets on bar
    /// close because the new bar has no clusters yet. The published ladder
    /// contributes only the best bid/ask touch lines (the real spread); the
    /// depth silhouette was retired after live use — it repeated the
    /// heatmap's right edge and buried the histogram. `bar_open_ms` is the
    /// forming bar's open time (`None` hides the histogram); `scale` is the
    /// chart's own price scale, so everything lines up 1:1 with the chart.
    pub fn draw_live_strip(
        &mut self,
        painter: &egui::Painter,
        strip: egui::Rect,
        scale: &PriceScale,
        canvas_background: egui::Color32,
        bar_open_ms: Option<i64>,
    ) {
        self.sync_published();
        painter.rect_filled(strip, egui::Rounding::ZERO, canvas_background);
        painter.line_segment(
            [strip.left_top(), strip.left_bottom()],
            egui::Stroke::new(
                1.0_f32,
                crate::theme::TEXT_MUTED.gamma_multiply(live_strip::STRIP_BORDER_ALPHA),
            ),
        );

        let ladder = self.published.ladder.clone();
        let frame = self.published.frame.clone();
        let colors = theme_bubble_rgb(self.config.theme);
        let clip = painter.with_clip_rect(strip);
        let rows_left = strip.left() + live_strip::STRIP_ROW_INSET_PX;

        // The forming bar's mirrored aggression histogram, from the same
        // projection clusters the bubbles draw — one engine, one aggregation
        // path. Empty whenever those layers publish nothing.
        let histogram = match (frame.as_deref(), bar_open_ms) {
            (Some(frame), Some(open_ms)) => live_strip::aggression_rows(
                &frame.projection.aggressions,
                open_ms,
                frame.projection.summarized,
                frame.projection.effective_grouping.bucket_width,
            ),
            _ => Vec::new(),
        };
        if !histogram.is_empty() {
            let bucket_width = frame
                .as_deref()
                .map(|frame| frame.projection.effective_grouping.bucket_width)
                .unwrap_or(self.published.base_price_grouping);
            let reference = live_strip::histogram_reference(&histogram);
            let centre_x = f32::midpoint(rows_left, strip.right());
            let half_width =
                (strip.right() - rows_left) / 2.0 * live_strip::HISTOGRAM_MAX_HALF_FRAC;
            let buy_fill = egui::Color32::from_rgb(colors.buy[0], colors.buy[1], colors.buy[2])
                .gamma_multiply(live_strip::HISTOGRAM_ALPHA);
            let sell_fill = egui::Color32::from_rgb(colors.sell[0], colors.sell[1], colors.sell[2])
                .gamma_multiply(live_strip::HISTOGRAM_ALPHA);
            for row in &histogram {
                // A plain row is one bucket tall; a regional mark declares the
                // whole region it covers. Zero-span marks (older projections)
                // fall back to the bucket width.
                let row_span = if row.price_span > Decimal::ZERO {
                    row.price_span
                } else {
                    bucket_width
                };
                // Ordered on screen, not by price: on an inverted scale the
                // row's higher edge is the lower pixel.
                let a = scale.y((row.price_bucket + row_span).to_f64().unwrap_or(f64::NAN));
                let b = scale.y(row.price_bucket.to_f64().unwrap_or(f64::NAN));
                if !a.is_finite() || !b.is_finite() {
                    continue;
                }
                let (top, bottom) = (a.min(b), a.max(b));
                let buy_extent = normalized_area_size(row.buy, reference) * half_width;
                if buy_extent > 0.0 {
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(centre_x, top),
                        egui::pos2(centre_x + buy_extent, bottom),
                    )
                    .intersect(strip);
                    if rect.is_positive() {
                        clip.rect_filled(rect, egui::Rounding::ZERO, buy_fill);
                    }
                }
                let sell_extent = normalized_area_size(row.sell, reference) * half_width;
                if sell_extent > 0.0 {
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(centre_x - sell_extent, top),
                        egui::pos2(centre_x, bottom),
                    )
                    .intersect(strip);
                    if rect.is_positive() {
                        clip.rect_filled(rect, egui::Rounding::ZERO, sell_fill);
                    }
                }
            }
        }

        // Touch markers last, readable over both layers.
        if let Some(ladder) = &ladder {
            let mark = |price: Option<Decimal>, rgb: [u8; 3]| {
                let Some(price) = price.and_then(|price| price.to_f64()) else {
                    return;
                };
                let y = scale.y(price);
                if !y.is_finite() {
                    return;
                }
                clip.line_segment(
                    [egui::pos2(rows_left, y), egui::pos2(strip.right(), y)],
                    egui::Stroke::new(
                        live_strip::TOUCH_MARKER_STROKE_PX,
                        egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                    ),
                );
            };
            mark(ladder.best_ask.map(BookLevel::price), colors.sell);
            mark(ladder.best_bid.map(BookLevel::price), colors.buy);
        }

        // Honest empty state: the strip is on, no data source has anything —
        // no book (capture off or no snapshot) and no forming-bar aggression.
        if ladder.is_none() && histogram.is_empty() {
            painter.text(
                egui::pos2(strip.center().x, strip.top() + 10.0),
                egui::Align2::CENTER_CENTER,
                "no book",
                egui::FontId::proportional(10.0),
                crate::theme::TEXT_MUTED,
            );
        }
    }
}
