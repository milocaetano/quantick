//! The `poc` script builtin end to end: engine trades, the engine's footprint
//! ladder per bar, the host, a compiled script.
//!
//! Golden over a committed trade fixture for the closed bars, a preview run
//! for the forming bar, and the honest `na` when a bar arrives without a
//! footprint.

use quantick_engine::{
    BarBuilder, DEFAULT_LEVEL_CAP, FootprintBuilder, TickBarBuilder, Trade, fixture,
};
use quantick_indicators::{FootprintBar, IndicatorBar, IndicatorHost, PlotId, golden};
use quantick_pine::{ScriptIndicator, compile};
use rust_decimal::Decimal;

const TRADES: &str = include_str!("fixtures/poc_trades.csv");
const EXPECTED: &str = include_str!("golden/poc.csv");
const SCRIPT: &str = "//@version=5\nindicator(\"poc probe\")\nplot(poc, \"poc\")\n";

fn script() -> ScriptIndicator {
    let compiled = match compile(SCRIPT, "poc_probe.pine") {
        Ok(compiled) => compiled,
        Err(errors) => panic!("the probe script must compile: {errors:?}"),
    };
    ScriptIndicator::new(compiled, SCRIPT)
}

fn trades() -> Vec<Trade> {
    fixture::parse_trades(TRADES).expect("the poc trade fixture should parse")
}

/// Fold the trades into tick(3) bars and, alongside, the engine footprint of
/// each bar at a 0.5 row width: the closed bars and the one still forming.
fn footprint_bars(trades: &[Trade]) -> (Vec<FootprintBar>, FootprintBar) {
    let mut bars = TickBarBuilder::new(3);
    let mut ladders = FootprintBuilder::new(Decimal::new(5, 1), DEFAULT_LEVEL_CAP);
    let mut closed = Vec::new();
    for trade in trades {
        ladders.push(trade);
        if let Some(bar) = bars.push(trade) {
            closed.push(FootprintBar::new(bar, ladders.close().as_ref()));
        }
    }
    let forming = bars.partial().cloned().expect("two prints stay forming");
    (closed, FootprintBar::new(forming, ladders.partial()))
}

#[test]
fn poc_series_matches_the_golden_over_fixture_trades() {
    let (closed, _) = footprint_bars(&trades());
    let projected: Vec<IndicatorBar> = closed.iter().map(IndicatorBar::from).collect();
    let mut first = script();
    golden::run_indicator(&mut first, &projected).expect("the probe never fails");
    let mut second = script();
    golden::run_indicator(&mut second, &projected).expect("the probe never fails");
    let csv = golden::plot_csv(&first);
    assert_eq!(csv, golden::plot_csv(&second), "two runs must agree");
    assert_eq!(csv.replace("\r\n", "\n"), EXPECTED.replace("\r\n", "\n"));
}

#[test]
fn preview_run_reads_the_forming_bars_poc() {
    let (closed, forming) = footprint_bars(&trades());
    let mut host = IndicatorHost::new();
    let id = host.add(Box::new(script()));
    for bar in &closed {
        host.push_closed_bar(bar);
    }
    host.set_partial(Some(&forming));
    let frame = host.preview(id).expect("a forming bar is previewed");
    assert_eq!(frame.values, vec![103.0]);
}

#[test]
fn a_bar_without_a_footprint_reads_na() {
    let (closed, forming) = footprint_bars(&trades());
    let mut host = IndicatorHost::new();
    let id = host.add(Box::new(script()));
    for bar in &closed {
        host.push_closed_bar(&bar.bar);
    }
    host.set_partial(Some(&forming.bar));
    let column = host
        .plots(id)
        .expect("the probe is hosted")
        .column(PlotId::new(0));
    assert_eq!(column.len(), 3);
    assert!(column.iter().all(|v| v.is_nan()), "got {column:?}");
    let frame = host.preview(id).expect("a forming bar is previewed");
    assert!(frame.values[0].is_nan(), "got {:?}", frame.values);
}
