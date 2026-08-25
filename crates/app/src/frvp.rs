//! Refresh pass for fixed-range volume profile drawings.
//!
//! The drawing owns *where* (two anchors, a bar range); the engine owns
//! *what* ([`ProfileFold`] over that range's bars); this module is the bridge
//! that keeps the two current. It runs once per frame per pane, before the
//! drawings paint, and it re-folds only when something the profile depends on
//! actually changed — the cache key names every such input, so the common
//! frame costs one key comparison per profile object and no folding at all.
//!
//! **A frame is never held hostage by a range.** A profile dropped on a time
//! chart can span the venue's whole backfilled history, and folding tens of
//! thousands of bars in the frame that placed it is how the app used to stall
//! for seconds. So the fold is *resumable*: each pass spends at most
//! [`fold_budget`] bars' worth of work, stores what it has, and the paint
//! draws the partial profile with the count still to go on its status line.
//! The trader watches it fill instead of watching the window freeze.
//!
//! Never on the per-trade path: ingestion does not know this module exists.

use quantick_engine::{
    Bar, BarFootprint, DEFAULT_LEVEL_CAP, ProfileFold, ValueArea, VolumeProfile,
};
use rust_decimal::Decimal;

use crate::drawings::{Drawings, FrvpCache, FrvpCacheKey, FrvpEmpty, FrvpPayload};
use crate::state::ChartState;

/// The registry id of the fixed-range-profile tool — the one string this
/// module and the pane share to recognise a profile object.
pub const TOOL_ID: &str = "fixed-range-profile";

/// How much folding one refresh pass may do, counted in bars for a venue
/// candle and in ladder rows for a bar with tape.
///
/// The unit is "one map touch": a venue candle joins as a *range* whatever
/// its width ([`ProfileFold::push_candle`]), while a tape bar costs one touch
/// per row it printed. At the ~1.3 µs per candle the engine's `profile_fold`
/// bench measures, this budget is about 2 ms — and 2 ms is chosen against the
/// frame it has to share, not against the 16.7 ms one in the abstract: the
/// scene this exists for is a chart carrying a hundred thousand candles and a
/// liquidity map, which already spends ten. A budget of 4 000 measured
/// `fps 53 · frame 18.9 ms` there; this one leaves the frame intact.
///
/// A range longer than the budget simply takes more passes: 25 000 candles
/// fill in about seventeen, a third of a second, with a profile on screen
/// from the first one. Raising it buys a faster fill and a longer frame;
/// lowering it buys the reverse. Never zero — a zero budget would fold
/// nothing, forever.
pub const DEFAULT_FOLD_BUDGET: usize = 1_500;

/// The budget this process folds at: [`DEFAULT_FOLD_BUDGET`], or whatever
/// `QUANTICK_FRVP_FOLD_BUDGET` names.
///
/// The override is not a preference knob, it is the door onto a *state*. At a
/// budget of one bar a fold advances one bar per frame, so the filling
/// profile and its progress line stay on screen as long as an operator — or a
/// capture run — needs to look at them; without it that state lasts a fifth of
/// a second and no screenshot can be aimed at it. A non-positive or
/// unparseable value is refused rather than guessed, and the default stands.
///
/// Read once per process, so the fold never touches the environment on a
/// frame.
#[must_use]
pub fn fold_budget() -> usize {
    static BUDGET: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *BUDGET.get_or_init(|| {
        std::env::var("QUANTICK_FRVP_FOLD_BUDGET")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|budget| *budget > 0)
            .unwrap_or(DEFAULT_FOLD_BUDGET)
    })
}

/// A range's fold of **closed** bars: where it got to, and the engine-side
/// accumulator it got there with.
///
/// Held on the [`FrvpCache`] beside the profile it is building, and kept
/// after it finishes — the forming bar joins a *copy* of it whenever the live
/// edge moves, which is cheaper than re-folding the range and is the only
/// reason a long range survives a running tape.
#[derive(Debug, Clone)]
pub struct FoldJob {
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
    pub fn over(group: Decimal, start: usize, end: usize) -> Self {
        Self {
            fold: ProfileFold::new(group, DEFAULT_LEVEL_CAP),
            next: start,
            end,
        }
    }
}

/// Everything one refresh reads. The partial ladder comes in as the pane's
/// throttled snapshot (not the live one), so the forming bar re-merges at
/// the snapshot cadence, never per paint.
pub struct RefreshInputs<'a> {
    pub state: &'a ChartState,
    /// The venue-history candles behind the tape — one entry per prefix slot,
    /// oldest first. With the payload's `approximate_history` on each joins
    /// the fold as its own approximated spread, labeled; otherwise a range
    /// over them is *partial coverage* and the cache says so. No ladder is
    /// built for any of them: the fold takes the candle.
    pub prefix: &'a [Bar],
    /// The throttled snapshot of the forming bar's ladder, if any.
    pub partial_ladder: Option<&'a BarFootprint>,
    /// Bumped whenever the snapshot above is re-taken.
    pub partial_version: u64,
    /// The footprint layer's capability block: a feed with no traded volume
    /// has no honest profile to offer.
    pub blocked: bool,
    /// Whether the feed infers aggressor sides rather than reporting them —
    /// stamped on the cache so the paint can label the delta honestly.
    pub side_inferred: bool,
    /// The oldest global slot the L2 heatmap covers this frame, `None` when
    /// the map is off or painted nothing. Presentation state: it moves the
    /// paint's fill→outline cut, never the fold, so it lives on the cache
    /// *beside* the key — a map growing must not re-merge anything.
    pub heat_first_slot: Option<usize>,
    /// Where the pointer hovers while a profile is being placed — the bar
    /// that completes a one-anchor draft's range, so the histogram is live
    /// under the drag instead of appearing only on release.
    pub draft_hover_bar: Option<f32>,
    /// The slot of a bar covering only part of its interval — the tape's
    /// first bar, which opened on a print inside the interval while the venue
    /// candle covering all of it was dropped at the seam. See
    /// [`Pane::partial_bucket_slot`](crate::pane::Pane::partial_bucket_slot).
    /// A range reaching it is short by whatever traded before the app
    /// connected, and says so rather than being quietly topped up.
    pub partial_bucket_slot: Option<usize>,
}

