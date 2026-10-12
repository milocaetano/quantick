//! Refresh pass for fixed-range volume profile drawings.
//!
//! The drawing owns *where* (two anchors, a bar range); the engine owns
//! *what* ([`quantick_engine::ProfileFold`] over that range's bars); this
//! module is the bridge that keeps the two current. The window runs it once
//! per frame per pane, before the drawings paint, and it re-folds only when
//! something the profile depends on actually changed. How much one pass may
//! fold is handed in ([`RefreshInputs::budget`]); the window owns the
//! process-wide number and its capture hook.

use quantick_anchored_studies::{ProfileInputs, ProfileRequest, RangeProfile};
use quantick_chart::state::ChartState;
use quantick_engine::{Bar, BarFootprint};

use crate::{DrawingTool, Drawings, FrvpPayload};

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

/// Everything one refresh reads. The partial ladder comes in as the pane's
/// throttled snapshot (not the live one), so the forming bar re-merges at
/// the snapshot cadence, never per paint.
pub struct RefreshInputs<'a> {
    pub state: &'a ChartState,
    /// How much folding one pass may do — the window's `fold_budget`, read
    /// once by the pane. Injected rather than read here so a test can state the budget it
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
    /// the pane's `partial_bucket_slot`.
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
/// Mutates only derived payload state ([`RangeProfile`]), which is excluded
/// from payload equality — so this pass can never register as a user edit
/// in the undo history, however often it runs.
pub fn refresh(drawings: &mut Drawings<DrawingTool>, inputs: &RefreshInputs<'_>) -> bool {
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
