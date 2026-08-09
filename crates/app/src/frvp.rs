//! Refresh pass for fixed-range volume profile drawings.
//!
//! The drawing owns *where* (two anchors, a bar range); the engine owns
//! *what* ([`VolumeProfile::merge`] over that range's footprint ladders);
//! this module is the bridge that keeps the two current. It runs once per
//! frame per pane, before the drawings paint, and it re-merges only when
//! something the profile depends on actually changed — the cache key names
//! every such input, so the common frame costs one key comparison per
//! profile object and no folding at all.
//!
//! Never on the per-trade path: ingestion does not know this module exists.

use quantick_engine::{BarFootprint, DEFAULT_LEVEL_CAP, VolumeProfile};
use rust_decimal::Decimal;

use crate::drawings::{Drawings, FrvpCache, FrvpCacheKey, FrvpEmpty, FrvpPayload};
use crate::state::ChartState;

/// The registry id of the fixed-range-profile tool — the one string this
/// module and the pane share to recognise a profile object.
pub const TOOL_ID: &str = "fixed-range-profile";

/// Everything one refresh reads. The partial ladder comes in as the pane's
/// throttled snapshot (not the live one), so the forming bar re-merges at
/// the snapshot cadence, never per paint.
pub struct RefreshInputs<'a> {
    pub state: &'a ChartState,
    /// Venue history candles before the trade-built bars. They carry no tape
    /// and therefore no ladder; a range over them is *partial coverage* and
    /// the cache says so.
    pub prefix_len: usize,
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
}

/// Bring every fixed-range-profile drawing's cached profile up to date.
///
/// Mutates only derived payload state ([`FrvpCache`]), which is excluded
/// from payload equality — so this pass can never register as a user edit
/// in the undo history, however often it runs.
pub fn refresh(drawings: &mut Drawings, inputs: &RefreshInputs<'_>) {
    for drawing in drawings.items_mut() {
        if drawing.tool.id() != TOOL_ID || drawing.points.len() < 2 {
            continue;
        }
        let (a, b) = (drawing.points[0].bar, drawing.points[1].bar);
        let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<FrvpPayload>() else {
            continue;
        };
        refresh_one(payload, a.min(b), a.max(b), inputs);
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
            refresh_one(payload, a.min(b), a.max(b), inputs);
        }
    }
}

/// The slots a `[min_bar, max_bar]` anchor span covers: every slot whose
/// centre (`slot + 0.5`) falls inside it, clamped to the slots that exist.
/// `last_slot` is the partial's slot when there is one, else the last closed.
fn covered_slots(min_bar: f32, max_bar: f32, last_slot: Option<usize>) -> Option<(usize, usize)> {
    let last = last_slot?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let start = (min_bar - 0.5).ceil().max(0.0) as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let end = (max_bar - 0.5).floor().max(0.0) as usize;
    let start = start.min(last);
    let end = end.min(last);
    (start <= end).then_some((start, end))
}

