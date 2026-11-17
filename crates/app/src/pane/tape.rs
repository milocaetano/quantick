//! A chart pane serves its own tape calls: the size rule's facts from its
//! bars and its view, and the gain, fold, view and opening scale its view keeps.
use super::ChartPane;
use crate::orderflow_view::OrderflowView;
use quantick_control::error::ControlError;
use quantick_control_handlers::bubble_scale::{BubbleScalePane, BubbleScalePort};
use quantick_control_handlers::tape_fold::TapeFoldPort;
use quantick_control_handlers::{opening_scale::OpeningScalePort, tape_view_set::TapeViewPort};
use quantick_control_schema::{bubble_scale::BubbleScaleTarget, opening_scale::OpeningScaleTarget};
use quantick_control_schema::{tape_fold::TapeFold, tape_view_set::TapeView};
use quantick_orderflow::HeatmapConfig;
/// The refusal for a pane without an order-flow view.
fn no_view() -> ControlError {
    ControlError::invalid_request("the requested pane has no order-flow view")
}
/// The pane's order-flow view, or the refusal.
fn tape(pane: &mut ChartPane) -> Result<&mut OrderflowView, ControlError> {
    pane.orderflow.as_mut().ok_or_else(no_view)
}
impl BubbleScalePort for ChartPane {
    fn bubble_scale_pane(&self) -> Result<BubbleScalePane, ControlError> {
        self.bubble_scale_facts().ok_or_else(no_view)
    }
    fn set_bubble_scale(
        &mut self,
        target: BubbleScaleTarget,
        scale: f32,
    ) -> Result<(bool, f32), ControlError> {
        let (view, target) = (tape(self)?, target.into());
        Ok((
            view.set_bubble_display_scale(target, scale),
            view.bubble_display_scale(target),
        ))
    }
}
impl TapeFoldPort for ChartPane {
    fn set_tape_fold(&mut self, fold: TapeFold) -> Result<bool, ControlError> {
        Ok(tape(self)?.set_lane_fold(fold.into()))
    }
    fn set_tape_market_fold(&mut self, on: bool) -> Result<bool, ControlError> {
        Ok(tape(self)?.edit_config(|c| c.live_lane.fold_market_closed = on))
    }
}
impl TapeViewPort for ChartPane {
    fn set_tape_view(&mut self, view: TapeView) -> Result<bool, ControlError> {
        Ok(tape(self)?.set_lane_view(view.into()))
    }
}
impl OpeningScalePort for ChartPane {
    fn tick_flow_active(&self) -> Result<bool, ControlError> {
        let tick_chart = self.bubble_scale_pane()?.tick_chart;
        let flow = self
            .orderflow
            .as_ref()
            .is_some_and(OrderflowView::flow_execution_active);
        Ok(tick_chart && flow)
    }
    fn set_opening_scale(
        &mut self,
        target: OpeningScaleTarget,
        ignore: bool,
    ) -> Result<bool, ControlError> {
        let view = tape(self)?;
        let set =
            |config: &mut HeatmapConfig| config.volume_dots.ignore_opening_burst_in_scale = ignore;
        Ok(match target {
            OpeningScaleTarget::Candle => view.set_ignore_flow_opening(ignore),
            OpeningScaleTarget::Tape => view.edit_config(set),
        })
    }
}
