//! Refresh pass for fixed-range volume profile drawings.
//!
//! The drawing owns *where* (two anchors, a bar range); the engine owns
//! *what* ([`quantick_engine::ProfileFold`] over that range's bars); this module is the bridge
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

#[cfg(test)]
use quantick_anchored_studies::FrvpEmpty;
use quantick_anchored_studies::{ProfileInputs, ProfileRequest, RangeProfile};
use quantick_engine::{Bar, BarFootprint};

use crate::drawings::{Drawings, FrvpPayload};
use crate::state::ChartState;
#[cfg(test)]
use quantick_anchored_studies::RangeProfile as FrvpCache;

/// The registry id of the fixed-range-profile tool — the one string this
/// module and the pane share to recognise a profile object.
pub const TOOL_ID: &str = "fixed-range-profile";

/// How much folding one refresh pass may do, counted in bars for a venue
/// candle and in ladder rows for a bar with tape.
///
/// The unit is "one map touch": a venue candle joins as a *range* whatever
/// its width ([`quantick_engine::ProfileFold::push_candle`]), while a tape bar costs one touch
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
///
/// The pass also *reads* the fold once (the core result derivation), which the budget does not
/// count: while the fold runs that read walks the pending spreads, so the true
/// per-pass cost is somewhat above the folding alone. It falls to a map clone
/// the moment the fold is sealed, which is the state a chart spends almost all
/// of its life in.
pub const DEFAULT_FOLD_BUDGET: usize = 1_500;

/// The budget the *process* was launched with: [`DEFAULT_FOLD_BUDGET`], or
/// whatever `QUANTICK_FRVP_FOLD_BUDGET` names.
///
/// The override is not a preference knob, it is the door onto a *state*. At a
/// budget of one bar a fold advances one bar per frame, so the filling
/// profile and its progress line stay on screen as long as an operator — or a
/// capture run — needs to look at them; without it that state lasts a third of
/// a second and no screenshot can be aimed at it. A non-positive or
/// unparseable value is refused rather than guessed, and the default stands.
///
/// Read once per process by the pane, which then hands the number to every
/// refresh ([`RefreshInputs::budget`]) — so the fold never touches the
/// environment on a frame, and a test states the budget it means instead of
/// inheriting whatever the shell exported.
#[must_use]
pub fn fold_budget() -> usize {
    static BUDGET: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *BUDGET.get_or_init(|| {
        // A capture hook: compiled only with the scenario harness (or under
        // test); a default build folds at the default budget.
        #[cfg(any(feature = "scenario-harness", test))]
        if let Some(budget) = std::env::var("QUANTICK_FRVP_FOLD_BUDGET")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|budget| *budget > 0)
        {
            return budget;
        }
        DEFAULT_FOLD_BUDGET
    })
}

