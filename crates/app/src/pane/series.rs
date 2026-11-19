//! The pane's series: the bars it holds, where each one sits in market time
//! and in slot space, how new trades and venue history arrive, and what the
//! indicator worker is told about every change.
//!
//! Two series compose one slot space — the venue prefix first, then the bars
//! the engine cut from prints — and every lookup is written once against that
//! composition, in `quantick_chart::pane_series`, so the drawings, the
//! indicators and the control plane read the same slot for the same instant.

use crate::indicator_worker::{IndicatorCommand, IndicatorSource, SlotId};
use crate::price_view::PriceView;

use super::ChartPane;
use quantick_chart_interaction::pane_history::PrefixInstallEffect;
use quantick_session::series::SeriesEffect;

impl ChartPane {
    // The slot reads, each one line onto the composed series that owns its
    // rule and its documentation: `quantick_chart::pane_series::PaneSeriesRead`.
    pub fn slots(&self) -> usize {
        self.series_read().slots()
    }
    pub fn closed_slots(&self) -> usize {
        self.series_read().closed_slots()
    }
    pub fn seam_slot(&self) -> usize {
        self.series_read().seam_slot()
    }
    pub fn partial_bucket_slot(&self) -> Option<usize> {
        self.series_read().partial_bucket_slot()
    }
    pub fn closed_bar(&self, slot: usize) -> Option<&quantick_engine::Bar> {
        self.series_read().closed_bar(slot)
    }
    pub fn slot_open_time(&self, slot: usize) -> Option<i64> {
        self.series_read().slot_open_time(slot)
    }
    pub fn slot_at_time(&self, ms: i64) -> Option<usize> {
        self.series_read().slot_at_time(ms)
    }
    pub fn covering_slot_at_time(&self, ms: i64) -> Option<usize> {
        self.series_read().covering_slot_at_time(ms)
    }
    pub(super) fn covered_window(&self) -> Option<(i64, i64)> {
        self.series_read().covered_window()
    }
    pub fn right_edge_time(&self) -> Option<i64> {
        self.series_read().right_edge_time(&self.model.viewport)
    }

    /// Reserve a slot and ask the worker to instantiate `source` behind it.
    pub fn add_indicator(&mut self, source: IndicatorSource) -> SlotId {
        let slot = self.indicators.allocate_slot(source.kind_id());
        self.indicator_worker
            .send(IndicatorCommand::Add { slot, source });
        slot
    }

    /// Ask the worker to replay the chart's bars from scratch — the one
    /// command behind spec switches, prepended history and source resets, so
    /// indicators inherit correct behavior for every rebuild path.
    pub fn send_indicator_rebuild(&mut self) {
        if let Some(owner) = self.orderflow.as_mut() {
            owner.reset_flow_executions();
        }
        self.replay_indicators();
    }

    /// Replay the bars into the indicators alone: a ladder refold's need.
    fn replay_indicators(&mut self) {
        self.lane.reset();
        self.indicator_worker.send(IndicatorCommand::Rebuild(
            self.series_read().closed_footprint_bars(),
            self.series_read().partial_footprint_bar(),
        ));
        self.publish_partial();
    }

    /// Apply what [`quantick_session::series::PaneSeries::install_venue_prefix`] decided to
    /// the view; whether the prefix or the seam lead changed.
    pub fn install_venue_history(
        &mut self,
        bars: Vec<quantick_engine::Bar>,
        lead: Option<quantick_engine::Bar>,
        covers_seam: bool,
        seam: Option<quantick_session::SeamTrace>,
    ) -> bool {
        let (history, id) = (&mut self.model.history, self.id);
        let install = self
            .series
            .install_venue_prefix(history, id, bars, lead, covers_seam, seam);
        self.apply_series_effects(install.effects);
        install.changed
    }

