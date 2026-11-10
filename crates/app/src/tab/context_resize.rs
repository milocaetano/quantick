//! The context stack as the window draws and addresses it. Its height rules,
//! folds and boundary operation are `quantick_workspace::canvas_layout::stack`'s.

use eframe::egui;
use quantick_control::wire::ActorKind;
use quantick_workspace::canvas::CanvasRect;
pub(crate) use quantick_workspace::canvas_layout::{
    ContextPairResult, ContextResizeError, ContextStack, ResizeContextPair, StackFrame,
};

use super::layout::TabCanvas;
use crate::canvas_layout::{self, PaneKind, RowAreas};
use crate::pane::{ChartPane, PaneSide};

/// The stack's frame for a column drawn at `column` holding `panes`.
/// Record the stack as drawn at `column` holding `panes`, reusing the
/// frame's storage; `None` when the column is not drawn.
pub(super) fn show_stack(
    stack: &mut ContextStack,
    column: Option<egui::Rect>,
    panes: &[ChartPane],
) {
    let Some(at) = column else {
        stack.frame = None;
        return;
    };
    let column = CanvasRect::new(at.left(), at.top(), at.width(), at.height());
    let pane_ids = Vec::new();
    let frame = stack.frame.get_or_insert(StackFrame { column, pane_ids });
    frame.column = column;
    frame.pane_ids.clear();
    frame.pane_ids.extend(panes.iter().map(|pane| pane.id));
}

impl TabCanvas {
    /// See `Tab::shows_context_charts`: held, built, and not collapsed.
    pub(crate) fn shows_context_charts(&self, time_panes: &[ChartPane]) -> bool {
        self.layout.shows_time() && !time_panes.is_empty() && !self.context_collapsed
    }

    pub(crate) fn pane_collapsed(&self, side: PaneSide) -> bool {
        let heights = &self.context_stack.heights;
        match side {
            PaneSide::Flow => self.flow_collapsed && self.layout.shows_time(),
            PaneSide::Time(slot) => {
                (self.context_collapsed && self.layout.shows_flow())
                    || heights
                        .get(slot)
                        .is_some_and(|height| height.is_collapsed())
            }
        }
    }

    pub(crate) fn context_height_shares(&self) -> Vec<f32> {
        self.context_stack.height_shares()
    }

    pub(crate) fn context_collapsed_slots(&self) -> Vec<bool> {
        self.context_stack.collapsed_slots()
    }

    pub(crate) fn expand_context_stack(&mut self) {
        self.context_stack.expand();
    }

    pub(crate) fn restore_context_heights(&mut self, shares: &[f32], collapsed: &[bool]) {
        self.context_stack.restore(shares, collapsed);
    }

    /// Fold the upper or the lower of two stacked charts, as a launch hook asks.
    #[cfg(any(feature = "scenario-harness", test))]
    pub(crate) fn fold_context_pane(&mut self, upper: bool) {
        self.context_stack.fold_one(upper);
    }

    /// Current geometry from the same splitter the painter uses. Old layouts
    /// and reordered/replaced panes cannot lend their rectangles to an action.
    pub(crate) fn context_stack_geometry(
        &self,
        time_panes: &[ChartPane],
    ) -> Option<(egui::Rect, RowAreas)> {
        let frame = self.context_stack.frame.as_ref()?;
        let kinds = self.layout.kinds().iter();
        let count = kinds.filter(|kind| **kind == PaneKind::Time).count();
        let count = count.min(time_panes.len());
        let mut ids = frame.pane_ids.iter().zip(time_panes);
        let same = count == frame.pane_ids.len() && ids.all(|(id, pane)| *id == pane.id);
        if !self.shows_context_charts(time_panes) || count < 2 || !same || frame.column.h <= 0.0 {
            return None;
        }
        let at = frame.column;
        let column = egui::Rect::from_min_size(egui::pos2(at.x, at.y), egui::vec2(at.w, at.h));
        let heights = &self.context_stack.heights[..count];
        Some((column, canvas_layout::split_column(column, heights)))
    }

    /// Pointer and admitted remote callers share this operation and its floors.
    pub(crate) fn resize_context_pair(
        &mut self,
        tab_id: u64,
        request: ResizeContextPair,
        actor: ActorKind,
        time_panes: &[ChartPane],
    ) -> Result<ContextPairResult, ContextResizeError> {
        let (column, bands) = self
            .context_stack_geometry(time_panes)
            .ok_or(ContextResizeError::NotDrawn)?;
        let index = self.context_stack.pair_index(&request)?;
        let column = CanvasRect::new(column.left(), column.top(), column.width(), column.height());
        let dividers: Vec<_> = (bands.dividers.iter())
            .map(|at| (at.top(), at.bottom()))
            .collect();
        let footer = crate::layout_strip::STRIP_HEIGHT;
        let wanted = request.wanted_y;
        let result = (self.context_stack).resize(index, wanted, column, &dividers, footer)?;
        if result.changed {
            tracing::info!(target: "quantick::app", event_code = "LAYOUT_CONTEXT_PAIR_RESIZED",
                tab_id = tab_id, upper_pane_id = request.upper_pane_id,
                lower_pane_id = request.lower_pane_id, ?actor,
                fraction = result.fraction, "a context boundary was resized");
        }
        Ok(result)
    }
}
