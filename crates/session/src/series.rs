//! A pane's two series and the venue prefix installed in front of them.
//!
//! The pane keeps owning its [`PaneSeries`]; this module decides what a
//! change to it does — a venue prefix and its seam lead, older history
//! prepended or recut, a backfill, a seed, a reset, a live print — and hands
//! the window the [`SeriesEffect`]s it then applies in order, so the window
//! only carries those effects out.

use std::ops::Range;

use quantick_chart::bar_progress::BarProgressReading;
use quantick_chart::indicator_views::IndicatorViews;
use quantick_chart::pane_series::PaneSeriesRead;
use quantick_chart::state::{ChartState, SpecSelector};
use quantick_chart::venue_history::VenueHistory;
use quantick_chart::viewport::Viewport;
use quantick_chart_interaction::pane_history::{HistoryState, PrefixChange, PrefixInstallEffect};
use quantick_engine::trade_tape::TradeTape;
use quantick_engine::{Bar, DealSample, Trade};

use crate::SeamTrace;
use crate::strategies::Forming;

/// The bars one pane holds: the venue prefix in front, then the bars the
/// engine cut from prints.
pub struct PaneSeries {
    pub state: ChartState,
    /// Venue candles in front of the trade-derived series, folded to the
    /// pane's interval.
    ///
    /// Deliberately outside `ChartState`, which rebuilds its bars from
    /// retained trades on every spec change and would eat a prefix kept
    /// inside it on the first chip click.
    ///
    /// Non-empty on any pane cutting by a foldable time interval: the split's
    /// time pane, and the flow pane whenever its spec is `BarSpec::Time`
    /// (audit S1). A venue candle has no tape, so the flow layers draw
    /// nothing over it.
    pub history_prefix: VenueHistory,
}

/// What the view must do after its series changed, in the order given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeriesEffect {
    /// A venue-prefix install's view effect; the indicator rebuild every
    /// history change ends in is one of them.
    Prefix(PrefixInstallEffect),
    /// Put the view's right edge back on `slot`, the bar now holding the
    /// market time it showed, `offset` into it.
    ReanchorViewport { slot: Option<usize>, offset: f32 },
    /// Re-hang the drawings by market time, off a series of `old_slots`.
    ReanchorDrawings(usize),
    /// Hand the indicators the first this-many closed slots as history.
    BackfillIndicators(usize),
    /// Hand the indicators the forming bar as it stands now.
    PublishPartial,
    /// Hand the order-flow engine the tape's own price grid.
    PublishTapePriceStep,
}

/// What installing one venue prefix did, for the window to apply.
#[derive(Debug, Clone, PartialEq)]
pub struct PrefixInstall {
    /// Whether the prefix or the seam lead changed.
    pub changed: bool,
    /// The view effects, in the order the window must apply them.
    pub effects: Vec<SeriesEffect>,
}

/// What one live print did to the series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveIngest {
    /// Closed slots the indicators take as history: bars the builder cut late
    /// from prints it held — Renko's, once it reads its step — landed behind
    /// the boundary, and are never a close handed to a strategy.
    pub backfilled: Option<usize>,
    /// Engine indices of the bars the print closed; one print can close
    /// several — a Renko print clearing several levels — and each is an event.
    pub closed: Range<usize>,
}

impl PaneSeries {
    #[must_use]
    pub fn new(state: ChartState) -> Self {
        Self {
            state,
            history_prefix: VenueHistory::default(),
        }
    }

    /// The bar rule the engine cuts this series by, as a config spells it.
    #[must_use]
    pub fn bar_config(&self) -> String {
        self.state.spec().to_config_string()
    }

    /// The composed slot reads over both series; see [`PaneSeriesRead`].
    #[must_use]
    pub fn read<'a>(&'a self, spec: &'a SpecSelector) -> PaneSeriesRead<'a> {
        PaneSeriesRead {
            history_prefix: &self.history_prefix,
            state: &self.state,
            spec,
        }
    }

    fn closed_slots(&self) -> usize {
        self.history_prefix.len() + self.state.bars().len()
    }

    /// Throw the bars away for a market that changed underneath, keeping
    /// the deal-counter readings when `keep_readings` rebuilds the same one.
    ///
    /// The drawings survive it: their re-anchor waits for bars to exist
    /// ([`Self::settle_reanchor`]), because an empty series answers nothing.
    pub fn reset(&mut self, history: &mut HistoryState, spec: &SpecSelector, keep_readings: bool) {
        // A second reset before the first settled must not overwrite the
        // baseline with the empty series it is looking at now.
        history.defer_reanchor(self.read(spec).slots());
        // The prefix is bar-indexed against a series that no longer exists,
        // and its seam was trimmed against a first bar that is gone. A replay
        // never has one today; the invariant must not depend on that.
        self.history_prefix.clear();
        if keep_readings {
            self.state.reset_series(spec.spec());
        } else {
            self.state = ChartState::new(spec.spec());
        }
        history.changed();
    }

