//! One projection policy for retained tape history and stateless previews.

use std::borrow::Cow;

use super::{
    AggressionPrimitive, PastTape, PastTapeMemory, PriceWindow, TapeDotFrame, TapeDotGeometry,
    TapeDotMemory, TapeDotView, TapeFacts, TapeOverlay, TapeSource, draws_bubble, merge_tape_dots,
    position_tape_at,
};
use crate::LiveEdge;
use crate::config::theme::OrderflowRenderStyle;

/// Merge a frame's tape into the dots a painter draws.
///
/// `marks` are the frame's marks as the painter would draw them. A sealed
/// published tape (`facts` with a seal) is read from its cells instead, only
/// where they can have changed.
pub fn project_tape_frame<'a>(
    marks: impl Into<Cow<'a, [AggressionPrimitive]>>,
    memory: Option<&mut TapeDotMemory>,
    style: &OrderflowRenderStyle,
    geometry: TapeDotGeometry,
    time: Option<(LiveEdge, i64)>,
    prices: Option<PriceWindow>,
    facts: Option<&TapeFacts>,
) -> Option<TapeDotFrame> {
    project_tape_frame_with_overlay(marks, memory, style, geometry, time, prices, facts, None)
}

/// [`project_tape_frame`] for a frame carrying accepted prints beside its
/// published tape ([`crate::engine::VisibleOrderflow::tape_overlay`]): the
/// overlay supersedes the cells it touches. Every path that cannot read the
/// sealed cells reads the complete marks, built from the overlay.
#[allow(clippy::too_many_arguments)]
pub fn project_tape_frame_with_overlay<'a>(
    marks: impl Into<Cow<'a, [AggressionPrimitive]>>,
    memory: Option<&mut TapeDotMemory>,
    style: &OrderflowRenderStyle,
    geometry: TapeDotGeometry,
    time: Option<(LiveEdge, i64)>,
    prices: Option<PriceWindow>,
    facts: Option<&TapeFacts>,
    overlay: Option<&TapeOverlay>,
) -> Option<TapeDotFrame> {
    let sizing = style.dot_sizing?;
    let marks = marks.into();
    if let (Some(memory), Some(time), Some(prices)) = (memory, time, prices) {
        // The accepted prints beside a frame carry their own horizon and
        // opening windows, spanning the published frame's.
        let mut view = tape_view(time, prices, geometry, facts);
        let opening_bursts = match overlay {
            Some(overlay) => {
                view.evicted_through_ms = overlay.evicted_through_ms;
                if style.ignore_opening_burst_in_scale {
                    overlay.opening_bursts.as_slice()
                } else {
                    &[]
                }
            }
            None => opening_bursts(style, facts),
        };
        if let Some(facts) = facts
            && let Some(frame) = memory.project_sealed(
                TapeSource {
                    facts,
                    overlay,
                    style,
                },
                marks.as_ref(),
                view,
                sizing,
                &style.bubbles,
                &style.live_lane,
                opening_bursts,
            )
        {
            return Some(frame);
        }
        return Some(memory.project(
            complete_marks(marks, overlay, style).as_ref(),
            view,
            sizing,
            &style.bubbles,
            &style.live_lane,
            opening_bursts,
        ));
    }
    // Standalone previews have no source lifetime to retain.
    let mut marks = complete_marks(marks, overlay, style).into_owned();
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

/// The past counterpart of [`project_tape_frame`]: the same frame, drawn at a
/// past instant from `memory`'s frozen blocks of `tape`.
#[must_use]
pub fn project_past_tape_frame(
    marks: &[AggressionPrimitive],
    memory: &mut PastTapeMemory,
    tape: &PastTape,
    style: &OrderflowRenderStyle,
    geometry: TapeDotGeometry,
    time: Option<(LiveEdge, i64)>,
    prices: Option<PriceWindow>,
) -> Option<TapeDotFrame> {
    let facts = tape.projection.tape_facts.as_deref();
    Some(memory.project(
        marks,
        tape,
        tape_view(time?, prices?, geometry, facts),
        style.dot_sizing?,
        &style.bubbles,
        &style.live_lane,
        opening_bursts(style, facts),
    ))
}

fn tape_view(
    (edge, dot_window_ms): (LiveEdge, i64),
    prices: PriceWindow,
    geometry: TapeDotGeometry,
    facts: Option<&TapeFacts>,
) -> TapeDotView {
    TapeDotView {
        now_ms: edge.now_ms,
        window_ms: edge.window_ms,
        dot_window_ms,
        evicted_through_ms: facts.and_then(|facts| facts.evicted_through_ms),
        prices,
        geometry,
    }
}

fn opening_bursts<'a>(style: &OrderflowRenderStyle, facts: Option<&'a TapeFacts>) -> &'a [i64] {
    if style.ignore_opening_burst_in_scale {
        facts.map_or(&[], |facts| facts.opening_bursts.as_slice())
    } else {
        &[]
    }
}

/// The marks a frame carrying an overlay stands for, as the painter draws
/// them; `marks` unchanged when it carries none.
fn complete_marks<'a>(
    marks: Cow<'a, [AggressionPrimitive]>,
    overlay: Option<&TapeOverlay>,
    style: &OrderflowRenderStyle,
) -> Cow<'a, [AggressionPrimitive]> {
    match overlay {
        Some(overlay) => Cow::Owned(
            overlay
                .projection()
                .aggressions
                .iter()
                .filter(|mark| draws_bubble(style, mark))
                .cloned()
                .collect(),
        ),
        None => marks,
    }
}
