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
