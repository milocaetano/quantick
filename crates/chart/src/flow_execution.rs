//! Canonical chart membership capture for the asynchronous regional FLOW session.
use crate::state::ChartState;
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{
        FlowChunk, FlowReference, FlowRequest, FlowRunner, FlowSession, FlowTail, FlowTapeView,
        OwnedFlowExecution,
    },
};
use rust_decimal::{Decimal, prelude::FromPrimitive as _};

mod chrome;
mod constants;
pub use chrome::{
    FLOW_CHROME_MAX_HEIGHT_PX, FLOW_CHROME_MAX_WIDTH_PX, FlowChromeLayout, SphereChrome,
    flow_chrome_layout, place_flow_chrome,
};
pub use constants::FLOW_EXECUTION_OFFSET;
use constants::{FLOW_MERGE_SUPPORT_RADIUS_PX, FLOW_RADIUS_LIMIT_PX};
mod geometry;
mod labels;
pub use geometry::FlowExecutionGeometry;
pub use labels::{FlowQuantityLabel, place_quantity_labels};
mod disc;
pub use disc::{FlowBounds, FlowDisc};
mod presentation;
pub use presentation::{FlowPresentation, FlowRegionRole, FlowRegionVisual};
mod reuse;
pub use reuse::{FlowRegionKey, FlowRunCache};
mod paint_state;
pub use paint_state::FlowPaintState;

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
    project_styled_flow_executions(
        enabled,
        state,
        session,
        FlowChartView {
            slots,
            pixels,
            range,
            clip,
            tick_bubbles: None,
        },
    );
}

/// The chart camera and its optional scoped tick-bubble appearance.
pub struct FlowChartView {
    pub slots: std::ops::Range<usize>,
    pub pixels: (f32, f32),
    pub range: (f64, f64),
    pub clip: (f64, f64),
    pub tick_bubbles: Option<quantick_orderflow::TickBubbleStyle>,
}

pub fn project_styled_flow_executions<R: FlowRunner>(
    enabled: bool,
    state: &ChartState,
    session: &mut FlowSession<R>,
    camera: FlowChartView,
) {
    let FlowChartView {
        slots,
        pixels,
        range,
        clip,
        tick_bubbles,
    } = camera;
    let Some(membership) = state
        .tick_membership()
        .filter(|_| enabled && !slots.is_empty())
    else {
        session.clear();
        return;
    };
    let Some(prices) = PriceWindow::from_f64_range(range) else {
        return;
    };
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
    carry_recut(state, session, epoch);
    let keep = session.keep_for(
        epoch,
        slots.clone(),
        state.bars().len() + usize::from(state.partial().is_some()),
        span,
    );
    // Tick spheres merge at a held price scale, so an axis fit does not
    // regroup them; the live window still decides what is on the axis.
    let merge_height = match tick_bubbles {
        Some(_) if range.1 > range.0 => {
            let span = range.1 - range.0;
            (session.merge_px_per_price(f64::from(pixels.1) / span) * span) as f32
        }
        _ => pixels.1,
    };
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
            height_px: merge_height,
            prices,
            reference: tick_bubbles.map_or(FlowReference::VisibleRegions, FlowReference::from),
            radius_limit: tick_bubbles
                .map_or(FLOW_RADIUS_LIMIT_PX, |style| style.bubbles().max_radius),
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
    session.note_tail(tail(state));
}

/// The newest print of `state`'s tape, and how much of it is history.
fn tail(state: &ChartState) -> Option<FlowTail> {
    let ordinal = state.trades().len().checked_sub(1)?;
    let trade = &state.trades()[ordinal];
    Some(FlowTail {
        backfill: state.backfill_trade_count(),
        ordinal,
        agg_id: trade.agg_id,
        timestamp_ms: trade.timestamp_ms,
    })
}

/// When `epoch` recut the tape the session last captured — older prints
/// added in front (a History install), every later print where it was —
/// carry the frame on screen into the new series instead of dropping it.
fn carry_recut<R: FlowRunner>(state: &ChartState, session: &mut FlowSession<R>, epoch: u64) {
    let (Some(old), Some(membership)) = (session.tail(), state.tick_membership()) else {
        return;
    };
    let Some(shift) = state.backfill_trade_count().checked_sub(old.backfill) else {
        return;
    };
    let same_tape = old.ordinal + shift < state.trades().len() && {
        let trade = &state.trades()[old.ordinal + shift];
        trade.agg_id == old.agg_id && trade.timestamp_ms == old.timestamp_ms
    };
    if same_tape {
        session.carry_across(epoch, |frame| {
            frame.carried(epoch, state.spec().parameter(), |ordinal| {
                let ordinal = ordinal + shift;
                let (slot, accepted) = membership.locate(ordinal)?;
                Some((ordinal, slot, accepted))
            })
        });
    }
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