    /// Carry out what the pane's series decided, in order.
    pub(crate) fn apply_series_effects(&mut self, effects: impl IntoIterator<Item = SeriesEffect>) {
        use PrefixInstallEffect::{
            RebuildIndicators, ShiftDrawings, ShiftIndicatorRows, ShiftViewport,
        };
        for effect in effects {
            match effect {
                SeriesEffect::Prefix(ShiftViewport(delta)) => {
                    self.model.viewport.shift_right_edge(delta)
                }
                SeriesEffect::Prefix(ShiftDrawings(delta)) => self.marks_mut().shift_bars(delta),
                SeriesEffect::Prefix(ShiftIndicatorRows(added)) => {
                    self.indicators.shift_rows(added)
                }
                SeriesEffect::Prefix(RebuildIndicators) => self.send_indicator_rebuild(),
                SeriesEffect::ReanchorViewport { slot, offset } => {
                    self.model.viewport.reanchor_history(slot, offset)
                }
                SeriesEffect::ReanchorDrawings(old_slots) => self.reanchor_drawings(old_slots),
                SeriesEffect::BackfillIndicators(slots) => {
                    self.lane.reset();
                    let mut history = self.series_read().closed_footprint_bars();
                    history.truncate(slots);
                    self.indicator_worker
                        .send(IndicatorCommand::Backfilled(history));
                }
                SeriesEffect::PublishPartial => self.publish_partial(),
                SeriesEffect::PublishTapePriceStep => self.publish_tape_price_step(),
                SeriesEffect::ReplayIndicators => self.replay_indicators(),
            }
        }
    }

    /// Apply the indicator worker's deltas, before the draw reads columns.
    pub fn apply_indicator_events(&mut self) {
        self.drain_indicator_events();
        let worker = &mut self.indicator_worker;
        let rebuild = self
            .series
            .indicator_row_drift(self.id, &mut self.indicators, |views| {
                let counts = worker.progress().counts;
                let idle = counts.queued + counts.inflight + counts.parked + counts.deferred == 0;
                if idle {
                    worker
                        .drain_events()
                        .into_iter()
                        .for_each(|event| views.apply(event));
                }
                idle
            });
        self.apply_series_effects(rebuild);
    }

    fn drain_indicator_events(&mut self) {
        for event in self.indicator_worker.drain_events() {
            self.indicators.apply(event);
        }
    }

    /// Hand the tape's own price grid — and the magnitude it prints at — to
    /// the order-flow engine, which sizes the capture bucket, and through it
    /// the footprint ladder's rows and the volume profile folded from them.
    ///
    /// Called after every ingest because the answer can only arrive after
    /// prints have. The view sends a command only when the answer changes, and
    /// a running GCD changes it a handful of times per session at most.
    fn publish_tape_price_step(&mut self) {
        // The engine first: a time pane has none, `panes_mut` fans every print
        // to it too, and a tuple scrutinee would read the grid before finding
        // that out — paying a read per print on a pane that can never use it.
        let Some(orderflow) = self.orderflow.as_mut() else {
            return;
        };
        if let Some(step) = self.series.state.tape_price_step() {
            orderflow.observe_tape_price_grid(step, self.series.state.tape_reference_price());
        }
    }

    /// Take a backfill batch into the series and hand the indicators the bars
    /// it produced.
    pub fn ingest_backfill(&mut self, trades: &[quantick_engine::Trade]) {
        let effects = self.series.ingest_backfill(&mut self.model.history, trades);
        self.apply_series_effects(effects);
    }

    /// Prepend older trades, keeping the view and the marks on the bars they
    /// were on, and return how many net bars the trades added.
    pub fn prepend_history(&mut self, trades: &[quantick_engine::Trade]) -> usize {
        let (history, viewport) = (&mut self.model.history, &self.model.viewport);
        let (added, effects) = self
            .series
            .prepend_history(history, viewport, &self.spec, trades);
        self.apply_series_effects(effects);
        added
    }

    /// Queue a large recut without replacing the readable chart.
    pub fn receive_history(
        &mut self,
        trades: std::sync::Arc<Vec<quantick_engine::Trade>>,
        defer: bool,
    ) -> bool {
        if defer {
            self.history_worker.enqueue(trades);
            true
        } else {
            self.prepend_history(&trades);
            false
        }
    }

