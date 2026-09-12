//! The pane's series: the bars it holds, where each one sits in market time
//! and in slot space, how new trades and venue history arrive, and what the
//! indicator worker is told about every change.
//!
//! Two series compose one slot space — the venue prefix first, then the bars
//! the engine cut from prints — and every lookup here is written once against
//! that composition so the drawings, the indicators and the control plane read
//! the same slot for the same instant. A pure move out of `pane.rs`.

use smallvec::SmallVec;

use crate::indicator_worker::{IndicatorCommand, IndicatorSource, SlotId};
use crate::price_view::PriceView;
use crate::state::{BarSpec, ChartState};
use crate::viewport::Viewport;

use super::{ChartPane, prefix_differs};

impl ChartPane {
    /// The bar spec implied by the current selector state.
    pub fn current_spec(&self) -> BarSpec {
        self.spec.spec()
    }

    /// How many bar slots the chart draws: the venue prefix, the closed bars
    /// the engine cut from trades, and the forming one after them.
    pub fn slots(&self) -> usize {
        self.closed_slots() + usize::from(self.state.partial().is_some())
    }

    /// Slots holding a *closed* bar — everything before the forming one.
    pub fn closed_slots(&self) -> usize {
        self.history_prefix.len() + self.state.bars().len()
    }

    /// The slot the trade-derived series starts at: the seam between venue
    /// candles and bars this app built from prints.
    pub fn seam_slot(&self) -> usize {
        self.history_prefix.len()
    }

    /// The slot of a bar that covers only *part* of the interval it occupies,
    /// when there is one.
    ///
    /// The tape's first bar opens on its first print, which lands somewhere
    /// inside the interval rather than on its edge — and the venue candle
    /// that did cover the whole interval was dropped at the seam
    /// (`trim_to_seam`) precisely because the two overlap. So that slot holds
    /// a short bar wearing a full bar's clothes: a range folding it is missing
    /// whatever traded before the app connected, measured on a live BTCUSDT
    /// connect at 36% of a 1-minute bucket and 94% of an hourly one.
    ///
    /// No volume is invented to close the gap. The profile says the bar is
    /// partly covered and lets the trader judge it — the same contract the
    /// approximated-from-OHLC label keeps.
    ///
    /// `None` for a pane that does not cut by time (a tick or volume bar owns
    /// no interval to fall short of), and for a first bar that opens exactly
    /// on its boundary.
    pub fn partial_bucket_slot(&self) -> Option<usize> {
        let interval = self.state.spec().time_interval_ms()?;
        let first = self.state.bars().first().or_else(|| self.state.partial())?;
        let opens_inside =
            crate::resample::bucket_start(first.open_time, interval) != first.open_time;
        opens_inside.then(|| self.seam_slot())
    }

    /// The closed bar in `slot`, from whichever series owns it.
    pub fn closed_bar(&self, slot: usize) -> Option<&quantick_engine::Bar> {
        self.history_prefix
            .get(slot)
            .or_else(|| self.state.bars().get(slot - self.history_prefix.len()))
    }

    /// When the bar in `slot` opened, across both series and the forming bar.
    ///
    /// Past the prefix this is the engine's own answer, shifted into the
    /// composed slot space — there is one rule for what a slot means and the
    /// prefix only moves where it starts.
    pub fn slot_open_time(&self, slot: usize) -> Option<i64> {
        match self.history_prefix.get(slot) {
            Some(bar) => Some(bar.open_time),
            None => self.state.slot_open_time(slot - self.seam_slot()),
        }
    }

    /// The slot showing market time `ms`, across both series.
    ///
    /// The seam rule keeps `open_time` non-decreasing across the join, so the
    /// question splits cleanly: anything from the first engine bar onward is
    /// the engine's own answer shifted by the prefix, anything before it is a
    /// search of the prefix.
    pub fn slot_at_time(&self, ms: i64) -> Option<usize> {
        let seam = self.seam_slot();
        if seam == 0 {
            return self.state.slot_at_time(ms);
        }
        if self
            .state
            .bars()
            .first()
            .or_else(|| self.state.partial())
            .is_some_and(|bar| bar.open_time <= ms)
        {
            return self.state.slot_at_time(ms).map(|slot| slot + seam);
        }
        let after = self
            .history_prefix
            .partition_point(|bar| bar.open_time <= ms);
        Some(after.saturating_sub(1))
    }

