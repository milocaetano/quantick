//! Chart-to-FLOW capture; admission and adoption live in the headless session.
use super::OrderflowView;
use eframe::egui;
use quantick_chart::state::ChartState;
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{
        FlowChunk, FlowProgress, FlowReference, FlowRequest, FlowSession, FlowTapeFrame,
        FlowTapeView, OwnedFlowExecution,
    },
};
use rust_decimal::{Decimal, prelude::FromPrimitive as _};
mod runner;
// Compact painted regions leave the surrounding candle path readable.
const FLOW_RADIUS_LIMIT_PX: f32 = 7.0;
// Merge regional neighbours independently of their painted quantity radius.
const FLOW_MERGE_SUPPORT_RADIUS_PX: f32 = 12.0;
pub(super) type FlowExecutionView = FlowSession<runner::FlowThread>;
impl OrderflowView {
    pub(crate) fn reset_flow_executions(&mut self) {
        self.flow_execution.clear();
    }
    pub(crate) fn flow_execution_active(&self) -> bool {
        FlowExecutionView::active(&self.config)
    }
    pub(crate) fn flow_execution_replaces_history(&self) -> bool {
        self.flow_execution.replaces_history(&self.config)
    }
    pub(crate) fn flow_execution_frame(&self) -> Option<&FlowTapeFrame> {
        self.flow_execution.frame()
    }
    pub(crate) fn flow_execution_progress(&self) -> FlowProgress {
        self.flow_execution.progress()
    }
    pub(crate) fn ignore_flow_opening(&self) -> bool {
        self.flow_execution.ignore_opening()
    }
    pub(crate) fn set_ignore_flow_opening(&mut self, value: bool) -> bool {
        self.flow_execution.set_ignore_opening(value)
    }
    pub(crate) fn project_flow_executions(
        &mut self,
        state: &ChartState,
        slots: std::ops::Range<usize>,
        pixels: egui::Vec2,
        range: (f64, f64),
        clip: (f64, f64),
    ) {
        if slots.is_empty() || !self.flow_execution_active() || state.tick_membership().is_none() {
            self.flow_execution.clear();
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
        let keep = self.flow_execution.keep_for(
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
            view: FlowTapeView {
                clip_left: Decimal::from_f64(clip.0).unwrap_or_default(),
                clip_right: Decimal::from_f64(clip.1).unwrap_or_default(),
                first_slot: slots.start,
                end_slot: slots.end,
                width_px: pixels.x,
                height_px: pixels.y,
                prices,
                reference: FlowReference::VisibleRegions,
                radius_limit: FLOW_RADIUS_LIMIT_PX,
                merge_support_radius: FLOW_MERGE_SUPPORT_RADIUS_PX,
                exclude_opening: self.flow_execution.ignore_opening(),
            },
        };
        self.flow_execution.project(request, |range| FlowChunk {
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
}

#[cfg(test)]
#[path = "flow_execution/tests/view.rs"]
mod tests;
