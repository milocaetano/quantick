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
    pub prefix_slots: usize,
    pub hydrating: bool,
}

impl<'a> From<&'a ChartPane> for PaneFacts<'a> {
    fn from(pane: &'a ChartPane) -> Self {
        Self {
            id: pane.id,
            series: &pane.state,
            drawings: &pane.drawings,
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

impl Observer for crate::app::alerts_runtime::Runtime {
    fn reset(&mut self, _reason: quantick_alerts::ResetReason) {}
    fn source(&mut self, source: Source<'_>) {
        let _ = (
            source.feed,
            source.symbol,
            source.generation,
            source.replay,
            source.connection,
            source.hydrating,
            self.clock_origin,
        );
    }
    fn pane(&mut self, facts: PaneFacts<'_>, observation: Option<Observation<'_>>) {
        let _ = (
            facts.id,
            facts.series,
            facts.drawings,
            facts.prefix_slots,
            facts.hydrating,
        );
        if let Some(observation) = observation {
            let _ = (
                observation.trade,
                observation.received_at_ms,
                observation.close,
            );
        }
    }
}

impl super::Tab {
    pub(crate) fn observe_alert_lifecycle(&self, observer: &mut dyn Observer) {
        observer.source(Source {
            feed: &self.active.0,
            symbol: &self.active.1,
            generation: self.link.feed_generation(),
            connection: self.link.feed_connection,
            hydrating: self.loading.is_active(crate::loading::LoadingTask::History),
            replay: self.link.replay.as_ref(),
        });
        for (pane, _) in self.panes() {
            observer.pane(pane.into(), None);
        }
    }
}
