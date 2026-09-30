//! The context stack's height rules and addressed, adjacent-boundary operation.

use eframe::egui;
use quantick_control::wire::ActorKind;
use smallvec::SmallVec;

use super::Tab;
use crate::canvas_layout::{self, MAX_CONTEXT_PANES, PaneKind, PaneWidth, RowAreas};
use crate::pane::PaneSide;

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
    pub(crate) fn pane_collapsed(&self, side: PaneSide) -> bool {
        match side {
            PaneSide::Flow => self.flow_collapsed && self.layout.shows_time(),
            PaneSide::Time(slot) => {
                (self.context_collapsed && self.layout.shows_flow())
                    || self
                        .context_stack
                        .heights
                        .get(slot)
                        .is_some_and(|height| height.is_collapsed())
            }
        }
    }

    /// Persist the trader's vertical sizing without tying the document to egui.
    pub(crate) fn context_height_shares(&self) -> Vec<f32> {
        self.context_stack
            .heights
            .iter()
            .map(|height| match *height {
                PaneWidth::Auto => 0.0,
                PaneWidth::Manual(share) | PaneWidth::Collapsed { restore: share } => share,
            })
            .collect()
    }

    pub(crate) fn context_collapsed_slots(&self) -> Vec<bool> {
        self.context_stack
            .heights
            .iter()
            .map(|height| height.is_collapsed())
            .collect()
    }

    pub(crate) fn expand_context_stack(&mut self) {
        if self.context_stack.heights.len() == 2
            && let Some((slot, restore)) =
                self.context_stack
                    .heights
                    .iter()
                    .enumerate()
                    .find_map(|(slot, height)| match *height {
                        PaneWidth::Collapsed { restore } => Some((slot, restore)),
                        _ => None,
                    })
        {
            self.context_stack.heights[slot] = PaneWidth::Manual(restore);
            self.context_stack.heights[1 - slot] = PaneWidth::Manual(1.0 - restore);
            return;
        }
        for height in &mut self.context_stack.heights {
            if let PaneWidth::Collapsed { restore } = *height {
                *height = PaneWidth::Manual(restore);
            }
        }
    }

    pub(crate) fn restore_context_heights(&mut self, shares: &[f32], collapsed: &[bool]) {
        self.context_stack.heights.clear();
        for (slot, share) in shares.iter().copied().take(MAX_CONTEXT_PANES).enumerate() {
            let size = if !share.is_finite() || !(0.0..=1.0).contains(&share) {
                PaneWidth::Auto
            } else if collapsed.get(slot).copied().unwrap_or(false) && share > 0.0 {
                PaneWidth::Collapsed { restore: share }
            } else if share > 0.0 {
                PaneWidth::Manual(share)
            } else {
                PaneWidth::Auto
            };
            self.context_stack.heights.push(size);
        }
        if !self.context_stack.heights.is_empty()
            && self
                .context_stack
                .heights
                .iter()
                .all(|height| height.is_collapsed())
        {
            self.context_stack.heights[0] = PaneWidth::Auto;
        }
    }

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
        tab_id: u64,
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
                tab_id = tab_id, upper_pane_id = request.upper_pane_id,
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
        if pair_height < floor + canvas_layout::COLLAPSED_PANE_WIDTH_PX {
            return Ok(ContextPairResult {
                fraction: (previous_y - column.top()) / column.height(),
                changed: false,
            });
        }
        let upper = self.heights[index];
        let lower = self.heights[index + 1];
        let upper_share = (previous_y - pair_top) / column.height();
        let lower_share = (pair_bottom - previous_y) / column.height();
        let pair_share = pair_height / column.height();
        let count = self.heights.len();
        let collapsed_share = |slot: usize| {
            let dividers = usize::from(slot > 0) + usize::from(slot + 1 < count);
            (canvas_layout::COLLAPSED_PANE_WIDTH_PX
                + dividers as f32 * canvas_layout::CANVAS_DIVIDER_PX / 2.0)
                / column.height()
        };
        let collapse_upper = wanted_y - pair_top < canvas_layout::COLLAPSE_AT_PX;
        let collapse_lower = pair_bottom - wanted_y < canvas_layout::COLLAPSE_AT_PX;
        if collapse_upper && !upper.is_collapsed() && !lower.is_collapsed() {
            self.heights[index] = PaneWidth::Collapsed {
                restore: upper_share,
            };
            self.heights[index + 1] = PaneWidth::Manual(pair_share - collapsed_share(index));
        } else if collapse_lower && !lower.is_collapsed() && !upper.is_collapsed() {
            self.heights[index] = PaneWidth::Manual(pair_share - collapsed_share(index + 1));
            self.heights[index + 1] = PaneWidth::Collapsed {
                restore: lower_share,
            };
        } else if !collapse_upper && !collapse_lower {
            // A short column can hold one open pane and a rail but cannot
            // satisfy two full floors. Reopening must still be reachable.
            let open_floor = if pair_height >= floor * 2.0 {
                floor
            } else {
                canvas_layout::COLLAPSE_AT_PX
            };
            let applied = wanted_y.clamp(pair_top + open_floor, pair_bottom - open_floor);
            self.heights[index] = PaneWidth::Manual((applied - pair_top) / column.height());
            self.heights[index + 1] = PaneWidth::Manual((pair_bottom - applied) / column.height());
        }
        let changed = self.heights[index] != upper || self.heights[index + 1] != lower;
        let applied = if changed {
            canvas_layout::split_column(column, &self.heights).dividers[index]
                .center()
                .y
        } else {
            previous_y
        };
        Ok(ContextPairResult {
            fraction: (applied - column.top()) / column.height(),
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
                assert!(stack.heights[1].is_collapsed());
                assert!(after.panes[1].height() <= canvas_layout::COLLAPSED_PANE_WIDTH_PX + 1.0);
            }
        }
    }

    #[test]
    fn either_context_pane_can_collapse_and_reopen_repeatedly() {
        let column = egui::Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(600.0, 900.0));
        let mut stack = ContextStack {
            heights: smallvec::smallvec![PaneWidth::Auto; 2],
            frame: None,
        };
        for slot in [0, 1, 1, 0] {
            let before = canvas_layout::split_column(column, &stack.heights);
            let edge = if slot == 0 {
                column.top()
            } else {
                column.bottom()
            };
            assert!(
                stack
                    .resize(0, edge, column, &before.dividers)
                    .unwrap()
                    .changed
            );
            assert!(stack.heights[slot].is_collapsed());
            let folded = canvas_layout::split_column(column, &stack.heights);
            assert!(
                (folded.panes[slot].height() - canvas_layout::COLLAPSED_PANE_WIDTH_PX).abs() < 1.0
            );
            assert!(folded.panes[1 - slot].height() > 800.0);
            assert!(
                stack
                    .resize(0, column.center().y, column, &folded.dividers)
                    .unwrap()
                    .changed
            );
            assert!(stack.heights.iter().all(|height| !height.is_collapsed()));
            let opened = canvas_layout::split_column(column, &stack.heights);
            assert!(
                opened
                    .panes
                    .iter()
                    .all(|pane| pane.height() >= canvas_layout::MIN_PANE_WIDTH_PX)
            );
        }
    }

    #[test]
    fn short_context_stack_can_reopen_either_rail() {
        let column = egui::Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(600.0, 440.0));
        let mut stack = ContextStack {
            heights: smallvec::smallvec![PaneWidth::Auto; 2],
            frame: None,
        };
        for slot in [0, 1, 1, 0] {
            let before = canvas_layout::split_column(column, &stack.heights);
            let edge = if slot == 0 {
                column.top()
            } else {
                column.bottom()
            };
            assert!(
                stack
                    .resize(0, edge, column, &before.dividers)
                    .unwrap()
                    .changed
            );
            assert!(stack.heights[slot].is_collapsed());
            let folded = canvas_layout::split_column(column, &stack.heights);
            assert!(
                stack
                    .resize(0, column.center().y, column, &folded.dividers)
                    .unwrap()
                    .changed
            );
            assert!(stack.heights.iter().all(|height| !height.is_collapsed()));
            let opened = canvas_layout::split_column(column, &stack.heights);
            assert!(
                opened
                    .panes
                    .iter()
                    .all(|pane| pane.height() > canvas_layout::COLLAPSE_AT_PX)
            );
        }
    }
}
