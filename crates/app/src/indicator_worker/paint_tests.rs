use super::*;
use crate::indicators::IndicatorViews;
use crate::indicators::library::EMBEDDED_SCRIPTS;
use quantick_engine::{Side, TickBarBuilder, Trade, golden as engine_golden};
use rust_decimal::Decimal;

/// `color.silver` in the dialect's palette — what a bullish biggest bar
/// wears with the script's declared defaults (force holds the loud
/// white/yellow pair; the range extreme is context and whispers).
const SILVER: Rgba8 = Rgba8::new(0xB2, 0xB5, 0xBE, 0xFF);

fn print(id: u64, price: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: 1_000 + id as i64 * 100,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// 22 tick(2) bars: twenty quiet ones (body and range 1), then one ten
/// times as wide, then quiet again.
///
/// The script's default windows are 20 bars, so bar 20 is the first that
/// can be judged at all — and it is the widest of its window by a factor
/// of ten, which is the least ambiguous thing a tape can say.
fn tape() -> Vec<Bar> {
    let mut trades = Vec::new();
    for bar in 0..20u64 {
        trades.push(print(bar * 2 + 1, 100));
        trades.push(print(bar * 2 + 2, 101));
    }
    trades.push(print(41, 100));
    trades.push(print(42, 110));
    trades.push(print(43, 100));
    trades.push(print(44, 101));
    engine_golden::replay(&mut TickBarBuilder::new(2), &trades)
}

#[test]
fn the_embedded_force_bar_paints_candles_through_the_whole_chain() {
    let bars = tape();
    assert_eq!(bars.len(), 22, "fixture shape");

    let source = EMBEDDED_SCRIPTS
        .iter()
        .find(|(name, _)| *name == "force_bar.pine")
        .expect("force_bar.pine is embedded")
        .1;

    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("script.force_bar");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Script {
            name: "force_bar.pine".to_owned(),
            text: source.to_owned(),
        },
    });
    worker.send(IndicatorCommand::Backfilled(bars.clone()));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }

    assert!(views.all()[0].error.is_none(), "the script loaded");
    assert_eq!(
        views.all()[0].rows,
        bars.len(),
        "a script with no plots still commits a row per bar — the row \
             count cannot come from a column that does not exist"
    );
    assert!(views.paints_any(), "the chart now has paint to look up");
    assert_eq!(
        views.bar_paint(20),
        Some(SILVER),
        "the widest bar of its window, bullish, with the declared defaults"
    );
    assert_eq!(views.bar_paint(0), None, "warm-up paints nothing");
    assert_eq!(
        views.bar_paint(21),
        None,
        "and the quiet bar after the big one is ordinary again"
    );
}
