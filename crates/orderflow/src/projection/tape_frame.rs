//! One projection policy for retained tape history and stateless previews.

use super::{
    AggressionPrimitive, PriceWindow, TapeDotFrame, TapeDotGeometry, TapeDotMemory, TapeDotView,
    TapeFacts, merge_tape_dots, position_tape_at,
};
use crate::LiveEdge;
use crate::config::theme::OrderflowRenderStyle;

pub fn project_tape_frame(
    mut marks: Vec<AggressionPrimitive>,
    memory: Option<&mut TapeDotMemory>,
    style: &OrderflowRenderStyle,
    geometry: TapeDotGeometry,
    time: Option<(LiveEdge, i64)>,
    prices: Option<PriceWindow>,
    facts: Option<&TapeFacts>,
) -> Option<TapeDotFrame> {
    let sizing = style.dot_sizing?;
    if let (Some(memory), Some((edge, dot_window_ms)), Some(prices)) = (memory, time, prices) {
        return Some(memory.project(
            &marks,
            TapeDotView {
                now_ms: edge.now_ms,
                window_ms: edge.window_ms,
                dot_window_ms,
                evicted_through_ms: facts.and_then(|facts| facts.evicted_through_ms),
                prices,
                geometry,
            },
            sizing,
            &style.bubbles,
            &style.live_lane,
            if style.ignore_opening_burst_in_scale {
                facts.map_or(&[], |facts| facts.opening_bursts.as_slice())
            } else {
                &[]
            },
        ));
    }
    // Standalone previews have no source lifetime to retain.
    if let Some((edge, dot_window_ms)) = time {
        position_tape_at(
            &mut marks,
            edge.now_ms,
            edge.window_ms,
            geometry.left_x,
            dot_window_ms,
        );
    }
    if let Some(prices) = prices {
        for trade in &mut marks {
            trade.y = prices.y_unclamped(trade.price).unwrap_or(trade.y);
        }
    }
    // Offscreen price vertices remain available for clipping the connecting
    // path; only on-screen discs participate in merging and volume sizing.
    marks.retain(|trade| trade.x > geometry.left_x && trade.x <= 1.0);
    let marks = merge_tape_dots(&marks, sizing, &style.bubbles, &style.live_lane, geometry);
    Some(TapeDotFrame {
        full_quantity: sizing
            .full_quantity(marks.iter().filter(|mark| geometry.visible(mark)), true),
        marks,
        max_radius: style.bubbles.max_radius,
    })
}
