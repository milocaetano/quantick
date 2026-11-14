//! Native facts only; reusable observation policy lives in `quantick_alerts`.
use super::{Observation, PaneFacts, Source};
use quantick_alerts::{ObservationKind, ResetReason, Unavailable, observer};
use quantick_stores::alerts::AlertRuntime;
use rust_decimal::prelude::ToPrimitive;

#[derive(Default)]
pub(crate) struct Cache(observer::Observer<quantick_engine::bar_registry::BarConfiguration>);
impl Cache {
    pub(crate) fn begin(&mut self) {
        self.0.begin();
    }
    pub(crate) fn end(&mut self, core: &mut AlertRuntime, utc_ms: i64, session_ms: u64) {
        self.0.end(&mut core.book, utc_ms, session_ms);
    }
    pub(crate) fn source(&mut self, source: Source<'_>) {
        let unavailable = if source.hydrating {
            Some(Unavailable::Hydrating)
        } else if source
            .replay
            .is_some_and(|replay| !replay.status.is_playing())
        {
            Some(Unavailable::ReplayPaused)
        } else if source.replay.is_none()
            && source.connection != quantick_feed::FeedConnectionState::Connected
        {
            Some(Unavailable::SourceInactive)
        } else {
            None
        };
        self.0.source(observer::Source {
            key: source
                .replay
                .map_or(observer::SourceKey::Live(source.feed), |replay| {
                    observer::SourceKey::Replay(&replay.session.path)
                }),
            symbol: source.symbol,
            generation: source.generation,
            availability: unavailable.map_or(Ok(()), Err),
        });
    }
    pub(crate) fn reset(&mut self, core: &mut AlertRuntime, reason: ResetReason) {
        self.0.reset(&mut core.book, reason);
    }
    pub(crate) fn pane(
        &mut self,
        core: &mut AlertRuntime,
        facts: PaneFacts<'_>,
        observation: Option<Observation<'_>>,
        session_ms: u64,
    ) {
        let (kind, close_slot, price, utc_ms) = observation.map_or_else(
            || {
                (
                    ObservationKind::Lifecycle,
                    None,
                    facts
                        .series
                        .partial()
                        .or_else(|| facts.series.bars().last())
                        .and_then(|bar| bar.close.to_f64()),
                    crate::metrics::wall_clock_ms(),
                )
            },
            |observation| {
                let (kind, close_slot, price) = observation.close.map_or_else(
                    || {
                        (
                            ObservationKind::Trade,
                            None,
                            observation.trade.price.to_f64(),
                        )
                    },
                    |(bar, slot)| {
                        (
                            ObservationKind::CommittedClose,
                            Some(slot),
                            bar.close.to_f64(),
                        )
                    },
                );
                (kind, close_slot, price, observation.received_at_ms)
            },
        );
        self.0.pane(
            &mut core.book,
            &mut core.events,
            observer::Pane {
                id: facts.id,
                spec: facts.series.spec(),
                drawing_revision: facts.drawings.revision(),
                hydrating: facts.hydrating,
            },
            observer::Observation {
                kind,
                prefix_slots: facts.prefix_slots,
                committed_slots: facts.series.bars().len(),
                has_partial: facts.series.partial().is_some(),
                close_slot,
                price,
                utc_ms,
                session_ms,
            },
            || facts.series.spec().to_config_string(),
            facts
                .drawings
                .items()
                .iter()
                .filter_map(|drawing| quantick_drawings::alerts::fact(drawing, facts.drawing_key)),
        );
    }
}
