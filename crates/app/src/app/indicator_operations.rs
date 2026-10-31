//! Worker and view effects for the headless indicator registry.
use crate::indicator_worker::{IndicatorCommand, IndicatorSource, SlotId};
use crate::pane::ChartPane;
use quantick_chart::indicator_registry::IndicatorHost;

impl IndicatorHost for ChartPane {
    fn pane_id(&self) -> u64 {
        self.id
    }
    fn add(&mut self, source: IndicatorSource) -> SlotId {
        self.add_indicator(source)
    }

    fn remove(&mut self, slot: SlotId) {
        // Render-side removal precedes the worker command; in-flight events
        // for a removed slot are discarded by the existing views owner.
        self.indicators.remove(slot);
        self.indicator_worker.send(IndicatorCommand::Remove(slot));
    }
}

pub(super) struct IndicatorViewAdapter<'a>(pub &'a mut super::ArrangementHost);

impl quantick_chart::indicator_registry::IndicatorViewHost for IndicatorViewAdapter<'_> {
    fn view_mut(
        &mut self,
        target: super::TabSlot,
    ) -> Option<&mut quantick_chart::indicator_views::IndicatorView> {
        if target.workspace != self.0.owner() {
            return None;
        }
        self.0
            .by_id_mut(target.tab)?
            .pane_at_mut(target.side.index())
            .filter(|pane| pane.id == target.pane)?
            .indicators
            .view_mut(target.slot)
    }
}
