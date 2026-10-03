//! Exact footprint grouping and compact quantity/zone presentation, shared
//! by chart adapters without a renderer.
use quantick_engine::{BarFootprint, FootprintLevel, Side, StackedZone};
use rust_decimal::{
    Decimal,
    prelude::{FromPrimitive as _, ToPrimitive as _},
};
use std::collections::BTreeMap;

pub use crate::constants::ADAPTIVE_FLOOR_BARS;
use crate::constants::{
    ADAPTIVE_FLOOR_PERCENTILE, QTY_MILLIONS_FROM, QTY_THOUSANDS_FROM, QTY_WHOLE_FROM,
};

/// Fold a ladder onto rows `k` buckets tall. `k = 1` is the identity; the
/// merge is exact because display buckets are integer multiples of capture
/// buckets sharing the zero anchor.
pub fn regroup(fp: &BarFootprint, k: i64) -> BTreeMap<i64, FootprintLevel> {
    let mut rows: BTreeMap<i64, FootprintLevel> = BTreeMap::new();
    for (&bucket, level) in fp.levels() {
        let row = rows.entry(bucket.div_euclid(k)).or_default();
        row.buy = row.buy.saturating_add(level.buy);
        row.sell = row.sell.saturating_add(level.sell);
        row.trade_count += level.trade_count;
    }
    rows
}

/// Abbreviate a quantity for a fixed-width cell: `58.1k`, `1.2M`, `736`,
/// `0.523`. Three decimals below 1 (a 1-minute BTC row's delta usually
/// lives there), two up to 100, so a dense ladder's cells stay the same
/// visual weight.
pub fn fmt_qty(qty: Decimal) -> String {
    let value = qty.to_f64().unwrap_or(0.0);
    let magnitude = value.abs();
    // Suffix thresholds sit at the value that *rounds* to the next unit:
    // 999.96k would print "1000.0k" — seven glyphs where the cell budget
    // assumes five — so it rolls to "1.0M" instead.
    if magnitude >= QTY_MILLIONS_FROM {
        format!("{:.1}M", value / 1_000_000.0)
    } else if magnitude >= QTY_THOUSANDS_FROM {
        format!("{:.1}k", value / 1_000.0)
    } else if magnitude >= QTY_WHOLE_FROM {
        format!("{value:.0}")
    } else if value == value.trunc() {
        // A whole number of contracts is written as one. "92.00" spends two
        // fifths of a cell on characters that carry nothing, and in a ladder
        // that width is not free — it is taken out of the font size every
        // other number is drawn at. Instruments that trade in fractions still
        // get their decimals below.
        format!("{value:.0}")
    } else if magnitude >= 1.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

/// A delta for display: a value that *rounds* to zero prints as an unsigned
/// `"0"` — "-0.00" reads as broken software, and the sign on nothing is a
/// wrong-side whisper. Returns `None` exactly when the row is balanced at
/// display resolution, so callers can also skip the winner color.
pub fn fmt_delta(delta: Decimal) -> Option<String> {
    let text = fmt_qty(delta);
    if text
        .trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        return None;
    }
    Some(text)
}

/// A bar's whole-ladder delta: who won the bar. Saturating, like every
/// other quantity fold here — a corrupt feed must not panic the paint.
pub fn bar_delta(fp: &BarFootprint) -> Decimal {
    fp.levels()
        .values()
        .fold(Decimal::ZERO, |sum, cell| sum.saturating_add(cell.delta()))
}

/// One stacked zone spanning one or more adjacent bars, in display buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneMark {
    pub first_slot: usize,
    pub last_slot: usize,
    pub low_bucket: i64,
    pub high_bucket: i64,
    pub side: Side,
}

