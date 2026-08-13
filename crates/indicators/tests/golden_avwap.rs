//! Golden + contract tests for the native Anchored VWAP.
//!
//! Written before the kernel (test-first): the fixtures pin the TradingView
//! model — from the anchor bar onward, `vwap = Σ(src·vol)/Σvol`, band
//! `±k·σ` with the volume-weighted `σ² = Σ(src²·vol)/Σvol − vwap²` — with
//! binary-exact numbers so the expected CSV is checkable by hand.

use quantick_engine::TickBarBuilder;
use quantick_indicators::native::AnchoredVwap;
use quantick_indicators::{Ctx, Indicator, IndicatorBar, InputValue, PlotId, SourceId, golden};

const TRADES: &str = include_str!("fixtures/trades_avwap.csv");
const EXPECTED_PLOTS: &str = include_str!("fixtures/avwap_plots.csv");

/// The anchor used across these tests: bar1's open_time in the fixture.
/// Bars whose `close_time` is earlier than the anchor stay `na`.
const ANCHOR_MS: i64 = 1_700_000_001_000;

fn fixture_indicator() -> AnchoredVwap {
    AnchoredVwap::new(
        ANCHOR_MS,
        SourceId::Hlc3,
        [(true, 1.0), (true, 2.0), (false, 3.0)],
    )
}

fn fixture_bars() -> Vec<IndicatorBar> {
    golden::bars_from_trades_csv(&mut TickBarBuilder::new(3), TRADES)
}

fn run(indicator: &mut AnchoredVwap, bars: &[IndicatorBar]) {
    golden::run_indicator(indicator, bars).expect("fixture evaluates");
}

#[test]
fn golden_anchored_vwap_round_trip() {
    golden::assert_golden(
        fixture_indicator,
        || TickBarBuilder::new(3),
        TRADES,
        EXPECTED_PLOTS,
    );
}

#[test]
fn declares_seven_plots_and_three_band_fills() {
    let indicator = fixture_indicator();
    let descriptor = indicator.descriptor();
    assert!(
        descriptor.overlay,
        "a price-source AVWAP overlays the chart"
    );
    assert_eq!(descriptor.plots.len(), 7, "vwap + three upper/lower pairs");
    assert_eq!(descriptor.plots[0].title, "vwap");
    assert_eq!(
        descriptor.fills.len(),
        3,
        "one translucent fill per band pair"
    );
}

#[test]
fn the_source_input_changes_the_accumulation() {
    // Same tape, source=high: pv = 8·112 + 8·144 + 16·96 = 3584, v = 32.
    let mut indicator = AnchoredVwap::new(ANCHOR_MS, SourceId::High, [(false, 1.0); 3]);
    run(&mut indicator, &fixture_bars());
    assert_eq!(indicator.plots().value(PlotId::new(0), 3), 112.0);
}

#[test]
fn bars_before_the_anchor_are_na_and_never_accumulate() {
    let mut indicator = fixture_indicator();
    run(&mut indicator, &fixture_bars());
    let plots = indicator.plots();
    assert_eq!(plots.len(), 4, "one row per closed bar, even pre-anchor");
    for plot in 0..7 {
        assert!(plots.value(PlotId::new(plot), 0).is_nan(), "plot {plot}");
    }
    // bar1 is the first participating bar: vwap == source, sigma == 0.
    assert_eq!(plots.value(PlotId::new(0), 1), 96.0);
    assert_eq!(plots.value(PlotId::new(1), 1), 96.0);
}

#[test]
fn anchor_on_the_last_bar_reads_that_bar_alone() {
    let mut indicator = AnchoredVwap::new(
        1_700_000_003_000,
        SourceId::Hlc3,
        [(true, 1.0), (false, 2.0), (false, 3.0)],
    );
    run(&mut indicator, &fixture_bars());
    let plots = indicator.plots();
    assert_eq!(plots.value(PlotId::new(0), 3), 80.0, "vwap = its own hlc3");
    assert_eq!(plots.value(PlotId::new(1), 3), 80.0, "sigma is exactly 0");
    assert!(plots.value(PlotId::new(0), 2).is_nan(), "bar2 predates it");
}

