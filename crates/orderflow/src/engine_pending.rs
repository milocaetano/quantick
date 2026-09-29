//! Compose an accepted tape suffix with an asynchronous published frame.
use super::{ProjectionRequest, VisibleOrderflow};
use crate::HeatmapConfig;
use crate::projection::{
    DOT_WINDOW_LADDER_MS, DotZoom, PendingTape, PriceWindow, VolumeDots, into_lane_of, lane_relabel,
};
use crate::timeline::{BarTimeline, LiveEdge};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;
use std::sync::Arc;

/// What a pending frame is placed on: the request's live edge and timeline,
/// the tape's dot windows and the price window.
struct PendingPlace {
    edge: LiveEdge,
    timeline: BarTimeline,
    dots: VolumeDots,
    prices: PriceWindow,
}

impl PendingPlace {
    fn of(
        pending: &PendingTape,
        config: &HeatmapConfig,
        request: &ProjectionRequest,
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
        Some(Self {
            edge,
            timeline,
            dots,
            prices,
        })
    }
}

impl VisibleOrderflow {
    /// Complete the tape without waiting for the book worker. Its next frame
    /// replaces the provisional attribution while preserving execution facts.
    pub fn with_pending_tape(
        pending: &PendingTape,
        config: &HeatmapConfig,
        request: &ProjectionRequest,
        published: Option<&Self>,
    ) -> Option<Self> {
        let place = PendingPlace::of(pending, config, request)?;
        Some(Self::completed(pending, config, request, published, &place))
    }

    /// [`Self::with_pending_tape`] for a painter that keeps the published
    /// tape ([`crate::projection::TapeDotMemory::project_sealed`]): a sealed
    /// published frame is kept as it is, and the suffix travels beside it as
    /// the cells it touches ([`Self::tape_overlay`]), so a frame costs what
    /// the suffix touches rather than what the tape window holds. The
    /// complete frame is built only where the kept one would not draw the
    /// same tape.
    pub fn with_pending_overlay(
        pending: &PendingTape,
        config: &HeatmapConfig,
        request: &ProjectionRequest,
        published: Option<&Self>,
    ) -> Option<Self> {
        let place = PendingPlace::of(pending, config, request)?;
        let regions = place.timeline.region_count();
        if let Some(frame) = published.filter(|frame| {
            frame.live_edge.is_some()
                && frame.slot_count > 0
                && frame.projection.volume_dots
                && request
                    .dot_zoom
                    .as_ref()
                    .is_some_and(|zoom| zoom.native_tape)
        }) {
            let lanes = (frame.slot_count != regions).then_some((regions, frame.slot_count));
            let relabel = lane_relabel(regions, frame.slot_count);
            let now_x = place
                .timeline
                .live_now_position()
                .map(|position| position.normalized)
                .map(|x| if lanes.is_some() { relabel(x) } else { x });
            if now_x == frame.projection.live_now_x
                && let Some(overlay) = pending.overlay(
                    &frame.projection,
                    config,
                    &place.timeline,
                    place.prices,
                    &place.dots,
                    lanes,
                )
            {
                return Some(Self {
                    projection: Arc::clone(&frame.projection),
                    first_bar_index: frame.first_bar_index,
                    slot_count: frame.slot_count,
                    live_edge: Some(place.edge),
                    volume_dots: Some(place.dots.scale(config, request.price_range)),
                    tape_overlay: Some(Arc::new(overlay)),
                });
            }
        }
        Some(Self::completed(pending, config, request, published, &place))
    }

    fn completed(
        pending: &PendingTape,
        config: &HeatmapConfig,
        request: &ProjectionRequest,
        published: Option<&Self>,
        place: &PendingPlace,
    ) -> Self {
        let mut projection = pending.project(
            published.map(|frame| frame.projection.as_ref()),
            config,
            &place.timeline,
            place.prices,
            &place.dots,
        );
        // The published depth map, liquidity events and candle marks are
        // normalized over the bar slice the worker built them on. Labelling
        // them with this request's slice would shift and stretch them behind
        // the candles on every pan, zoom or new bar until the worker
        // republishes, so the frame keeps that slice and the pending prints
        // move into its lane at the same place in the tape.
        let regions = place.timeline.region_count();
        let (first_bar_index, slot_count) = match published {
            Some(frame) if frame.live_edge.is_some() && frame.slot_count > 0 => {
                if frame.slot_count != regions {
                    into_lane_of(&mut projection, regions, frame.slot_count);
                }
                (frame.first_bar_index, frame.slot_count)
            }
            _ => (request.first_bar_index, regions),
        };
        Self {
            projection: Arc::new(projection),
            first_bar_index,
            slot_count,
            live_edge: Some(place.edge),
            volume_dots: Some(place.dots.scale(config, request.price_range)),
            tape_overlay: None,
        }
    }

    /// The projection this frame stands for: the published one with every
    /// pending print placed on it where the tape travels as an overlay, built
    /// on first use. The painter never needs it; any other reader does.
    #[must_use]
    pub fn tape_projection(&self) -> &crate::projection::HeatmapProjection {
        self.tape_overlay
            .as_deref()
            .map_or(&self.projection, crate::projection::TapeOverlay::projection)
    }
}
