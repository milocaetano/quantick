//! The window's reads for `quantick_control_handlers::health`: its frame cost,
//! and each tab's feed readings, loading work and panes, lent by the tab.

use crate::app::control_host::ControlPort;
use crate::{control::pane_side_dto, orderflow_view::OrderflowView, tab::Tab};
use quantick_chart::loading::LoadingTracker;
use quantick_control::wire::WireU64;
use quantick_control_handlers::health::{
    FrameReading, HealthScopePort, HealthTab, PaneHealthRead, TabVisit,
};
use quantick_control_schema::health::{FeedIntegritySnapshot, IntakeLagSnapshot};
use quantick_feed::FeedLatency;

impl HealthScopePort for dyn ControlPort {
    const HIGH_LAG_MS: i64 = crate::metrics::HIGH_LAG_MS;
    fn frame(&self) -> FrameReading {
        self.health_reads().frame_metrics()
    }
    fn visit_health_tabs(&self, visit: &mut TabVisit<'_>) {
        let tabs = self.tab_reads().tabs().iter_with_ids();
        visit(&mut tabs.map(|(tab_id, tab)| (tab_id, tab as &dyn HealthTab)));
    }
}

impl HealthTab for Tab {
    fn feed_integrity(&self) -> FeedIntegritySnapshot {
        let integrity = self.link.feed_integrity;
        FeedIntegritySnapshot {
            anomalies: WireU64::new(integrity.anomalies),
            missing_messages: WireU64::new(integrity.missing_messages),
            unknown_loss: WireU64::new(integrity.unknown_loss),
            non_monotonic: WireU64::new(integrity.non_monotonic),
        }
    }
    fn loading(&self) -> &LoadingTracker {
        &self.loading
    }
    fn arrival_latency_ms(&self) -> Option<i64> {
        self.link.trade_arrival_ms()
    }
    fn latency_split(&self) -> Option<FeedLatency> {
        self.link.latency_split()
    }
    fn intake_lag(&self) -> Option<IntakeLagSnapshot> {
        self.link.intake_lag.summary().map(Into::into)
    }
    fn trade_gate(&self) -> Option<&quantick_feed::sync::Desync> {
        self.paper.trade_gate()
    }
    fn visit_panes(&self, visit: &mut dyn FnMut(&mut dyn Iterator<Item = PaneHealthRead<'_>>)) {
        visit(&mut self.panes().map(|(pane, side)| PaneHealthRead {
            pane_id: pane.id,
            side: pane_side_dto(side),
            indicators: &pane.indicators,
            orderflow: pane.orderflow.as_ref().map(OrderflowView::cached_health),
        }));
    }
}

/// A tab lending its readings to the owner, on a fake window with no
/// application behind it.
#[cfg(test)]
mod port_tests {
    use super::*;
    use crate::app::{TabsPort, control_host::tests::fake::FakeWindow};

    #[test]
    fn a_fresh_tab_lends_its_panes_and_no_tape_or_anomaly() {
        let window = FakeWindow::new();
        let reads = window.tab_reads();
        let (_, tab) = reads.tabs().iter_with_ids().next().expect("one tab");
        assert_eq!(tab.feed_integrity().anomalies.get(), 0);
        assert_eq!(
            (tab.arrival_latency_ms(), tab.latency_split()),
            (None, None)
        );
        let mut panes = Vec::new();
        tab.visit_panes(&mut |all| panes.extend(all.map(|pane| pane.pane_id)));
        let ids: Vec<u64> = tab.panes().map(|(pane, _)| pane.id).collect();
        assert_eq!(panes, ids);
    }
}