fn refresh_one(payload: &mut FrvpPayload, min_bar: f32, max_bar: f32, inputs: &RefreshInputs<'_>) {
    let closed_len = inputs.state.bars().len();
    let closed_total = inputs.prefix_len + closed_len;
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
    };
    if let Some(cache) = payload.cache.as_mut().filter(|cache| cache.key == key) {
        // Key hit: the fold is current. Presentation state still follows the
        // frame — the map's boundary moves without invalidating the merge.
        cache.heat_first_slot = inputs.heat_first_slot;
        return;
    }

    if inputs.blocked {
        payload.cache = Some(FrvpCache {
            key,
            profile: None,
            empty: Some(FrvpEmpty::Blocked),
            bars_covered: 0,
            bars_total,
            heat_first_slot: inputs.heat_first_slot,
        });
        return;
    }

    // The state-bar ladders the span reaches. Prefix slots have no ladder by
    // construction; the difference between what the span asks for and what
    // the tape can answer is `bars_covered < bars_total`, spoken by paint.
    let ladders_all = inputs.state.bar_footprints();
    let mut ladders: Vec<&BarFootprint> = Vec::new();
    if span.is_some() {
        // State-bar coverage exists only where the span reaches past the
        // prefix and into the closed bars at all.
        if end_slot >= inputs.prefix_len && start_slot < closed_total && closed_total > 0 {
            let lo = start_slot.max(inputs.prefix_len) - inputs.prefix_len;
            let hi_state = end_slot.min(closed_total - 1) - inputs.prefix_len;
            if lo <= hi_state {
                for ladder in ladders_all.iter().skip(lo).take(hi_state - lo + 1) {
                    // A bar whose ladder printed nothing contributes nothing;
                    // it still counts as covered — the tape answered "no
                    // trades", which is data, not absence of data.
                    ladders.push(ladder);
                }
            }
        }
        if include_partial && let Some(partial) = inputs.partial_ladder {
            ladders.push(partial);
        }
    }
    let bars_covered = ladders.len();

    let profile = VolumeProfile::merge(ladders, DEFAULT_LEVEL_CAP).map(|profile| {
        let fraction = Decimal::from(payload.value_area_pct) / Decimal::ONE_HUNDRED;
        let value_area = profile.value_area(fraction);
        (profile, value_area)
    });
    let empty = profile.is_none().then_some(FrvpEmpty::NoTape);
    payload.cache = Some(FrvpCache {
        key,
        profile,
        empty,
        bars_covered,
        bars_total,
        heat_first_slot: inputs.heat_first_slot,
    });
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
            prefix_len: 0,
            partial_ladder: None,
            partial_version: 0,
            blocked,
            side_inferred: false,
            heat_first_slot: None,
            draft_hover_bar: None,
        }
    }

    #[test]
    fn refresh_merges_the_covered_closed_bars() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.9);

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

    #[test]
    fn refresh_skips_when_nothing_changed_and_recomputes_on_anchor_move() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.9);

        refresh(&mut drawings, &inputs(&state, false));
        let first = cache_of(&drawings);
        refresh(&mut drawings, &inputs(&state, false));
        assert_eq!(
            cache_of(&drawings).key,
            first.key,
            "same inputs, same key, no recompute"
        );

        // Narrow the range to one bar: the key and the merge both change.
        drawings.move_anchor(0, 1, ChartPoint::at(0.9, 105.0));
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
        place_frvp(&mut drawings, 0.0, 2.9);
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
        place_frvp(&mut drawings, 0.0, 3.9);

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
        drawings.move_anchor(0, 1, ChartPoint::at(2.9, 105.0));
        refresh(&mut drawings, &with_partial);
        let cache = cache_of(&drawings);
        assert!(!cache.key.include_partial);
        assert_eq!(cache.profile.expect("tape").0.total_volume(), dec("6"));
    }

    #[test]
    fn prefix_candles_count_toward_total_but_not_covered() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        // Slots 0-1 are venue prefix, 2-4 are the three state bars.
        place_frvp(&mut drawings, 0.0, 4.9);
        let with_prefix = RefreshInputs {
            prefix_len: 2,
            ..inputs(&state, false)
        };
        refresh(&mut drawings, &with_prefix);
        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_total, 5);
        assert_eq!(cache.bars_covered, 3, "prefix candles carry no tape");
        assert_eq!(cache.profile.expect("tape").0.total_volume(), dec("6"));
    }

    #[test]
    fn a_range_entirely_on_the_prefix_is_no_tape() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 1.9);
        let with_prefix = RefreshInputs {
            prefix_len: 2,
            ..inputs(&state, false)
        };
        refresh(&mut drawings, &with_prefix);
        let cache = cache_of(&drawings);
        assert_eq!(cache.bars_total, 2);
        assert_eq!(cache.bars_covered, 0);
        assert!(cache.profile.is_none());
        assert_eq!(cache.empty, Some(FrvpEmpty::NoTape));
    }

    #[test]
    fn blocked_capability_stores_the_block_not_a_profile() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.9);
        refresh(&mut drawings, &inputs(&state, true));
        let cache = cache_of(&drawings);
        assert!(cache.profile.is_none());
        assert_eq!(cache.empty, Some(FrvpEmpty::Blocked));
    }

    #[test]
    fn refresh_never_touches_the_undo_history() {
        let state = state_with_tape();
        let mut drawings = Drawings::default();
        place_frvp(&mut drawings, 0.0, 2.9);
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
        place_frvp(&mut drawings, 0.0, 0.9);
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
        place_frvp(&mut drawings, 0.0, 2.9);
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
        place_frvp(&mut drawings, 0.0, 2.9);
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