    /// The slot whose bar *covers* market time `ms`, or `None` when this
    /// pane's tape does not reach that instant at all.
    ///
    /// [`Self::slot_at_time`] clamps at both ends — an instant older than the
    /// series answers slot 0, a newer one the newest slot — which is what a
    /// drawing anchor being re-hung on the closest bar wants. A mark standing
    /// for a *fill* wants the opposite answer at those edges: outside the
    /// window the pane holds, the tape on screen cannot prove the print
    /// happened, and a mark on the wrong bar is worse than no mark. Without
    /// it a replay seek — which wipes the bars and keeps the round trips,
    /// because they happened — stacks every earlier trade on whichever bar
    /// sits at the edge, and they pile up there as the replay runs on.
    ///
    /// The window is [`Self::covered_window`]'s. Between its two ends
    /// `slot_at_time` is already exact, so only the edges are decided here.
    ///
    /// Third of three deliberately different answers to one lookup, and the
    /// list is meant to stay at three. [`Self::slot_at_time`] clamps at both
    /// ends; `slot_of_time` refuses the old end but lets the new one *run on*
    /// past the newest bar, because a trend line pointing into the future
    /// belongs there; this one refuses both, because a fill does not happen
    /// in the future and did not happen before the tape began.
    ///
    /// One instant per call. A caller asking about many instants against one
    /// cut of the series — the trade paint, twice per closed round trip per
    /// frame — takes `covered_window` itself and tests against the two
    /// numbers, rather than re-deriving them thousands of times a second on
    /// the paint path.
    pub fn covering_slot_at_time(&self, ms: i64) -> Option<usize> {
        let (oldest, newest) = self.covered_window()?;
        if ms < oldest || ms > newest {
            return None;
        }
        self.slot_at_time(ms)
    }

    /// The stretch of market time this pane's bars cover, both ends
    /// inclusive: the oldest bar's open to the newest bar's last print.
    ///
    /// The oldest bar is the venue prefix's first candle when there is a
    /// prefix, so the lower end is that candle's open rather than a print —
    /// which is what "the chart reaches back this far" means to someone
    /// reading the screen, candles or prints. `None` for a pane with no bars.
    pub(super) fn covered_window(&self) -> Option<(i64, i64)> {
        let newest = self
            .state
            .partial()
            .or_else(|| self.state.bars().last())
            .or_else(|| self.history_prefix.last())
            .map(|bar| bar.close_time)?;
        Some((self.slot_open_time(0)?, newest))
    }

    /// The market time under the right edge of the candles' pane, or `None`
    /// while the view follows live (the right edge is the newest bar by
    /// definition, so there is nothing to remember) or when there are no bars.
    pub fn right_edge_time(&self) -> Option<i64> {
        if self.viewport.follows_live() {
            return None;
        }
        let slots = self.slots();
        let edge = self.viewport.right_edge_bar(slots);
        // Panning into the empty space past the newest bar puts the edge off
        // the series; the newest bar is the market time it is closest to.
        let slot = (edge.floor().max(0.0) as usize).min(slots.saturating_sub(1));
        self.slot_open_time(slot)
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
        self.lane.reset();
        self.indicator_worker.send(IndicatorCommand::Rebuild(
            self.closed_bars(),
            self.state.partial().cloned(),
        ));
        self.publish_partial();
    }

    /// Every closed bar the pane shows, prefix first — what an indicator is
    /// computed over, so an average spans the venue history rather than
    /// restarting at the first print this session saw.
    fn closed_bars(&self) -> Vec<quantick_engine::Bar> {
        let mut bars = Vec::with_capacity(self.closed_slots());
        bars.extend_from_slice(&self.history_prefix);
        bars.extend_from_slice(self.state.bars());
        bars
    }

