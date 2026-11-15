//! The pane's side of its series: how new trades and venue history arrive,
//! what moves with them on the pane (the view, the marks, the indicator rows),
//! and what the indicator worker is told about every change.
//!
//! The bars themselves live in the tab session ([`quantick_session::PaneSeries`],
//! keyed by the pane's id); the composed slot reads are written once in
//! `quantick_chart::pane_series`, so the drawings, the indicators and the
//! control plane read the same slot for the same instant.

use crate::indicator_worker::{IndicatorCommand, IndicatorSource, SlotId};
use crate::price_view::PriceView;
use crate::state::ChartState;

use super::PaneRef;
use quantick_chart_interaction::pane_history::{PrefixChange, PrefixInstallEffect};

impl super::ChartPane {
    /// Reserve a slot and ask the worker to instantiate `source` behind it.
    pub fn add_indicator(&mut self, source: IndicatorSource) -> SlotId {
        let slot = self.indicators.allocate_slot(source.kind_id());
        self.indicator_worker
            .send(IndicatorCommand::Add { slot, source });
        slot
    }

    fn drain_indicator_events(&mut self) {
        for event in self.indicator_worker.drain_events() {
            self.indicators.apply(event);
        }
    }

    /// Ask for a re-anchor once there are bars, for drawings adopted onto an
    /// empty series — the layout seeding a pane before its first print.
    /// Asking now would mark every anchor off a series that does not exist.
    pub fn defer_reanchor(&mut self) {
        self.model.history.defer_reanchor(0);
    }
}

impl<P: super::LentPane, S: super::LentSeries, W: super::LentWorkers> super::PaneAt<P, S, W> {
    /// The bars this pane cuts from the tape.
    pub(crate) fn series_state(&self) -> &ChartState {
        self.series.state()
    }
    /// The venue candles folded in front of the trade-derived bars.
    pub(crate) fn venue_prefix(&self) -> &quantick_chart::venue_history::VenueHistory {
        self.series.prefix()
    }
    /// The composed reads — see `quantick_chart::pane_series::PaneSeriesRead`.
    pub(crate) fn series_read(&self) -> quantick_drawings::gesture::PaneSeriesRead<'_> {
        self.series.read(&self.spec)
    }
    // The slot reads, each one line onto the composed series that owns its
    // rule and its documentation.
    pub fn slots(&self) -> usize {
        self.series_read().slots()
    }
    pub fn closed_slots(&self) -> usize {
        self.series.closed_slots()
    }
    pub fn seam_slot(&self) -> usize {
        self.series.prefix().len()
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

    pub fn history_pending(&self) -> bool {
        self.workers.history.pending()
    }
    pub fn history_failed(&self) -> bool {
        self.workers.history.failed()
    }
    /// Pages this pane accepted and has not published yet.
    pub fn unpublished_history(&self) -> Vec<std::sync::Arc<Vec<quantick_engine::Trade>>> {
        self.workers.history.unpublished_pages()
    }
}

