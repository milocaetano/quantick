//! Composed venue-prefix and trade-series coordinates, shared by pane readers.
use crate::state::{ChartState, SpecSelector};
use crate::viewport::Viewport;
#[derive(Clone, Copy)]
pub struct PaneSeriesRead<'a> {
    pub history_prefix: &'a [quantick_engine::Bar],
    pub state: &'a ChartState,
    pub spec: &'a SpecSelector,
}

impl<'a> PaneSeriesRead<'a> {
    pub fn partial_bucket_slot(&self) -> Option<usize> {
        let interval = self.state.spec().time_interval_ms()?;
        // The first print, not the bar's open: a venue lead in front of it
        // completes the candle, never the prints a ladder folds.
        let first = self.state.first_print_open_ms()?;
        let opens_inside =
            quantick_engine::time_bucket::time_bucket_start(first, interval) != first;
        opens_inside.then(|| self.seam_slot())
    }
    pub fn covering_slot_at_time(&self, ms: i64) -> Option<usize> {
        let (oldest, newest) = self.covered_window()?;
        if ms < oldest || ms > newest {
            return None;
        }
        self.slot_at_time(ms)
    }
    pub fn covered_window(&self) -> Option<(i64, i64)> {
        let newest = self
            .state
            .partial()
            .or_else(|| self.state.bars().last())
            .or_else(|| self.history_prefix.last())
            .map(|bar| bar.close_time)?;
        Some((self.slot_open_time(0)?, newest))
    }
    pub fn right_edge_time(&self, viewport: &crate::viewport::Viewport) -> Option<i64> {
        if viewport.follows_live() {
            return None;
        }
        let slots = self.slots();
        let (slot, _) =
            quantick_chart_interaction::pane_history::HistoryState::edge_anchor(&viewport, slots);
        self.slot_open_time(slot)
    }
    pub fn closed_bars(&self) -> Vec<quantick_engine::Bar> {
        let mut bars = Vec::with_capacity(self.closed_slots());
        bars.extend_from_slice(&self.history_prefix);
        bars.extend_from_slice(self.state.bars());
        bars
    }
    pub fn history_view_anchor(
        &self,
        viewport: &crate::viewport::Viewport,
    ) -> (Option<i64>, f32, usize) {
        let slots = self.slots();
        let (_, offset) =
            quantick_chart_interaction::pane_history::HistoryState::edge_anchor(viewport, slots);
        (self.right_edge_time(viewport), offset, slots)
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

    /// The closed bar in `slot`, from whichever series owns it.
    pub fn closed_bar(&self, slot: usize) -> Option<&'a quantick_engine::Bar> {
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

    /// Where a market instant sits on this pane's series, as a fractional
    /// slot — the strict answer, behind re-anchoring and every edit arriving
    /// from another pane.
    ///
    /// Bar centres, not edges: an anchor is being asked which *bar* it belongs
    /// to, and the middle of that bar is where it reads as being on it. `None`
    /// means the series does not reach the instant at all.
    ///
    /// Deliberately not what `ChartPane::reproject` uses. That one is answering a
    /// different question — *where do I paint a mark whose instant may be off
    /// my series?* — so it clamps to the nearest edge and reports the clamp,
    /// which is what the fade is drawn from. This one is asked before the
    /// store is written, where a clamp would silently move the trader's mark
    /// onto data it has nothing to do with. Same lookup, opposite answer at
    /// the edges, on purpose.
    ///
    /// Also not `covering_slot_at_time`, which refuses the future end
    /// as well: a drawing may point past the newest bar, a fill may not.
    pub fn slot_of_time(&self, time: i64) -> Option<f32> {
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

    /// Where an instant later than this pane's newest bar falls, as a
    /// fractional slot past the end. `None` unless the pane's bars run on a
    /// fixed interval — see [`Self::anchor_time`] for why a tick chart has no
    /// answer here.
    pub fn future_slot_at_time(&self, time: i64) -> Option<f32> {
        let interval = self.spec.spec().time_interval_ms()?;
        let last = self.slots().checked_sub(1)?;
        let last_open = self.slot_open_time(last)?;
        if let Some(law) = calendar_law(interval) {
            // Whole buckets ahead by the calendar, then how far into its own
            // bucket the instant sits — a month is as long as it is.
            let buckets = law.index(time) - law.index(last_open);
            if buckets < 1 {
                return None;
            }
            let start = law.start(time);
            let length = law.end(start) - start;
            #[allow(clippy::cast_precision_loss)]
            return Some(
                last as f32 + buckets as f32 + (time - start) as f32 / length.max(1) as f32,
            );
        }
        let ahead = time.checked_sub(last_open)?;
        if ahead < interval {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(last as f32 + ahead as f32 / interval as f32)
    }

    /// The market time behind a fractional bar slot, for anchors that may have
    /// to be re-expressed on another pane (§D7 of the drawing-tools design).
    ///
    /// Only a slot that actually holds a bar has an instant behind it: the
    /// empty space past the newest bar is future the tape has not written, and
    /// naming a time there would be an invention. `None` is the honest answer
    /// there, and it is what keeps such an anchor out of a shared drawing.
    pub fn anchor_time(&self, bar: f32) -> Option<i64> {
        let slot = Viewport::slot_of(bar)?;
        let slots = self.slots();
        if slot < slots {
            return self.slot_open_time(slot);
        }
        // Past the newest bar. Traders draw here constantly — a channel or a
        // trend line pointing into the empty space to the right of the tape
        // is the normal way to say "if this continues". Refusing the whole
        // gesture a time would block sharing exactly where it is most used.
        //
        // On a *time* chart that space has an exact clock: the bars are one
        // fixed interval apart, so the slot after the last one is the last
        // one plus that interval. Nothing is inferred. A calendar interval —
        // Monday weeks, months — steps by the engine's bucket law instead:
        // the slot after February is 1 March, not February plus a mean month.
        //
        // On a tick or volume chart it does not: the next bar happens when
        // enough trades happen, and no elapsed time can be named for it. That
        // stays `None` — an invented timestamp is worse than a control that
        // says why it is off.
        let interval = self.spec.spec().time_interval_ms()?;
        let last = slots.checked_sub(1)?;
        let ahead = i64::try_from(slot - last).ok()?;
        let last_open = self.slot_open_time(last)?;
        if let Some(law) = calendar_law(interval) {
            return Some(law.step(last_open, ahead));
        }
        last_open.checked_add(ahead.checked_mul(interval)?)
    }

    /// The candle behind a slot, the forming bar included — the one lookup
    /// every candle-reading snap shares.
    pub fn candle_at_slot(&self, slot: usize) -> Option<&'a quantick_engine::Bar> {
        self.closed_bar(slot)
            .or_else(|| (slot == self.closed_slots()).then(|| self.state.partial())?)
    }

    /// The last `want` closed bars of the live series — never venue-prefix
    /// candles, whose bodies measure another ruler — for warming a strategy
    /// trigger at arm or re-arm time.
    pub fn warmup_bars(&self, want: usize) -> Vec<quantick_engine::Bar> {
        let slots = self.slots();
        let first_live = self.seam_slot();
        (slots.saturating_sub(want).max(first_live)..slots)
            .filter_map(|slot| self.closed_bar(slot).cloned())
            .collect()
    }
}

/// The bucket law of a calendar interval — Monday weeks or months — whose
/// empty future slots step by the calendar rather than by a fixed length.
fn calendar_law(interval_ms: i64) -> Option<quantick_engine::time_bucket::TimeBucketLaw> {
    quantick_engine::time_bucket::TimeBucketLaw::of(interval_ms).filter(|law| law.is_calendar())
}

/// The bar under the pointer, and the instant it opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerBar {
    /// The slot, in the pane's composed slot space (venue prefix included).
    pub slot: usize,
    /// When that bar opened, in Unix milliseconds.
    pub open_time_unix_ms: i64,
    /// The pane's time interval, when it cuts by time: a calendar one tags
    /// the date rather than the clock.
    pub interval_ms: Option<i64>,
}

impl PaneSeriesRead<'_> {
    pub fn pointer_bar(
        &self,
        viewport: &Viewport,
        x: f32,
        history_right: f32,
        total: usize,
    ) -> Option<PointerBar> {
        if total == 0 || x > history_right {
            return None;
        }
        let slot = viewport.slot_at_x(x, history_right, total)?;
        Some(PointerBar {
            slot,
            open_time_unix_ms: self.slot_open_time(slot)?,
            interval_ms: self.spec.spec().time_interval_ms(),
        })
    }
}
