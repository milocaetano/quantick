//! Compose an accepted tape suffix with an asynchronous published frame.
use super::{ProjectionRequest, VisibleOrderflow};
use crate::HeatmapConfig;
use crate::projection::{
    DOT_WINDOW_LADDER_MS, DotZoom, HeatmapProjection, PendingTape, PriceWindow, VolumeDots,
};
use crate::timeline::{BarTimeline, LiveEdge};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;
use std::sync::Arc;

impl VisibleOrderflow {
    /// Complete the tape without waiting for the book worker. Its next frame
    /// replaces the provisional attribution while preserving execution facts.
    pub fn with_pending_tape(
        pending: &PendingTape,
        config: &HeatmapConfig,
        request: &ProjectionRequest,
        published: Option<&Self>,
    ) -> Option<Self> {
        let latest = pending.latest_ms()?;
        let reference_ms = request.lane_reference_ms.unwrap_or(15_000);
        let edge = LiveEdge {
            now_ms: request.lane_now_ms.unwrap_or(latest).max(latest),
            window_ms: config.lane_window_ms(reference_ms),
            reference_ms,
            on_newest_bar: request.on_newest_bar,
        };
        let timeline = BarTimeline::from_bars(
            request.first_bar_index,
            &request.closed,
            request.partial.as_ref(),
            Some(edge),
        )
        .with_full_lane_coverage();
        let fallback = DotZoom {
            native_tape: true,
            tape_window_ms: DOT_WINDOW_LADDER_MS[0],
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            lane_bars: Vec::new(),
        };
        let dots = VolumeDots::resolve(
            request
                .dot_zoom
                .as_ref()
                .filter(|zoom| zoom.native_tape)
                .unwrap_or(&fallback),
            &request.closed,
            request.partial.as_ref(),
        );
        let prices = PriceWindow::new(
            Decimal::from_f64(request.price_range.0)?,
            Decimal::from_f64(request.price_range.1)?,
        )?;
        let mut projection = pending.project(
            published.map(|frame| frame.projection.as_ref()),
            config,
            &timeline,
            prices,
            &dots,
        );
        // The published depth map, liquidity events and candle marks are
        // normalized over the bar slice the worker built them on. Labelling
        // them with this request's slice would shift and stretch them behind
        // the candles on every pan, zoom or new bar until the worker
        // republishes, so the frame keeps that slice and the pending prints
        // move into its lane at the same place in the tape.
        let regions = timeline.region_count();
        let (first_bar_index, slot_count) = match published {
            Some(frame) if frame.live_edge.is_some() && frame.slot_count > 0 => {
                if frame.slot_count != regions {
                    into_lane_of(&mut projection, regions, frame.slot_count);
                }
                (frame.first_bar_index, frame.slot_count)
            }
            _ => (request.first_bar_index, regions),
        };
        Some(Self {
            projection: Arc::new(projection),
            first_bar_index,
            slot_count,
            live_edge: Some(edge),
            volume_dots: Some(dots.scale(config, request.price_range)),
        })
    }
}

/// Move the tape's marks from the lane of a frame split into `from` regions
/// to the lane of one split into `to`: the lane is the last region, and a
/// mark keeps its fraction of it. Only the pending tape's marks are live
/// here; the published candle marks already speak in `to` regions.
fn into_lane_of(projection: &mut HeatmapProjection, from: usize, to: usize) {
    let (from, to) = (from as f64, to as f64);
    let relabel = |x: f64| (to - 1.0 + (x * from - (from - 1.0))) / to;
    for mark in projection.aggressions.iter_mut().filter(|mark| mark.live) {
        mark.x = relabel(mark.x);
    }
    projection.live_now_x = projection.live_now_x.map(relabel);
}