#[test]
fn zero_volume_bars_read_na_without_dividing() {
    let bar = IndicatorBar {
        open_time: ANCHOR_MS,
        close_time: ANCHOR_MS + 100,
        open: 100.0,
        high: 101.0,
        low: 99.0,
        close: 100.0,
        buy_volume: 0.0,
        sell_volume: 0.0,
        trade_count: 0.0,
    };
    let mut indicator = fixture_indicator();
    let cvd = [0.0];
    let mut ctx = Ctx {
        bar_index: 0,
        cvd: &cvd,
    };
    indicator.on_close(&bar, &mut ctx).expect("evaluates");
    assert!(indicator.plots().value(PlotId::new(0), 0).is_nan());
}

#[test]
fn preview_commits_nothing_and_matches_the_commit_path() {
    let bars = fixture_bars();
    let mut committed = fixture_indicator();
    run(&mut committed, &bars);

    // Re-run withholding the last bar, preview it any number of times, then
    // commit it: the preview values must equal the eventual commit row and
    // the previews must not have advanced the accumulators.
    let mut staged = fixture_indicator();
    run(&mut staged, &bars[..3]);
    let mut cvd: Vec<f64> = Vec::new();
    let mut sum = 0.0;
    for bar in &bars {
        sum += bar.delta();
        cvd.push(sum);
    }
    for _ in 0..3 {
        let mut ctx = Ctx {
            bar_index: 3,
            cvd: &cvd,
        };
        let frame = staged.preview(&bars[3], &mut ctx).expect("previews");
        for plot in 0..7 {
            let expected = committed.plots().value(PlotId::new(plot), 3);
            let got = frame.values[plot];
            assert!(
                got == expected || (got.is_nan() && expected.is_nan()),
                "plot {plot}: preview {got} vs commit {expected}"
            );
        }
    }
    assert_eq!(staged.plots().len(), 3, "previews never commit a row");
    let mut ctx = Ctx {
        bar_index: 3,
        cvd: &cvd,
    };
    staged.on_close(&bars[3], &mut ctx).expect("commits");
    assert_eq!(
        staged.plots().value(PlotId::new(0), 3),
        committed.plots().value(PlotId::new(0), 3),
        "previewing must not have advanced committed state"
    );
}

#[test]
fn reset_then_replay_reproduces_the_golden() {
    let bars = fixture_bars();
    let mut indicator = fixture_indicator();
    run(&mut indicator, &bars);
    indicator.reset();
    assert_eq!(indicator.plots().len(), 0);
    run(&mut indicator, &bars);
    assert!(
        golden::diff_plot_csv(EXPECTED_PLOTS, &golden::plot_csv(&indicator)).is_none(),
        "replay after reset should match the golden fixture"
    );
}

#[test]
fn rebind_binds_every_declared_input() {
    // The settings port: values out, values in, values back out — honestly.
    let indicator = fixture_indicator();
    let declared = indicator.descriptor().inputs.len();
    let values = indicator.input_values();
    assert_eq!(values.len(), declared);
    assert_eq!(values[0], InputValue::Int(ANCHOR_MS), "anchor time (ms)");
    assert_eq!(values[1], InputValue::Source(SourceId::Hlc3));

    let mut edited = values.clone();
    edited[1] = InputValue::Source(SourceId::Close);
    let rebound = indicator
        .rebind(&edited)
        .expect("an indicator with inputs must rebind");
    assert_eq!(
        rebound.input_values()[1],
        InputValue::Source(SourceId::Close)
    );
    assert_eq!(
        rebound.input_values()[0],
        InputValue::Int(ANCHOR_MS),
        "untouched values survive the round trip"
    );
}