    pub fn history_pending(&self) -> bool {
        self.history_worker.pending()
    }

    pub fn history_failed(&self) -> bool {
        self.history_worker.failed()
    }
    pub fn retry_history(&mut self) -> bool {
        self.history_worker.retry()
    }

    /// Hold this pane's rebuild while a history run pages; release to
    /// rebuild once with everything it brought.
    pub fn hold_history(&mut self, held: bool) {
        self.history_worker.hold(held);
    }

    /// Pages this pane accepted and has not published yet.
    pub fn unpublished_history(&self) -> Vec<std::sync::Arc<Vec<quantick_engine::Trade>>> {
        self.history_worker.unpublished_pages()
    }

    /// Install a complete recut and catch up the live tail before moving anchors.
    pub fn prepare_history(&mut self) -> bool {
        self.history_worker.poll(&self.series.state)
    }

    pub fn install_history(&mut self) -> bool {
        let Some(candidate) = self.history_worker.take_ready(&self.series.state) else {
            return false;
        };
        let (history, viewport) = (&mut self.model.history, &self.model.viewport);
        let (old, effects) = self
            .series
            .install_recut(history, viewport, &self.spec, candidate);
        self.apply_series_effects(effects);
        self.history_worker.retire(old);
        true
    }

    /// See [`quantick_drawings::gesture::PaneMarksMut::reanchor`].
    pub fn reanchor_drawings(&mut self, old_slots: usize) {
        self.marks_mut().reanchor(old_slots);
    }

    /// Ask for a re-anchor once there are bars, for drawings adopted onto an
    /// empty series — the layout seeding a pane before its first print.
    /// Asking now would mark every anchor off a series that does not exist.
    pub fn defer_reanchor(&mut self) {
        self.model.history.defer_reanchor(0);
    }

    pub fn settle_pending_reanchor(&mut self) {
        let settled = self
            .series
            .settle_reanchor(&mut self.model.history, &self.spec);
        self.apply_series_effects(settled);
    }

    /// Rewrite the market instants behind the selected object's anchors after
    /// a move that changed their bar positions (drag or keyboard nudge).
    ///
    /// Without this the mark moves on this chart and its shared twin stays
    /// where it was: market time is what the other panes read, so a move that
    /// does not update it has moved only half the object.
    pub fn retime_selected(&mut self) {
        let projection = quantick_drawings::gesture::DrawingProjection {
            series: self.series.read(&self.spec),
            viewport: &self.model.viewport,
            indicators: &self.indicators,
        };
        projection.retime_selected(&mut self.drawings);
    }

    /// Throw away this pane's bars, keeping the spec its own selectors ask
    /// for and the marks the trader drew.
    ///
    /// Called when the market underneath changes — a feed switch, a source
    /// reset — because a bar index means nothing across two streams. The
    /// drawings survive it: their anchors carry market time, so they are
    /// re-expressed against the refilled series rather than discarded
    /// ([`Self::settle_pending_reanchor`]). The re-anchor waits for bars to
    /// exist, because an empty series can answer nothing.
    pub fn reset_series(&mut self) {
        self.reset_series_with(false);
    }

    /// Reset the tape while optionally retaining deal-counter readings for a
    /// rebuild of the same market.
    pub fn reset_series_with(&mut self, keep_readings: bool) {
        self.history_worker.cancel();
        if let Some(owner) = self.orderflow.as_mut() {
            owner.reset_flow_executions();
        }
        self.series
            .reset(&mut self.model.history, &self.spec, keep_readings);
        self.lane.reset();
        self.publish_partial();
        self.model.reset_series_view();
        // Framing dies with the series; orientation is the trader's standing
        // choice about the view, not about these bars — it survives the way
        // the drawings do.
        let inverted = self.price_view.is_inverted();
        self.price_view = PriceView::new();
        self.price_view.set_inverted(inverted);
        self.frame.auto_range = None;
        self.hover_pos = None;
    }

