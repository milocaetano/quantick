//! The live strip: a narrow column between the chart body and the price
//! gutter showing the forming bar's aggression histogram (Live Edge,
//! phases 4–5; depth silhouette retired after live use — it only repeated
//! the heatmap's right edge and buried the histogram).
//!
//! Buys grow rightward from the strip's centre, sells leftward, each bar's
//! width on the same square-root area rule the bubbles use, normalized by the
//! forming bar's own biggest bucket. The rows come from the projection's
//! aggression clusters — the one engine code path — filtered to the forming
//! bar, so the histogram resets on bar close simply because the new bar has
//! no clusters yet. It works on any source that streams trades (replay
//! included); with book capture on, the best bid/ask touch lines mark the
//! real spread over it.
//!
//! This module owns the pure, testable math (bucketing, references, the
//! histogram); painting lives in `OrderflowView::draw_live_strip`, which
//! reads the published ladder's best bid/ask and the frame.

use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::orderflow::projection::AggressionPrimitive;

/// Width of the strip, in pixels. The proposal band is 72–96 px: wide enough
/// for the histogram to read, narrow enough to never crowd the chart.
pub(crate) const LIVE_STRIP_WIDTH_PX: f32 = 84.0;

/// Stroke of the best bid/ask touch markers, in pixels.
pub(crate) const TOUCH_MARKER_STROKE_PX: f32 = 1.5;

/// Alpha of the strip's left border line, against the chart body.
pub(crate) const STRIP_BORDER_ALPHA: f32 = 0.3;

/// Left inset of the strip's content, in pixels, so the border stays visible.
pub(crate) const STRIP_ROW_INSET_PX: f32 = 1.0;

/// Opacity of the histogram bars.
pub(crate) const HISTOGRAM_ALPHA: f32 = 0.8;

/// Widest histogram bar, as a fraction of the strip's half width, leaving a
/// sliver of background visible even at full scale.
pub(crate) const HISTOGRAM_MAX_HALF_FRAC: f32 = 0.94;

/// One histogram row: the forming bar's aggression at one price bucket,
/// both sides together because the drawing mirrors them around one centre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistogramRow {
    /// Inclusive lower price edge, in the projection's own bucket space.
    pub price_bucket: Decimal,
    /// Price height the row covers from its lower edge — the widest span any
    /// contributing mark declared. One visual row on a plain tape; a regional
    /// fold's whole region when the regional fold is on, so a four-row
    /// region's quantity is never drawn as one row's bar.
    pub price_span: Decimal,
    /// Summed buy-aggression quantity in the bucket.
    pub buy: Decimal,
    /// Summed sell-aggression quantity in the bucket.
    pub sell: Decimal,
}

/// Split one mark's quantity into the two sides it actually carries.
///
/// Single-sided marks — every tape print — land wholly on their own side, with
/// no arithmetic and no rounding. Only a two-sided summary is divided, by the
/// buy share the projection already computed.
fn split_by_side(cluster: &AggressionPrimitive) -> (Decimal, Decimal) {
    let share = f64::from(cluster.buy_share);
    if !share.is_finite() || share >= 1.0 {
        return match cluster.side {
            Side::Buy => (cluster.quantity, Decimal::ZERO),
            Side::Sell => (Decimal::ZERO, cluster.quantity),
        };
    }
    if share <= 0.0 {
        return (Decimal::ZERO, cluster.quantity);
    }
    let Ok(buy_share) = Decimal::try_from(share) else {
        return match cluster.side {
            Side::Buy => (cluster.quantity, Decimal::ZERO),
            Side::Sell => (Decimal::ZERO, cluster.quantity),
        };
    };
    // Rounded to the quantity's own scale: the share is an `f32`, so the raw
    // product carries float noise a contract count never has. The remainder is
    // taken by subtraction, so the two sides still add up to exactly what
    // traded whatever the rounding did.
    let buy = (cluster.quantity * buy_share).round_dp(cluster.quantity.scale());
    (buy, cluster.quantity - buy)
}

/// Sum the forming bar's aggression clusters per price bucket. `bar_open_ms`
/// is the forming bar's open time: clusters that ended before it belong to
/// closed bars and are dropped, which is the whole "resets on close" rule.
/// Ascending bucket order, deterministic.
///
/// `summarized` says whether the frame's candle marks are bar summaries. When
/// they are, a print inside the tape's window is deliberately carried twice —
/// once as a tape mark and once inside its bar's pie — so only the pies are
/// read here. Summing both would count the same contract twice and quietly
/// lengthen the newest rows.
///
/// `grouping` is the frame's visual row height, and every mark is snapped to
/// it. The tape clusters at capture resolution while the candles cluster at
/// the display grouping, so without this one price arrives as two keys and the
/// strip draws two rows for it, each sized against a width that matches
/// neither.
pub(crate) fn aggression_rows(
    aggressions: &[AggressionPrimitive],
    bar_open_ms: i64,
    summarized: bool,
    grouping: Decimal,
) -> Vec<HistogramRow> {
    let mut buckets: std::collections::BTreeMap<Decimal, (Decimal, Decimal, Decimal)> =
        std::collections::BTreeMap::new();
    let width = if grouping > Decimal::ZERO {
        grouping
    } else {
        Decimal::ONE
    };
    for cluster in aggressions {
        if cluster.last_timestamp_ms < bar_open_ms {
            continue;
        }
        if summarized && cluster.live {
            continue;
        }
        let row = (cluster.price_bucket / width).floor() * width;
        let entry = buckets
            .entry(row)
            .or_insert((Decimal::ZERO, Decimal::ZERO, Decimal::ZERO));
        // A summary mark carries both sides at once, and its `side` is only
        // the dominant one. Reading that as "all of it was buying" puts a
        // whole bar's volume in one column of a mirrored histogram.
        let (buy, sell) = split_by_side(cluster);
        entry.0 += buy;
        entry.1 += sell;
        entry.2 = entry.2.max(cluster.price_span.max(width));
    }
    buckets
        .into_iter()
        .map(|(price_bucket, (buy, sell, price_span))| HistogramRow {
            price_bucket,
            price_span,
            buy,
            sell,
        })
        .collect()
}

