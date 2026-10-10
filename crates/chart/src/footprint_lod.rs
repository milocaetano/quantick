//! The footprint layer's level of detail: which [`DetailLevel`] a zoom
//! supports and which display multiple of the capture grid its rows take,
//! the row side held through auto-fit jitter by a two-sided dead band, the
//! width side strict so a level enters and leaves at the same zoom.
//!
//! The effective level is the *minimum* of what the candle width and the
//! price-row height allow; a thin base row is first answered by coarsening
//! the display grouping (an integer multiple from [`GROUP_SNAP`]) before a
//! level is dropped. Every answer here is a pure function of pixels; the
//! window asks, then paints.

pub use crate::constants::{
    AXIS_CLEARANCE_PX, COMPACT_FROM_PX, COMPACT_MIN_ROW, COMPACT_MIN_WIDTH, DETAILED_FROM_PX,
    DETAILED_MIN_ROW, DETAILED_MIN_WIDTH, GLYPH_EM, GROUP_SNAP, LADDER_MIN_FONT_PX,
    LEVEL_ENTRY_MARGIN, MARKS_MIN_WIDTH, PROFILE_MIN_WIDTH, QUANTITY_GLYPHS, QUANTITY_PADDING_PX,
    QUANTITY_PX, TYPICAL_BODY_FRAC,
};
use crate::constants::{LEVEL_HYSTERESIS, PROFILE_HYSTERESIS};

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
    /// Resolve zoom detail. Candle width is answered strictly, so a level
    /// enters and leaves at the same width (`compact_min`, `detailed_min`);
    /// row height carries a two-sided dead band against auto-fit jitter
    /// (`LEVEL_HYSTERESIS`, Marks/Profile the narrower `PROFILE_HYSTERESIS`).
    /// `profile_row_px` configures Profile's row floor; the first frame takes
    /// the strict answer.
    pub fn resolve(
        &mut self,
        candle_width: f32,
        base_row_px: f32,
        profile_row_px: f32,
        compact_min: f32,
        detailed_min: f32,
    ) -> DetailLevel {
        let at_zoom = |factor| {
            level_for(
                candle_width,
                base_row_px * factor,
                profile_row_px,
                compact_min,
                detailed_min,
            )
        };
        let strict = at_zoom(1.0);
        let hysteresis = if matches!(
            (self.level, strict),
            (Some(DetailLevel::Profile), DetailLevel::Marks)
                | (Some(DetailLevel::Marks), DetailLevel::Profile)
        ) {
            PROFILE_HYSTERESIS
        } else {
            LEVEL_HYSTERESIS
        };
        let level = match self.level {
            // Only one level of jitter is sticky. Larger gaps can come from
            // stale auto-fit spans and must snap to the strict answer.
            Some(current) if (strict as i8 - current as i8).abs() > 1 => strict,
            Some(current) if strict < current => {
                let relaxed = at_zoom(hysteresis);
                if relaxed < current { strict } else { current }
            }
            Some(current) if strict > current => {
                let confirmed = at_zoom(1.0 / hysteresis);
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
/// row floor. `compact_min` and `detailed_min` are the candle widths the two
/// text levels start at.
#[must_use]
pub fn level_for(
    candle_width: f32,
    base_row_px: f32,
    profile_row_px: f32,
    compact_min: f32,
    detailed_min: f32,
) -> DetailLevel {
    let row_reachable = |min_row: f32| display_multiple(base_row_px, min_row).is_some();
    if candle_width >= detailed_min && row_reachable(DETAILED_MIN_ROW) {
        DetailLevel::Detailed
    } else if candle_width >= compact_min && row_reachable(COMPACT_MIN_ROW) {
        DetailLevel::Compact
    } else if candle_width >= PROFILE_MIN_WIDTH && row_reachable(profile_row_px) {
        DetailLevel::Profile
    } else if candle_width >= MARKS_MIN_WIDTH {
        DetailLevel::Marks
    } else {
        DetailLevel::Off
    }
}

/// Where a style's Detailed starts, in the floors' units: the trader's ladder
/// entry width (`detailed_from_px`) carried to the style by its typographic
/// floor's ratio to the ladder's, so one setting moves every style. A width
/// past the deepest zoom `detail_scale` allows is answered at that zoom
/// instead — never under the style's own `typographic` floor — so a style the
/// registry offers stays drawable.
#[must_use]
pub fn detailed_entry_width(typographic: f32, detailed_from_px: f32, detail_scale: f32) -> f32 {
    let deepest = crate::viewport::MAX_CANDLE_WIDTH / detail_scale.max(1e-3);
    (typographic * detailed_from_px / DETAILED_MIN_WIDTH).min(deepest.max(typographic))
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

    /// One wheel notch: 50 px of scroll at the chart's `SCROLL_ZOOM_PX` of
    /// 300 zooms by 2^(50/300).
    const WHEEL_STEP: f32 = 1.122_462;

    #[test]
    fn a_level_is_left_at_the_same_width_it_was_entered() {
        // Zoom in notch by notch from Off to past Detailed, then back out by
        // the same notches: every width must show the same level both ways,
        // or the trader feels the dead band as a wheel that does not reverse.
        let widths: Vec<f32> = (0..30).map(|i| 4.0 * WHEEL_STEP.powi(i)).collect();
        let detailed = DETAILED_FROM_PX;
        let mut memory = LevelMemory::default();
        let mut zoom = |width: f32| memory.resolve(width, 12.0, 4.0, COMPACT_FROM_PX, detailed);
        let entering: Vec<DetailLevel> = widths.iter().map(|w| zoom(*w)).collect();
        let mut leaving: Vec<DetailLevel> = widths.iter().rev().map(|w| zoom(*w)).collect();
        leaving.reverse();
        assert_eq!(entering.first(), Some(&DetailLevel::Off));
        assert_eq!(entering.last(), Some(&DetailLevel::Detailed));
        assert_eq!(entering, leaving, "widths: {widths:?}");
    }

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
