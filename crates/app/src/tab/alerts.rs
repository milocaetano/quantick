//! Borrowed observation docking. No queue, history scan, or rule decisions.
use crate::pane::ChartPane;
use quantick_engine::{Bar, Trade};

/// The actual source, including replay recording and playback state.
pub(crate) struct Source<'a> {
    pub feed: &'a str,
    pub symbol: &'a str,
    pub generation: u64,
    pub connection: quantick_feed::FeedConnectionState,
    pub hydrating: bool,
    pub replay: Option<&'a quantick_feed::ReplayLink>,
}

/// Read-only facts at the exact close/trade boundary. Geometry remains borrowed
/// from the drawing owner until the native adapter projects `DrawingFact`s.
pub(crate) struct PaneFacts<'a> {
    pub id: u64,
    pub series: &'a crate::state::ChartState,
    pub drawings: &'a crate::drawings::Drawings,
    pub drawing_key: Option<&'a quantick_workspace::market_settings::DrawingKey>,
    pub prefix_slots: usize,
    pub hydrating: bool,
}

impl<'a> From<&'a ChartPane> for PaneFacts<'a> {
    fn from(pane: &'a ChartPane) -> Self {
        Self {
            id: pane.id,
            series: &pane.state,
            drawings: &pane.drawings,
            drawing_key: pane.layout.drawings_key.as_ref(),
            prefix_slots: pane.seam_slot(),
            hydrating: pane.history_pending(),
        }
    }
}

pub(crate) struct Observation<'a> {
    pub trade: &'a Trade,
    pub received_at_ms: i64,
    /// Every close, including equal timestamps, has its own composed slot.
    /// `None` is the live trade observation after all its closes.
    pub close: Option<(&'a Bar, usize)>,
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

mod cache;
pub(crate) use cache::Cache;

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
            self.clock_origin.elapsed().as_millis() as u64,
        );
    }
}

pub(crate) fn observe_alert_lifecycle(tab: &super::Tab, observer: &mut dyn Observer) {
    observer.source(Source {
        feed: &tab.active.0,
        symbol: &tab.active.1,
        generation: tab.link.feed_generation(),
        connection: tab.link.feed_connection,
        hydrating: tab.loading.is_active(crate::loading::LoadingTask::History),
        replay: tab.link.replay.as_ref(),
    });
    for (pane, _) in tab.panes() {
        observer.pane(pane.into(), None);
    }
}
