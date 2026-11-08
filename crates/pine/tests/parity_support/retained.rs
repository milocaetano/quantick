//! Independent retained-child fixtures remain separate from the proxy denominator.
use quantick_indicators::{Ctx, Indicator, IndicatorBar};
use quantick_pine::{ScriptIndicator, compile, libraries::compile_with_libraries};

pub fn scalar_goldens() -> usize {
    let cases = [
        ("types/scalars", true),
        ("version/v5", true),
        ("version/v6", true),
        ("semantics/conditional", false),
        ("semantics/loop", false),
        ("libraries/main", true),
    ];
    let mut cells = 0;
    for (case, input_column) in cases {
        let source = std::fs::read_to_string(
            super::parity_support::root().join(format!("fixtures/{case}.pine")),
        )
        .unwrap();
        let csv = std::fs::read_to_string(
            super::parity_support::root().join(format!("golden/{case}.csv")),
        )
        .unwrap();
        let compiled = if case.starts_with("libraries/") {
            compile_with_libraries(&source, case, |key| {
                (key == "local/Stats/1")
                    .then(|| include_str!("../fixtures/libraries/stats.pine").into())
            })
        } else {
            compile(&source, case)
        }
        .unwrap_or_else(|e| panic!("{case}: {e:?}"));
        let mut script = ScriptIndicator::new(compiled, &source);
        for (index, line) in csv
            .lines()
            .filter(|line| !line.starts_with('#'))
            .skip(1)
            .enumerate()
        {
            let row: Vec<_> = line
                .split(',')
                .map(|cell| {
                    if cell == "na" {
                        f64::NAN
                    } else {
                        cell.parse::<f64>().unwrap()
                    }
                })
                .collect();
            let close = if input_column {
                row[0]
            } else {
                (index + 1) as f64
            };
            let expected = if input_column { &row[1..] } else { &row };
            let bar = IndicatorBar {
                open_time: 0,
                close_time: 1,
                open: close,
                high: close,
                low: close,
                close,
                buy_volume: 0.0,
                sell_volume: 0.0,
                trade_count: 0.0,
            };
            let mut ctx = Ctx {
                bar_index: index,
                cvd: &[0.0; 5],
            };
            let before = super::parity_support::snapshot(&script);
            let first = script.preview(&bar, &mut ctx).unwrap();
            let second = script.preview(&bar, &mut ctx).unwrap();
            assert_eq!(format!("{first:?}"), format!("{second:?}"), "{case}");
            assert_eq!(before, super::parity_support::snapshot(&script), "{case}");
            script.on_close(&bar, &mut ctx).unwrap();
            assert_eq!(expected.len(), first.values.len(), "{case}");
            for (column, value) in expected.iter().enumerate() {
                for actual in [
                    first.values[column],
                    script.plots().column(script.descriptor().plots[column].id)[index],
                ] {
                    assert!(
                        actual == *value || actual.is_nan() && value.is_nan(),
                        "{case} row {index} column {column}: {actual} != {value}"
                    );
                }
                cells += 1;
            }
        }
    }
    cells
}