    /// Put `bars` in front of the trade-derived series, or take the prefix
    /// away when they are empty.
    ///
    /// Everything anchored to a bar index moves with the change, exactly as a
    /// trade-history prepend moves it: the viewport keeps its right edge, the
    /// drawings keep their bars, the indicator columns keep their candles
    /// until the rebuild lands. Returns whether anything changed.
    pub fn install_history_prefix(&mut self, bars: Vec<quantick_engine::Bar>) -> bool {
        // Any time-cutting pane may carry one (audit S1) — the flow pane
        // showing time bars included. On a pane with a tape the flow layers
        // simply have nothing to draw over the prefix: a venue candle has no
        // prints in it, and the projection maps only the engine's own bars
        // (see `draw_chart`'s timeline).
        if !prefix_differs(&self.history_prefix, &bars) {
            return false;
        }
        let before = self.history_prefix.len();
        self.history_prefix = bars;
        self.bump_pagination_revision();
        // The prefix moves under a chart the user is already reading, so
        // everything anchored to a bar index moves with it — in either
        // direction. It grows when history lands; it shrinks when a coarser
        // fold makes fewer bars of the same span, or when older trades push
        // the seam back and the overlapping buckets leave.
        let delta = self.history_prefix.len() as isize - before as isize;
        self.viewport.shift_right_edge(delta);
        self.drawings.shift_bars(delta);
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
        self.indicator_worker
            .send(IndicatorCommand::Backfilled(self.closed_bars()));
        let partial = self.partial_command();
        self.indicator_worker.send(partial);
        self.publish_tape_price_step();
    }

    /// Prepend older trades and shift everything anchored to a bar index by the
    /// number of bars they added, which is what this returns.
    pub fn prepend_history(&mut self, trades: &[quantick_engine::Trade]) -> usize {
        if !trades.is_empty() {
            self.bump_pagination_revision();
        }
        // Older bars shift every index up; keep the view steady.
        let added = self.state.prepend_history(trades);
        self.viewport.shift_right_edge(added as isize);
        self.drawings.shift_bars(added as isize);
        // Indicator columns shift with them: the rebuild below is a round-trip
        // away, and until it lands every value would otherwise be drawn
        // `added` slots off its own candle.
        self.indicators.shift_rows(added);
        self.publish_tape_price_step();
        // Older trades re-cut every bar; replay from scratch.
        self.send_indicator_rebuild();
        added
    }

    /// Where a market instant sits on this pane's series, as a fractional
    /// slot — the strict answer, behind re-anchoring and every edit arriving
    /// from another pane.
    ///
    /// Bar centres, not edges: an anchor is being asked which *bar* it belongs
    /// to, and the middle of that bar is where it reads as being on it. `None`
    /// means the series does not reach the instant at all.
    ///
    /// Deliberately not what [`Self::reproject`] uses. That one is answering a
    /// different question — *where do I paint a mark whose instant may be off
    /// my series?* — so it clamps to the nearest edge and reports the clamp,
    /// which is what the fade is drawn from. This one is asked before the
    /// store is written, where a clamp would silently move the trader's mark
    /// onto data it has nothing to do with. Same lookup, opposite answer at
    /// the edges, on purpose.
    ///
    /// Also not [`Self::covering_slot_at_time`], which refuses the future end
    /// as well: a drawing may point past the newest bar, a fill may not.
    pub(super) fn slot_of_time(&self, time: i64) -> Option<f32> {
        // Past the newest bar first: on a time chart that space has an exact
        // clock, and asking `slot_at_time` there would clamp a future anchor
        // onto the right edge instead of letting it run on.
        if let Some(future) = self.future_slot_at_time(time) {
            return Some(future + 0.5);
        }
        // Before the first bar this pane holds. `slot_at_time` answers slot 0
        // there, which is a clamp and not a location — taking it would put the
        // anchor on a bar it has nothing to do with and say nothing about it.
        // `None` is what the off-series fade and the refused drag both read.
        if self.slot_open_time(0).is_some_and(|first| time < first) {
            return None;
        }
        let slots = self.slots();
        let slot = self.slot_at_time(time)?.min(slots.checked_sub(1)?);
        #[allow(clippy::cast_precision_loss)]
        Some(slot as f32 + 0.5)
    }

