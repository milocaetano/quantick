//! Chart-to-FLOW capture; admission and adoption live in the headless session.
use super::OrderflowView;
use eframe::egui;
use quantick_chart::state::ChartState;
use quantick_orderflow::projection::flow_tape::{FlowProgress, FlowSession, FlowTapeFrame};
mod runner;
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
    pub(crate) fn flow_execution_handle(&self) -> Option<&std::sync::Arc<FlowTapeFrame>> {
        self.flow_execution.frame_handle()
    }
    pub(crate) fn flow_execution_progress(&self) -> FlowProgress {
        self.flow_execution.progress()
    }
    pub(crate) fn ignore_flow_opening(&self) -> bool {
        self.flow_execution.ignore_opening()
    }
    pub(crate) fn set_ignore_flow_opening(&mut self, value: bool) -> bool {
        let changed = self.flow_execution.set_ignore_opening(value);
        if changed {
            self.look.note_edit();
        }
        changed
    }
    pub(crate) fn project_flow_executions(
        &mut self,
        state: &ChartState,
        slots: std::ops::Range<usize>,
        pixels: egui::Vec2,
        range: (f64, f64),
        clip: (f64, f64),
    ) {
        quantick_chart::flow_execution::project_flow_executions(
            self.flow_execution_active(),
            state,
            &mut self.flow_execution,
            slots,
            (pixels.x, pixels.y),
            range,
            clip,
        );
    }
}

#[cfg(test)]
#[path = "flow_execution/tests/view.rs"]
mod tests;
