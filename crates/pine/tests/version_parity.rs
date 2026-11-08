//! C81: paired primary-reference version semantics and retained v5 behavior.
use quantick_indicators::{Ctx, Indicator, IndicatorBar, PlotId};
use quantick_pine::{ScriptIndicator, compile};

#[test]
fn version_six_semantic_parity() {
    for (source, golden) in [
        (
            include_str!("fixtures/version/v5.pine"),
            include_str!("golden/version/v5.csv"),
        ),
        (
            include_str!("fixtures/version/v6.pine"),
            include_str!("golden/version/v6.csv"),
        ),
    ] {
        let compiled = compile(source, "version").unwrap_or_else(|e| panic!("{source}\n{e:?}"));
        let mut script = ScriptIndicator::new(compiled, source);
        for (index, line) in golden.lines().skip(1).enumerate() {
            let row: Vec<f64> = line.split(',').map(|s| s.parse().unwrap()).collect();
            let bar = IndicatorBar {
                open_time: 0,
                close_time: 1,
                open: row[0],
                high: row[0],
                low: row[0],
                close: row[0],
                buy_volume: 0.0,
                sell_volume: 0.0,
                trade_count: 0.0,
            };
            let mut ctx = Ctx {
                bar_index: index,
                cvd: &[0.0; 3],
            };
            let preview = script.preview(&bar, &mut ctx).unwrap();
            assert_eq!(preview.values, &row[1..], "bar {index}: {source}");
            assert_eq!(
                script.preview(&bar, &mut ctx).unwrap().values,
                preview.values
            );
            script.on_close(&bar, &mut ctx).unwrap();
            for (column, expected) in row[1..].iter().enumerate() {
                assert_eq!(script.plots().column(PlotId::new(column))[index], *expected);
            }
        }
    }
    for line in include_str!("fixtures/version/rejected-v6.txt")
        .lines()
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
    {
        let source = format!(
            "//@version=6\nindicator(\"reject\")\n{}\n",
            line.replace("\\n", "\n")
        );
        assert!(compile(&source, "version").is_err(), "accepted {source}");
    }
}
