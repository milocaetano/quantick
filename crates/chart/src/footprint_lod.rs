//! The footprint layer's level of detail: which [`DetailLevel`] a zoom
//! supports and which display multiple of the capture grid its rows take,
//! both held through boundary jitter by a two-sided dead band.
//!
//! The effective level is the *minimum* of what the candle width and the
//! price-row height allow; a thin base row is first answered by coarsening
//! the display grouping (an integer multiple from [`GROUP_SNAP`]) before a
//! level is dropped. Every answer here is a pure function of pixels; the
//! window asks, then paints.

use crate::constants::LEVEL_HYSTERESIS;
pub use crate::constants::{
    COMPACT_MIN_ROW, COMPACT_MIN_WIDTH, DETAILED_MIN_ROW, GLYPH_EM, GROUP_SNAP, LADDER_MIN_FONT_PX,
    MARKS_MIN_WIDTH, PROFILE_MIN_WIDTH, QUANTITY_GLYPHS, QUANTITY_PADDING_PX, QUANTITY_PX,
    TYPICAL_BODY_FRAC,
};

/// How much detail the current zoom supports. Ordered: more detail is greater.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DetailLevel {
    /// Below every readable threshold: the layer paints nothing and the
    /// legend says why, because an on-but-invisible layer reads as broken.
    Off,
    /// POC dot and stacked-zone marks only. Nothing here is a number.
    Marks,
    /// Textless histogram per row, POC emphasized, zone ticks on the edge.
    Profile,
    /// One abbreviated delta number per row.
    Compact,
    /// The full sell × buy ladder with imbalance highlights and the extreme
    /// ratio badges.
    Detailed,
}

/// The detail level and the row multiple last answered, per pane — the
/// sticky state the dead band is measured from.
#[derive(Debug, Default)]
pub struct LevelMemory {
    level: Option<DetailLevel>,
    k: Option<i64>,
}

impl LevelMemory {
    /// The level this zoom supports, sticky in BOTH directions (see
    /// [`LEVEL_HYSTERESIS`]). `profile_row_px` is the configured Profile
    /// floor — the "how fine may the bands get" knob.
    ///
    /// The dead band is two-sided on purpose: the price auto-fit breathes
    /// with every pan and print, so the row height crosses a floor and
    /// crosses back with a centimetre of mouse travel. With instant
    /// upgrades against banded downgrades, the boundary blinks — up at
    /// once, down 15% later, up at once again. A change in either
    /// direction now has to clear the floor with 15% to spare before the
    /// level moves; only the very first frame takes the strict answer.
    pub fn resolve(
        &mut self,
        candle_width: f32,
        base_row_px: f32,
        profile_row_px: f32,
        detailed_min: f32,
    ) -> DetailLevel {
        let strict = level_for(candle_width, base_row_px, profile_row_px, detailed_min);
        let level = match self.level {
            // The dead band defends exactly ONE step of boundary jitter.
            // Further than that, the sticky state is not jitter — it is a
            // leftover from another zoom era (the first frames' wild
            // auto-fit spans) — and holding it is how "rows 100.00" wedges
            // on a chart whose strict answer is Detailed.
            Some(current) if (strict as i8 - current as i8).abs() > 1 => strict,
            Some(current) if strict < current => {
                let relaxed = level_for(
                    candle_width * LEVEL_HYSTERESIS,
                    base_row_px * LEVEL_HYSTERESIS,
                    profile_row_px,
                    detailed_min,
                );
                if relaxed < current { strict } else { current }
            }
            Some(current) if strict > current => {
                let confirmed = level_for(
                    candle_width / LEVEL_HYSTERESIS,
                    base_row_px / LEVEL_HYSTERESIS,
                    profile_row_px,
                    detailed_min,
                );
                if confirmed >= strict { strict } else { current }
            }
            _ => strict,
        };
        self.level = Some(level);
        level
    }

