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
use crate::constants::{LEVEL_HYSTERESIS, MAX_SPLIT_HINT_STRETCH, PROFILE_HYSTERESIS};

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

/// The shortest a display row may draw at each level, in pixels: the price
/// rows merge (a coarser display multiple) until they reach it.
///
/// Profile's floor is the trader's band fineness; the two text levels share
/// one configured floor, Compact a pixel under Detailed as the constants have
/// it. Always passed whole, so no caller falls back to a default floor
/// silently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowFloors {
    /// Profile's band floor.
    pub profile: f32,
    /// Compact's row floor.
    pub compact: f32,
    /// Detailed's row floor.
    pub detailed: f32,
}

impl RowFloors {
    /// The floors from the two settings: `profile_row_px` for the bands and
    /// `number_row_px` for Detailed, Compact keeping its one-pixel lead.
    #[must_use]
    pub const fn new(profile_row_px: f32, number_row_px: f32) -> Self {
        Self {
            profile: profile_row_px,
            compact: number_row_px - (DETAILED_MIN_ROW - COMPACT_MIN_ROW),
            detailed: number_row_px,
        }
    }

    /// The floor a level's rows are held to; Marks and Off take Profile's.
    #[must_use]
    pub fn for_level(self, level: DetailLevel) -> f32 {
        match level {
            DetailLevel::Detailed => self.detailed,
            DetailLevel::Compact => self.compact,
            _ => self.profile,
        }
    }
}

/// What the row height last allowed and the row multiple last answered, per
/// pane — the sticky state the row-side dead band is measured from. Candle
/// width keeps no memory: it is answered strictly every frame.
#[derive(Debug, Default)]
pub struct LevelMemory {
    row_level: Option<DetailLevel>,
    k: Option<i64>,
}

