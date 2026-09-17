//! Resumable anchored range-profile computation and honest coverage.
use quantick_engine::{
    Bar, BarFootprint, DEFAULT_LEVEL_CAP, ProfileFold, ValueArea, VolumeProfile,
};
use rust_decimal::Decimal;
/// Borrowed source facts; the caller owns tape retention and scheduling.
pub struct ProfileInputs<'a> {
    pub closed: &'a [Bar],
    pub ladders: &'a [BarFootprint],
    pub prefix: &'a [Bar],
    pub partial_ladder: Option<&'a BarFootprint>,
    pub group: Decimal,
    pub series_revision: u64,
    pub partial_version: u64,
    pub blocked: bool,
    pub side_inferred: bool,
    pub partial_bucket_slot: Option<usize>,
    /// Map touches per pass; a whole ladder can overshoot this budget.
    pub budget: usize,
}
pub struct ProfileRequest {
    pub min_bar: f32,
    pub max_bar: f32,
    pub extend_right: bool,
    pub approximate_history: bool,
    pub value_area_pct: u8,
}
/// A range's fold of **closed** bars: where it got to, and the engine-side
/// accumulator it got there with.
///
/// Held on the [`RangeProfile`] beside the profile it is building, and kept
/// after it finishes — the forming bar joins a *copy* of it whenever the live
/// edge moves, which is cheaper than re-folding the range and is the only
/// reason a long range survives a running tape.
#[derive(Debug, Clone)]
struct FoldJob {
    fold: ProfileFold,
    /// The next global slot to fold; `end` is inclusive.
    next: usize,
    end: usize,
}

impl FoldJob {
    /// A fold about to run over the global slots `start..=end`, folding
    /// ladders grouped at `group`.
    ///
    /// # Panics
    ///
    /// Panics if `group` is not positive — the same configuration contract as
    /// [`ProfileFold::new`].
    #[must_use]
    fn over(group: Decimal, start: usize, end: usize) -> Self {
        Self {
            fold: ProfileFold::new(group, DEFAULT_LEVEL_CAP),
            next: start,
            end,
        }
    }