    /// The drawings' re-anchor a reset or an empty-series adoption deferred,
    /// once the series holds bars.
    pub fn settle_reanchor(
        &self,
        history: &mut HistoryState,
        spec: &SpecSelector,
    ) -> Option<SeriesEffect> {
        let old_slots = history.settle_reanchor(self.read(spec).slots())?;
        Some(SeriesEffect::ReanchorDrawings(old_slots))
    }

    /// Fill a series opened mid-session from the trades another pane of the
    /// same market already holds, keeping the backfill/live boundary where
    /// it was: a trade streamed live must not become history because this
    /// view was opened late.
    pub fn seed_from(
        &mut self,
        history: &mut HistoryState,
        trades: &TradeTape,
        backfill_count: usize,
        deal_samples: &[DealSample],
    ) -> Vec<SeriesEffect> {
        if !trades.is_empty() {
            history.changed();
        }
        self.state.observe_deals_batch(deal_samples);
        let split = backfill_count.min(trades.len());
        self.state.ingest_backfill(trades.range(..split));
        for trade in trades.since(split) {
            self.state.ingest_live(trade);
        }
        // One rebuild rather than one command per trade: the worker is being
        // handed a whole history, not watching it arrive.
        vec![
            SeriesEffect::PublishTapePriceStep,
            SeriesEffect::Prefix(PrefixInstallEffect::RebuildIndicators),
        ]
    }

    /// Take a backfill batch; the indicators are handed the bars it produced.
    pub fn ingest_backfill(
        &mut self,
        history: &mut HistoryState,
        trades: &[Trade],
    ) -> Vec<SeriesEffect> {
        if !trades.is_empty() {
            history.changed();
        }
        self.state.ingest_backfill(trades);
        vec![
            SeriesEffect::BackfillIndicators(self.closed_slots()),
            SeriesEffect::PublishPartial,
            SeriesEffect::PublishTapePriceStep,
        ]
    }

    /// Take one live print.
    ///
    /// The forming bar is not published here: it changes with every print
    /// and only its latest value is ever used, so the window publishes it
    /// once per drain. Closed bars stay per print.
    pub fn ingest_live(&mut self, trade: &Trade) -> LiveIngest {
        let bars_before = self.state.bars().len();
        self.state.ingest_live(trade);
        let live_from = bars_before.max(self.state.backfill_boundary().unwrap_or(0));
        LiveIngest {
            backfilled: (live_from > bars_before).then(|| self.history_prefix.len() + live_from),
            closed: live_from..self.state.bars().len(),
        }
    }

    /// The bar still forming, for an alarm's preview: its progress where the
    /// spec measures one, and the slot it will close at.
    #[must_use]
    pub fn forming(&self) -> Option<Forming> {
        Some(Forming {
            bar: self.state.partial().cloned()?,
            progress: (self.state.bar_progress_at(None))
                .and_then(BarProgressReading::fixed_progress),
            slot: self.closed_slots(),
        })
    }

    /// Prepend older trades; the net bars they added, and what keeps the
    /// view, the marks and the indicator columns on the bars they were on.
    pub fn prepend_history(
        &mut self,
        history: &mut HistoryState,
        viewport: &Viewport,
        spec: &SpecSelector,
        trades: &[Trade],
    ) -> (usize, Vec<SeriesEffect>) {
        if !trades.is_empty() {
            history.changed();
        }
        let anchor = self.history_anchor(viewport, spec);
        let added = self.state.prepend_history(trades);
        (added, self.history_prepend_effects(spec, added, anchor))
    }

    /// Swap in the history worker's complete recut; the state it replaced,
    /// for the worker to retire, and the same effects as a prepend.
    pub fn install_recut(
        &mut self,
        history: &mut HistoryState,
        viewport: &Viewport,
        spec: &SpecSelector,
        candidate: ChartState,
    ) -> (ChartState, Vec<SeriesEffect>) {
        let anchor = self.history_anchor(viewport, spec);
        let added = candidate
            .bars()
            .len()
            .saturating_sub(self.state.bars().len());
        let old = std::mem::replace(&mut self.state, candidate);
        history.changed();
        (old, self.history_prepend_effects(spec, added, anchor))
    }

    /// The market time under the view's right edge, the offset into that
    /// bar and the slot count, on a series a recut can reshape.
    fn history_anchor(
        &self,
        viewport: &Viewport,
        spec: &SpecSelector,
    ) -> Option<(Option<i64>, f32, usize)> {
        let path_dependent = self.state.spec().definition().path_dependent;
        path_dependent.then(|| self.read(spec).history_view_anchor(viewport))
    }