/// Full-width reference for the histogram: the forming bar's own biggest
/// single-side bucket, per the "normalized by the bar" rule — the bar's
/// heaviest price level always reaches full width, whatever its size.
pub(crate) fn histogram_reference(rows: &[HistogramRow]) -> Decimal {
    rows.iter()
        .map(|row| row.buy.max(row.sell))
        .max()
        .unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    /// A minimal cluster: only the fields the histogram reads carry data.
    fn cluster(side: Side, bucket: &str, quantity: &str, last_ms: i64) -> AggressionPrimitive {
        AggressionPrimitive {
            agg_id: 1,
            agg_ids: vec![1],
            generation: None,
            side,
            consumed_side: match side {
                Side::Buy => quantick_orderbook::BookSide::Ask,
                Side::Sell => quantick_orderbook::BookSide::Bid,
            },
            quantity: dec(quantity),
            buy_share: match side {
                Side::Buy => 1.0,
                Side::Sell => 0.0,
            },
            live: false,
            price_bucket: dec(bucket),
            price_span: Decimal::ONE,
            trade_count: 1,
            first_timestamp_ms: last_ms,
            last_timestamp_ms: last_ms,
            matched_quantity: Decimal::ZERO,
            matched_fraction: 0.0,
            liquidity_event_ids: Vec::new(),
            x: 0.5,
            y: 0.5,
            size: 0.5,
            folded_marks: 0,
        }
    }

    #[test]
    fn the_histogram_keeps_only_the_forming_bar_and_sums_per_bucket() {
        let clusters = vec![
            // Closed-bar cluster: ended before the forming bar opened.
            cluster(Side::Buy, "100", "50", 900),
            // Forming bar: two buys in one bucket, one sell in another.
            cluster(Side::Buy, "100", "2", 1_100),
            cluster(Side::Buy, "100", "3", 1_200),
            cluster(Side::Sell, "99", "4", 1_150),
        ];
        let rows = aggression_rows(&clusters, 1_000, false, Decimal::ONE);
        assert_eq!(
            rows,
            vec![
                HistogramRow {
                    price_bucket: dec("99"),
                    price_span: Decimal::ONE,
                    buy: Decimal::ZERO,
                    sell: dec("4"),
                },
                HistogramRow {
                    price_bucket: dec("100"),
                    price_span: Decimal::ONE,
                    buy: dec("5"),
                    sell: Decimal::ZERO,
                },
            ]
        );
        // The bar's heaviest single-side bucket sets full width.
        assert_eq!(histogram_reference(&rows), dec("5"));
        assert_eq!(histogram_reference(&[]), Decimal::ZERO);
    }
    /// A summarized frame carries the forming bar's prints twice on purpose —
    /// once on the tape, once inside the bar's pie — so the strip reads the
    /// pies alone. Summing both counted the same contract twice and quietly
    /// lengthened exactly the rows a trader is watching.
    #[test]
    fn a_summarized_frame_is_not_counted_twice() {
        let mut pie = cluster(Side::Buy, "100", "10", 1_500);
        pie.live = false;
        pie.buy_share = 0.6;
        let mut tape = cluster(Side::Buy, "100", "6", 1_500);
        tape.live = true;

        let summarized = aggression_rows(&[pie.clone(), tape.clone()], 1_000, true, Decimal::ONE);
        assert_eq!(summarized.len(), 1);
        assert_eq!(
            summarized[0].buy + summarized[0].sell,
            dec("10"),
            "the pie already holds the tape's prints"
        );
        assert_eq!(
            (summarized[0].buy, summarized[0].sell),
            (dec("6"), dec("4")),
            "a two-sided mark is split by its buy share, not dumped on one column"
        );

        // Without the summary the two panes are disjoint, so both are read.
        let raw = aggression_rows(&[pie, tape], 1_000, false, Decimal::ONE);
        assert_eq!(raw[0].buy + raw[0].sell, dec("16"));
    }

    /// The tape clusters at capture resolution and the candles at the display
    /// grouping, so one price arrives as two keys. The strip snaps both to the
    /// frame's own row height or it draws two rows for one price, each sized
    /// against a width that matches neither.
    #[test]
    fn marks_from_both_panes_land_in_one_row() {
        let mut fine = cluster(Side::Buy, "100.03", "2", 1_500);
        fine.live = true;
        let mut coarse = cluster(Side::Buy, "100.00", "3", 1_500);
        coarse.live = false;
        let rows = aggression_rows(&[fine, coarse], 1_000, false, dec("0.04"));
        assert_eq!(rows.len(), 1, "one price, one row");
        assert_eq!(rows[0].price_bucket, dec("100.00"));
        assert_eq!(rows[0].buy, dec("5"));
    }
}