/// Everything one refresh reads. The partial ladder comes in as the pane's
/// throttled snapshot (not the live one), so the forming bar re-merges at
/// the snapshot cadence, never per paint.
pub struct RefreshInputs<'a> {
    pub state: &'a ChartState,
    /// How much folding one pass may do — [`fold_budget`], read once by the
    /// pane. Injected rather than read here so a test can state the budget it
    /// is asserting against, and so an operator exporting the env var cannot
    /// change what the suite means.
    pub budget: usize,
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
    /// paint's fill→outline cut, never the fold, so it lives on the drawing payload
    /// outside the computation key — a map growing must not re-merge anything.
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
    let core = ProfileInputs {
        closed: inputs.state.bars(),
        ladders: inputs.state.bar_footprints(),
        prefix: inputs.prefix,
        partial_ladder: inputs.partial_ladder,
        group: inputs.state.footprint_group(),
        series_revision: inputs.state.series_revision(),
        partial_version: inputs.partial_version,
        blocked: inputs.blocked,
        side_inferred: inputs.side_inferred,
        partial_bucket_slot: inputs.partial_bucket_slot,
        budget: inputs.budget,
    };
    let mut folding = false;
    for drawing in drawings.items_mut() {
        if drawing.tool.id() != TOOL_ID || drawing.points.len() < 2 {
            continue;
        }
        let (a, b) = (drawing.points[0].bar, drawing.points[1].bar);
        let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<FrvpPayload>() else {
            continue;
        };
        payload.heat_first_slot = inputs.heat_first_slot;
        folding |= RangeProfile::refresh(
            &mut payload.cache,
            ProfileRequest {
                min_bar: a.min(b),
                max_bar: a.max(b),
                extend_right: payload.extend_right,
                approximate_history: payload.approximate_history,
                value_area_pct: payload.value_area_pct,
            },
            &core,
        );
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
            payload.heat_first_slot = inputs.heat_first_slot;
            folding |= RangeProfile::refresh(
                &mut payload.cache,
                ProfileRequest {
                    min_bar: a.min(b),
                    max_bar: a.max(b),
                    extend_right: payload.extend_right,
                    approximate_history: payload.approximate_history,
                    value_area_pct: payload.value_area_pct,
                },
                &core,
            );
        }
    }
    folding
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_FRVP_FOLD_BUDGET"];

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
            budget: DEFAULT_FOLD_BUDGET,
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

    /// A bar closing under a range that reaches the live edge must *extend*
    /// the fold, never restart it.
    ///
    /// This is the flicker a trader reported: on a rolling replay the fold was
    /// thrown away and rebuilt from the range's first bar on every close, far
    /// faster than a long range can finish, so the drawing spent its life
    /// painting a fully-drawn histogram of the range's oldest bars — its own
    /// POC and value area, normalised against its own busiest row — and
    /// snapping to a different one the next frame.
    #[test]
    fn a_bar_closing_extends_a_live_edge_fold_instead_of_restarting_it() {
        let mut state = state_with_tape();
        let mut drawings = Drawings::default();
        // Right anchor past the newest candle: the ordinary "profile up to
        // now" drag, which clamps `end_slot` to the live edge.
        place_frvp(&mut drawings, 0.0, 40.0);
        // One bar of work per pass, so a fold in flight is observable at all.
        // With the whole fixture folding in a single pass, a restart and an
        // append reach the same numbers and the test would pass either way —
        // which is exactly what it did before the budget was tightened.
        fn tight(state: &ChartState) -> RefreshInputs<'_> {
            RefreshInputs {
                budget: 1,
                ..inputs(state, false)
            }
        }
        for _ in 0..10 {
            refresh(&mut drawings, &tight(&state));
        }
        let settled = cache_of(&drawings);
        assert!(
            !settled.output().folding,
            "ten passes at one bar each should finish"
        );
        let folded_before = settled.output().bars_folded;
        assert!(folded_before >= 3, "the fixture folds three closed bars");

        // Two more prints close another bar under the range's right edge.
        state.ingest_live(&trade(7, "105", "1", Side::Buy));
        state.ingest_live(&trade(8, "106", "1", Side::Sell));
        refresh(&mut drawings, &tight(&state));
        let after = cache_of(&drawings);

        assert!(
            after.output().bars_folded >= folded_before,
            "the fold restarted: {} bars folded on the frame after one bar \
             closed, {folded_before} before",
            after.output().bars_folded,
        );
        assert!(
            !after.output().folding,
            "an append of one ladder left the fold in flight",
        );
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
        assert_eq!(
            cache.output().bars_total,
            3,
            "three candles under the rectangle"
        );
        assert_eq!(cache.output().bars_covered, 3);
        assert_eq!(
            cache
                .output()
                .profile
                .expect("range has tape")
                .0
                .total_volume(),
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
        assert_eq!(
            cache.output().bars_partly_covered,
            1,
            "the seam bar is in range"
        );
        assert!(cache.output().key.partly_covered);
        // The caveat costs no volume: the short bar still folds its own tape.
        assert_eq!(
            cache.output().profile.expect("tape").0.total_volume(),
            dec("6"),
            "nothing is invented and nothing is dropped"
        );

        // Drag off the seam bar and the caveat goes with it.
        drawings.move_anchor(0, 0, ChartPoint::at(1.0, 100.0));
        refresh(&mut drawings, &seam);
        let cache = cache_of(&drawings);
        assert_eq!(cache.output().bars_partly_covered, 0);
        assert!(!cache.output().key.partly_covered);
    }

    /// A pane with no seam bar to speak of stays silent — the caveat is not
    /// stamped on every profile just because the plumbing exists.
    #[test]
    fn without_a_partial_bucket_no_caveat_is_claimed() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);
        refresh(&mut drawings, &inputs(&state, false));
        assert_eq!(cache_of(&drawings).output().bars_partly_covered, 0);
        assert!(!cache_of(&drawings).output().key.partly_covered);
    }

    #[test]
    fn refresh_merges_the_covered_closed_bars() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.0);

        refresh(&mut drawings, &inputs(&state, false));

        let cache = cache_of(&drawings);
        assert_eq!(cache.output().bars_total, 3);
        assert_eq!(cache.output().bars_covered, 3);
        let (profile, value_area) = cache.output().profile.expect("range has tape");
        // Six trades of qty 1 across the three closed bars.
        assert_eq!(profile.total_volume(), dec("6"));
        assert!(value_area.is_some());
        assert!(cache.output().empty.is_none());
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
        assert_eq!(cache.output().bars_total, 25_003);
        assert!(
            cache.output().folding,
            "the fold is still running after one pass"
        );
        assert!(
            cache.output().bars_approximated <= fold_budget(),
            "one pass folded {} candles, past the {} budget",
            cache.output().bars_approximated,
            fold_budget()
        );
        assert!(
            cache.output().profile.is_some(),
            "what has been folded is already on screen — a partial profile, not a blank"
        );
        assert!(
            cache.output().empty.is_none(),
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
            let folded = cache.output().bars_covered + cache.output().bars_approximated;
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
        assert!(!cache.output().folding, "the fold finished");
        assert_eq!(
            cache.output().bars_approximated,
            25_000,
            "every venue candle joined"
        );
        assert_eq!(cache.output().bars_covered, 3, "and the three tape bars");
        let (profile, value_area) = cache.output().profile.expect("the range folded");
        // 4 units per candle (1.5 buy + 2.5 sell) plus the six tape trades.
        assert_eq!(profile.total_volume(), dec("100006"));
        assert!(value_area.is_some());
        // Still one profile object: the fill must not have left the range
        // claiming more bars than the chart has.
        assert_eq!(cache.output().bars_total, 25_003);
    }

    /// A live tape must not keep a long fold from finishing.
    ///
    /// This is the bug the trader saw as flicker: prints kept landing, every
    /// one of them moved something the fold's key looked at, and the range
    /// restarted before it could finish — filling part-way, dropping what it
    /// had, beginning again, forever, and burning a budget of folding every
    /// frame to do it. So the tape here does what a tape does: a new print on
    /// every pass, and a new forming-bar snapshot with it. Neither touches the
    /// *closed* bars this fold is over, so neither may cost it its progress.
    #[test]
    fn a_long_fold_converges_while_the_tape_keeps_printing() {
        let mut state = state_with_tape();
        let prefix: Vec<quantick_engine::Bar> = (0..25_000).map(venue_candle).collect();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 25_004.0);

        let mut passes = 0u64;
        loop {
            // One print per pass — the forming bar grows and its snapshot is
            // re-taken, which is worse than the pane's real throttle.
            state.ingest_live(&trade(
                100 + passes,
                "101",
                "0.5",
                if passes.is_multiple_of(2) {
                    Side::Buy
                } else {
                    Side::Sell
                },
            ));
            let partial = state.partial_footprint().cloned();
            let moving = RefreshInputs {
                prefix: &prefix,
                partial_ladder: partial.as_ref(),
                partial_version: passes + 1,
                ..inputs(&state, false)
            };
            let folding = refresh(&mut drawings, &moving);
            passes += 1;
            assert!(
                passes < 100,
                "the fold never converged: {} of {} bars after {passes} passes",
                cache_of(&drawings).output().bars_folded,
                cache_of(&drawings).output().bars_total
            );
            if !folding {
                break;
            }
        }

        let cache = cache_of(&drawings);
        assert_eq!(
            cache.output().bars_approximated,
            25_000,
            "every candle joined once"
        );
        // The tape kept printing, so bars kept closing and the range's right
        // edge followed them: the three it started with, the forming one, and
        // whatever closed along the way.
        assert!(
            cache.output().bars_covered >= 4,
            "the tape bars joined too, not just the candles: {}",
            cache.output().bars_covered
        );
        assert!(
            cache.output().profile.expect("folded").0.total_volume() > dec("100006"),
            "every candle's volume, and the tape's on top of it"
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
        assert_eq!(cache.output().bars_total, 1_003);
        assert!(
            cache.output().bars_approximated <= 1_000,
            "the new range folded only its own candles, not the old range's"
        );
        assert!(
            !cache.output().folding,
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
        assert!(!done.output().folding);
        assert_eq!(done.output().bars_total, 25_004);
        assert_eq!(
            done.output().bars_covered,
            4,
            "three closed tape bars and the forming one"
        );
        // 25 000 candles of 4 units each, six units of closed tape, and the
        // forming bar's single print.
        let volume = done.output().profile.expect("folded").0.total_volume();
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
            after.output().bars_folded,
            done.output().bars_folded,
            "not one bar was folded again"
        );
        assert_eq!(after.output().bars_approximated, 25_000);
        assert_eq!(
            after
                .output()
                .profile
                .expect("still folded")
                .0
                .total_volume(),
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
            cache_of(&drawings).output().key,
            first.output().key,
            "same inputs, same key, no recompute"
        );

        // Narrow the range to one bar: the key and the merge both change.
        drawings.move_anchor(0, 1, ChartPoint::at(0.0, 105.0));
        refresh(&mut drawings, &inputs(&state, false));
        let narrowed = cache_of(&drawings);
        assert_ne!(narrowed.output().key, first.output().key);
        assert_eq!(narrowed.output().bars_total, 1);
        assert_eq!(
            narrowed
                .output()
                .profile
                .expect("bar 0 has tape")
                .0
                .total_volume(),
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
        assert_ne!(
            after.output().key,
            before.output().key,
            "a refold re-keys the cache"
        );
        assert_eq!(
            after.output().profile.expect("still tape").0.group(),
            dec("2")
        );
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
        assert!(cache.output().key.include_partial);
        assert_eq!(cache.output().bars_total, 4);
        assert_eq!(cache.output().bars_covered, 4);
        // Six closed trades plus the forming bar's one.
        assert_eq!(
            cache.output().profile.expect("tape").0.total_volume(),
            dec("7")
        );

        // A range that stops short of the forming bar leaves it out.
        drawings.move_anchor(0, 1, ChartPoint::at(2.0, 105.0));
        refresh(&mut drawings, &with_partial);
        let cache = cache_of(&drawings);
        assert!(!cache.output().key.include_partial);
        assert_eq!(
            cache.output().profile.expect("tape").0.total_volume(),
            dec("6")
        );
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
        assert_eq!(cache.output().bars_total, 5);
        assert_eq!(cache.output().bars_covered, 3, "tape bars stay exact");
        assert_eq!(
            cache.output().bars_approximated,
            2,
            "venue candles join, labeled"
        );
        // Six units of tape plus one unit per approximated candle.
        assert_eq!(
            cache.output().profile.expect("fold").0.total_volume(),
            dec("8")
        );
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
        assert_eq!(cache.output().bars_total, 2);
        assert_eq!(cache.output().bars_covered, 0);
        assert_eq!(cache.output().bars_approximated, 0);
        assert!(cache.output().profile.is_none());
        assert_eq!(cache.output().empty, Some(FrvpEmpty::NoTape));

        // Toggling back on re-keys and the approximated fold appears.
        drawings.items_mut()[0]
            .payload
            .as_any_mut()
            .downcast_mut::<FrvpPayload>()
            .unwrap()
            .approximate_history = true;
        refresh(&mut drawings, &with_prefix);
        let cache = cache_of(&drawings);
        assert_eq!(cache.output().bars_approximated, 2);
        assert_eq!(
            cache
                .output()
                .profile
                .expect("approximated fold")
                .0
                .total_volume(),
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
        assert!(cache.output().profile.is_none());
        assert_eq!(cache.output().empty, Some(FrvpEmpty::Blocked));
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
        assert_eq!(
            cache.output().key.end_slot,
            2,
            "edge is the newest closed bar"
        );
        assert_eq!(cache.output().bars_total, 3);
        assert_eq!(
            cache.output().profile.expect("tape").0.total_volume(),
            dec("6")
        );

        // A new bar closes: the same drawing re-keys and grows on its own.
        state.ingest_live(&trade(7, "105", "1", Side::Buy));
        assert_eq!(state.bars().len(), 4);
        refresh(&mut drawings, &inputs(&state, false));
        let grown = cache_of(&drawings);
        assert_eq!(grown.output().key.end_slot, 3);
        assert_eq!(
            grown.output().profile.expect("tape").0.total_volume(),
            dec("8")
        );

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
        assert!(cache_of(&drawings).output().key.include_partial);

        // Off again: back to exactly the anchors' own range.
        drawings.items_mut()[0]
            .payload
            .as_any_mut()
            .downcast_mut::<FrvpPayload>()
            .unwrap()
            .extend_right = false;
        refresh(&mut drawings, &with_partial);
        let released = cache_of(&drawings);
        assert_eq!(released.output().key.end_slot, 0);
        assert_eq!(
            released.output().profile.expect("tape").0.total_volume(),
            dec("2")
        );
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
        assert_eq!(
            drawings.items()[0]
                .payload
                .as_any()
                .downcast_ref::<FrvpPayload>()
                .unwrap()
                .heat_first_slot,
            None
        );

        let with_heat = RefreshInputs {
            heat_first_slot: Some(1),
            ..inputs(&state, false)
        };
        refresh(&mut drawings, &with_heat);
        let after = cache_of(&drawings);
        assert_eq!(
            after.output().key,
            before.output().key,
            "the fold's key is untouched"
        );
        assert_eq!(
            drawings.items()[0]
                .payload
                .as_any()
                .downcast_ref::<FrvpPayload>()
                .unwrap()
                .heat_first_slot,
            Some(1)
        );
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
        assert_ne!(cache_of(&drawings).output().key, before.output().key);
    }
}