    /// Carry this fold on to a later last slot, keeping every bar already in
    /// it.
    ///
    /// Legal even once the fold has been sealed: `ProfileFold::seal` only
    /// collapses the pending spreads, and its contract is that collapsing
    /// early and collapsing late reach the same rows. So a bar closing under a
    /// range that reaches the live edge costs one more `push_ladder`, not a
    /// fold of the whole range from its first bar.
    fn extend_to(&mut self, end: usize) {
        self.end = end;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrvpEmpty {
    /// The bars in range carry no footprint ladders — venue history candles,
    /// or a range dropped where nothing traded.
    NoTape,
    /// The feed reports no traded volume, so no honest profile exists.
    Blocked,
}

/// What one refresh computed for one object. Derived state: deliberately
/// excluded from equality and presets, the same rule as `Drawing::off_series`,
/// so a recompute can never read as a user edit to the undo history.
#[derive(Debug, Clone)]
pub struct RangeProfile {
    /// The inputs this result was computed from; `frvp::refresh` skips the
    /// merge while it matches.
    key: FrvpCacheKey,
    /// The folded profile and its value area, when the range had tape.
    profile: Option<(VolumeProfile, Option<ValueArea>)>,
    /// Why `profile` is `None`, when it is.
    empty: Option<FrvpEmpty>,
    /// Bars whose ladders went into the fold (the partial counts once).
    bars_covered: usize,
    /// The same count without the forming bar — the part the fold itself
    /// holds. The forming bar joins a *copy* of the fold every time its
    /// ladder is re-snapshotted, so its contribution is re-derived rather
    /// than accumulated, and this is the base it is re-derived from.
    closed_covered: usize,
    /// Bars folded from an **approximated** ladder — venue candles with no
    /// tape, their volume spread over their own high–low. Spoken by the
    /// status line, never blended away.
    bars_approximated: usize,
    /// Bars folded that cover only *part* of the interval they occupy — in
    /// practice the tape's first bar, whose venue candle was dropped at the
    /// seam. 0 or 1 today. How much volume they are short by is unknowable
    /// from here and is never invented; the status line names the bar and
    /// lets the trader judge it.
    bars_partly_covered: usize,
    /// Bars of the range the fold has reached, contributing or not. Equals
    /// [`bars_total`](Self::bars_total) once the fold is done; below it while
    /// [`job`](Self::job) is still running, which is what the status line
    /// counts out.
    bars_folded: usize,
    /// Bars the anchors span on the chart, prefix candles included.
    bars_total: usize,
    /// Whether the fold has bars left to reach. While it is true,
    /// [`profile`](Self::profile) holds the profile of the bars folded *so
    /// far* — real data, just not all of it — and the status line says how
    /// many are still to come. A range too long to fold in one frame is drawn
    /// filling rather than not drawn at all.
    folding: bool,
    /// The fold over the range's **closed** bars, kept after it finishes: the
    /// forming bar is added to a copy of it whenever its ladder moves, which
    /// is what keeps a live edge from re-folding the range ten times a
    /// second. `None` only when the range reaches no bar at all.
    job: Option<FoldJob>,
}

/// Read-only result projection; borrowed data stays in its computation owner.
#[derive(Debug, Clone, Copy)]
pub struct ProfileOutput<'a> {
    pub key: FrvpCacheKey,
    pub profile: Option<&'a (VolumeProfile, Option<ValueArea>)>,
    pub empty: Option<FrvpEmpty>,
    pub bars_covered: usize,
    pub closed_covered: usize,
    pub bars_approximated: usize,
    pub bars_partly_covered: usize,
    pub bars_folded: usize,
    pub bars_total: usize,
    pub folding: bool,
}

/// Everything the fold depends on. Anchor moves change the slots, a refold
/// changes the group, a rebuild of the bars or ladders bumps
/// `series_revision`, the
/// live edge bumps `partial_snapshot`.
///
/// What is *absent* here is as deliberate as what is present. A print that
/// only extends the forming bar moves `ChartState::timeline_revision` and
/// nothing else about the closed bars, and the count of closed bars moves on
/// every close even for a range nowhere near the live edge. Keying on either
/// restarted a long fold tens of times a second, so it never finished — the
/// closed bars a range covers are named by its slots plus
/// the supplied series revision,
/// and nothing else moves them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrvpCacheKey {
    pub start_slot: usize,
    pub end_slot: usize,
    pub group: Decimal,
    pub series_revision: u64,
    pub include_partial: bool,
    pub partial_snapshot: u64,
    pub value_area_pct: u8,
    pub blocked: bool,
    /// Whether the feed *infers* aggressor sides (tick rule) rather than
    /// reporting them. In the key so a feed switch re-stamps the label: a
    /// delta whose sides were guessed must say so, like the footprint legend
    /// does.
    pub side_inferred: bool,
    /// Whether venue-history candles are folded in as approximated ladders
    /// — the payload's own switch, in the key so toggling it re-folds.
    pub approximate: bool,
    /// Whether the range reaches a bar covering only part of its interval
    /// (see [`RangeProfile::bars_partly_covered`]). In the key so dragging off that bar
    /// clears the caveat and dragging back onto it restores it.
    pub partly_covered: bool,
}

impl FrvpCacheKey {
    /// Whether both keys describe the same fold of **closed** bars — every
    /// field but the forming bar's snapshot.
    ///
    /// The forming bar is the one input that moves without anything else
    /// moving, several times a second. Telling that case apart is what lets
    /// the closed fold stand while only the live edge is re-derived.
    #[must_use]
    pub fn same_fold(&self, other: &Self) -> bool {
        Self {
            partial_snapshot: other.partial_snapshot,
            ..*self
        } == *other
    }