impl super::PaneMut<'_> {
    /// The bars this pane cuts from the tape, for the few writers outside the
    /// series passes.
    #[cfg(test)]
    pub(crate) fn series_state_mut(&mut self) -> &mut ChartState {
        self.series.state_mut()
    }

    /// Put this pane on `spec` outright, selectors included. Startup-scoped: the caller is a
    /// workspace restoring the bar rule this pane was last read on, into a pane that has not drawn
    /// a frame yet; a live change goes through [`crate::state::SpecSelector::pending`] so the
    /// frame carrying it paints the loading overlay before the rebuild replays the tape.
    ///
    /// The selectors move with the spec because the header's bars menu reads them: setting the state alone
    /// would restore a chart whose own controls disagreed with it, and the first touch of the
    /// parameter would snap it back to a rule the trader never chose.
    pub fn set_spec(&mut self, spec: impl Into<crate::state::BarConfiguration>) {
        let spec = spec.into();
        let changed = self.series.state().spec() != &spec;
        self.spec.set(spec);
        self.series.state_mut().set_spec(spec);
        if changed {
            self.pane.bump_pagination_revision();
        }
    }

    /// Re-cut every retained print from the retained deal-counter readings.
    pub fn rebuild_bars(&mut self) {
        self.series.state_mut().rebuild_bars();
        self.pane.bump_pagination_revision();
    }

    /// Ask the worker to replay the chart's bars from scratch — the one
    /// command behind spec switches, prepended history and source resets, so
    /// indicators inherit correct behavior for every rebuild path.
    pub fn send_indicator_rebuild(&mut self) {
        if let Some(owner) = self.orderflow.as_mut() {
            owner.reset_flow_executions();
        }
        self.lane.reset();
        self.indicator_worker.send(IndicatorCommand::Rebuild(
            self.series.closed_bars(),
            self.series.state().partial().cloned(),
        ));
        self.publish_partial();
    }

    /// [`Self::install_history_prefix`], with the venue's seam `lead` merged
    /// into the first engine bar (`ChartState::set_venue_lead`), and whether
    /// it leaves nothing of that bar's bucket unaccounted for.
    pub fn install_venue_history(
        &mut self,
        bars: Vec<quantick_engine::Bar>,
        lead: Option<quantick_engine::Bar>,
        covers_seam: bool,
    ) -> bool {
        let (state, prefix) = self.series.parts_mut();
        let lead_changed = state.set_venue_lead(lead, covers_seam);
        let change = prefix.install(bars, &mut self.pane.model.history, lead_changed);
        for effect in change.install_effects() {
            match effect {
                PrefixInstallEffect::ShiftViewport(delta) => {
                    self.model.viewport.shift_right_edge(delta)
                }
                PrefixInstallEffect::ShiftDrawings(delta) => self.marks_mut().shift_bars(delta),
                PrefixInstallEffect::ShiftIndicatorRows(added) => self.indicators.shift_rows(added),
                PrefixInstallEffect::RebuildIndicators => self.send_indicator_rebuild(),
            }
        }
        change != PrefixChange::Unchanged
    }

    /// Apply the indicator worker's deltas, before the draw reads columns.
    pub fn apply_indicator_events(&mut self) {
        self.pane.drain_indicator_events();
        let bars = self.series.closed_slots();
        if self.indicators.row_drift(bars).is_none() {
            return;
        }
        // Behind is normal while the worker has work. Idle, and with what it had
        // already published taken in, a difference means a rebuild was missed.
        let counts = self.indicator_worker.progress().counts;
        if counts.queued + counts.inflight + counts.parked + counts.deferred == 0 {
            self.pane.drain_indicator_events();
            if let Some(rows) = self.indicators.answer_row_drift(bars) {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_ROWS_DRIFT",
                    pane = self.id,
                    rows,
                    bars,
                    action = "rebuild_indicators",
                    "indicator rows disagree with the pane's bars"
                );
                self.send_indicator_rebuild();
            }
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
        let Some(orderflow) = self.pane.orderflow.as_mut() else {
            return;
        };
        let state = self.series.state();
        if let Some(step) = state.tape_price_step() {
            orderflow.observe_tape_price_grid(step, state.tape_reference_price());
        }
    }

    /// Take a backfill batch into the series and hand the indicators the bars
    /// it produced.
    pub fn ingest_backfill(&mut self, trades: &[quantick_engine::Trade]) {
        if !trades.is_empty() {
            self.pane.bump_pagination_revision();
        }
        self.series.state_mut().ingest_backfill(trades);
        self.lane.reset();
        self.indicator_worker
            .send(IndicatorCommand::Backfilled(self.series.closed_bars()));
        let partial = self.partial_command();
        self.indicator_worker.send(partial);
        self.publish_tape_price_step();
    }

    /// Prepend older trades, keeping the view and the marks on the bars they
    /// were on, and return how many net bars the trades added.
    pub fn prepend_history(&mut self, trades: &[quantick_engine::Trade]) -> usize {
        if !trades.is_empty() {
            self.pane.bump_pagination_revision();
        }
        let anchor = self.history_anchor();
        let added = self.series.state_mut().prepend_history(trades);
        self.finish_history_prepend(added, anchor);
        added
    }

    /// Where the view stands in market time, on a series whose re-cut can
    /// change how many bars it holds.
    fn history_anchor(&self) -> Option<(Option<i64>, f32, usize)> {
        let path_dependent = self.series.state().spec().definition().path_dependent;
        path_dependent.then(|| self.series_read().history_view_anchor(&self.model.viewport))
    }

    fn finish_history_prepend(&mut self, added: usize, anchor: Option<(Option<i64>, f32, usize)>) {
        if let Some((edge_time, offset, old_slots)) = anchor {
            // A Renko series re-cut from an older first print can hold a
            // different number of bricks, so the view and the marks go back
            // to market time.
            let slot = edge_time.and_then(|ms| self.slot_at_time(ms));
            self.model.viewport.reanchor_history(slot, offset);
            self.reanchor_drawings(old_slots);
        } else {
            // Older bars shift every index up by `added`; keep the view steady.
            self.model.viewport.shift_right_edge(added as isize);
            self.marks_mut().shift_bars(added as isize);
        }
        // Indicator columns shift with them: the rebuild below is a round-trip
        // away, and until it lands every value would otherwise be drawn
        // `added` slots off its own candle.
        self.indicators.shift_rows(added);
        self.publish_tape_price_step();
        // Older trades re-cut every bar; replay from scratch.
        self.send_indicator_rebuild();
    }

    /// Queue a large recut without replacing the readable chart.
    pub fn receive_history(
        &mut self,
        trades: std::sync::Arc<Vec<quantick_engine::Trade>>,
        defer: bool,
    ) -> bool {
        if defer {
            self.workers.history.enqueue(trades);
            true
        } else {
            self.prepend_history(&trades);
            false
        }
    }

    pub fn retry_history(&mut self) -> bool {
        self.workers.history.retry()
    }

    /// Hold this pane's rebuild while a history run pages; release to
    /// rebuild once with everything it brought.
    pub fn hold_history(&mut self, held: bool) {
        self.workers.history.hold(held);
    }

    /// Install a complete recut and catch up the live tail before moving anchors.
    pub fn prepare_history(&mut self) -> bool {
        self.workers.history.poll(self.series.state())
    }

    /// Land the recut the history worker finished into the tab session's
    /// series for this pane, moving the view and the marks with it.
    pub fn install_history(&mut self) -> bool {
        let Some(candidate) = self.workers.history.take_ready(self.series.state()) else {
            return false;
        };
        let anchor = self.history_anchor();
        let added = candidate
            .bars()
            .len()
            .saturating_sub(self.series.state().bars().len());
        let old = self.series.replace_state(candidate);
        self.pane.bump_pagination_revision();
        self.finish_history_prepend(added, anchor);
        self.workers.history.retire(old);
        true
    }

    /// See [`quantick_drawings::gesture::PaneMarksMut::reanchor`].
    pub fn reanchor_drawings(&mut self, old_slots: usize) {
        self.marks_mut().reanchor(old_slots);
    }

    pub fn settle_pending_reanchor(&mut self) {
        let slots = self.slots();
        let Some(old_slots) = self.model.history.settle_reanchor(slots) else {
            return;
        };
        self.reanchor_drawings(old_slots);
    }

    /// Rewrite the market instants behind the selected object's anchors after
    /// a move that changed their bar positions (drag or keyboard nudge).
    ///
    /// Without this the mark moves on this chart and its shared twin stays
    /// where it was: market time is what the other panes read, so a move that
    /// does not update it has moved only half the object.
    pub fn retime_selected(&mut self) {
        let pane = &mut *self.pane;
        let projection = quantick_drawings::gesture::DrawingProjection {
            series: self.series.read(&pane.spec),
            viewport: &pane.model.viewport,
            indicators: &pane.indicators,
        };
        projection.retime_selected(&mut pane.drawings);
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
        self.workers.history.cancel();
        if let Some(owner) = self.orderflow.as_mut() {
            owner.reset_flow_executions();
        }
        // A second reset before the first settled must not overwrite the
        // baseline with the empty series it is looking at now.
        let slots = self.slots();
        self.model.history.defer_reanchor(slots);
        // The prefix is bar-indexed against a series that no longer exists,
        // and its seam was trimmed against a first bar that is gone. A replay
        // never has one today; the invariant must not depend on that.
        self.series.reset(self.spec.spec(), keep_readings);
        self.lane.reset();
        self.publish_partial();
        let pane = &mut *self.pane;
        pane.bump_pagination_revision();
        pane.model.reset_series_view();
        // Framing dies with the series; orientation is the trader's standing
        // choice about the view, not about these bars — it survives the way
        // the drawings do.
        let inverted = pane.price_view.is_inverted();
        pane.price_view = PriceView::new();
        pane.price_view.set_inverted(inverted);
        pane.frame.auto_range = None;
        pane.hover_pos = None;
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
        if !trades.is_empty() {
            self.pane.bump_pagination_revision();
        }
        self.series.seed(trades, backfill_count, deal_samples);
        // One rebuild rather than one command per trade: the worker is being
        // handed a whole history, not watching it arrive.
        self.publish_tape_price_step();
        self.send_indicator_rebuild();
    }

    /// [`Self::ingest_live_trade_observed`], with nobody listening.
    #[cfg(test)]
    pub fn ingest_live_trade(&mut self, trade: &quantick_engine::Trade) {
        self.ingest_live_trade_observed(trade, &mut |_, _, _| {});
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
    /// `closed_observer` hears each bar the print closed, at its *composed*
    /// slot (venue history prefix + live bars) — the space the drawings'
    /// anchors live in, which is why the tab queues strategy closes from it.
    pub(crate) fn ingest_live_trade_observed(
        &mut self,
        trade: &quantick_engine::Trade,
        closed_observer: &mut dyn FnMut(PaneRef<'_>, &quantick_engine::Bar, usize),
    ) {
        if let Some(orderflow) = self.orderflow.as_mut() {
            orderflow.record_trade(trade);
        }
        let closes = self.series.ingest_live(trade);
        self.publish_tape_price_step();
        let seam = self.series.prefix().len();
        if let Some(live_from) = closes.history_until {
            self.lane.reset();
            let mut history = self.series.closed_bars();
            history.truncate(seam + live_from);
            self.indicator_worker
                .send(IndicatorCommand::Backfilled(history));
        }
        // One print can close several bars — a Renko print clearing several
        // levels — and each is an event.
        for index in closes.closed {
            let view = self.view();
            closed_observer(view, &self.series.state().bars()[index], seam + index);
            let closed = self.series.state().bars()[index].clone();
            self.lane.reset();
            self.indicator_worker
                .send(IndicatorCommand::BarClosed(closed));
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
        let state = self.series.state();
        self.pane
            .lane
            .command(state.partial().cloned(), state.trades())
    }
}

#[cfg(test)]
impl super::PaneMut<'_> {
    /// Put `bars` in front of the trade-derived series, or take the prefix
    /// away when they are empty.
    ///
    /// Everything anchored to a bar index moves with the change, exactly as a
    /// trade-history prepend moves it: the viewport keeps its right edge, the
    /// drawings keep their bars, the indicator columns keep their candles
    /// until the rebuild lands. Returns whether anything changed.
    pub fn install_history_prefix(&mut self, bars: Vec<quantick_engine::Bar>) -> bool {
        self.install_venue_history(bars, None, false)
    }

    pub(crate) fn hold_history_publication(&mut self, held: bool) {
        self.workers.history.held = held;
    }

    /// Make this pane's rebuild read as failed until a retry.
    pub(crate) fn fail_history_publication(&mut self) {
        self.workers.history.fail = true;
    }
}

#[cfg(test)]
mod alert_observation_tests;
