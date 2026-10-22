//! The order-flow pane facts the bubble and tape rules read, and keeping what
//! they admit: bubble size, the tape fold and the opening-burst scale.
use super::price_axis::{invalid, pane};
use crate::{app::control_host::ControlPort, orderflow_view::OrderflowView};
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::bubble_scale::{BubbleScalePane, BubbleScalePort};
use quantick_control_handlers::{opening_scale::OpeningScalePort, tape_fold::*};
use quantick_control_schema::tape_fold::TapeFold;
use quantick_control_schema::{bubble_scale::BubbleScaleTarget, opening_scale::OpeningScaleTarget};
use quantick_orderflow::HeatmapConfig;
const NO_VIEW: &str = "the requested pane has no order-flow view";
/// The addressed pane's order-flow view, and whether the pane draws tick bars.
fn tape(
    port: &mut dyn ControlPort,
    tab: u64,
    id: u64,
) -> Result<(&mut OrderflowView, bool), ControlError> {
    let pane = pane(port, WireU64::new(tab), WireU64::new(id))?;
    let tick_chart = pane.state.tick_membership().is_some();
    Ok((
        pane.orderflow.as_mut().ok_or_else(|| invalid(NO_VIEW))?,
        tick_chart,
    ))
}
impl BubbleScalePort for dyn ControlPort {
    fn bubble_scale_pane(&mut self, tab: u64, id: u64) -> Result<BubbleScalePane, ControlError> {
        let pane = pane(self, WireU64::new(tab), WireU64::new(id))?;
        pane.bubble_scale_facts().ok_or_else(|| invalid(NO_VIEW))
    }
    fn set_bubble_scale(
        &mut self,
        tab: u64,
        id: u64,
        target: BubbleScaleTarget,
        scale: f32,
    ) -> Result<(bool, f32), ControlError> {
        let (view, target) = (tape(self, tab, id)?.0, target.into());
        Ok((
            view.set_bubble_display_scale(target, scale),
            view.bubble_display_scale(target),
        ))
    }
}
impl TapeFoldPort for dyn ControlPort {
    fn tape_fold_pane(&mut self, tab: u64, id: u64) -> Result<TapeFoldPane, ControlError> {
        let config = tape(self, tab, id)?.0.cached_config();
        let (tape_on, tape_only) = (config.lane_enabled(), config.tape_only());
        Ok(TapeFoldPane { tape_on, tape_only })
    }
    fn set_tape_fold(&mut self, tab: u64, id: u64, fold: TapeFold) -> Result<bool, ControlError> {
        let set = |config: &mut HeatmapConfig| config.live_lane.fold = fold.into();
        Ok(tape(self, tab, id)?.0.edit_config(set))
    }
}
impl OpeningScalePort for dyn ControlPort {
    fn tick_flow_active(&mut self, tab: u64, id: u64) -> Result<bool, ControlError> {
        let (view, tick_chart) = tape(self, tab, id)?;
        Ok(tick_chart && view.flow_execution_active())
    }
    fn set_opening_scale(
        &mut self,
        tab: u64,
        id: u64,
        target: OpeningScaleTarget,
        ignore: bool,
    ) -> Result<bool, ControlError> {
        let view = tape(self, tab, id)?.0;
        let set =
            |config: &mut HeatmapConfig| config.volume_dots.ignore_opening_burst_in_scale = ignore;
        Ok(match target {
            OpeningScaleTarget::Candle => view.set_ignore_flow_opening(ignore),
            OpeningScaleTarget::Tape => view.edit_config(set),
        })
    }
}
