//! C41: hand-calculated commit/preview golden, recorded before IR implementation.
use quantick_indicators::{Ctx, Indicator, IndicatorBar, PlotId};
use quantick_pine::{ScriptIndicator, compile};

#[test]
fn slot_ir_commit_preview_parity() {
    let source = include_str!("fixtures/ir/rollback.pine");
    let compiled = compile(source, "rollback").unwrap_or_else(|errors| panic!("{errors:?}"));
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
