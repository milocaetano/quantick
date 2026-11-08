//! C41: hand-calculated commit/preview golden, recorded before IR implementation.
use quantick_indicators::{Ctx, Indicator, IndicatorBar, PlotId};
use quantick_pine::{ScriptIndicator, compile};

#[test]
fn slot_ir_commit_preview_parity() {
    let source = include_str!("fixtures/ir/rollback.pine");
    let mut compiled = compile(source, "rollback").unwrap_or_else(|errors| panic!("{errors:?}"));
    // Production execution must use lowered instructions even when the source
    // syntax is absent. A wrapper around the old tree walker fails here.
    compiled.ast = quantick_pine::ast::Ast::new();
    let mut script = ScriptIndicator::new(compiled, source);
    for line in include_str!("golden/ir/rollback.csv").lines().skip(1) {
        let cells: Vec<_> = line.split(',').collect();
        let bar_index: usize = cells[0].parse().unwrap();
        let close = (bar_index + 1) as f64 * 10.0;
        let bar = IndicatorBar {
            open_time: 0,
            close_time: 1,
            open: close,
            high: close,
            low: close,
            close,
            buy_volume: 1.0,
            sell_volume: 0.0,
            trade_count: 1.0,
        };
        let mut ctx = Ctx {
            bar_index,
            cvd: &[1.0, 2.0, 3.0],
        };
        let actual = if cells[1] == "preview" {
            let committed_before = format!("{:?}", script.plots());
            let preview = script.preview(&bar, &mut ctx).unwrap();
            assert_eq!(committed_before, format!("{:?}", script.plots()));
            preview.values
        } else {
            script.on_close(&bar, &mut ctx).unwrap();
            (0..6)
                .map(|column| script.plots().column(PlotId::new(column))[bar_index])
                .collect()
        };
        assert_eq!(actual.len(), cells.len() - 2);
        for (value, expected) in actual.iter().zip(&cells[2..]) {
            let expected: f64 = expected.parse().unwrap();
            assert!(
                *value == expected || value.is_nan() && expected.is_nan(),
                "{line}: {actual:?}"
            );
        }
    }
}

#[test]
fn slot_ir_eager_paths_history_functions() {
    let source = "//@version=5\nindicator(\"paths\")\nf(x) => ta.ema(x, 2)\ny = ta.crossover(close, 4) ? f(close) : f(close + 10)\nh = f(close)[1]\nplot(y)\nplot(h)\n";
    let mut script = ScriptIndicator::new(compile(source, "paths").unwrap(), source);
    let mut yes = quantick_indicators::ta::Ema::new(2);
    let mut no = quantick_indicators::ta::Ema::new(2);
    let mut cross = quantick_indicators::ta::CrossOver::new();
    let mut previous = f64::NAN;
    for (bar_index, close) in [3.0, 6.0, 2.0, 9.0, 1.0, 8.0].into_iter().enumerate() {
        let bar = IndicatorBar {
            open_time: 0,
            close_time: 1,
            open: close,
            high: close,
            low: close,
            close,
            buy_volume: 1.0,
            sell_volume: 0.0,
            trade_count: 1.0,
        };
        let mut ctx = Ctx {
            bar_index,
            cvd: &[],
        };
        let first = yes.push(close);
        let second = no.push(close + 10.0);
        let expected = if cross.push(close, 4.0) {
            first
        } else {
            second
        };
        for _ in 0..3 {
            let preview = script.preview(&bar, &mut ctx).unwrap();
            assert!(
                preview.values[0] == expected || preview.values[0].is_nan() && expected.is_nan()
            );
            assert!(
                preview.values[1] == previous || preview.values[1].is_nan() && previous.is_nan()
            );
        }
        script.on_close(&bar, &mut ctx).unwrap();
        let value = script.plots().column(PlotId::new(0))[bar_index];
        assert!(value == expected || value.is_nan() && expected.is_nan());
        previous = first;
    }
}