    /// Re-express this pane's drawings against the series it holds now,
    /// `old_slots` being the length of the series they were anchored to.
    ///
    /// The one call behind every re-cut — a timeframe switch, a bar-kind
    /// switch, a rewind, a symbol change. Marks are never dropped by a state
    /// change: the trader placed them and the trader removes them.
    pub fn reanchor_drawings(&mut self, old_slots: usize) {
        let new_slots = self.slots();
        // Taken out of `self` for the call: the store has to be handed this
        // pane's own time→slot answer, and a closure borrowing `&self` cannot
        // coexist with a `&mut` borrow of one of its fields.
        let mut drawings = std::mem::take(&mut self.drawings);
        drawings.reanchor(old_slots, new_slots, |time| self.slot_of_time(time));
        self.drawings = drawings;
    }

    /// Ask for a re-anchor once there are bars, for drawings adopted onto an
    /// empty series — the layout seeding a pane before its first print.
    /// Asking now would mark every anchor off a series that does not exist.
    pub fn defer_reanchor(&mut self) {
        self.pending_reanchor.get_or_insert(0);
    }

    pub fn settle_pending_reanchor(&mut self) {
        let Some(old_slots) = self.pending_reanchor else {
            return;
        };
        if self.slots() == 0 {
            return;
        }
        self.pending_reanchor = None;
        self.reanchor_drawings(old_slots);
    }

    /// Rewrite the market instants behind the selected object's anchors after
    /// a move that changed their bar positions (drag or keyboard nudge).
    ///
    /// Without this the mark moves on this chart and its shared twin stays
    /// where it was: market time is what the other panes read, so a move that
    /// does not update it has moved only half the object.
    pub fn retime_selected(&mut self) {
        let Some(index) = self.drawings.selected() else {
            return;
        };
        let Some(drawing) = self.drawings.items().get(index) else {
            return;
        };
        // Collected first so the immutable borrow of the store ends before
        // the write; every shipped tool has at most four anchors.
        let times: SmallVec<[Option<i64>; 4]> = drawing
            .points
            .iter()
            .map(|point| self.anchor_time(point.bar))
            .collect();
        self.drawings.set_times(index, &times);
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
        // A second reset before the first settled must not overwrite the
        // baseline with the empty series it is looking at now.
        let slots = self.slots();
        self.pending_reanchor.get_or_insert(slots);
        // The prefix is bar-indexed against a series that no longer exists,
        // and its seam was trimmed against a first bar that is gone. A replay
        // never has one today; the invariant must not depend on that.
        self.history_prefix.clear();
        self.state = ChartState::new(self.current_spec());
        self.lane.reset();
        self.publish_partial();
        self.bump_pagination_revision();
        self.viewport = Viewport::new();
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
    pub fn seed_from(&mut self, trades: &[quantick_engine::Trade], backfill_count: usize) {
        if !trades.is_empty() {
            self.bump_pagination_revision();
        }
        let split = backfill_count.min(trades.len());
        self.state.ingest_backfill(&trades[..split]);
        for trade in &trades[split..] {
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
        // At most one bar closes per trade (an atomic market event is never
        // split), so "grew" identifies exactly the bar that closed.
        let bars_after = self.state.bars().len();
        if bars_after > bars_before
            && let Some(closed) = self.state.bars().last().cloned()
        {
            self.lane.reset();
            self.indicator_worker
                .send(IndicatorCommand::BarClosed(closed.clone()));
            // Queued for the armed instances only while any exist: an idle
            // chart clones nothing on the per-trade path. The slot is the
            // *composed* one (venue history prefix + live bars) — the same
            // space the drawings' anchors live in, or the region's time
            // window would be off by the prefix length.
            if !self.strategies.anchors.is_empty() {
                let slot = self.history_prefix.len() + bars_after - 1;
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

    /// Where an instant later than this pane's newest bar falls, as a
    /// fractional slot past the end. `None` unless the pane's bars run on a
    /// fixed interval — see [`Self::anchor_time`] for why a tick chart has no
    /// answer here.
    pub(super) fn future_slot_at_time(&self, time: i64) -> Option<f32> {
        let interval = self.spec.spec().time_interval_ms()?;
        let last = self.slots().checked_sub(1)?;
        let last_open = self.slot_open_time(last)?;
        let ahead = time.checked_sub(last_open)?;
        if ahead < interval {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(last as f32 + ahead as f32 / interval as f32)
    }
}
