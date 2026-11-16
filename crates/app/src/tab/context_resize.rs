//! The context stack as the window draws and addresses it. Its height rules,
//! folds and boundary operation are `quantick_workspace::canvas_layout::stack`'s.

use eframe::egui;
use quantick_control::wire::ActorKind;
use quantick_workspace::canvas::{CanvasRect, PaneMode, seat_mode};
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
        self.layout.shows_time() && !time_panes.is_empty() && !self.folds().0
    }

    /// Folded by the canvas model; while the columns draw, read off its rails.
    pub(crate) fn pane_collapsed(&self, side: PaneSide) -> bool {
        if !self.column_painted() {
            let (seat, seats) = (side.index().checked_sub(1), self.seats.current(&self.state));
            return seat_mode(&self.state, &seats, seat) == Some(PaneMode::Minimized);
        }
        let railed = |seat| {
            let mut held = self.seats.held().iter();
            held.any(|(slot, at)| *at == seat && self.state.railed().contains(slot))
        };
        match side {
            PaneSide::Flow => self.folds().1 && self.layout.shows_time(),
            PaneSide::Time(seat) => (self.folds().0 && self.layout.shows_flow()) || railed(seat),
        }
    }

    /// `op` on the stack as the canvas model folds it: `canvas_layout::on_stack`.
    pub(crate) fn on_stack<R>(&mut self, op: impl FnOnce(&mut ContextStack) -> R) -> R {
        let model = (&mut self.state, &mut self.seats, self.layout.is_column());
        canvas_layout::on_stack(&mut self.context_stack, model, op)
    }

    pub(crate) fn context_height_shares(&self) -> Vec<f32> {
        self.context_stack.height_shares()
    }

    pub(crate) fn context_collapsed_slots(&self) -> Vec<bool> {
        let model = (&self.state, &self.seats, self.layout.is_column());
        canvas_layout::stack_collapsed(&self.context_stack, model)
    }

    pub(crate) fn restore_context_heights(&mut self, shares: &[f32], collapsed: &[bool]) {
        self.on_stack(|stack| stack.restore(shares, collapsed));
    }

    /// Fold the upper or the lower of two stacked charts, as a launch hook asks.
    #[cfg(any(feature = "scenario-harness", test))]
    pub(crate) fn fold_context_pane(&mut self, upper: bool) {
        self.on_stack(|stack| stack.fold_one(upper));
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
        let result =
            self.on_stack(|stack| stack.resize(index, wanted, column, &dividers, footer))?;
        if result.changed {
            tracing::info!(target: "quantick::app", event_code = "LAYOUT_CONTEXT_PAIR_RESIZED",
                tab_id = tab_id, upper_pane_id = request.upper_pane_id,
                lower_pane_id = request.lower_pane_id, ?actor,
                fraction = result.fraction, "a context boundary was resized");
        }
        Ok(result)
    }
}