    fn history_prepend_effects(
        &self,
        spec: &SpecSelector,
        added: usize,
        anchor: Option<(Option<i64>, f32, usize)>,
    ) -> Vec<SeriesEffect> {
        use PrefixInstallEffect::{
            RebuildIndicators, ShiftDrawings, ShiftIndicatorRows, ShiftViewport,
        };
        let mut effects = match anchor {
            // A Renko series re-cut from an older first print can hold a
            // different number of bricks, so the view and the marks go back
            // to market time.
            Some((edge_time, offset, old_slots)) => vec![
                SeriesEffect::ReanchorViewport {
                    slot: edge_time.and_then(|ms| self.read(spec).slot_at_time(ms)),
                    offset,
                },
                SeriesEffect::ReanchorDrawings(old_slots),
            ],
            // Older bars shift every index up by `added`; keep the view steady.
            None => vec![
                SeriesEffect::Prefix(ShiftViewport(added as isize)),
                SeriesEffect::Prefix(ShiftDrawings(added as isize)),
            ],
        };
        // Indicator columns shift with them: the rebuild is a round-trip
        // away, and until it lands every value would otherwise be drawn
        // `added` slots off its own candle. Older trades re-cut every bar,
        // so the indicators replay from scratch.
        effects.extend([
            SeriesEffect::Prefix(ShiftIndicatorRows(added)),
            SeriesEffect::PublishTapePriceStep,
            SeriesEffect::Prefix(RebuildIndicators),
        ]);
        effects
    }

    /// The rebuild owed when the indicator rows disagree with this series'
    /// closed bars, logged beside pane `pane`; said once per disagreement.
    ///
    /// Behind is normal while the worker has work. `settle` says whether it
    /// is idle, taking in what it had already published when it is; idle, a
    /// difference means a rebuild was missed.
    pub fn indicator_row_drift(
        &self,
        pane: u64,
        views: &mut IndicatorViews,
        settle: impl FnOnce(&mut IndicatorViews) -> bool,
    ) -> Option<SeriesEffect> {
        self.row_drift_effect(pane, views, settle)
    }

    pub(crate) fn row_drift_effect<V: IndicatorRows>(
        &self,
        pane: u64,
        views: &mut V,
        settle: impl FnOnce(&mut V) -> bool,
    ) -> Option<SeriesEffect> {
        let bars = self.closed_slots();
        views.row_drift(bars)?;
        if !settle(views) {
            return None;
        }
        let rows = views.answer_row_drift(bars)?;
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "INDICATOR_ROWS_DRIFT",
            pane,
            rows,
            bars,
            action = "rebuild_indicators",
            "indicator rows disagree with the pane's bars"
        );
        Some(SeriesEffect::Prefix(PrefixInstallEffect::RebuildIndicators))
    }

    /// Put `bars` in front of the engine's bars, the venue's seam `lead`
    /// merged into the first one (`ChartState::set_venue_lead`); `covers_seam`
    /// says nothing of that bar's bucket is left unaccounted for. A seam bar
    /// that changed, or says why it holds no lead, is logged beside pane
    /// `pane`.
    pub fn install_venue_prefix(
        &mut self,
        history: &mut HistoryState,
        pane: u64,
        bars: Vec<Bar>,
        lead: Option<Bar>,
        covers_seam: bool,
        seam: Option<SeamTrace>,
    ) -> PrefixInstall {
        let had = seam.and_then(|_| self.state.venue_lead().cloned());
        let lead_changed = self.state.set_venue_lead(lead, covers_seam);
        let change = self.history_prefix.install(bars, history, lead_changed);
        if let Some(seam) = seam {
            log_seam_lead(pane, seam, had.as_ref(), self.state.venue_lead());
        }
        PrefixInstall {
            changed: change != PrefixChange::Unchanged,
            effects: change.install_effects().map(SeriesEffect::Prefix).collect(),
        }
    }
}

/// The indicator rows a series is checked against: the window's
/// [`IndicatorViews`], which own the drift and its once-only answer.
pub(crate) trait IndicatorRows {
    fn row_drift(&mut self, bars: usize) -> Option<usize>;
    fn answer_row_drift(&mut self, bars: usize) -> Option<usize>;
}

impl IndicatorRows for IndicatorViews {
    fn row_drift(&mut self, bars: usize) -> Option<usize> {
        IndicatorViews::row_drift(self, bars)
    }
    fn answer_row_drift(&mut self, bars: usize) -> Option<usize> {
        IndicatorViews::answer_row_drift(self, bars)
    }
}

/// The action a seam line reports, or `None` when the seam bar neither
/// changed nor has a reason to say it holds no lead.
pub(crate) fn seam_lead_action(
    token: &'static str,
    had: Option<&Bar>,
    seated: Option<&Bar>,
) -> Option<&'static str> {
    if had == seated && (seated.is_some() || token == "nothing_before_first_trade") {
        return None;
    }
    Some(if seated.is_some() {
        "merged_into_seam_bar"
    } else {
        token
    })
}

fn log_seam_lead(pane: u64, seam: SeamTrace, had: Option<&Bar>, seated: Option<&Bar>) {
    let Some(action) = seam_lead_action(seam.token, had, seated) else {
        return;
    };
    tracing::info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "SEAM_LEAD",
        pane,
        interval_ms = seam.interval_ms,
        first_print_ms = seam.first_print_ms,
        lead_open_ms = seated.map_or(0, |bar| bar.open_time),
        lead_trades = seated.map_or(0, |bar| bar.trade_count),
        action,
        "the venue's history of the seam bucket before the first trade"
    );
}