    /// The display multiple, with the same dead band the level has: the
    /// price auto-fit breathes with every new high of the live bar, and a
    /// ladder that restructures from 2-tick to 5-tick rows on one print and
    /// back on the next is unreadable. The current `k` survives until it is
    /// 15% past failing its floor, and a finer one is adopted only once it
    /// clears the floor with 15% to spare.
    pub fn resolve_multiple(&mut self, base_row_px: f32, min_row_px: f32) -> Option<i64> {
        let strict = display_multiple(base_row_px, min_row_px);
        let snap_position = |k: i64| GROUP_SNAP.iter().position(|snap| *snap == k);
        let k = match (self.k, strict) {
            (Some(current), Some(strict_k)) if current != strict_k => {
                // Same one-step rule as the level: the dead band defends
                // boundary jitter, never a multiple wedged eras away (the
                // snap quantization can leave the strict answer exactly on
                // its floor, where the 15% adoption margin is unreachable —
                // without this, a stale 10 000× from the first frames'
                // auto-fit span holds forever).
                let one_step_apart = matches!(
                    (snap_position(current), snap_position(strict_k)),
                    (Some(a), Some(b)) if a.abs_diff(b) <= 1
                );
                if !one_step_apart {
                    strict_k
                } else if strict_k > current {
                    if base_row_px * current as f32 >= min_row_px / LEVEL_HYSTERESIS {
                        current
                    } else {
                        strict_k
                    }
                } else if base_row_px * strict_k as f32 >= min_row_px * LEVEL_HYSTERESIS {
                    strict_k
                } else {
                    current
                }
            }
            (_, strict) => strict?,
        };
        self.k = Some(k);
        Some(k)
    }
}

/// What `candle_width` and the *achievable* row height allow. A thin base row
/// is not a refusal — the display grouping can merge up to [`GROUP_SNAP`]'s
/// largest multiple — so each level asks whether some multiple reaches its
/// row floor.
#[must_use]
pub fn level_for(
    candle_width: f32,
    base_row_px: f32,
    profile_row_px: f32,
    detailed_min: f32,
) -> DetailLevel {
    let row_reachable = |min_row: f32| display_multiple(base_row_px, min_row).is_some();
    if candle_width >= detailed_min && row_reachable(DETAILED_MIN_ROW) {
        DetailLevel::Detailed
    } else if candle_width >= COMPACT_MIN_WIDTH && row_reachable(COMPACT_MIN_ROW) {
        DetailLevel::Compact
    } else if candle_width >= PROFILE_MIN_WIDTH && row_reachable(profile_row_px) {
        DetailLevel::Profile
    } else if candle_width >= MARKS_MIN_WIDTH {
        DetailLevel::Marks
    } else {
        DetailLevel::Off
    }
}

/// The smallest snap multiple whose rows reach `min_row_px`, or `None` when
/// even the coarsest is too thin (a chart zoomed so far out that one snap row
/// is still under the floor).
#[must_use]
pub fn display_multiple(base_row_px: f32, min_row_px: f32) -> Option<i64> {
    GROUP_SNAP
        .into_iter()
        .find(|k| base_row_px * (*k as f32) >= min_row_px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_multiple_snaps_to_round_row_groups() {
        assert_eq!(display_multiple(12.0, 11.0), Some(1));
        assert_eq!(display_multiple(6.0, 11.0), Some(2));
        assert_eq!(display_multiple(1.0, 11.0), Some(20));
        assert_eq!(display_multiple(0.1, 11.0), Some(200));
        assert_eq!(display_multiple(0.1, 4.0), Some(50));
        // The fallback-grid regression: a 0.01 capture grid on an index
        // future leaves base rows at ~0.026 px — the ladder must still
        // reach a drawable row instead of locking the level at Marks.
        assert_eq!(display_multiple(0.026, 4.0), Some(200));
        assert_eq!(display_multiple(0.026, 12.0), Some(500));
        assert_eq!(display_multiple(0.0001, 12.0), None);
    }
}
