//! C1/C2: shipped assets through the public compiler and Indicator port.
use quantick_indicators::{Ctx, Indicator, IndicatorBar, InputValue};
use quantick_pine::{ScriptIndicator, compile};

#[path = "fixtures/popular_pine/reference.rs"]
mod reference;

const SCRIPTS: &[(&str, &str)] = &[
    ("sma", include_str!("../../app/scripts/sma.pine")),
    ("ema", include_str!("../../app/scripts/ema.pine")),
    ("wma", include_str!("../../app/scripts/wma.pine")),
    ("vwma", include_str!("../../app/scripts/vwma.pine")),
    ("hma", include_str!("../../app/scripts/hma.pine")),
    ("rma", include_str!("../../app/scripts/rma.pine")),
    ("rsi", include_str!("../../app/scripts/rsi.pine")),
    (
        "stochastic",
        include_str!("../../app/scripts/stochastic.pine"),
    ),
    (
        "stochastic_rsi",
        include_str!("../../app/scripts/stochastic_rsi.pine"),
    ),
    ("macd", include_str!("../../app/scripts/macd.pine")),
    (
        "bollinger_bands",
        include_str!("../../app/scripts/bollinger_bands.pine"),
    ),
    ("atr", include_str!("../../app/scripts/atr.pine")),
    ("adx", include_str!("../../app/scripts/adx.pine")),
    ("cci", include_str!("../../app/scripts/cci.pine")),
    ("roc", include_str!("../../app/scripts/roc.pine")),
    ("momentum", include_str!("../../app/scripts/momentum.pine")),
    (
        "williams_r",
        include_str!("../../app/scripts/williams_r.pine"),
    ),
    ("obv", include_str!("../../app/scripts/obv.pine")),
    ("mfi", include_str!("../../app/scripts/mfi.pine")),
    ("cmf", include_str!("../../app/scripts/cmf.pine")),
    (
        "donchian_channels",
        include_str!("../../app/scripts/donchian_channels.pine"),
    ),
    (
        "keltner_channels",
        include_str!("../../app/scripts/keltner_channels.pine"),
    ),
];

fn instance(id: &str, source: &str, short: bool) -> Box<dyn Indicator> {
    let compiled = compile(source, id).unwrap_or_else(|errors| panic!("{id}: {errors:?}"));
    let base = ScriptIndicator::new(compiled, source);
    if SCRIPTS.iter().any(|(name, _)| *name == id) {
        assert_eq!(
            base.descriptor().overlay,
            matches!(
                id,
                "sma"
                    | "ema"
                    | "wma"
                    | "vwma"
                    | "hma"
                    | "rma"
                    | "bollinger_bands"
                    | "donchian_channels"
                    | "keltner_channels"
            ),
            "{id}: pane placement"
        );
    }
    if short {
        let values = base
            .input_values()
            .into_iter()
            .map(|value| match value {
                InputValue::Int(_) => InputValue::Int(3),
                other => other,
            })
            .collect::<Vec<_>>();
        base.rebind(&values).unwrap_or_else(|| Box::new(base))
    } else {
        Box::new(base)
    }
}

fn assert_value(actual: f64, expected: f64, context: &str) {
    assert!(
        actual.is_nan() && expected.is_nan()
            || actual.is_finite()
                && expected.is_finite()
                && (actual - expected).abs() <= 1e-8 + 1e-8 * expected.abs(),
        "{context}: actual={actual}, expected={expected}"
    );
}

fn committed(indicator: &dyn Indicator) -> Vec<Vec<f64>> {
    indicator
        .descriptor()
        .plots
        .iter()
        .map(|plot| indicator.plots().column(plot.id).to_vec())
        .collect()
}

