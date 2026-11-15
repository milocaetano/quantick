//! One pane's bar series, owned by the tab session and keyed by the pane's id.
//!
//! Two series compose one slot space: the venue candles folded in front, then
//! the bars the engine cut from the tape. The prefix is kept outside
//! [`ChartState`] on purpose: the state rebuilds its bars from retained trades
//! on every spec change and would eat a prefix kept inside it on the first
//! chip click. Every slot read is written once against the composition, in
//! [`PaneSeriesRead`], so the drawings, the indicators and the control plane
//! read the same slot for the same instant.

use std::collections::BTreeMap;

use quantick_chart::pane_series::PaneSeriesRead;
use quantick_chart::state::{BarConfiguration, ChartState, SpecSelector};
use quantick_chart::venue_history::VenueHistory;
use quantick_engine::trade_tape::TradeTape;
use quantick_engine::{Bar, DealSample, Trade};

use crate::TabSession;

/// One pane's bars: the venue prefix and the trade-derived series behind it.
pub struct PaneSeries {
    state: ChartState,
    history_prefix: VenueHistory,
}

impl std::fmt::Debug for PaneSeries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaneSeries")
            .field("spec", self.state.spec())
            .field("prefix", &self.history_prefix.len())
            .field("bars", &self.state.bars().len())
            .finish()
    }
}

impl PaneSeries {
    /// An empty series cutting by `spec`.
    #[must_use]
    pub fn new(spec: impl Into<BarConfiguration>) -> Self {
        Self {
            state: ChartState::new(spec),
            history_prefix: VenueHistory::default(),
        }
    }

    /// The bars the engine cut from the tape.
    #[must_use]
    pub fn state(&self) -> &ChartState {
        &self.state
    }

    /// See [`Self::state`].
    pub fn state_mut(&mut self) -> &mut ChartState {
        &mut self.state
    }

    /// The venue candles in front of the trade-derived bars.
    #[must_use]
    pub fn prefix(&self) -> &VenueHistory {
        &self.history_prefix
    }

    /// See [`Self::prefix`].
    pub fn prefix_mut(&mut self) -> &mut VenueHistory {
        &mut self.history_prefix
    }

    /// Both halves at once, for a writer that moves one against the other.
    pub fn parts_mut(&mut self) -> (&mut ChartState, &mut VenueHistory) {
        (&mut self.state, &mut self.history_prefix)
    }

    /// The composed reads, against the bar rule the pane's selectors hold.
    #[must_use]
    pub fn read<'a>(&'a self, spec: &'a SpecSelector) -> PaneSeriesRead<'a> {
        PaneSeriesRead {
            history_prefix: self.history_prefix.as_slice(),
            state: &self.state,
            spec,
        }
    }

    /// How many closed slots the series composes: the prefix, then the
    /// engine's closed bars.
    #[must_use]
    pub fn closed_slots(&self) -> usize {
        self.history_prefix.len() + self.state.bars().len()
    }

    /// Every closed bar, prefix first: what an indicator rebuild replays.
    #[must_use]
    pub fn closed_bars(&self) -> Vec<Bar> {
        let mut bars = Vec::with_capacity(self.closed_slots());
        bars.extend_from_slice(&self.history_prefix);
        bars.extend_from_slice(self.state.bars());
        bars
    }

    /// Swap in a recut built beside the frame, handing back the one it replaces.
    pub fn replace_state(&mut self, state: ChartState) -> ChartState {
        std::mem::replace(&mut self.state, state)
    }

    /// Throw away the bars and the prefix, cutting by `spec` from now on;
    /// `keep_readings` keeps the deal-counter readings for a rebuild of the
    /// same market.
    pub fn reset(&mut self, spec: impl Into<BarConfiguration>, keep_readings: bool) {
        self.history_prefix.clear();
        if keep_readings {
            self.state.reset_series(spec);
        } else {
            self.state = ChartState::new(spec);
        }
    }

    /// Fill from the trades another pane of the same market holds, keeping
    /// the backfill/live boundary where it was: a trade streamed live must not
    /// become history because this view was opened late.
    pub fn seed(&mut self, trades: &TradeTape, backfill_count: usize, deals: &[DealSample]) {
        self.state.observe_deals_batch(deals);
        let split = backfill_count.min(trades.len());
        self.state.ingest_backfill(trades.range(..split));
        for trade in trades.since(split) {
            self.state.ingest_live(trade);
        }
    }

    /// Take one live print; answers the engine-bar indices it closed as live
    /// events. Bars the builder cut late behind the backfill boundary (Renko's,
    /// once it reads its step) come back as `history_until`: history the
    /// indicators take as history, never a close handed to a strategy.
    pub fn ingest_live(&mut self, trade: &Trade) -> LiveCloses {
        let before = self.state.bars().len();
        self.state.ingest_live(trade);
        let live_from = before.max(self.state.backfill_boundary().unwrap_or(0));
        LiveCloses {
            history_until: (live_from > before).then_some(live_from),
            closed: live_from..self.state.bars().len(),
        }
    }
}

impl PaneSeries {
    /// This series as reaching back sees it, beside the facts its pane holds.
    #[must_use]
    pub fn view(
        &self,
        rebuild_pending: bool,
        rebuild_failed: bool,
        first_visible_slot: Option<usize>,
    ) -> crate::PaneView<'_> {
        crate::PaneView {
            interval_ms: self.state.spec().time_interval_ms(),
            first_print_open_ms: self.state.first_print_open_ms(),
            first_bar: self.state.bars().first(),
            partial: self.state.partial(),
            rebuild_pending,
            rebuild_failed,
            first_visible_slot,
        }
    }
}

/// What one live print closed — see [`PaneSeries::ingest_live`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveCloses {
    /// Set when bars landed behind the boundary: the engine bars up to here
    /// are history now.
    pub history_until: Option<usize>,
    /// Engine-bar indices closed as live events, oldest first.
    pub closed: std::ops::Range<usize>,
}

/// The tab's panes' series, keyed by `ChartPane.id`.
pub(crate) type SeriesBook = BTreeMap<u64, PaneSeries>;

impl TabSession {
    /// Give pane `id` an empty series cutting by `spec`, keeping one it
    /// already has.
    pub fn open_pane(&mut self, id: u64, spec: impl Into<BarConfiguration>) -> &mut PaneSeries {
        self.series
            .entry(id)
            .or_insert_with(|| PaneSeries::new(spec))
    }

    /// Drop pane `id`'s series with the pane.
    pub fn close_pane(&mut self, id: u64) -> Option<PaneSeries> {
        self.series.remove(&id)
    }

    /// Pane `id`'s series.
    #[must_use]
    pub fn series(&self, id: u64) -> Option<&PaneSeries> {
        self.series.get(&id)
    }

    /// See [`Self::series`].
    pub fn series_mut(&mut self, id: u64) -> Option<&mut PaneSeries> {
        self.series.get_mut(&id)
    }

    /// Every pane's series at once, by id: what a walk over several panes
    /// borrows apart.
    pub fn series_by_id_mut(&mut self) -> impl Iterator<Item = (u64, &mut PaneSeries)> {
        self.series.iter_mut().map(|(id, series)| (*id, series))
    }
}