/// Bring every fixed-range-profile drawing's cached profile up to date.
///
/// Returns whether any fold is still in flight — the caller paints the
/// partial profile and asks for another frame, which is what turns a long
/// range into a fill instead of a freeze.
///
/// Mutates only derived payload state ([`FrvpCache`]), which is excluded
/// from payload equality — so this pass can never register as a user edit
/// in the undo history, however often it runs.
pub fn refresh(drawings: &mut Drawings, inputs: &RefreshInputs<'_>) -> bool {
    let mut folding = false;
    for drawing in drawings.items_mut() {
        if drawing.tool.id() != TOOL_ID || drawing.points.len() < 2 {
            continue;
        }
        let (a, b) = (drawing.points[0].bar, drawing.points[1].bar);
        let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<FrvpPayload>() else {
            continue;
        };
        folding |= refresh_one(payload, a.min(b), a.max(b), inputs);
    }
    // The in-flight draft folds too, with the hovered bar standing in for
    // the second anchor — the histogram forms under the drag, instead of the
    // trader shaping a range blind and seeing the data only on release.
    if let Some(draft) = drawings
        .draft_mut()
        .filter(|draft| draft.tool.id() == TOOL_ID)
    {
        let span = match (draft.points.first(), draft.points.get(1)) {
            (Some(a), Some(b)) => Some((a.bar, b.bar)),
            (Some(a), None) => inputs.draft_hover_bar.map(|hover| (a.bar, hover)),
            _ => None,
        };
        if let Some((a, b)) = span
            && let Some(payload) = draft.payload.as_any_mut().downcast_mut::<FrvpPayload>()
        {
            folding |= refresh_one(payload, a.min(b), a.max(b), inputs);
        }
    }
    folding
}

/// The slots a `[min_bar, max_bar]` anchor span covers: the candle each
/// anchor was dropped **on**, and everything between them, clamped to the
/// slots that exist. `last_slot` is the partial's slot when there is one,
/// else the last closed.
///
/// A slot's centre is its own integer coordinate and it owns the half-open
/// interval `[slot - 0.5, slot + 0.5)`: the convention
/// [`Viewport::x_at_bar_position`](crate::viewport::Viewport::x_at_bar_position)
/// paints with and `Pane::drawing_point_at` inverts. Rounding is therefore
/// the whole rule — `round(coord)` *is* "which candle is under this pixel".
///
/// The tool snaps no anchor, so a real drag lands mid-candle every time. That
/// is why this rounds instead of taking the first and last centres strictly
/// inside the span: `ceil`/`floor` would step *past* both endpoints and drop
/// the two candles the trader could see under their own cursor, and the count
/// would wobble between 83, 84 and 85 on sub-pixel luck for one repeated
/// gesture.
///
/// `None` when the span reaches no candle at all, including one lying
/// entirely off either end. A rectangle over no candles has no profile, and
/// clamping it onto the nearest one would answer a question nobody asked
/// with a histogram indistinguishable from real data.
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