/// Replay authored collections and object graphs, including every preview output.
/// The CSV values are acceptance data, not additional proxy observations.
pub fn graph_goldens() -> (usize, String) {
    use std::fmt::Write;
    let cases = [
        ("collections/annotations", false, false),
        ("collections/boundaries", false, false),
        ("collections/defaults", false, false),
        ("collections/eager", false, false),
        ("collections/search", false, false),
        ("collections/slices", false, false),
        ("collections/sorting", false, false),
        ("collections/statistics", false, false),
        ("libraries/mixed", true, true),
        ("libraries/eager", false, false),
        ("libraries/history_graph", true, false),
        ("libraries/defaults", true, false),
        ("libraries/varip_graph", false, false),
    ];
    let mut transcript = String::new();
    let mut cells = 0;
    for (case, input, header) in cases {
        let source = std::fs::read_to_string(
            super::parity_support::root().join(format!("fixtures/{case}.pine")),
        )
        .unwrap();
        let csv = std::fs::read_to_string(
            super::parity_support::root().join(format!("golden/{case}.csv")),
        )
        .unwrap();
        let compiled = compile(&source, case).unwrap_or_else(|e| panic!("{case}: {e:?}"));
        let mut script = ScriptIndicator::new(compiled, &source);
        for (step, line) in csv.lines().skip(usize::from(header)).enumerate() {
            let row: Vec<f64> = line
                .split(',')
                .map(|s| {
                    if s == "na" {
                        f64::NAN
                    } else {
                        s.parse().unwrap()
                    }
                })
                .collect();
            let expected = if input { &row[1..] } else { &row };
            let persistent = case == "libraries/varip_graph";
            let index = if persistent { step / 3 } else { step };
            let close = if input {
                row[0]
            } else if case.starts_with("collections/") {
                10.0
            } else {
                1.0
            };
            let bar = IndicatorBar {
                open_time: 0,
                close_time: 1,
                open: close,
                high: close,
                low: close,
                close,
                buy_volume: if case == "libraries/defaults" {
                    3.0
                } else {
                    1.0
                },
                sell_volume: if case == "libraries/defaults" {
                    4.0
                } else {
                    0.0
                },
                trade_count: 1.0,
            };
            let mut ctx = Ctx {
                bar_index: index,
                cvd: &[],
            };
            let previews = if persistent {
                usize::from(step % 3 < 2)
            } else {
                2
            };
            for _ in 0..previews {
                let before = super::parity_support::snapshot(&script);
                let preview = script.preview(&bar, &mut ctx).unwrap();
                assert_values(case, &preview.values, expected);
                writeln!(transcript, "{case}/{step} preview {preview:?}").unwrap();
                assert_eq!(before, super::parity_support::snapshot(&script));
            }
            if !persistent || step % 3 == 2 {
                script.on_close(&bar, &mut ctx).unwrap();
                let actual: Vec<_> = script
                    .descriptor()
                    .plots
                    .iter()
                    .map(|p| script.plots().column(p.id)[index])
                    .collect();
                assert_values(case, &actual, expected);
                writeln!(
                    transcript,
                    "{case}/{step} commit {}",
                    super::parity_support::snapshot(&script)
                )
                .unwrap();
            }
            cells += expected.len();
        }
    }
    (cells, transcript)
}

fn assert_values(case: &str, actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len(), "{case}");
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            super::parity_support::agrees(*actual, *expected),
            "{case}: {actual} != {expected}"
        );
    }
}

pub fn negative_golden() {
    let csv = include_str!("../golden/collections/negative.csv");
    assert_eq!(csv.lines().next(), Some("version,result"));
    for line in csv.lines().skip(1) {
        let (version, expected) = line.split_once(',').unwrap();
        let source = std::fs::read_to_string(
            super::parity_support::root()
                .join(format!("fixtures/collections/negative_v{version}.pine")),
        )
        .unwrap();
        let mut script = ScriptIndicator::new(compile(&source, "negative").unwrap(), &source);
        let bar = IndicatorBar {
            open_time: 0,
            close_time: 1,
            open: 10.0,
            high: 10.0,
            low: 10.0,
            close: 10.0,
            buy_volume: 1.0,
            sell_volume: 0.0,
            trade_count: 1.0,
        };
        let result = script.on_close(
            &bar,
            &mut Ctx {
                bar_index: 0,
                cvd: &[],
            },
        );
        if expected == "bounds error" {
            assert!(result.is_err());
        } else {
            result.unwrap();
            let values: Vec<_> = expected
                .split(';')
                .map(|s| s.parse::<f64>().unwrap())
                .collect();
            let actual: Vec<_> = script
                .descriptor()
                .plots
                .iter()
                .map(|p| script.plots().column(p.id)[0])
                .collect();
            assert_values("negative", &actual, &values);
        }
    }
}
