//! Borrowed observation docking. No queue, history scan, or rule decisions.
use crate::pane::ChartPane;
pub(crate) use quantick_stores::alerts::{Observation, PaneFacts, Source};

/// Pane facts at the boundary; the cache that reads them lives in `stores`.
/// A function, not `From`: a trait impl on the public `PaneFacts` would make
/// `ChartPane` effectively public and trip `private_interfaces`.
pub(crate) fn facts(pane: &ChartPane) -> PaneFacts<'_> {
    PaneFacts {
        id: pane.id,
        series: pane.series_state(),
        drawings: &pane.drawings,
        drawing_key: pane.layout.drawings_key.as_ref(),
        prefix_slots: pane.seam_slot(),
        hydrating: pane.history_pending(),
    }
}

pub(crate) trait Observer {
    fn reset(&mut self, reason: quantick_alerts::ResetReason);
    fn source(&mut self, source: Source<'_>);
    /// `None` refreshes lifecycle availability without evaluating a price.
    fn pane(&mut self, facts: PaneFacts<'_>, observation: Option<Observation<'_>>);
}

impl Observer for () {
    fn reset(&mut self, _: quantick_alerts::ResetReason) {}
    fn source(&mut self, _: Source<'_>) {}
    fn pane(&mut self, _: PaneFacts<'_>, _: Option<Observation<'_>>) {}
}

impl Observer for crate::app::alerts_runtime::Runtime {
    fn reset(&mut self, reason: quantick_alerts::ResetReason) {
        self.cache.reset(&mut self.core, reason);
    }
    fn source(&mut self, source: Source<'_>) {
        self.cache.source(source);
    }
    fn pane(&mut self, facts: PaneFacts<'_>, observation: Option<Observation<'_>>) {
        self.cache.pane(
            &mut self.core,
            facts,
            observation,
            crate::metrics::wall_clock_ms,
            self.clock_origin.elapsed().as_millis() as u64,
        );
    }
}

/// The feed projection: the link's connection and replay state, as facts.
pub(crate) fn observe_alert_lifecycle(tab: &super::Tab, observer: &mut dyn Observer) {
    observer.source(Source {
        feed: &tab.active.0,
        symbol: &tab.active.1,
        generation: tab.link.feed_generation(),
        connected: tab.link.feed_connection == quantick_feed::FeedConnectionState::Connected,
        hydrating: tab.loading.is_active(crate::loading::LoadingTask::History),
        replay: (tab.link.replay.as_ref())
            .map(|replay| (replay.session.path.as_path(), replay.status.is_playing())),
    });
    for (pane, _) in tab.panes() {
        observer.pane(facts(pane), None);
    }
}
