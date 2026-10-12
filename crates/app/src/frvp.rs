//! The window's side of fixed-range volume profile drawings.
//!
//! The refresh pass itself — [`refresh`], [`RefreshInputs`], [`TOOL_ID`] and
//! [`DEFAULT_FOLD_BUDGET`] — lives in `quantick_drawings::frvp`, with its
//! tests, and is re-exported here for the pane. What stays is what belongs
//! to the process: [`fold_budget`], the per-pass fold budget the pane hands
//! every refresh, and the `QUANTICK_FRVP_FOLD_BUDGET` capture hook that
//! overrides it.

#[cfg(test)]
use crate::drawings::{Drawings, FrvpPayload};
#[cfg(test)]
use crate::state::ChartState;
#[cfg(test)]
use quantick_anchored_studies::RangeProfile as FrvpCache;

pub use quantick_drawings::frvp::{DEFAULT_FOLD_BUDGET, RefreshInputs, TOOL_ID, refresh};

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
        if let Some(budget) = crate::hooks::captured::var("QUANTICK_FRVP_FOLD_BUDGET")
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|budget| *budget > 0)
        {
            return budget;
        }
        DEFAULT_FOLD_BUDGET
    })
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
}