/// Refresh one profile object, folding at most [`fold_budget`] this pass.
/// Returns whether its fold is still in flight.
fn refresh_one(
    payload: &mut FrvpPayload,
    min_bar: f32,
    max_bar: f32,
    inputs: &RefreshInputs<'_>,
) -> bool {
    let closed_len = inputs.state.bars().len();
    let closed_total = inputs.prefix.len() + closed_len;
    let partial_slot = inputs.partial_ladder.is_some().then_some(closed_total);
    let last_slot = partial_slot.or_else(|| closed_total.checked_sub(1));

    // The developing mode: the right edge is the newest slot, whatever the
    // second anchor says. The anchors are never touched — the cache key's
    // `end_slot` carries the resolved edge, and every bar close moves it.
    let max_bar = if payload.extend_right {
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
        group: inputs.state.footprint_group(),
        timeline_revision: inputs.state.timeline_revision(),
        closed_len,
        include_partial,
        partial_snapshot: if include_partial {
            inputs.partial_version
        } else {
            0
        },
        value_area_pct: payload.value_area_pct,
        blocked: inputs.blocked,
        side_inferred: inputs.side_inferred,
        approximate: payload.approximate_history,
        partly_covered,
    };
    // Everything the *closed* fold depends on is unchanged — at most the
    // forming bar moved. Its ladder is re-snapshotted several times a second,
    // and treating that as a new fold is a treadmill a long range never gets
    // off: it would fill part-way, throw away what it had and start again,
    // flickering the histogram and the status line on every tick. The forming
    // bar is not in the fold, so a new snapshot costs a re-read and nothing
    // else — and a fold still running simply carries on where it was.
    if let Some(cache) = payload.cache.as_mut().filter(|c| c.key.same_fold(&key)) {
        let live_edge_moved = cache.key.partial_snapshot != key.partial_snapshot;
        cache.key = key;
        // Presentation state follows the frame either way — the heatmap's
        // boundary moving must never re-fold or re-read anything.
        cache.heat_first_slot = inputs.heat_first_slot;
        if cache.folding {
            advance(cache, payload.value_area_pct, inputs);
        } else if live_edge_moved {
            derive(cache, payload.value_area_pct, inputs);
        }
        return cache.folding;
    }

    if inputs.blocked {
        payload.cache = Some(FrvpCache {
            key,
            profile: None,
            empty: Some(FrvpEmpty::Blocked),
            bars_covered: 0,
            closed_covered: 0,
            bars_approximated: 0,
            bars_partly_covered: 0,
            bars_folded: 0,
            bars_total,
            heat_first_slot: inputs.heat_first_slot,
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
        FoldJob::over(inputs.state.footprint_group(), start, last_closed)
    });
    let mut cache = FrvpCache {
        key,
        profile: None,
        empty: job.is_none().then_some(FrvpEmpty::NoTape),
        bars_covered: 0,
        closed_covered: 0,
        bars_approximated: 0,
        bars_partly_covered: usize::from(partly_covered),
        bars_folded: 0,
        bars_total,
        heat_first_slot: inputs.heat_first_slot,
        folding: job.is_some(),
        job,
    };
    advance(&mut cache, payload.value_area_pct, inputs);
    let folding = cache.folding;
    payload.cache = Some(cache);
    folding
}

/// Spend one pass's [`fold_budget`] on `cache`'s closed-bar fold, then read
/// the profile out of it.
///
/// The read is a snapshot of *what has been folded so far* — a profile of the
/// range's first N bars, not a guess at its last. That is what the paint is
/// allowed to draw and the status line is obliged to qualify.
fn advance(cache: &mut FrvpCache, value_area_pct: u8, inputs: &RefreshInputs<'_>) {
    let Some(job) = cache.job.as_mut() else {
        return;
    };
    let prefix_len = inputs.prefix.len();
    let closed_total = prefix_len + inputs.state.bars().len();
    let ladders = inputs.state.bar_footprints();
    let approximate = cache.key.approximate;

    let mut spent = 0usize;
    let budget = fold_budget();
    while spent < budget && job.next <= job.end {
        let slot = job.next;
        if slot < prefix_len {
            // A venue candle joins as a range, not as a ladder: one bar's
            // worth of work whatever its price width.
            if approximate && job.fold.push_candle(&inputs.prefix[slot]) {
                cache.bars_approximated += 1;
            }
            spent += 1;
        } else if let Some(ladder) = ladders
            .get(slot - prefix_len)
            .filter(|_| slot < closed_total)
        {
            // A bar whose ladder printed nothing contributes nothing; it
            // still counts as covered — the tape answered "no trades", which
            // is data, not absence of data.
            job.fold.push_ladder(ladder);
            cache.closed_covered += 1;
            spent += ladder.levels().len().max(1);
        }
        job.next += 1;
        cache.bars_folded += 1;
    }
    cache.folding = job.next <= job.end;
    derive(cache, value_area_pct, inputs);
}

/// Read the profile out of the closed-bar fold, with the forming bar added to
/// a copy of it when the range reaches one.
///
/// Called once per fold pass, and again — on its own — every time the forming
/// bar's ladder is re-snapshotted. That is the whole point of keeping the two
/// apart: the live edge costs one ladder push and one read, never a re-fold.
fn derive(cache: &mut FrvpCache, value_area_pct: u8, inputs: &RefreshInputs<'_>) {
    let Some(job) = cache.job.as_ref() else {
        return;
    };
    let partial = cache
        .key
        .include_partial
        .then_some(inputs.partial_ladder)
        .flatten();
    let profile = match partial {
        Some(partial) => {
            let mut with_partial = job.fold.clone();
            with_partial.push_ladder(partial);
            with_partial.profile()
        }
        None => job.fold.profile(),
    };
    cache.bars_covered = cache.closed_covered + usize::from(partial.is_some());
    let fraction = Decimal::from(value_area_pct) / Decimal::ONE_HUNDRED;
    cache.profile = profile.map(|profile: VolumeProfile| {
        let value_area: Option<ValueArea> = profile.value_area(fraction);
        (profile, value_area)
    });
    if !cache.folding {
        // Folded to the end: the profile is the whole range's, the forming
        // bar included, and only now may an empty one be called empty.
        cache.bars_folded = cache.bars_total;
        cache.empty = cache.profile.is_none().then_some(FrvpEmpty::NoTape);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawings::{ChartPoint, DRAWING_TOOLS, DrawingTool};
    use crate::state::BarSpec;
    use quantick_engine::{Side, Trade};
    use rust_decimal::Decimal;
    use std::str::FromStr as _;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn trade(agg_id: u64, price: &str, quantity: &str, side: Side) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: 1_700_000_000_000 + agg_id as i64 * 1000,
            price: dec(price),
            quantity: dec(quantity),
            side,
        }
    }

    fn frvp_tool() -> DrawingTool {
        DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == TOOL_ID)
            .expect("frvp is registered")
    }

    /// Three closed tick-2 bars (six trades) plus a partial, footprints on.
    fn state_with_tape() -> ChartState {
        let mut state = ChartState::new(BarSpec::Tick(2));
        state.set_footprint_enabled(true);
        state.set_footprint_group(dec("1"));
        for (i, price) in ["100", "101", "100", "102", "101", "103", "104"]
            .iter()
            .enumerate()
        {
            state.ingest_live(&trade(
                i as u64,
                price,
                "1",
                if i % 2 == 0 { Side::Buy } else { Side::Sell },
            ));
        }
        assert_eq!(state.bars().len(), 3);
        assert!(state.partial().is_some());
        state
    }

    fn place_frvp(drawings: &mut Drawings, from_bar: f32, to_bar: f32) {
        drawings.place(frvp_tool(), ChartPoint::at(from_bar, 100.0));
        drawings.place(frvp_tool(), ChartPoint::at(to_bar, 105.0));
    }

    fn cache_of(drawings: &Drawings) -> FrvpCache {
        drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<FrvpPayload>()
            .expect("frvp payload")
            .cache
            .clone()
            .expect("refresh installed a cache")
    }

    fn inputs<'a>(state: &'a ChartState, blocked: bool) -> RefreshInputs<'a> {
        RefreshInputs {
            state,
            prefix: &[],
            partial_ladder: None,
            partial_version: 0,
            blocked,
            side_inferred: false,
            heat_first_slot: None,
            draft_hover_bar: None,
            partial_bucket_slot: None,
        }
    }

    /// The fold reads an anchor the way the paint writes one: an integer bar
    /// coordinate is a candle's *centre*. A range dragged from one candle's
    /// centre to another's therefore folds both end candles — the rectangle
    /// on screen and the bars behind the histogram are the same bars.
    #[test]
    fn covered_slots_folds_exactly_the_drawn_rectangle() {
        // Centre-to-centre over candles 100..=184 is 85 candles, both ends
        // included. Reading a centre as `slot + 0.5` drops the candle under
        // the right edge and pulls in half of the one left of the box.
        assert_eq!(covered_slots(100.0, 184.0, Some(300)), Some((100, 184)));
        // A coordinate belongs to the candle it lands *on*: candle N owns
        // `[N - 0.5, N + 0.5)`, so 99.5 is candle 100 and 184.4 is candle 184.
        assert_eq!(covered_slots(99.5, 184.4, Some(300)), Some((100, 184)));
    }

    /// The tool snaps no anchor, so every real drag ends mid-candle. Taking
    /// the first and last centres strictly *inside* the span (`ceil`/`floor`)
    /// stepped past both endpoints and folded 83 bars for an 85-candle drag —
    /// and gave a different count each time the same gesture was repeated.
    #[test]
    fn covered_slots_folds_the_candles_a_mid_candle_drag_lands_on() {
        // Pressed inside candle 100, released inside candle 184.
        assert_eq!(covered_slots(100.4, 183.6, Some(300)), Some((100, 184)));
        assert_eq!(covered_slots(99.7, 184.3, Some(300)), Some((100, 184)));
        // The same gesture, jittered by a sub-pixel, folds the same bars.
        for (lo, hi) in [(99.6, 183.51), (100.49, 184.49), (99.51, 184.2)] {
            assert_eq!(
                covered_slots(lo, hi, Some(300)),
                Some((100, 184)),
                "drag {lo}..{hi} is the same 85 candles"
            );
        }
    }

    /// Both anchors on one candle fold that candle. Dropping it left the
    /// trader with a rectangle, a "no tape in range" label and a bar that
    /// visibly traded.
    #[test]
    fn covered_slots_folds_the_single_candle_under_a_dot() {
        assert_eq!(covered_slots(5.0, 5.0, Some(300)), Some((5, 5)));
    }

    /// A range over no candles has no profile. Clamping it onto the nearest
    /// slot drew that slot's histogram — real-looking data for a rectangle
    /// the trader put nowhere near it.
    #[test]
    fn covered_slots_rejects_a_range_that_reaches_no_candle() {
        assert_eq!(covered_slots(-50.0, -40.0, Some(300)), None, "left of data");
        assert_eq!(
            covered_slots(400.0, 450.0, Some(300)),
            None,
            "past the tape"
        );
        // A short drag from inside candle 10 to inside candle 11 is those two
        // candles — there is no "between candles" to land in.
        assert_eq!(covered_slots(10.2, 10.8, Some(300)), Some((10, 11)));
        assert_eq!(covered_slots(10.2, 10.4, Some(300)), Some((10, 10)));
        // Left of candle 0 entirely: both ends round to -1.
        assert_eq!(covered_slots(-1.4, -0.6, Some(300)), None, "left of slot 0");
        // The edges still clamp *into* the data when the span overlaps it.
        assert_eq!(covered_slots(-50.0, 2.0, Some(300)), Some((0, 2)));
        assert_eq!(covered_slots(298.0, 450.0, Some(300)), Some((298, 300)));
        assert_eq!(covered_slots(0.0, 10.0, None), None, "no slots exist yet");
    }

    /// End to end: a range anchored centre-to-centre folds the tape of both
    /// end bars, not one of them.
    #[test]
    fn refresh_folds_both_end_bars_of_a_centre_to_centre_range() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        // Bars 0..=2 hold two trades of qty 1 each.
        place_frvp(&mut drawings, 0.0, 2.0);
        refresh(&mut drawings, &inputs(&state, false));
        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_total, 3, "three candles under the rectangle");
        assert_eq!(cache.bars_covered, 3);
        assert_eq!(
            cache.profile.expect("range has tape").0.total_volume(),
            dec("6"),
            "the bar under the right edge is part of the profile"
        );
    }

    /// The tape's first bar opens on a print inside its interval, and the
    /// venue candle that covered the rest was dropped at the seam. A range
    /// reaching that bar is short by whatever traded before the app
    /// connected — it says so, and nothing tops the volume back up.
    #[test]
    fn a_range_reaching_the_seam_bar_says_it_is_partly_covered() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);

        let seam = RefreshInputs {
            partial_bucket_slot: Some(0),
            ..inputs(&state, false)
        };
        refresh(&mut drawings, &seam);
        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_partly_covered, 1, "the seam bar is in range");
        assert!(cache.key.partly_covered);
        // The caveat costs no volume: the short bar still folds its own tape.
        assert_eq!(
            cache.profile.expect("tape").0.total_volume(),
            dec("6"),
            "nothing is invented and nothing is dropped"
        );

        // Drag off the seam bar and the caveat goes with it.
        drawings.move_anchor(0, 0, ChartPoint::at(1.0, 100.0));
        refresh(&mut drawings, &seam);
        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_partly_covered, 0);
        assert!(!cache.key.partly_covered);
    }

    /// A pane with no seam bar to speak of stays silent — the caveat is not
    /// stamped on every profile just because the plumbing exists.
    #[test]
    fn without_a_partial_bucket_no_caveat_is_claimed() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);
        refresh(&mut drawings, &inputs(&state, false));
        assert_eq!(cache_of(&drawings).bars_partly_covered, 0);
        assert!(!cache_of(&drawings).key.partly_covered);
    }

    #[test]
    fn refresh_merges_the_covered_closed_bars() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);

        refresh(&mut drawings, &inputs(&state, false));

        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_total, 3);
        assert_eq!(cache.bars_covered, 3);
        let (profile, value_area) = cache.profile.expect("range has tape");
        // Six trades of qty 1 across the three closed bars.
        assert_eq!(profile.total_volume(), dec("6"));
        assert!(value_area.is_some());
        assert!(cache.empty.is_none());
    }

    /// A venue candle at a price a time chart actually trades at, wide enough
    /// that spelling its spread out would cost hundreds of rows.
    fn venue_candle(i: usize) -> quantick_engine::Bar {
        let low = 36_000 + (i % 500) as i64;
        quantick_engine::Bar {
            open_time: 1_699_000_000_000 + i as i64 * 60_000,
            close_time: 1_699_000_000_000 + (i as i64 + 1) * 60_000 - 1,
            open: Decimal::from(low),
            high: Decimal::from(low + 400),
            low: Decimal::from(low),
            close: Decimal::from(low + 200),
            buy_volume: dec("1.5"),
            sell_volume: dec("2.5"),
            trade_count: 140,
        }
    }

    /// **The freeze, reproduced.** A profile dropped on a time chart spans the
    /// venue's backfilled history — twenty-five thousand candles is an
    /// ordinary afternoon's worth. Folding that range inside the frame that
    /// placed it is what locked the whole app up for seconds: every candle
    /// spelled out as its own ladder, every ladder merged, all before the next
    /// paint.
    ///
    /// The contract now: **one pass never folds the whole range.** It spends
    /// its budget, leaves the fold in flight, and hands the paint a real
    /// profile of what it got through. Set [`DEFAULT_FOLD_BUDGET`] to the range's
    /// length and this test fails — which is exactly the old behaviour.
    #[test]
    fn a_range_over_a_whole_venue_history_never_folds_in_one_pass() {
        let state = state_with_tape();
        let prefix: Vec<quantick_engine::Bar> = (0..25_000).map(venue_candle).collect();
        let mut drawings = Drawings::default();
        // Slot 0 to the forming bar: prefix, the three closed bars, the
        // partial — the whole chart, which is how a trader drops one.
        place_frvp(&mut drawings, 0.0, 25_003.0);
        let with_prefix = RefreshInputs {
            prefix: &prefix,
            partial_ladder: None,
            ..inputs(&state, false)
        };

        let folding = refresh(&mut drawings, &with_prefix);
        assert!(
            folding,
            "a 25k-bar range must leave the fold in flight, not swallow it in one frame"
        );
        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_total, 25_003);
        assert!(cache.folding, "the fold is still running after one pass");
        assert!(
            cache.bars_approximated <= fold_budget(),
            "one pass folded {} candles, past the {} budget",
            cache.bars_approximated,
            fold_budget()
        );
        assert!(
            cache.profile.is_some(),
            "what has been folded is already on screen — a partial profile, not a blank"
        );
        assert!(
            cache.empty.is_none(),
            "a fold still running is not an empty range"
        );
    }

    /// The other half of the contract: passes converge, and what they
    /// converge on is the whole range — every candle counted, every unit of
    /// volume conserved. A profile that never finished would be worse than a
    /// freeze.
    #[test]
    fn the_passes_converge_on_the_whole_range() {
        let state = state_with_tape();
        let prefix: Vec<quantick_engine::Bar> = (0..25_000).map(venue_candle).collect();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 25_003.0);
        let with_prefix = RefreshInputs {
            prefix: &prefix,
            partial_ladder: None,
            ..inputs(&state, false)
        };

        let mut passes = 0;
        let mut folded_before = 0;
        loop {
            let folding = refresh(&mut drawings, &with_prefix);
            passes += 1;
            let cache = cache_of(&drawings);
            let folded = cache.bars_covered + cache.bars_approximated;
            assert!(
                folded > folded_before,
                "pass {passes} folded nothing; the fill would never finish"
            );
            folded_before = folded;
            assert!(passes < 100, "a 25k range should not need 100 passes");
            if !folding {
                break;
            }
        }

        let cache = cache_of(&drawings);
        assert!(!cache.folding, "the fold finished");
        assert_eq!(cache.bars_approximated, 25_000, "every venue candle joined");
        assert_eq!(cache.bars_covered, 3, "and the three tape bars");
        let (profile, value_area) = cache.profile.expect("the range folded");
        // 4 units per candle (1.5 buy + 2.5 sell) plus the six tape trades.
        assert_eq!(profile.total_volume(), dec("100006"));
        assert!(value_area.is_some());
        // Still one profile object: the fill must not have left the range
        // claiming more bars than the chart has.
        assert_eq!(cache.bars_total, 25_003);
    }

    /// A live tape must not keep a long fold from finishing.
    ///
    /// The forming bar's ladder is re-snapshotted several times a second. If
    /// that restarted the fold, a range this long would fill part-way, throw
    /// away what it had and start again — forever, and burning a budget's
    /// worth of work every frame to do it. The forming bar is not part of the
    /// fold at all, so a new snapshot must cost nothing but a re-read.
    #[test]
    fn a_long_fold_converges_while_the_forming_bar_keeps_moving() {
        let state = state_with_tape();
        let partial = state.partial_footprint().expect("forming bar").clone();
        let prefix: Vec<quantick_engine::Bar> = (0..25_000).map(venue_candle).collect();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 25_004.0);

        let mut passes = 0u64;
        loop {
            // A fresh snapshot on *every* pass — worse than the pane's real
            // throttle, so a fold that survives this survives any tape.
            let moving = RefreshInputs {
                prefix: &prefix,
                partial_ladder: Some(&partial),
                partial_version: passes + 1,
                ..inputs(&state, false)
            };
            let folding = refresh(&mut drawings, &moving);
            passes += 1;
            assert!(
                passes < 100,
                "the fold never converged: {} of {} bars after {passes} passes",
                cache_of(&drawings).bars_folded,
                cache_of(&drawings).bars_total
            );
            if !folding {
                break;
            }
        }

        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_approximated, 25_000, "every candle joined once");
        assert_eq!(
            cache.bars_covered, 4,
            "three closed tape bars and the forming one"
        );
        assert_eq!(
            cache.profile.expect("folded").0.total_volume(),
            dec("100007")
        );
    }

    /// The fold reruns from scratch when the range changes mid-fill — a
    /// trader dragging an anchor while the first fold is still running must
    /// not be shown the old range's rows under the new rectangle.
    #[test]
    fn moving_an_anchor_mid_fold_restarts_the_fold_on_the_new_range() {
        let state = state_with_tape();
        let prefix: Vec<quantick_engine::Bar> = (0..25_000).map(venue_candle).collect();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 25_003.0);
        let with_prefix = RefreshInputs {
            prefix: &prefix,
            partial_ladder: None,
            ..inputs(&state, false)
        };
        assert!(refresh(&mut drawings, &with_prefix), "still folding");

        // Drag the left anchor most of the way right: a much shorter range.
        drawings.items_mut()[0].points[0].bar = 24_000.0;
        refresh(&mut drawings, &with_prefix);
        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_total, 1_003);
        assert!(
            cache.bars_approximated <= 1_000,
            "the new range folded only its own candles, not the old range's"
        );
        assert!(
            !cache.folding,
            "a range inside one budget finishes in the pass that started it"
        );
    }

    /// The forming bar's ladder is re-snapshotted about ten times a second.
    /// Treating that as a new fold is a treadmill a long range never gets off
    /// — the fold restarts before it can finish, so the profile stays stuck
    /// at "folding" forever and the chart pays for the range every tenth of a
    /// second. The closed bars did not move, so their fold stands and only
    /// the live edge is re-derived.
    #[test]
    fn a_moving_forming_bar_never_refolds_the_range() {
        let state = state_with_tape();
        let partial = state.partial_footprint().expect("forming bar").clone();
        let prefix: Vec<quantick_engine::Bar> = (0..25_000).map(venue_candle).collect();
        let mut drawings = Drawings::default();
        // Slot 25_003 is the forming bar; the anchor reaches past it and
        // clamps onto it.
        place_frvp(&mut drawings, 0.0, 25_004.0);
        let first = RefreshInputs {
            prefix: &prefix,
            partial_ladder: Some(&partial),
            partial_version: 1,
            ..inputs(&state, false)
        };
        let mut passes = 0;
        while refresh(&mut drawings, &first) {
            passes += 1;
            assert!(passes < 100, "the fold should converge");
        }
        let done = cache_of(&drawings);
        assert!(!done.folding);
        assert_eq!(done.bars_total, 25_004);
        assert_eq!(
            done.bars_covered, 4,
            "three closed tape bars and the forming one"
        );
        // 25 000 candles of 4 units each, six units of closed tape, and the
        // forming bar's single print.
        let volume = done.profile.clone().expect("folded").0.total_volume();
        assert_eq!(volume, dec("100007"));

        // The live edge moves: same range, same closed bars, new snapshot.
        let bumped = RefreshInputs {
            prefix: &prefix,
            partial_ladder: Some(&partial),
            partial_version: 2,
            ..inputs(&state, false)
        };
        assert!(
            !refresh(&mut drawings, &bumped),
            "a live-edge bump must not open a new fold"
        );
        let after = cache_of(&drawings);
        assert_eq!(
            after.bars_folded, done.bars_folded,
            "not one bar was folded again"
        );
        assert_eq!(after.bars_approximated, 25_000);
        assert_eq!(
            after.profile.expect("still folded").0.total_volume(),
            volume,
            "and the forming bar is still in the profile"
        );
    }

    #[test]
    fn refresh_skips_when_nothing_changed_and_recomputes_on_anchor_move() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);

        refresh(&mut drawings, &inputs(&state, false));
        let first = cache_of(&drawings);
        refresh(&mut drawings, &inputs(&state, false));
        assert_eq!(
            cache_of(&drawings).key,
            first.key,
            "same inputs, same key, no recompute"
        );

        // Narrow the range to one bar: the key and the merge both change.
        drawings.move_anchor(0, 1, ChartPoint::at(0.0, 105.0));
        refresh(&mut drawings, &inputs(&state, false));
        let narrowed = cache_of(&drawings);
        assert_ne!(narrowed.key, first.key);
        assert_eq!(narrowed.bars_total, 1);
        assert_eq!(
            narrowed.profile.expect("bar 0 has tape").0.total_volume(),
            dec("2")
        );
    }

    #[test]
    fn refresh_recomputes_when_the_group_refolds() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);
        refresh(&mut drawings, &inputs(&state, false));
        let before = cache_of(&drawings);

        let mut state = state;
        state.set_footprint_group(dec("2"));
        refresh(&mut drawings, &inputs(&state, false));
        let after = cache_of(&drawings);
        assert_ne!(after.key, before.key, "a refold re-keys the cache");
        assert_eq!(after.profile.expect("still tape").0.group(), dec("2"));
    }

    #[test]
    fn partial_bar_joins_only_when_the_range_reaches_it() {
        let state = state_with_tape();
        let partial = state.partial_footprint().expect("forming bar").clone();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 3.0);

        let with_partial = RefreshInputs {
            partial_ladder: Some(&partial),
            partial_version: 7,
            ..inputs(&state, false)
        };
        refresh(&mut drawings, &with_partial);
        let cache = cache_of(&drawings);
        assert!(cache.key.include_partial);
        assert_eq!(cache.bars_total, 4);
        assert_eq!(cache.bars_covered, 4);
        // Six closed trades plus the forming bar's one.
        assert_eq!(cache.profile.expect("tape").0.total_volume(), dec("7"));

        // A range that stops short of the forming bar leaves it out.
        drawings.move_anchor(0, 1, ChartPoint::at(2.0, 105.0));
        refresh(&mut drawings, &with_partial);
        let cache = cache_of(&drawings);
        assert!(!cache.key.include_partial);
        assert_eq!(cache.profile.expect("tape").0.total_volume(), dec("6"));
    }

    /// Two venue candles standing in for a prefix: $2-tall bodies with a
    /// real taker split, one unit of volume each.
    fn prefix_bars() -> Vec<quantick_engine::Bar> {
        (0..2)
            .map(|i| quantick_engine::Bar {
                open_time: 1_699_999_000_000 + i * 60_000,
                close_time: 1_699_999_060_000 + i * 60_000,
                open: dec("99"),
                high: dec("100.9"),
                low: dec("99"),
                close: dec("100.5"),
                buy_volume: dec("0.6"),
                sell_volume: dec("0.4"),
                trade_count: 10,
            })
            .collect()
    }

    /// A mixed range folds both worlds: the tape bars exactly, the venue
    /// prefix as approximated ladders — counted apart, never blended away.
    #[test]
    fn prefix_candles_join_approximated_and_are_counted_apart() {
        let state = state_with_tape();
        let prefix = prefix_bars();
        let mut drawings = Drawings::default();
        // Slots 0-1 are venue prefix, 2-4 are the three state bars.
        place_frvp(&mut drawings, 0.0, 4.0);

        let with_prefix = RefreshInputs {
            prefix: &prefix,
            ..inputs(&state, false)
        };
        refresh(&mut drawings, &with_prefix);
        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_total, 5);
        assert_eq!(cache.bars_covered, 3, "tape bars stay exact");
        assert_eq!(cache.bars_approximated, 2, "venue candles join, labeled");
        // Six units of tape plus one unit per approximated candle.
        assert_eq!(cache.profile.expect("fold").0.total_volume(), dec("8"));
    }

    /// The off-switch restores exactly the pre-approximation behaviour: a
    /// prefix-only range is honest empty again.
    #[test]
    fn approximation_off_leaves_the_prefix_as_no_tape() {
        let state = state_with_tape();
        let prefix = prefix_bars();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 1.0);
        drawings.items_mut()[0]
            .payload
            .as_any_mut()
            .downcast_mut::<FrvpPayload>()
            .unwrap()
            .approximate_history = false;

        let with_prefix = RefreshInputs {
            prefix: &prefix,
            ..inputs(&state, false)
        };
        refresh(&mut drawings, &with_prefix);
        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_total, 2);
        assert_eq!(cache.bars_covered, 0);
        assert_eq!(cache.bars_approximated, 0);
        assert!(cache.profile.is_none());
        assert_eq!(cache.empty, Some(FrvpEmpty::NoTape));

        // Toggling back on re-keys and the approximated fold appears.
        drawings.items_mut()[0]
            .payload
            .as_any_mut()
            .downcast_mut::<FrvpPayload>()
            .unwrap()
            .approximate_history = true;
        refresh(&mut drawings, &with_prefix);
        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_approximated, 2);
        assert_eq!(
            cache.profile.expect("approximated fold").0.total_volume(),
            dec("2")
        );
    }

    #[test]
    fn blocked_capability_stores_the_block_not_a_profile() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);
        refresh(&mut drawings, &inputs(&state, true));
        let cache = cache_of(&drawings);
        assert!(cache.profile.is_none());
        assert_eq!(cache.empty, Some(FrvpEmpty::Blocked));
    }

    #[test]
    fn refresh_never_touches_the_undo_history() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);
        let depth = drawings.undo_depth();
        refresh(&mut drawings, &inputs(&state, false));
        assert_eq!(
            drawings.undo_depth(),
            depth,
            "derived-state refresh is not an edit"
        );
    }

    /// The developing mode: with `extend_right` on, the range's right edge is
    /// the newest slot however short the anchors fall, every closed bar grows
    /// the fold, and switching it off restores exactly the drawn range.
    #[test]
    fn extend_right_follows_the_tape_and_releases_cleanly() {
        let mut state = state_with_tape();
        let mut drawings = Drawings::default();
        // Anchors cover only bar 0; the tape has three closed bars.
        place_frvp(&mut drawings, 0.0, 0.0);
        drawings.items_mut()[0]
            .payload
            .as_any_mut()
            .downcast_mut::<FrvpPayload>()
            .unwrap()
            .extend_right = true;

        refresh(&mut drawings, &inputs(&state, false));
        let cache = cache_of(&drawings);
        assert_eq!(cache.key.end_slot, 2, "edge is the newest closed bar");
        assert_eq!(cache.bars_total, 3);
        assert_eq!(cache.profile.expect("tape").0.total_volume(), dec("6"));

        // A new bar closes: the same drawing re-keys and grows on its own.
        state.ingest_live(&trade(7, "105", "1", Side::Buy));
        assert_eq!(state.bars().len(), 4);
        refresh(&mut drawings, &inputs(&state, false));
        let grown = cache_of(&drawings);
        assert_eq!(grown.key.end_slot, 3);
        assert_eq!(grown.profile.expect("tape").0.total_volume(), dec("8"));

        // The forming bar joins too, through the snapshot. (Trade 7 closed
        // bar 3 exactly, so a fresh print opens the live bar first.)
        state.ingest_live(&trade(8, "106", "1", Side::Sell));
        let partial = state.partial_footprint().cloned();
        assert!(partial.is_some());
        let with_partial = RefreshInputs {
            partial_ladder: partial.as_ref(),
            partial_version: 1,
            ..inputs(&state, false)
        };
        refresh(&mut drawings, &with_partial);
        assert!(cache_of(&drawings).key.include_partial);

        // Off again: back to exactly the anchors' own range.
        drawings.items_mut()[0]
            .payload
            .as_any_mut()
            .downcast_mut::<FrvpPayload>()
            .unwrap()
            .extend_right = false;
        refresh(&mut drawings, &with_partial);
        let released = cache_of(&drawings);
        assert_eq!(released.key.end_slot, 0);
        assert_eq!(released.profile.expect("tape").0.total_volume(), dec("2"));
    }

    /// The map's boundary is presentation state: it lands on the cache every
    /// refresh — including key hits — and never re-keys the fold. A growing
    /// heatmap must move the paint's cut, not re-merge anything.
    #[test]
    fn heat_boundary_rides_the_cache_without_rekeying_the_fold() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);
        refresh(&mut drawings, &inputs(&state, false));
        let before = cache_of(&drawings);
        assert_eq!(before.heat_first_slot, None);

        let with_heat = RefreshInputs {
            heat_first_slot: Some(1),
            ..inputs(&state, false)
        };
        refresh(&mut drawings, &with_heat);
        let after = cache_of(&drawings);
        assert_eq!(after.key, before.key, "the fold's key is untouched");
        assert_eq!(after.heat_first_slot, Some(1));
    }

    #[test]
    fn value_area_fraction_rides_the_cache_key() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);
        refresh(&mut drawings, &inputs(&state, false));
        let before = cache_of(&drawings);

        drawings.items_mut()[0]
            .payload
            .as_any_mut()
            .downcast_mut::<FrvpPayload>()
            .unwrap()
            .value_area_pct = 90;
        refresh(&mut drawings, &inputs(&state, false));
        assert_ne!(cache_of(&drawings).key, before.key);
    }
}
