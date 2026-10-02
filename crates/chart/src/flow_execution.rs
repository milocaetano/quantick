//! Canonical chart membership capture for the asynchronous regional FLOW session.
use crate::state::ChartState;
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{
        FlowChunk, FlowReference, FlowRequest, FlowRunner, FlowSession, FlowTapeView,
        OwnedFlowExecution,
    },
};
use rust_decimal::{Decimal, prelude::FromPrimitive as _};

// Compact marks and a separate spatial support preserve the surrounding candle path.
const FLOW_RADIUS_LIMIT_PX: f32 = 12.0;
// A small geometric support pools unresolved neighbours without letting the
// largest volume in another region determine which executions belong together.
const FLOW_MERGE_SUPPORT_RADIUS_PX: f32 = 6.0;

/// Capture canonical membership when the caller enables regional FLOW.
pub fn project_flow_executions<R: FlowRunner>(
    enabled: bool,
    state: &ChartState,
    session: &mut FlowSession<R>,
    slots: std::ops::Range<usize>,
    pixels: (f32, f32),
    range: (f64, f64),
    clip: (f64, f64),
) {
    if !enabled || slots.is_empty() || state.tick_membership().is_none() {
        session.clear();
        return;
    }
    let Some(prices) = Decimal::from_f64(range.0)
        .zip(Decimal::from_f64(range.1))
        .and_then(|(low, high)| PriceWindow::new(low, high))
    else {
        return;
    };
    let membership = state.tick_membership().unwrap();
    let span = |slots: std::ops::Range<usize>| {
        membership
            .range(slots.start)
            .map_or(state.trades().len(), |range| range.start)
            ..slots
                .end
                .checked_sub(1)
                .and_then(|slot| membership.range(slot))
                .map_or(state.trades().len(), |range| range.end)
    };
    let epoch = state.series_revision();
    let keep = session.keep_for(
        epoch,
        slots.clone(),
        state.bars().len() + usize::from(state.partial().is_some()),
        span,
    );
    let request = FlowRequest {
        epoch,
        layout_revision: 0,
        source_count: state.trades().len(),
        requested: span(slots.clone()),
        keep,
        opening_windows: membership.opening_windows().to_vec(),
        opening_ordinals: membership.opening_ordinals().collect(),
        view: FlowTapeView {
            clip_left: Decimal::from_f64(clip.0).unwrap_or_default(),
            clip_right: Decimal::from_f64(clip.1).unwrap_or_default(),
            first_slot: slots.start,
            end_slot: slots.end,
            width_px: pixels.0,
            height_px: pixels.1,
            prices,
            reference: FlowReference::VisibleRegions,
            radius_limit: FLOW_RADIUS_LIMIT_PX,
            merge_support_radius: FLOW_MERGE_SUPPORT_RADIUS_PX,
            exclude_opening: session.ignore_opening(),
        },
    };
    session.project(request, |range| FlowChunk {
        epoch,
        ticks_per_bar: state.spec().parameter(),
        executions: state
            .trades()
            .range(range.clone())
            .enumerate()
            .filter_map(|(offset, trade)| {
                let ordinal = range.start + offset;
                let (slot, accepted_ordinal) = membership.locate(ordinal)?;
                Some(OwnedFlowExecution {
                    ordinal,
                    slot,
                    accepted_ordinal,
                    trade: trade.clone(),
                })
            })
            .collect(),
    });
}

/// Factual regional price on the current chart axis, including inverted views.
pub fn flow_price_y(
    prices: PriceWindow,
    price: Decimal,
    top: f32,
    height: f32,
    inverted: bool,
) -> f32 {
    let normalized = prices.y_unclamped(price).unwrap_or_default() as f32;
    top + height
        * if inverted {
            1.0 - normalized
        } else {
            normalized
        }
}

#[cfg(test)]
#[path = "flow_execution_tests.rs"]
mod tests;
