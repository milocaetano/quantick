use quantick_engine::{Bar, BarBuilder as _, Side, TimeBarBuilder, Trade};
use rust_decimal::Decimal;

use crate::IndicatorHost;
use crate::native::Cvd;
use crate::output::PlotId;
use crate::spot_perp_cvd::{CvdPoint, OppositeSpan, spot_perp_cvd};

const MINUTE_MS: i64 = 60_000;

fn trade(agg_id: u64, timestamp_ms: i64, side: Side, quantity: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms,
        price: Decimal::from(100),
        quantity: Decimal::from(quantity),
        side,
    }
}

/// Minute bars from the engine's own aggregator, the forming one last.
fn minute_bars(trades: &[Trade]) -> Vec<Bar> {
    let mut builder = TimeBarBuilder::new(MINUTE_MS);
    let mut bars = Vec::new();
    for trade in trades {
        builder.push_into(trade, &mut bars);
    }
    bars.extend(builder.partial().cloned());
    bars
}

fn perp_trades() -> Vec<Trade> {
    vec![
        trade(1, 0, Side::Buy, 5),
        trade(2, 60_000, Side::Sell, 8),
        trade(3, 120_000, Side::Sell, 4),
        trade(4, 180_000, Side::Buy, 10),
    ]
}

fn spot_trades() -> Vec<Trade> {
    vec![
        trade(1, 30_000, Side::Buy, 2),
        trade(2, 90_000, Side::Buy, 1),
        trade(3, 150_000, Side::Buy, 1),
        trade(4, 210_000, Side::Sell, 6),
    ]
}

/// The CVD column the chart's own `Cvd` pane draws for `bars`.
fn cvd_column(bars: &[Bar]) -> Vec<f64> {
    let mut host = IndicatorHost::new();
    let id = host.add(Box::new(Cvd::new()));
    host.rebuild(bars, None);
    host.plots(id)
        .expect("hosted cvd")
        .column(PlotId::new(0))
        .to_vec()
}

fn points(times: &[i64], values: &[f64]) -> Vec<CvdPoint> {
    times
        .iter()
        .zip(values)
        .map(|(&time_ms, &value)| CvdPoint { time_ms, value })
        .collect()
}

#[test]
fn lines_reuse_cvd_and_spans_match_golden() {
    let perp_bars = minute_bars(&perp_trades());
    let spot_bars = minute_bars(&spot_trades());
    let model = spot_perp_cvd(&perp_bars, &spot_bars);

    let perp_times: Vec<i64> = perp_bars.iter().map(|bar| bar.close_time).collect();
    let spot_times: Vec<i64> = spot_bars.iter().map(|bar| bar.close_time).collect();
    assert_eq!(model.perp, points(&perp_times, &cvd_column(&perp_bars)));
    assert_eq!(model.spot, points(&spot_times, &cvd_column(&spot_bars)));
    // And the golden values those columns hold.
    assert_eq!(
        model.perp,
        points(&[0, 60_000, 120_000, 180_000], &[5.0, -3.0, -7.0, 3.0])
    );
    assert_eq!(
        model.spot,
        points(&[30_000, 90_000, 150_000, 210_000], &[2.0, 3.0, 4.0, -2.0])
    );

    // Perp turns negative at 60 000 while spot stays up, until perp turns
    // back at 180 000; spot turns negative at 210 000 and the tape ends so.
    assert_eq!(
        model.opposite_spans,
        vec![
            OppositeSpan {
                start_ms: 60_000,
                end_ms: 180_000,
                ongoing: false,
            },
            OppositeSpan {
                start_ms: 210_000,
                end_ms: 210_000,
                ongoing: true,
            },
        ]
    );
}

#[test]
fn rounding_residue_around_zero_is_not_a_divergence() {
    // 0.1 + 0.2 - 0.3 leaves about 5.6e-17 in f64: a flat leg, not a buyer.
    let fractional = |agg_id, timestamp_ms, side, quantity: &str| Trade {
        quantity: quantity.parse().expect("fixture decimal"),
        ..trade(agg_id, timestamp_ms, side, 0)
    };
    let perp_bars = minute_bars(&[
        fractional(1, 0, Side::Buy, "0.1"),
        fractional(2, 60_000, Side::Buy, "0.2"),
        fractional(3, 120_000, Side::Sell, "0.3"),
    ]);
    let spot_bars = minute_bars(&[trade(1, 150_000, Side::Sell, 4)]);
    let model = spot_perp_cvd(&perp_bars, &spot_bars);
    let last = model.perp.last().expect("perp line").value;
    assert!(last != 0.0 && last.abs() < 1e-12, "{last}");
    assert!(
        model.opposite_spans.is_empty(),
        "{:?}",
        model.opposite_spans
    );
}

#[test]
fn one_leg_alone_has_no_spans() {
    let perp_bars = minute_bars(&perp_trades());
    let model = spot_perp_cvd(&perp_bars, &[]);
    assert_eq!(model.perp.len(), perp_bars.len());
    assert!(model.spot.is_empty());
    assert!(model.opposite_spans.is_empty());
}