/// Coalesce per-bar stacked zones across adjacent bars: a zone continuing at
/// an overlapping price range in the next bar is one market fact, not two
/// marks. Then keep at most `cap`, tallest stacks first — and report whether
/// anything was dropped, because a silently thinned signal reads as absence.
pub fn coalesce_zones(mut zones: Vec<(usize, StackedZone)>, cap: usize) -> (Vec<ZoneMark>, bool) {
    zones.sort_by_key(|(slot, zone)| (*slot, zone.low_bucket, zone.side == Side::Sell));
    let mut marks: Vec<ZoneMark> = Vec::new();
    for (slot, zone) in zones {
        let merged = marks.iter_mut().find(|mark| {
            mark.side == zone.side
                && slot > mark.first_slot
                && slot <= mark.last_slot + 1
                && zone.low_bucket <= mark.high_bucket
                && zone.high_bucket >= mark.low_bucket
        });
        match merged {
            Some(mark) => {
                mark.last_slot = mark.last_slot.max(slot);
                mark.low_bucket = mark.low_bucket.min(zone.low_bucket);
                mark.high_bucket = mark.high_bucket.max(zone.high_bucket);
            }
            None => marks.push(ZoneMark {
                first_slot: slot,
                last_slot: slot,
                low_bucket: zone.low_bucket,
                high_bucket: zone.high_bucket,
                side: zone.side,
            }),
        }
    }
    let dropped = marks.len() > cap;
    if dropped {
        // Tallest stacks carry the most memory; ties resolve by place so the
        // pick is deterministic frame over frame.
        marks.sort_by_key(|mark| {
            (
                std::cmp::Reverse(mark.high_bucket - mark.low_bucket),
                mark.first_slot,
                mark.low_bucket,
            )
        });
        marks.truncate(cap);
        marks.sort_by_key(|mark| (mark.first_slot, mark.low_bucket));
    }
    (marks, dropped)
}

