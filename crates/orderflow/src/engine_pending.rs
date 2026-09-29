//! Compose an accepted tape suffix with an asynchronous published frame.
use super::{ProjectionRequest, VisibleOrderflow};
use crate::HeatmapConfig;
use crate::projection::{DOT_WINDOW_LADDER_MS, DotZoom, PendingTape, PriceWindow, VolumeDots};
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
            tape_only: true,
            tape_window_ms: DOT_WINDOW_LADDER_MS[0],
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            lane_bars: Vec::new(),
        };
        let dots = VolumeDots::resolve(
            request
                .dot_zoom
                .as_ref()
                .filter(|zoom| zoom.tape_only)
                .unwrap_or(&fallback),
            &request.closed,
            request.partial.as_ref(),
        );
        let prices = PriceWindow::new(
            Decimal::from_f64(request.price_range.0)?,
            Decimal::from_f64(request.price_range.1)?,
        )?;
        let projection = pending.project(
            published.map(|frame| frame.projection.as_ref()),
            config,
            &timeline,
            prices,
            &dots,
        );
        Some(Self {
            projection: Arc::new(projection),
            first_bar_index: request.first_bar_index,
            slot_count: timeline.region_count(),
            live_edge: Some(edge),
            volume_dots: Some(dots.scale(config, request.price_range)),
        })
    }
}
