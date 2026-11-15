//! The alert observer cache: the window's projected facts in, the reusable
//! policy in `quantick_alerts::observer` out. It never reads a clock or a feed
//! type; the window projects its link and panes into these borrowed facts.
use super::AlertRuntime;
use quantick_alerts::{ObservationKind, ResetReason, Unavailable, observer};
use quantick_engine::{Bar, Trade};
use rust_decimal::prelude::ToPrimitive;

/// The source a tab observes, projected from its feed link, including replay
/// recording and playback state.
pub struct Source<'a> {
    pub feed: &'a str,
    pub symbol: &'a str,
    pub generation: u64,
    /// The live connection is up. A replay source never reads it.
    pub connected: bool,
    pub hydrating: bool,
    /// The replay recording and whether it plays; `None` is the live feed.
    pub replay: Option<(&'a std::path::Path, bool)>,
}

/// Read-only facts at the exact close/trade boundary. Geometry remains borrowed
/// from the drawing owner until the native adapter projects `DrawingFact`s.
pub struct PaneFacts<'a> {
    pub id: u64,
    pub series: &'a quantick_chart::state::ChartState,
    pub drawings: &'a quantick_drawings::Drawings<quantick_drawings::DrawingTool>,
    pub drawing_key: Option<&'a quantick_workspace::market_settings::DrawingKey>,
    pub prefix_slots: usize,
    pub hydrating: bool,
}

pub struct Observation<'a> {
    pub trade: &'a Trade,
    pub received_at_ms: i64,
    /// Every close, including equal timestamps, has its own composed slot.
    /// `None` is the live trade observation after all its closes.
    pub close: Option<(&'a Bar, usize)>,
}

/// App-wide: one cache serves every tab's panes, beside the alert book.
#[derive(Default)]
pub struct Cache(observer::Observer<quantick_engine::bar_registry::BarConfiguration>);
impl Cache {
    pub fn begin(&mut self) {
        self.0.begin();
    }
    pub fn end(&mut self, core: &mut AlertRuntime, utc_ms: i64, session_ms: u64) {
        self.0.end(&mut core.book, utc_ms, session_ms);
    }
    pub fn source(&mut self, source: Source<'_>) {
        let unavailable = if source.hydrating {
            Some(Unavailable::Hydrating)
        } else if source.replay.is_some_and(|(_, playing)| !playing) {
            Some(Unavailable::ReplayPaused)
        } else if source.replay.is_none() && !source.connected {
            Some(Unavailable::SourceInactive)
        } else {
            None
        };
        self.0.source(observer::Source {
            key: source
                .replay
                .map_or(observer::SourceKey::Live(source.feed), |(path, _)| {
                    observer::SourceKey::Replay(path)
                }),
            symbol: source.symbol,
            generation: source.generation,
            availability: unavailable.map_or(Ok(()), Err),
        });
    }
    pub fn reset(&mut self, core: &mut AlertRuntime, reason: ResetReason) {
        self.0.reset(&mut core.book, reason);
    }
    /// `lifecycle_utc_ms` is read only for a lifecycle refresh; a print carries
    /// its own arrival time, so the trade path never asks for a clock.
    pub fn pane(
        &mut self,
        core: &mut AlertRuntime,
        facts: PaneFacts<'_>,
        observation: Option<Observation<'_>>,
        lifecycle_utc_ms: impl FnOnce() -> i64,
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
                    lifecycle_utc_ms(),
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