#[test]
fn popular_pine_golden_outputs() {
    assert_eq!(SCRIPTS.len(), 22);
    for scenario in [
        "mixed",
        "flat",
        "zero_price",
        "zero_volume",
        "missing_volume",
    ] {
        let bars = reference::fixture(scenario, 120);
        for short in [false, true] {
            for &(id, source) in SCRIPTS {
                let expected = reference::expected(id, &bars, short);
                let mut indicator = instance(id, source, short);
                assert_eq!(indicator.descriptor().plots.len(), expected.len(), "{id}");
                for (bar_index, bar) in bars.iter().enumerate() {
                    indicator
                        .on_close(
                            bar,
                            &mut Ctx {
                                bar_index,
                                cvd: &[],
                            },
                        )
                        .unwrap_or_else(|error| panic!("{id}: {error:?}"));
                    for (column, plot) in indicator.descriptor().plots.iter().enumerate() {
                        assert_value(
                            indicator.plots().column(plot.id)[bar_index],
                            expected[column][bar_index],
                            &format!("{id}/{scenario}/short={short}/{bar_index}/{column}"),
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn popular_pine_preview_rollback() {
    for scenario in ["mixed", "flat", "zero_volume", "missing_volume"] {
        let bars = reference::fixture(scenario, 64);
        for &(id, source) in SCRIPTS {
            let mut clean = instance(id, source, true);
            let mut previewed = instance(id, source, true);
            for (bar_index, bar) in bars.iter().enumerate() {
                let mut ctx = Ctx {
                    bar_index,
                    cvd: &[],
                };
                let before = committed(previewed.as_ref());
                // Preview a different forming bar, then the eventual closing bar twice.
                let mut partial = *bar;
                partial.close += 5.0;
                partial.high += 10.0;
                partial.buy_volume *= 4.0;
                previewed.preview(&partial, &mut ctx).unwrap();
                let first = previewed.preview(bar, &mut ctx).unwrap().values;
                let second = previewed.preview(bar, &mut ctx).unwrap().values;
                assert_eq!(
                    format!("{before:?}"),
                    format!("{:?}", committed(previewed.as_ref()))
                );
                clean.on_close(bar, &mut ctx).unwrap();
                previewed.on_close(bar, &mut ctx).unwrap();
                for (column, plot) in clean.descriptor().plots.iter().enumerate() {
                    let expected = clean.plots().column(plot.id)[bar_index];
                    for actual in [
                        first[column],
                        second[column],
                        previewed.plots().column(plot.id)[bar_index],
                    ] {
                        assert_value(
                            actual,
                            expected,
                            &format!("{id}/{scenario}/{bar_index}/{column}"),
                        );
                    }
                }
            }
            let first = committed(previewed.as_ref());
            previewed.reset();
            for (bar_index, bar) in bars.iter().enumerate() {
                previewed
                    .on_close(
                        bar,
                        &mut Ctx {
                            bar_index,
                            cvd: &[],
                        },
                    )
                    .unwrap();
            }
            assert_eq!(
                format!("{first:?}"),
                format!("{:?}", committed(previewed.as_ref())),
                "{id}: reset/replay"
            );
        }
    }
}

#[test]
fn popular_pine_hand_calculated_trend_anchors() {
    // 100..199, high/low +/-1, volume 10: period 3 formulas have simple limits.
    let bars: Vec<_> = (0..100)
        .map(|i| IndicatorBar {
            open_time: i,
            close_time: i + 1,
            open: 100.0 + i as f64,
            high: 101.0 + i as f64,
            low: 99.0 + i as f64,
            close: 100.0 + i as f64,
            buy_volume: 6.0,
            sell_volume: 4.0,
            trade_count: 1.0,
        })
        .collect();
    let anchors: &[(&str, &[f64])] = &[
        ("sma", &[198.0]),
        ("ema", &[198.0]),
        ("wma", &[198.0 + 1.0 / 3.0]),
        ("vwma", &[198.0]),
        ("hma", &[199.0 + 1.0 / 3.0]),
        ("rma", &[197.0]),
        ("rsi", &[100.0]),
        ("stochastic", &[75.0, 75.0]),
        ("stochastic_rsi", &[f64::NAN, f64::NAN]),
        ("macd", &[0.0, 0.0, 0.0]),
        ("atr", &[2.0]),
        ("adx", &[50.0, 0.0, 100.0]),
        ("cci", &[100.0]),
        ("roc", &[300.0 / 196.0]),
        ("momentum", &[3.0]),
        ("williams_r", &[-25.0]),
        ("obv", &[990.0]),
        ("mfi", &[100.0]),
        ("cmf", &[0.0]),
        ("donchian_channels", &[198.0, 200.0, 196.0]),
        ("keltner_channels", &[198.0, 202.0, 194.0]),
    ];
    for &(id, expected) in anchors {
        let source = SCRIPTS.iter().find(|(name, _)| *name == id).unwrap().1;
        let mut script = instance(id, source, true);
        let batch = reference::expected(id, &bars, true);
        for (bar_index, bar) in bars.iter().enumerate() {
            script
                .on_close(
                    bar,
                    &mut Ctx {
                        bar_index,
                        cvd: &[],
                    },
                )
                .unwrap();
        }
        for (column, value) in expected.iter().enumerate() {
            assert_value(batch[column][99], *value, id);
            assert_value(
                script.plots().column(script.descriptor().plots[column].id)[99],
                *value,
                id,
            );
        }
    }
    let expected = reference::expected("bollinger_bands", &bars, true);
    let width = 2.0 * (2.0_f64 / 3.0).sqrt();
    for (column, value) in [198.0, 198.0 + width, 198.0 - width].iter().enumerate() {
        assert_value(expected[column][99], *value, "Bollinger hand anchor");
    }
}

#[test]
#[ignore = "diagnostic: record identical baseline and 22-indicator replay scenarios"]
fn popular_pine_replay_budget() {
    use std::time::Instant;
    let bars = reference::fixture("mixed", 2000);
    // Nine legacy scripts, then exact 22 market indicators, compiled outside timer.
    let legacy = [
        include_str!("../../app/scripts/ema.pine"),
        include_str!("../../app/scripts/cvd.pine"),
        include_str!("../../app/scripts/delta_histogram.pine"),
        include_str!("../../app/scripts/vwap_cumulative.pine"),
        include_str!("../../app/scripts/zigzag.pine"),
        include_str!("../../app/scripts/range_box.pine"),
        include_str!("../../app/scripts/copilot.pine"),
        include_str!("../../app/scripts/force_bar.pine"),
        include_str!("../../app/scripts/exhaustion_reversal.pine"),
    ];
    for (label, sources) in [
        ("legacy-nine", legacy.to_vec()),
        (
            "market-22",
            SCRIPTS.iter().map(|(_, source)| *source).collect(),
        ),
    ] {
        let mut scripts: Vec<_> = sources
            .iter()
            .map(|source| instance(label, source, false))
            .collect();
        let start = Instant::now();
        for _ in 0..5 {
            for script in &mut scripts {
                script.reset();
                for (bar_index, bar) in bars.iter().enumerate() {
                    script
                        .on_close(
                            bar,
                            &mut Ctx {
                                bar_index,
                                cvd: &[],
                            },
                        )
                        .unwrap();
                }
            }
        }
        println!(
            "{label}: all-script 2000-bar replay {:?} (5-run average)",
            start.elapsed() / 5
        );
    }
}

#[test]
fn popular_pine_derived_lengths_remain_load_time_and_bounded() {
    let source = SCRIPTS.iter().find(|(id, _)| *id == "hma").unwrap().1;
    let base = instance("hma", source, false);
    let bars = reference::fixture("mixed", 550);
    let close: Vec<_> = bars.iter().map(|b| b.close).collect();
    for length in [1, 2, 3, 9, 16, 500] {
        let mut inputs = base.input_values();
        inputs[0] = InputValue::Int(length);
        let mut script = base.rebind(&inputs).unwrap();
        let expected = reference::hma(&close, length as usize);
        for (bar_index, bar) in bars.iter().enumerate() {
            script
                .on_close(
                    bar,
                    &mut Ctx {
                        bar_index,
                        cvd: &[],
                    },
                )
                .unwrap();
            assert_value(
                script.plots().column(script.descriptor().plots[0].id)[bar_index],
                expected[bar_index],
                &format!("HMA length={length} bar={bar_index}"),
            );
        }
    }
    for expression in [
        "math.max(1,int(close))",
        "int(math.sqrt(-1))",
        "math.floor(0)",
        "math.round(100001.0)",
        "math.max(0,-1)",
    ] {
        let text = format!(
            "//@version=5\nindicator(\"invalid length\")\nplot(ta.wma(close,{expression}))\n"
        );
        let errors = compile(&text, "invalid")
            .err()
            .expect("invalid length is rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.code == quantick_pine::ErrorCode::PineSeriesLength),
            "{expression}: {errors:?}"
        );
    }
}