    /// Whether `other` is this same fold with **more closed bars on its right**
    /// — the range unchanged where it starts, grown at the live edge.
    ///
    /// This is the case the type's own doc warns about, arriving through the
    /// one door it left open. A range whose right anchor sits at or past the
    /// newest candle has its `end_slot` *clamped* to the live edge, so every
    /// close moves it, [`same_fold`](Self::same_fold) says no, and a fold that
    /// had finished is thrown away and restarted from the range's first bar —
    /// on a rolling replay, faster than it can finish.
    ///
    /// A right edge that only grew has invalidated nothing already folded: it
    /// appended. Telling that apart turns a per-close re-fold of the whole
    /// range into a per-close push of one ladder.
    #[must_use]
    pub fn grown_right(&self, other: &Self) -> bool {
        other.end_slot > self.end_slot
            && Self {
                end_slot: other.end_slot,
                // The forming bar moves as ever, and `include_partial` flips
                // with it the first time the range's right edge reaches a bar
                // that has begun forming.
                partial_snapshot: other.partial_snapshot,
                include_partial: other.include_partial,
                ..*self
            } == *other
    }
}

fn covered_slots(min_bar: f32, max_bar: f32, last_slot: Option<usize>) -> Option<(usize, usize)> {
    let last = last_slot?;
    let (first_slot, last_slot_hit) = (min_bar.round(), max_bar.round());
    // Decided in float, before any cast: a float→int cast saturates, so a
    // span wholly left of slot 0 or wholly past `last` would otherwise
    // collapse onto an edge slot and fold it as if it had been asked for.
    #[allow(clippy::cast_precision_loss)]
    if last_slot_hit < 0.0 || first_slot > last as f32 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let start = first_slot.max(0.0) as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let end = (last_slot_hit.max(0.0) as usize).min(last);
    (start <= end).then_some((start, end))
}