    /// Fill a pane opened mid-session from the trades another pane of the same
    /// market already holds, keeping the backfill/live boundary where it was:
    /// a trade that was streamed live must not become "history" just because
    /// this view was opened late.
    pub fn seed_from(
        &mut self,
        trades: &quantick_engine::trade_tape::TradeTape,
        backfill_count: usize,
        deal_samples: &[quantick_engine::DealSample],
    ) {
        let history = &mut self.model.history;
        let effects = self
            .series
            .seed_from(history, trades, backfill_count, deal_samples);
        self.apply_series_effects(effects);
    }

    /// [`Self::ingest_live_trade_observed`], with nobody listening.
    #[cfg(test)]
    pub fn ingest_live_trade(&mut self, trade: &quantick_engine::Trade) {
        self.ingest_live_trade_observed(trade, 0, &mut |_, _, _| {});
    }

    /// Take one live trade into the series, the tape and the indicators.
    ///
    /// The forming bar is *not* published here: it changes with every print
    /// and only its latest value is ever used, so the caller sends one
    /// [`Self::publish_partial`] at the end of the drain instead. A 500-print
    /// batch was 500 bar clones down the channel for the worker to collapse
    /// back into one. Closed bars stay per trade — each is a distinct event
    /// the indicators have to see.
    ///
    /// `alarm_tag` rides each close to the indicators and back on the
    /// alarms it raises, naming the market it closed on.
    ///
    /// `closed_observer` hears each bar the print closed, with its footprint
    /// summary, at its *composed* slot (venue history prefix + live bars) —
    /// the space the drawings' anchors live in, which is why the tab queues
    /// strategy closes from it.
    pub(crate) fn ingest_live_trade_observed(
        &mut self,
        trade: &quantick_engine::Trade,
        alarm_tag: u64,
        closed_observer: &mut dyn FnMut(&Self, &quantick_indicators::FootprintBar, usize),
    ) {
        if let Some(orderflow) = self.orderflow.as_mut() {
            orderflow.record_trade(trade);
        }
        let ingest = self.series.ingest_live(trade);
        self.publish_tape_price_step();
        self.apply_series_effects(ingest.backfilled.map(SeriesEffect::BackfillIndicators));
        for index in ingest.closed {
            let closed = self.series_read().footprint_bar(index);
            closed_observer(self, &closed, self.series.history_prefix.len() + index);
            self.lane.reset();
            self.indicator_worker
                .send(IndicatorCommand::BarClosed(closed, alarm_tag));
        }
    }

    /// Hand the indicators the forming bar as it stands now.
    ///
    /// Sent once per drain that took in live trades — see
    /// [`Self::ingest_live_trade_observed`] for why it is not sent per trade.
    pub fn publish_partial(&mut self) {
        let command = self.partial_command();
        self.indicator_worker.send(command);
    }

    /// Publish only the forming run's unsent suffix; the worker retains it.
    /// Rebuilds and lane enable cold-seed the current run once.
    fn partial_command(&mut self) -> IndicatorCommand {
        let partial = self.series_read().partial_footprint_bar();
        self.lane.command(partial, self.series.state.trades())
    }
}

#[cfg(test)]
impl ChartPane {
    /// Put `bars` in front of the trade-derived series, or take the prefix
    /// away when they are empty.
    ///
    /// Everything anchored to a bar index moves with the change, exactly as a
    /// trade-history prepend moves it: the viewport keeps its right edge, the
    /// drawings keep their bars, the indicator columns keep their candles
    /// until the rebuild lands. Returns whether anything changed.
    pub fn install_history_prefix(&mut self, bars: Vec<quantick_engine::Bar>) -> bool {
        self.install_venue_history(bars, None, false, None)
    }

    pub(crate) fn hold_history_publication(&mut self, held: bool) {
        self.history_worker.held = held;
    }

    /// Make this pane's rebuild read as failed until a retry.
    pub(crate) fn fail_history_publication(&mut self) {
        self.history_worker.fail = true;
    }
}

#[cfg(test)]
mod alert_observation_tests;