impl LevelMemory {
    /// Resolve zoom detail: the minimum of what the candle width allows,
    /// answered strictly so a level enters and leaves at the same width
    /// (`compact_min`, `detailed_min`), and what the row height allows, held
    /// through a two-sided dead band against auto-fit jitter
    /// (`LEVEL_HYSTERESIS`, Marks/Profile the narrower `PROFILE_HYSTERESIS`).
    /// The band never touches width in either direction, so rows sitting
    /// inside it cannot make the way in differ from the way out.
    /// `floors` holds each level's row floor ([`RowFloors`]); the first
    /// frame takes the strict answer.
    pub fn resolve(
        &mut self,
        candle_width: f32,
        base_row_px: f32,
        floors: RowFloors,
        compact_min: f32,
        detailed_min: f32,
    ) -> DetailLevel {
        let by_row = |factor: f32| {
            level_for(
                f32::INFINITY,
                base_row_px * factor,
                floors,
                compact_min,
                detailed_min,
            )
        };
        let strict = by_row(1.0);
        let hysteresis = if matches!(
            (self.row_level, strict),
            (Some(DetailLevel::Profile), DetailLevel::Marks)
                | (Some(DetailLevel::Marks), DetailLevel::Profile)
        ) {
            PROFILE_HYSTERESIS
        } else {
            LEVEL_HYSTERESIS
        };
        let row_level = match self.row_level {
            // Only one level of jitter is sticky. Larger gaps can come from
            // stale auto-fit spans and must snap to the strict answer.
            Some(current) if (strict as i8 - current as i8).abs() > 1 => strict,
            Some(current) if strict < current => {
                if by_row(hysteresis) < current {
                    strict
                } else {
                    current
                }
            }
            Some(current) if strict > current => {
                if by_row(1.0 / hysteresis) >= strict {
                    strict
                } else {
                    current
                }
            }
            _ => strict,
        };
        self.row_level = Some(row_level);
        let by_width = level_for(
            candle_width,
            f32::INFINITY,
            floors,
            compact_min,
            detailed_min,
        );
        by_width.min(row_level)
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
/// row floor ([`RowFloors`]). `compact_min` and `detailed_min` are the candle
/// widths the two text levels start at.
#[must_use]
pub fn level_for(
    candle_width: f32,
    base_row_px: f32,
    floors: RowFloors,
    compact_min: f32,
    detailed_min: f32,
) -> DetailLevel {
    let row_reachable = |min_row: f32| display_multiple(base_row_px, min_row).is_some();
    if candle_width >= detailed_min && row_reachable(floors.detailed) {
        DetailLevel::Detailed
    } else if candle_width >= compact_min && row_reachable(floors.compact) {
        DetailLevel::Compact
    } else if candle_width >= PROFILE_MIN_WIDTH && row_reachable(floors.profile) {
        DetailLevel::Profile
    } else if candle_width >= MARKS_MIN_WIDTH {
        DetailLevel::Marks
    } else {
        DetailLevel::Off
    }
}

/// Where a style's Detailed starts, in the floors' units: the trader's ladder
/// entry width (`detailed_from_px`) carried to the style by its typographic
/// floor's ratio to the ladder's, so one setting moves every style, never
/// under the style's own `typographic` floor. A width past the deepest zoom
/// `detail_scale` allows is answered at that zoom instead, so a style the
/// registry offers stays drawable. On screen that is the zoom ceiling, which
/// every style's uncapped text floor fits under (pinned by
/// `every_style_is_reachable_at_some_zoom_and_every_detail_scale`), so the
/// cap never draws text that does not fit. High scales can push the full
/// ladder there; Compact's entry is capped the same way but rarely reaches
/// it.
#[must_use]
pub fn detailed_entry_width(typographic: f32, detailed_from_px: f32, detail_scale: f32) -> f32 {
    let configured = (typographic * detailed_from_px / DETAILED_MIN_WIDTH).max(typographic);
    configured.min(deepest_width(detail_scale))
}

/// Where Compact starts, in the floors' units: the trader's
/// `compact_from_px`, never past the ladder's Detailed entry (`detailed_from_px`)
/// — the summary numbers come before the full ladder, not after it — and
/// never past the deepest zoom `detail_scale` allows, so the legend's "numbers
/// at N× this width" always names a zoom the wheel can reach.
#[must_use]
pub fn compact_entry_width(compact_from_px: f32, detailed_from_px: f32, detail_scale: f32) -> f32 {
    compact_from_px
        .min(detailed_from_px)
        .min(deepest_width(detail_scale))
}

/// The widest candle the zoom can reach, in the floors' units.
fn deepest_width(detail_scale: f32) -> f32 {
    crate::viewport::MAX_CANDLE_WIDTH / detail_scale.max(1e-3)
}

/// How many times taller the rows must draw before each capture-grid row is
/// its own row, for the legend's "single rows at N× height", rounded up to a
/// tenth. The last merge only lets go once the single row clears its floor by
/// [`LevelMemory::resolve_multiple`]'s dead band, so the stretch named is
/// that one, not the bare floor. `None` when rows are not merged, when `k`
/// is the coarsest-snap fallback (`None` from `resolve_multiple`: no stretch
/// the hint could name honestly), or past [`MAX_SPLIT_HINT_STRETCH`].
#[must_use]
pub fn height_to_split(k: Option<i64>, base_row_px: f32, min_row_px: f32) -> Option<f32> {
    if k? <= 1 || base_row_px <= 0.0 {
        return None;
    }
    let taller = (min_row_px * LEVEL_HYSTERESIS / base_row_px * 10.0).ceil() / 10.0;
    (taller > 1.0 && taller <= MAX_SPLIT_HINT_STRETCH).then_some(taller)
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

    /// The default floors, as the settings ship them.
    const FLOORS: RowFloors = RowFloors::new(4.0, DETAILED_MIN_ROW);

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
        let mut zoom = |width: f32| memory.resolve(width, 12.0, FLOORS, COMPACT_FROM_PX, detailed);
        let entering: Vec<DetailLevel> = widths.iter().map(|w| zoom(*w)).collect();
        let mut leaving: Vec<DetailLevel> = widths.iter().rev().map(|w| zoom(*w)).collect();
        leaving.reverse();
        assert_eq!(entering.first(), Some(&DetailLevel::Off));
        assert_eq!(entering.last(), Some(&DetailLevel::Detailed));
        assert_eq!(entering, leaving, "widths: {widths:?}");
    }

    #[test]
    fn rows_inside_the_dead_band_do_not_make_width_asymmetric() {
        // Rows that settled inside the band below Detailed's row floor keep
        // the row side at Detailed; zooming out past the width threshold
        // drops the level, and zooming back in must restore it on the same
        // width — the band must not be re-asked of the width step.
        let coarsest = *GROUP_SNAP.last().unwrap() as f32;
        let row = |share: f32| DETAILED_MIN_ROW / coarsest * share;
        let detailed = DETAILED_FROM_PX;
        let mut memory = LevelMemory::default();
        let mut at = |width: f32, share: f32| {
            memory.resolve(width, row(share), FLOORS, COMPACT_FROM_PX, detailed)
        };
        assert_eq!(at(detailed * 1.2, 1.5), DetailLevel::Detailed);
        assert_eq!(at(detailed * 1.2, 0.95), DetailLevel::Detailed);
        assert_eq!(at(detailed * 0.99, 0.95), DetailLevel::Compact);
        assert_eq!(at(detailed * 1.01, 0.95), DetailLevel::Detailed);
        assert_eq!(at(detailed * 0.99, 0.95), DetailLevel::Compact);
    }

    #[test]
    fn entry_widths_stay_ordered_reachable_and_above_the_floor() {
        // A detailed setting under the ladder's floor is answered at the floor.
        let floor = DETAILED_MIN_WIDTH * 2.0;
        let low = detailed_entry_width(floor, DETAILED_MIN_WIDTH * 0.5, 1.0);
        assert!((low - floor).abs() < 1e-3, "{low}");
        // A compact setting past the detailed one is answered at the detailed.
        let compact = compact_entry_width(120.0, 90.0, 1.0);
        assert!((compact - 90.0).abs() < 1e-3, "{compact}");
        // And never past the deepest zoom the scale allows.
        let deepest = crate::viewport::MAX_CANDLE_WIDTH / 2.0;
        let compact = compact_entry_width(160.0, 240.0, 2.0);
        assert!((compact - deepest).abs() < 1e-3, "{compact}");
    }

    #[test]
    fn default_number_floor_keeps_the_constant_floors() {
        // The setting at its default is exactly the old fixed floors, so a
        // trader who never touches it sees the same bars.
        let floors = RowFloors::new(4.0, DETAILED_MIN_ROW);
        assert!((floors.detailed - DETAILED_MIN_ROW).abs() < f32::EPSILON);
        assert!((floors.compact - COMPACT_MIN_ROW).abs() < f32::EPSILON);
        assert!((floors.for_level(DetailLevel::Profile) - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_finer_number_floor_splits_rows_at_the_same_row_height() {
        // Same 9 px per tick: the default floor merges two ticks per row, a
        // 9 px floor gives each tick its own row - twice the rows on screen.
        let coarse = RowFloors::new(4.0, DETAILED_MIN_ROW);
        let fine = RowFloors::new(4.0, 9.0);
        for level in [DetailLevel::Detailed, DetailLevel::Compact] {
            assert_eq!(display_multiple(9.0, coarse.for_level(level)), Some(2));
            assert_eq!(display_multiple(9.0, fine.for_level(level)), Some(1));
        }
        // And the level itself arrives at a shorter row.
        let rows = DETAILED_MIN_ROW / *GROUP_SNAP.last().unwrap() as f32 * 0.8;
        let at = |floors| level_for(f32::INFINITY, rows, floors, 0.0, 0.0);
        assert_eq!(at(coarse), DetailLevel::Profile);
        assert_eq!(at(fine), DetailLevel::Detailed);
    }

    #[test]
    fn merged_rows_name_the_stretch_that_splits_them() {
        // 6 px rows under a 12 px floor merge two at a time; the hint names
        // the stretch past the dead band, and at exactly that stretch the
        // held multiple lets go - one notch short, it still holds.
        let mut memory = LevelMemory::default();
        assert_eq!(memory.resolve_multiple(6.0, 12.0), Some(2));
        let taller = height_to_split(Some(2), 6.0, 12.0).unwrap();
        assert!((taller - 2.3).abs() < 1e-4, "{taller}");
        assert_eq!(memory.resolve_multiple(6.0 * 2.0, 12.0), Some(2));
        assert_eq!(memory.resolve_multiple(6.0 * taller, 12.0), Some(1));
        // Not merged, or nothing to measure: no hint.
        assert_eq!(height_to_split(Some(1), 6.0, 12.0), None);
        assert_eq!(height_to_split(Some(2), 0.0, 12.0), None);
        // A held merge inside the dead band still names a stretch.
        assert!(height_to_split(Some(2), 12.5, 12.0).is_some());
    }

    #[test]
    fn the_split_hint_only_names_a_reachable_stretch() {
        // The coarsest-snap fallback has no honest answer.
        assert_eq!(height_to_split(None, 0.0001, 12.0), None);
        // A stretch past the bound is not a nudge the hint should promise.
        let past = 12.0 * LEVEL_HYSTERESIS / (MAX_SPLIT_HINT_STRETCH + 1.0);
        assert_eq!(height_to_split(Some(20), past, 12.0), None);
        let within = 12.0 * LEVEL_HYSTERESIS / (MAX_SPLIT_HINT_STRETCH - 1.0);
        assert!(height_to_split(Some(2), within, 12.0).is_some());
    }

    #[test]
    fn a_wide_detail_scale_caps_every_entry_at_the_deepest_zoom() {
        // A style whose entry is past the deepest zoom at a wide scale is
        // answered at that zoom: on screen that is the full ceiling, wider
        // than the text needs, so the style stays reachable and legible.
        let scale = 6.0;
        let typographic = 120.0;
        let entry = detailed_entry_width(typographic, DETAILED_FROM_PX, scale);
        let deepest = crate::viewport::MAX_CANDLE_WIDTH / scale;
        assert!((entry - deepest).abs() < 1e-3, "{entry}");
        assert!(entry * scale >= typographic, "{entry}");
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
