use super::*;
use crate::indicators::IndicatorViews;
use crate::indicators::library::EMBEDDED_SCRIPTS;
use quantick_engine::{Side, TickBarBuilder, Trade, golden as engine_golden};
use rust_decimal::Decimal;

fn print(id: u64, price: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: 1_000 + id as i64 * 100,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// Quiet bars before the force bar. The 20-bar body average this script
/// defaults to first exists on bar 20 (`body[1]` is `na` on bar 0 and
/// that NaN sits in the window until it slides out), so a fixture with
/// exactly 20 would put the force bar on the very first bar that can be
/// judged at all — and any edit that shifts warm-up by one would fail
/// this test with an empty marker column, which reads as a broken
/// channel rather than as a fixture one bar short.
const WARMUP_BARS: usize = 25;

/// 29 tick(2) bars sized for the script's *defaults*: the quiet bullish
/// run above (body 1) to warm a 20-bar body average and a 10-bar extreme,
/// then a body-10 bar taking out the high, then three bearish bars
/// handing 80% of it back.
fn tape() -> Vec<Bar> {
    // Two prints per bar, so each pair below *is* one bar's open and
    // close (and, with no third print, its low and high).
    let mut prices: Vec<i64> = Vec::new();
    for _ in 0..WARMUP_BARS {
        prices.extend([100, 101]);
    }
    // 20: the force bar — body 10 against an average of 1, high 111
    // against a 10-bar extreme of 101.
    prices.extend([101, 111]);
    // 21..23: three bearish bars, the last closing at 103 — 80% of the
    // force bar's range given back, on the third bar of the run.
    prices.extend([111, 109]);
    prices.extend([109, 107]);
    prices.extend([107, 103]);

    let trades: Vec<Trade> = prices
        .iter()
        .enumerate()
        .map(|(index, price)| print(index as u64 + 1, *price))
        .collect();
    engine_golden::replay(&mut TickBarBuilder::new(2), &trades)
}

/// Rows of `title` carrying a mark, read the way the renderer reads them.
fn marks(views: &IndicatorViews, title: &str) -> Vec<usize> {
    let view = &views.all()[0];
    let index = view
        .descriptor
        .plots
        .iter()
        .position(|plot| plot.title == title)
        .unwrap_or_else(|| panic!("plot {title:?} is declared"));
    view.columns[index]
        .iter()
        .enumerate()
        .filter(|(_, value)| !value.is_nan())
        .map(|(row, _)| row)
        .collect()
}

#[test]
fn the_embedded_exhaustion_reversal_marks_through_the_whole_chain() {
    let bars = tape();
    assert_eq!(bars.len(), WARMUP_BARS + 4, "fixture shape");

    let source = EMBEDDED_SCRIPTS
        .iter()
        .find(|(name, _)| *name == "exhaustion_reversal.pine")
        .expect("exhaustion_reversal.pine is embedded")
        .1;

    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("script.exhaustion_reversal");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Script {
            name: "exhaustion_reversal.pine".to_owned(),
            text: source.to_owned(),
        },
    });
    worker.send(IndicatorCommand::Backfilled(bars.clone()));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }

    assert!(views.all()[0].error.is_none(), "the script loaded");
    assert_eq!(views.all()[0].rows, bars.len());
    assert_eq!(
        marks(&views, "Exhaustion reversal: sell"),
        vec![WARMUP_BARS + 3],
        "the triangle lands on the bar closing the give-back, with the \
             defaults a trader gets from the menu — no test-only inputs"
    );
    assert_eq!(
        marks(&views, "Exhaustion reversal: buy"),
        Vec::<usize>::new(),
        "and the other side stays empty on a tape that only fades a rally"
    );
}