impl RangeProfile {
    /// Borrow current results without cloning rows or traversing history.
    #[must_use]
    pub fn output(&self) -> ProfileOutput<'_> {
        ProfileOutput {
            key: self.key,
            profile: self.profile.as_ref(),
            empty: self.empty,
            bars_covered: self.bars_covered,
            closed_covered: self.closed_covered,
            bars_approximated: self.bars_approximated,
            bars_partly_covered: self.bars_partly_covered,
            bars_folded: self.bars_folded,
            bars_total: self.bars_total,
            folding: self.folding,
        }
    }

    /// Refresh the optional owned computation; true means closed work remains.
    pub fn refresh(
        current: &mut Option<Self>,
        request: ProfileRequest,
        inputs: &ProfileInputs<'_>,
    ) -> bool {
        let min_bar = request.min_bar;
        let max_bar = request.max_bar;
        let closed_len = inputs.closed.len();
        let closed_total = inputs.prefix.len() + closed_len;
        let partial_slot = inputs.partial_ladder.is_some().then_some(closed_total);
        let last_slot = partial_slot.or_else(|| closed_total.checked_sub(1));

        // The developing mode: the right edge is the newest slot, whatever the
        // second anchor says. The anchors are never touched — the cache key's
        // `end_slot` carries the resolved edge, and every bar close moves it.
        let max_bar = if request.extend_right {
            f32::MAX
        } else {
            max_bar
        };
        let span = covered_slots(min_bar, max_bar, last_slot);
        let (start_slot, end_slot) = span.unwrap_or((0, 0));
        let bars_total = span.map_or(0, |(start, end)| end - start + 1);
        let include_partial = span.is_some() && partial_slot.is_some_and(|slot| slot <= end_slot);
        // A range reaching the tape's first bar folds a bar that covers only part
        // of its interval. In the key, so the caveat cannot outlive the range that
        // earned it.
        let partly_covered = span.is_some()
            && inputs
                .partial_bucket_slot
                .is_some_and(|slot| start_slot <= slot && slot <= end_slot);

        let key = FrvpCacheKey {
            start_slot,
            end_slot,
            group: inputs.group,
            series_revision: inputs.series_revision,

            include_partial,
            partial_snapshot: if include_partial {
                inputs.partial_version
            } else {
                0
            },
            value_area_pct: request.value_area_pct,
            blocked: inputs.blocked,
            side_inferred: inputs.side_inferred,
            approximate: request.approximate_history,
            partly_covered,
        };
        // Everything the *closed* fold depends on is unchanged — at most the
        // forming bar moved. Its ladder is re-snapshotted several times a second,
        // and treating that as a new fold is a treadmill a long range never gets
        // off: it would fill part-way, throw away what it had and start again,
        // flickering the histogram and the status line on every tick. The forming
        // bar is not in the fold, so a new snapshot costs a re-read and nothing
        // else — and a fold still running simply carries on where it was.
        // ...and the same is true of a right edge that only *grew*. A range whose
        // right anchor sits at or past the newest candle has its `end_slot`
        // clamped to the live edge, so every close moves it — and treating that as
        // a new range restarted the fold from the range's first bar on every bar
        // close, which on a rolling replay is faster than the fold can finish. The
        // profile then never leaves its part-built state, and what gets painted is
        // not a faint version of the answer: it is a fully drawn histogram of the
        // range's *oldest* bars, normalised against its own busiest row, with its
        // own POC and value area. That is the flicker. A grown right edge appends
        // bars; it invalidates none, so the fold carries on and takes them.
        let carry_on = current
            .as_ref()
            .is_some_and(|c| c.key.same_fold(&key) || c.key.grown_right(&key));
        if let Some(cache) = current.as_mut().filter(|_| carry_on) {
            let live_edge_moved = cache.key.partial_snapshot != key.partial_snapshot;
            let grew = key.end_slot > cache.key.end_slot;
            cache.key = key;
            if grew {
                // The forming bar is never in the fold, so the last *closed* slot
                // is what the job runs to.
                let last_closed = if include_partial {
                    key.end_slot.saturating_sub(1)
                } else {
                    key.end_slot
                };
                cache.bars_total = bars_total;
                if let Some(job) = cache.job.as_mut() {
                    job.extend_to(last_closed);
                    cache.folding = job.next <= job.end;
                }
            }
            if cache.folding {
                cache.advance(request.value_area_pct, inputs);
            } else if live_edge_moved || grew {
                cache.derive(request.value_area_pct, inputs);
            }
            return cache.folding;
        }

        if inputs.blocked {
            *current = Some(RangeProfile {
                key,
                profile: None,
                empty: Some(FrvpEmpty::Blocked),
                bars_covered: 0,
                closed_covered: 0,
                bars_approximated: 0,
                bars_partly_covered: 0,
                bars_folded: 0,
                bars_total,
                folding: false,
                job: None,
            });
            return false;
        }

        // A fresh range: open a fold over its **closed** bars and spend this
        // pass's budget on it. Nothing is folded eagerly here — `advance` is the
        // only place bars enter a profile, so one budget governs the first pass
        // and every later one. The forming bar is not part of the fold at all;
        // `derive` adds it to a copy, every time it moves.
        let job = span.map(|(start, end)| {
            let last_closed = if include_partial {
                end.saturating_sub(1)
            } else {
                end
            };
            FoldJob::over(inputs.group, start, last_closed)
        });
        let mut cache = RangeProfile {
            key,
            profile: None,
            empty: job.is_none().then_some(FrvpEmpty::NoTape),
            bars_covered: 0,
            closed_covered: 0,
            bars_approximated: 0,
            bars_partly_covered: usize::from(partly_covered),
            bars_folded: 0,
            bars_total,
            folding: job.is_some(),
            job,
        };
        cache.advance(request.value_area_pct, inputs);
        let folding = cache.folding;
        *current = Some(cache);
        folding
    }
    fn advance(&mut self, value_area_pct: u8, inputs: &ProfileInputs<'_>) {
        let Some(job) = self.job.as_mut() else {
            return;
        };
        let prefix_len = inputs.prefix.len();
        let closed_total = prefix_len + inputs.closed.len();
        let ladders = inputs.ladders;
        let approximate = self.key.approximate;

        // With approximation off, no prefix slot can contribute anything: walking
        // them one budget at a time would spend seventeen passes counting out
        // `loading N of 25003` over bars that were never going to join.
        if !approximate && job.next < prefix_len {
            self.bars_folded += prefix_len - job.next;
            job.next = prefix_len;
        }

        let mut spent = 0usize;
        while spent < inputs.budget && job.next <= job.end {
            let slot = job.next;
            if slot < prefix_len {
                // A venue candle joins as a range, not as a ladder: one bar's
                // worth of work whatever its price width.
                if job.fold.push_candle(&inputs.prefix[slot]) {
                    self.bars_approximated += 1;
                }
                spent += 1;
            } else if let Some(ladder) = ladders
                .get(slot - prefix_len)
                .filter(|_| slot < closed_total)
            {
                // A bar whose ladder printed nothing contributes nothing; it
                // still counts as covered — the tape answered "no trades", which
                // is data, not absence of data. A ladder the fold *refuses* is
                // another matter: its buckets never aligned, so it is not in the
                // profile and must not be counted as if it were.
                if job.fold.push_ladder(ladder) {
                    self.closed_covered += 1;
                }
                spent += ladder.levels().len().max(1);
            }
            job.next += 1;
            self.bars_folded += 1;
        }
        self.folding = job.next <= job.end;
        if !self.folding {
            // Last input in: fold the pending spreads once, so the re-reads the
            // live edge triggers from here on cost a clone of the rows and no
            // spread folding at all.
            job.fold.seal();
        }
        self.derive(value_area_pct, inputs);
    }

    /// Read the profile out of the closed-bar fold, with the forming bar added to
    /// a copy of it when the range reaches one.
    ///
    /// Called once per fold pass, and again — on its own — every time the forming
    /// bar's ladder is re-snapshotted. That is the whole point of keeping the two
    /// apart: the live edge costs one ladder push and one read, never a re-fold.
    fn derive(&mut self, value_area_pct: u8, inputs: &ProfileInputs<'_>) {
        let Some(job) = self.job.as_ref() else {
            return;
        };
        let partial = self
            .key
            .include_partial
            .then_some(inputs.partial_ladder)
            .flatten();
        // A forming-bar snapshot taken before a group refold carries the previous
        // base grouping, and its buckets never aligned with this fold's. It is
        // refused rather than folded, and — this is the honesty half — it is not
        // counted as covered either, so the status line says "profile from N of M
        // bars" for the frame or two until the next snapshot lands, instead of
        // claiming a bar that is not in the histogram.
        let mut joined_partial = false;
        let profile = match partial {
            Some(partial) => {
                let mut with_partial = job.fold.clone();
                joined_partial = with_partial.push_ladder(partial);
                with_partial.profile()
            }
            None => job.fold.profile(),
        };
        self.bars_covered = self.closed_covered + usize::from(joined_partial);
        let fraction = Decimal::from(value_area_pct) / Decimal::ONE_HUNDRED;
        self.profile = profile.map(|profile: VolumeProfile| {
            let value_area: Option<ValueArea> = profile.value_area(fraction);
            (profile, value_area)
        });
        if !self.folding {
            // Folded to the end: the profile is the whole range's, the forming
            // bar included, and only now may an empty one be called empty.
            self.bars_folded = self.bars_total;
            self.empty = self.profile.is_none().then_some(FrvpEmpty::NoTape);
        }
    }
}

#[cfg(test)]
mod tests;
