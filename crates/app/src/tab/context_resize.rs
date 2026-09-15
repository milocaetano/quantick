//! The context stack's height rules and addressed, adjacent-boundary operation.

use eframe::egui;
use quantick_control::wire::ActorKind;
use smallvec::SmallVec;

use super::Tab;
use crate::canvas_layout::{self, MAX_CONTEXT_PANES, PaneKind, PaneWidth, RowAreas};

/// Height choices survive hidden layouts; frame geometry only describes a drawn stack.
#[derive(Default)]
pub(super) struct ContextStack {
    pub heights: SmallVec<[PaneWidth; MAX_CONTEXT_PANES]>,
    pub frame: Option<StackFrame>,
}

pub(super) struct StackFrame {
    pub column: egui::Rect,
    pub pane_ids: SmallVec<[u64; MAX_CONTEXT_PANES]>,
}

/// Stable identities prevent a delayed operation from resizing replacement panes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResizeContextPair {
    pub upper_pane_id: u64,
    pub lower_pane_id: u64,
    pub wanted_y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ContextPairResult {
    pub fraction: f32,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContextResizeError {
    NotDrawn,
    NotAdjacent,
    InvalidCoordinate,
}

impl std::fmt::Display for ContextResizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotDrawn => "the current context stack must be visible and drawn before resizing",
            Self::NotAdjacent => "the named panes are not an adjacent upper/lower context pair",
            Self::InvalidCoordinate => "the requested divider coordinate must be finite",
        })
    }
}

impl Tab {
    /// Current geometry from the same splitter the painter uses. Old layouts
    /// and reordered/replaced panes cannot lend their rectangles to an action.
    pub(crate) fn context_stack_geometry(&self) -> Option<(egui::Rect, RowAreas)> {
        let frame = self.context_stack.frame.as_ref()?;
        let count = self
            .layout
            .kinds()
            .iter()
            .filter(|kind| **kind == PaneKind::Time)
            .count()
            .min(self.time_panes.len());
        if !self.shows_context_charts()
            || count < 2
            || count != frame.pane_ids.len()
            || !frame
                .pane_ids
                .iter()
                .zip(&self.time_panes)
                .all(|(id, pane)| *id == pane.id)
            || frame.column.height() <= 0.0
        {
            return None;
        }
        Some((
            frame.column,
            canvas_layout::split_column(frame.column, &self.context_stack.heights[..count]),
        ))
    }

    /// Pointer and admitted remote callers share this operation and its floors.
    pub(crate) fn resize_context_pair(
        &mut self,
        request: ResizeContextPair,
        actor: ActorKind,
    ) -> Result<ContextPairResult, ContextResizeError> {
        let (column, bands) = self
            .context_stack_geometry()
            .ok_or(ContextResizeError::NotDrawn)?;
        let frame = self
            .context_stack
            .frame
            .as_ref()
            .ok_or(ContextResizeError::NotDrawn)?;
        let index = frame
            .pane_ids
            .windows(2)
            .position(|pair| pair == [request.upper_pane_id, request.lower_pane_id])
            .ok_or(ContextResizeError::NotAdjacent)?;
        let result = self
            .context_stack
            .resize(index, request.wanted_y, column, &bands.dividers)?;
        if result.changed {
            tracing::info!(target: "quantick::app", event_code = "LAYOUT_CONTEXT_PAIR_RESIZED",
                tab_id = self.id, upper_pane_id = request.upper_pane_id,
                lower_pane_id = request.lower_pane_id, ?actor,
                fraction = result.fraction, "a context boundary was resized");
        }
        Ok(result)
    }
}

impl ContextStack {
    fn resize(
        &mut self,
        index: usize,
        wanted_y: f32,
        column: egui::Rect,
        dividers: &[egui::Rect],
    ) -> Result<ContextPairResult, ContextResizeError> {
        if !wanted_y.is_finite() {
            return Err(ContextResizeError::InvalidCoordinate);
        }
        let pair_top = if index == 0 {
            column.top()
        } else {
            dividers[index - 1].center().y
        };
        let pair_bottom = dividers
            .get(index + 1)
            .map_or(column.bottom(), |divider| divider.center().y);
        let pair_height = pair_bottom - pair_top;
        // Height buys the pane-local footer too, so it cannot consume the chart's floor.
        let floor = canvas_layout::MIN_PANE_WIDTH_PX + crate::layout_strip::STRIP_HEIGHT;
        let previous_y = dividers[index].center().y;
        let wanted_y = if pair_height >= floor * 2.0 {
            wanted_y.clamp(pair_top + floor, pair_bottom - floor)
        } else {
            previous_y
        };
        let changed = (wanted_y - previous_y).abs() > f32::EPSILON;
        if changed {
            let mut previous = column.top();
            for slot in 0..=dividers.len() {
                let boundary = if slot == dividers.len() {
                    column.bottom()
                } else if slot == index {
                    wanted_y
                } else {
                    dividers[slot].center().y
                };
                self.heights[slot] =
                    PaneWidth::Manual(((boundary - previous) / column.height()).max(0.0));
                previous = boundary;
            }
        }
        Ok(ContextPairResult {
            fraction: (wanted_y - column.top()) / column.height(),
            changed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizing_one_pair_keeps_its_neighbor_and_short_columns_stay_put() {
        for height in [1500.0, 150.0] {
            let column =
                egui::Rect::from_min_size(egui::pos2(10.0, 80.0), egui::vec2(600.0, height));
            let mut stack = ContextStack {
                heights: smallvec::smallvec![PaneWidth::Auto; 3],
                frame: None,
            };
            let before = canvas_layout::split_column(column, &stack.heights);
            let result = stack
                .resize(0, column.bottom(), column, &before.dividers)
                .unwrap();
            let after = canvas_layout::split_column(column, &stack.heights);
            assert!((before.dividers[1].center().y - after.dividers[1].center().y).abs() < 0.001);
            if height < 300.0 {
                assert!(!result.changed);
                assert_eq!(before.dividers, after.dividers);
            } else {
                assert!(result.changed);
                let floor = canvas_layout::MIN_PANE_WIDTH_PX + crate::layout_strip::STRIP_HEIGHT;
                assert!(
                    (after.dividers[1].center().y - after.dividers[0].center().y - floor).abs()
                        < 0.001
                );
            }
        }
    }
}