/// The adaptive imbalance quantity floor: the 60th percentile of per-row
/// total volume over the newest closed bars. One fixed number cannot serve
/// WIN contracts and BTC fractions at once (20 is right on one and absurd on
/// the other); a percentile of what is actually printing adapts to the
/// instrument and the regime. Closed bars only, independent of what is on
/// screen: a floor that moved with every live print or every pan would
/// rewrite the highlights of history while the trader reads them. The
/// config surface adds a manual override on top.
pub fn adaptive_min_qty<'a>(ladders: impl Iterator<Item = &'a BarFootprint>) -> Decimal {
    let mut volumes: Vec<f64> = ladders
        .flat_map(|fp| fp.levels().values())
        .map(|level| level.volume().to_f64().unwrap_or(0.0))
        .collect();
    if volumes.is_empty() {
        return Decimal::ZERO;
    }
    // Only the p60 is read, so partition around it instead of ordering the
    // whole vector: linear rather than n log n over up to a few thousand
    // rows. The caller caches this until its source changes.
    let index = (volumes.len().saturating_sub(1)) * ADAPTIVE_FLOOR_PERCENTILE / 100;
    let (_, p60, _) = volumes.select_nth_unstable_by(index, f64::total_cmp);
    Decimal::from_f64(*p60).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::{DEFAULT_LEVEL_CAP, FootprintBuilder, Trade};
    use std::str::FromStr as _;
    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }
    #[test]
    fn regrouping_by_integer_multiples_is_exact() {
        let mut builder = FootprintBuilder::new(dec("0.5"), DEFAULT_LEVEL_CAP);
        for (i, price) in ["100.0", "100.5", "101.0", "101.5"].iter().enumerate() {
            builder.push(&Trade {
                agg_id: i as u64,
                timestamp_ms: i as i64,
                price: dec(price),
                quantity: dec("1"),
                side: Side::Buy,
            });
        }
        let fp = builder.close().unwrap();
        let rows = regroup(&fp, 2);
        // Buckets 200..=203 halve into rows 100 and 101, two units each.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[&100].buy, dec("2"));
        assert_eq!(rows[&101].buy, dec("2"));
        // k = 1 is the identity.
        assert_eq!(&regroup(&fp, 1), fp.levels());
    }

    #[test]
    fn quantities_abbreviate_into_fixed_weight_cells() {
        assert_eq!(fmt_qty(dec("58100")), "58.1k");
        assert_eq!(fmt_qty(dec("1230000")), "1.2M");
        assert_eq!(fmt_qty(dec("736")), "736");
        assert_eq!(fmt_qty(dec("0.5234")), "0.523");
        assert_eq!(fmt_qty(dec("12.345")), "12.35");
        assert_eq!(fmt_qty(dec("-1500")), "-1.5k");
        // A value that rounds past its suffix rolls to the next one: never
        // "1000.0k" — seven glyphs where the cell budget assumes five.
        assert_eq!(fmt_qty(dec("999960")), "1.0M");
        assert_eq!(fmt_qty(dec("999.96")), "1.0k");
    }

    /// A delta that rounds to zero at display resolution has no sign and no
    /// text at all: "-0.00" reads as broken software, and the minus on
    /// nothing is a wrong-side whisper (panel must-fix).
    #[test]
    fn display_zero_deltas_are_never_signed() {
        assert_eq!(fmt_delta(dec("-0.0004")), None);
        assert_eq!(fmt_delta(dec("0.0003")), None);
        assert_eq!(fmt_delta(dec("0")), None);
        assert_eq!(fmt_delta(dec("-0.43")).as_deref(), Some("-0.430"));
        assert_eq!(fmt_delta(dec("58100")).as_deref(), Some("58.1k"));
    }

    #[test]
    fn adjacent_bars_at_one_price_coalesce_into_one_zone_mark() {
        let zone = |low: i64, high: i64| StackedZone {
            low_bucket: low,
            high_bucket: high,
            side: Side::Buy,
        };
        let (marks, dropped) = coalesce_zones(
            vec![
                (10, zone(100, 103)),
                (11, zone(101, 104)),
                (14, zone(100, 103)),
            ],
            24,
        );
        assert!(!dropped);
        assert_eq!(
            marks,
            vec![
                ZoneMark {
                    first_slot: 10,
                    last_slot: 11,
                    low_bucket: 100,
                    high_bucket: 104,
                    side: Side::Buy,
                },
                // Slot 14 does not touch slot 11: a separate market fact.
                ZoneMark {
                    first_slot: 14,
                    last_slot: 14,
                    low_bucket: 100,
                    high_bucket: 103,
                    side: Side::Buy,
                },
            ]
        );
    }

    #[test]
    fn the_zone_cap_keeps_the_tallest_stacks_and_says_it_dropped_some() {
        let zones: Vec<(usize, StackedZone)> = (0..40)
            .map(|i| {
                (
                    i * 2, // gaps, so nothing coalesces
                    StackedZone {
                        low_bucket: 1000 + i as i64 * 10,
                        high_bucket: 1000 + i as i64 * 10 + (i as i64 % 7),
                        side: Side::Sell,
                    },
                )
            })
            .collect();
        let (marks, dropped) = coalesce_zones(zones, 5);
        assert!(dropped);
        assert_eq!(marks.len(), 5);
        // Every survivor is at least as tall as the tallest loser would be.
        assert!(
            marks
                .iter()
                .all(|mark| mark.high_bucket - mark.low_bucket >= 5)
        );
    }

    #[test]
    fn the_adaptive_floor_reads_the_screens_own_percentile() {
        let mut builder = FootprintBuilder::new(dec("1"), DEFAULT_LEVEL_CAP);
        for (i, qty) in ["1", "2", "3", "4", "100"].iter().enumerate() {
            builder.push(&Trade {
                agg_id: i as u64,
                timestamp_ms: i as i64,
                price: Decimal::from(100 + i as i64),
                quantity: dec(qty),
                side: Side::Buy,
            });
        }
        let fp = builder.close().unwrap();
        let floor = adaptive_min_qty(std::iter::once(&fp));
        // Five levels, p60 lands on the third-smallest volume: one big
        // print does not drag the floor up to itself.
        assert_eq!(floor, dec("3"));
        assert_eq!(adaptive_min_qty(std::iter::empty()), Decimal::ZERO);
    }

    /// The bar's delta is the sum of its rows', and a bar balanced at
    /// display resolution prints no chip at all.
    #[test]
    fn a_bars_delta_is_the_sum_of_its_rows() {
        let mut builder = FootprintBuilder::new(dec("1"), DEFAULT_LEVEL_CAP);
        for (i, (price, qty, side)) in [
            ("100", "3", Side::Buy),
            ("101", "1", Side::Sell),
            ("102", "0.5", Side::Buy),
        ]
        .into_iter()
        .enumerate()
        {
            builder.push(&Trade {
                agg_id: i as u64,
                timestamp_ms: i as i64,
                price: dec(price),
                quantity: dec(qty),
                side,
            });
        }
        assert_eq!(bar_delta(&builder.close().unwrap()), dec("2.5"));

        let mut builder = FootprintBuilder::new(dec("1"), DEFAULT_LEVEL_CAP);
        for (i, side) in [Side::Buy, Side::Sell].into_iter().enumerate() {
            builder.push(&Trade {
                agg_id: i as u64,
                timestamp_ms: i as i64,
                price: dec("100"),
                quantity: dec("2"),
                side,
            });
        }
        let flat = builder.close().unwrap();
        assert_eq!(bar_delta(&flat), Decimal::ZERO);
        assert_eq!(fmt_delta(bar_delta(&flat)), None, "no winner, no chip");
    }
}
