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
use crate::state::ChartState;
use crate::viewport::Viewport;

use super::ChartPane;
use quantick_chart_interaction::pane_history::PrefixChange;

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
        self.lane.reset();
        self.indicator_worker.send(IndicatorCommand::Rebuild(
            self.series_read().closed_bars(),
            self.state.partial().cloned(),
        ));
        self.publish_partial();
    }

    /// Put `bars` in front of the trade-derived series, or take the prefix
    /// away when they are empty.
    ///
    /// Everything anchored to a bar index moves with the change, exactly as a
    /// trade-history prepend moves it: the viewport keeps its right edge, the
    /// drawings keep their bars, the indicator columns keep their candles
    /// until the rebuild lands. Returns whether anything changed.
    #[cfg(test)]
    pub fn install_history_prefix(&mut self, bars: Vec<quantick_engine::Bar>) -> bool {
        self.install_venue_history(bars, None, false)
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
        let lead_changed = self.state.set_venue_lead(lead, covers_seam);
        let change = self
            .history_prefix
            .install(bars, &mut self.model.history, lead_changed);
        let delta = match change {
            PrefixChange::Unchanged => return false,
            PrefixChange::LeadOnly => {
                self.send_indicator_rebuild();
                return true;
            }
            PrefixChange::Replace { delta } => delta,
        };
        self.model.viewport.shift_right_edge(delta);
        self.marks_mut().shift_bars(delta);
        // Indicator columns have no signed shift: on growth they are nudged so
        // the frames before the rebuild lands draw each value against its own
        // candle, and on a shrink the rebuild below re-cuts them wholesale a
        // round trip later. Drawings get no such second chance, which is why
        // they take the signed delta above.
        if let Ok(added) = usize::try_from(delta) {
            self.indicators.shift_rows(added);
        }
        self.send_indicator_rebuild();
        true
    }

    /// Apply the indicator worker's deltas, before the draw reads columns.
    pub fn apply_indicator_events(&mut self) {
        self.drain_indicator_events();
        let bars = self.history_prefix.len() + self.state.bars().len();
        if self.indicators.row_drift(bars).is_none() {
            return;
        }
        // Behind is normal while the worker has work. Idle, and with what it had
        // already published taken in, a difference means a rebuild was missed.
        let counts = self.indicator_worker.progress().counts;
        if counts.queued + counts.inflight + counts.parked + counts.deferred == 0 {
            self.drain_indicator_events();
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
        if let Some(step) = self.state.tape_price_step() {
            orderflow.observe_tape_price_grid(step, self.state.tape_reference_price());
        }
    }

    /// Take a backfill batch into the series and hand the indicators the bars
    /// it produced.
    pub fn ingest_backfill(&mut self, trades: &[quantick_engine::Trade]) {
        if !trades.is_empty() {
            self.bump_pagination_revision();
        }
        self.state.ingest_backfill(trades);
        self.lane.reset();
        self.indicator_worker.send(IndicatorCommand::Backfilled(
            self.series_read().closed_bars(),
        ));
        let partial = self.partial_command();
        self.indicator_worker.send(partial);
        self.publish_tape_price_step();
    }

    /// Prepend older trades, keeping the view and the marks on the bars they
    /// were on, and return how many net bars the trades added.
    pub fn prepend_history(&mut self, trades: &[quantick_engine::Trade]) -> usize {
        if !trades.is_empty() {
            self.bump_pagination_revision();
        }
        let path_dependent = self.state.spec().definition().path_dependent;
        let anchor =
            path_dependent.then(|| self.series_read().history_view_anchor(&self.model.viewport));
        let added = self.state.prepend_history(trades);
        self.finish_history_prepend(added, anchor);
        added
    }

    fn finish_history_prepend(&mut self, added: usize, anchor: Option<(Option<i64>, f32, usize)>) {
        if let Some((edge_time, offset, old_slots)) = anchor {
            // A Renko series re-cut from an older first print can hold a
            // different number of bricks, so the view and the marks go back
            // to market time.
            self.model
                .viewport
                .reanchor_history(edge_time.and_then(|ms| self.slot_at_time(ms)), offset);
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
        self.history_worker.poll(&self.state)
    }

    #[cfg(test)]
    pub(crate) fn hold_history_publication(&mut self, held: bool) {
        self.history_worker.held = held;
    }

    /// Make this pane's rebuild read as failed until a retry.
    #[cfg(test)]
    pub(crate) fn fail_history_publication(&mut self) {
        self.history_worker.fail = true;
    }

    pub fn install_history(&mut self) -> bool {
        let Some(candidate) = self.history_worker.take_ready(&self.state) else {
            return false;
        };
        let anchor = self
            .state
            .spec()
            .definition()
            .path_dependent
            .then(|| self.series_read().history_view_anchor(&self.model.viewport));
        let added = candidate
            .bars()
            .len()
            .saturating_sub(self.state.bars().len());
        let old = std::mem::replace(&mut self.state, candidate);
        self.bump_pagination_revision();
        self.finish_history_prepend(added, anchor);
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
        let Some(old_slots) = self.model.history.settle_reanchor(self.slots()) else {
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
        let projection = quantick_drawings::gesture::DrawingProjection {
            series: quantick_drawings::gesture::PaneSeriesRead {
                history_prefix: &self.history_prefix,
                state: &self.state,
                spec: &self.spec,
            },
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
        // A second reset before the first settled must not overwrite the
        // baseline with the empty series it is looking at now.
        let slots = self.slots();
        self.model.history.defer_reanchor(slots);
        // The prefix is bar-indexed against a series that no longer exists,
        // and its seam was trimmed against a first bar that is gone. A replay
        // never has one today; the invariant must not depend on that.
        self.history_prefix.clear();
        if keep_readings {
            self.state.reset_series(self.spec.spec());
        } else {
            self.state = ChartState::new(self.spec.spec());
        }
        self.lane.reset();
        self.publish_partial();
        self.bump_pagination_revision();
        self.model.viewport = Viewport::new();
        // Framing dies with the series; orientation is the trader's standing
        // choice about the view, not about these bars — it survives the way
        // the drawings do.
        let inverted = self.price_view.is_inverted();
        self.price_view = PriceView::new();
        self.price_view.set_inverted(inverted);
        self.frame.auto_range = None;
        self.hover_pos = None;
        // Bars queued for the strategies belong to the series that just
        // died; the tab disarms the instances with the reset's own reason.
        self.strategies.pending.clear();
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
            self.bump_pagination_revision();
        }
        self.state.observe_deals_batch(deal_samples);
        let split = backfill_count.min(trades.len());
        self.state.ingest_backfill(trades.range(..split));
        for trade in trades.since(split) {
            self.state.ingest_live(trade);
        }
        // One rebuild rather than one command per trade: the worker is being
        // handed a whole history, not watching it arrive.
        self.publish_tape_price_step();
        self.send_indicator_rebuild();
    }

    /// Take one live trade into the series, the tape and the indicators.
    ///
    /// The forming bar is *not* published here: it changes with every print
    /// and only its latest value is ever used, so the caller sends one
    /// [`Self::publish_partial`] at the end of the drain instead. A 500-print
    /// batch was 500 bar clones down the channel for the worker to collapse
    /// back into one. Closed bars stay per trade — each is a distinct event
    /// the indicators have to see.
    pub fn ingest_live_trade(&mut self, trade: &quantick_engine::Trade) {
        if let Some(orderflow) = self.orderflow.as_mut() {
            orderflow.record_trade(trade);
        }
        let bars_before = self.state.bars().len();
        self.state.ingest_live(trade);
        self.publish_tape_price_step();
        // Bars the builder cut late from prints it held — Renko's, once it
        // reads its step — landed behind the boundary: history the indicators
        // take as history, and never a close handed to a strategy.
        let live_from = bars_before.max(self.state.backfill_boundary().unwrap_or(0));
        if live_from > bars_before {
            self.lane.reset();
            let mut history = self.series_read().closed_bars();
            history.truncate(self.history_prefix.len() + live_from);
            self.indicator_worker
                .send(IndicatorCommand::Backfilled(history));
        }
        // One print can close several bars — a Renko print clearing several
        // levels — and each is an event.
        for index in live_from..self.state.bars().len() {
            let closed = self.state.bars()[index].clone();
            self.lane.reset();
            self.indicator_worker
                .send(IndicatorCommand::BarClosed(closed.clone()));
            // Queued for the armed instances only while any exist: an idle
            // chart clones nothing on the per-trade path. The slot is the
            // *composed* one (venue history prefix + live bars) — the same
            // space the drawings' anchors live in, or the region's time
            // window would be off by the prefix length.
            if !self.strategies.anchors.is_empty() {
                let slot = self.history_prefix.len() + index;
                self.strategies.pending.push((closed, slot));
            }
        }
    }

    /// Hand the indicators the forming bar as it stands now.
    ///
    /// Sent once per drain that took in live trades — see
    /// [`Self::ingest_live_trade`] for why it is not sent per trade.
    pub fn publish_partial(&mut self) {
        let command = self.partial_command();
        self.indicator_worker.send(command);
    }

    /// Publish only the forming run's unsent suffix; the worker retains it.
    /// Rebuilds and lane enable cold-seed the current run once.
    fn partial_command(&mut self) -> IndicatorCommand {
        self.lane
            .command(self.state.partial().cloned(), self.state.trades())
    }
}
