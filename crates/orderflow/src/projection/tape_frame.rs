//! One projection policy for retained tape history and stateless previews.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::{
    AggressionPrimitive, HeatmapProjection, PastTape, PastTapeMemory, PriceWindow, TapeDotFrame,
    TapeDotGeometry, TapeDotMemory, TapeDotView, TapeFacts, TapeOverlay, TapeSource, TapeWork,
    draws_bubble, merge_tape_dots, position_tape_at,
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
        let (view, opening_bursts) = frame_view(time, prices, geometry, style, facts, overlay);
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

/// What [`project_tape_frame_with_overlay`] would ask of `memory` for this
/// frame, measured without doing it, so that work too large for a frame can
/// run where no frame waits for it ([`TapeRebuild`]).
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn tape_frame_work(
    memory: &TapeDotMemory,
    marks: &[AggressionPrimitive],
    style: &OrderflowRenderStyle,
    geometry: TapeDotGeometry,
    time: Option<(LiveEdge, i64)>,
    prices: Option<PriceWindow>,
    facts: Option<&TapeFacts>,
    overlay: Option<&TapeOverlay>,
) -> TapeWork {
    let (Some(sizing), Some(time), Some(prices)) = (style.dot_sizing, time, prices) else {
        return TapeWork::default();
    };
    let (view, _) = frame_view(time, prices, geometry, style, facts, overlay);
    facts
        .and_then(|facts| {
            memory.sealed_work(
                TapeSource {
                    facts,
                    overlay,
                    style,
                },
                view,
                sizing,
            )
        })
        .unwrap_or_else(|| {
            // The complete path reads the published marks and the pending
            // prints folded onto them.
            let mut work = memory.complete_work(marks, view, sizing);
            work.cells += overlay.map_or(0, |overlay| overlay.cells.len());
            work
        })
}

/// The frame `memory`'s retained groups draw for this frame, reconciling
/// nothing: what stands in while a [`TapeRebuild`] catches the memory up.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn project_retained_tape_frame(
    marks: &[AggressionPrimitive],
    memory: &TapeDotMemory,
    style: &OrderflowRenderStyle,
    geometry: TapeDotGeometry,
    time: Option<(LiveEdge, i64)>,
    prices: Option<PriceWindow>,
    facts: Option<&TapeFacts>,
    overlay: Option<&TapeOverlay>,
) -> Option<TapeDotFrame> {
    let sizing = style.dot_sizing?;
    let (view, opening_bursts) = frame_view(time?, prices?, geometry, style, facts, overlay);
    Some(memory.draw_retained(
        marks,
        view,
        sizing,
        &style.bubbles,
        &style.live_lane,
        opening_bursts,
    ))
}

/// One frame's tape as a painter hands it over: the frame's projection, the
/// style its marks are chosen by, and how the tape is drawn.
#[derive(Clone, Copy)]
pub struct TapeFrameInputs<'a> {
    /// The frame's projection: its aggressions and its tape facts.
    pub projection: &'a Arc<HeatmapProjection>,
    /// The style [`draws_bubble`] chooses the drawn marks by.
    pub bubbles: &'a OrderflowRenderStyle,
    /// The style the tape is sized and merged with.
    pub style: &'a OrderflowRenderStyle,
    pub geometry: TapeDotGeometry,
    pub time: Option<(LiveEdge, i64)>,
    pub prices: Option<PriceWindow>,
    /// The accepted prints beside the published tape.
    pub overlay: Option<&'a Arc<TapeOverlay>>,
}

impl<'a> TapeFrameInputs<'a> {
    /// The marks the painter draws, borrowed when it draws every one.
    #[must_use]
    pub fn marks(&self) -> Cow<'a, [AggressionPrimitive]> {
        let all = &self.projection.aggressions;
        if all.iter().all(|mark| draws_bubble(self.bubbles, mark)) {
            Cow::Borrowed(all.as_slice())
        } else {
            Cow::Owned(
                all.iter()
                    .filter(|mark| draws_bubble(self.bubbles, mark))
                    .cloned()
                    .collect(),
            )
        }
    }

    /// The frame's published tape facts.
    #[must_use]
    pub fn facts(&self) -> Option<&'a TapeFacts> {
        self.projection.tape_facts.as_deref()
    }

    /// The frame with everything it reads owned, for `memory`.
    #[must_use]
    pub fn rebuild(&self, memory: Arc<TapeDotMemory>) -> TapeRebuild {
        TapeRebuild {
            memory,
            abandoned: Arc::default(),
            projection: Arc::clone(self.projection),
            bubbles: self.bubbles.clone(),
            style: self.style.clone(),
            geometry: self.geometry,
            time: self.time,
            prices: self.prices,
            overlay: self.overlay.cloned(),
        }
    }
}

/// One frame's tape projection with everything it reads owned, so the
/// reconciliation it runs on `memory` can happen off the painting thread.
/// [`Self::run`] is [`project_tape_frame_with_overlay`] itself: the memory it
/// returns is exactly the one that frame would have left behind. Nothing in
/// it is copied on the painting thread: the memory and the frame are shared,
/// and copied, if at all, where the rebuild runs.
pub struct TapeRebuild {
    pub memory: Arc<TapeDotMemory>,
    /// Raised when the rebuild can no longer be adopted: it stops early.
    pub abandoned: Arc<AtomicBool>,
    pub projection: Arc<HeatmapProjection>,
    pub bubbles: OrderflowRenderStyle,
    pub style: OrderflowRenderStyle,
    pub geometry: TapeDotGeometry,
    pub time: Option<(LiveEdge, i64)>,
    pub prices: Option<PriceWindow>,
    pub overlay: Option<Arc<TapeOverlay>>,
}

impl TapeRebuild {
    /// Reconcile the memory as the frame would have.
    #[must_use]
    pub fn run(self) -> TapeDotMemory {
        let mut memory = Arc::unwrap_or_clone(self.memory);
        memory.abandon_when(Some(Arc::clone(&self.abandoned)));
        let inputs = TapeFrameInputs {
            projection: &self.projection,
            bubbles: &self.bubbles,
            style: &self.style,
            geometry: self.geometry,
            time: self.time,
            prices: self.prices,
            overlay: self.overlay.as_ref(),
        };
        project_tape_frame_with_overlay(
            inputs.marks(),
            Some(&mut memory),
            &self.style,
            self.geometry,
            self.time,
            self.prices,
            inputs.facts(),
            self.overlay.as_deref(),
        );
        memory.abandon_when(None);
        memory
    }
}

/// The view and opening windows one frame projects with. The accepted prints
/// beside a frame carry their own horizon and opening windows, spanning the
/// published frame's.
fn frame_view<'f>(
    time: (LiveEdge, i64),
    prices: PriceWindow,
    geometry: TapeDotGeometry,
    style: &OrderflowRenderStyle,
    facts: Option<&'f TapeFacts>,
    overlay: Option<&'f TapeOverlay>,
) -> (TapeDotView, &'f [i64]) {
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
    (view, opening_bursts)
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
