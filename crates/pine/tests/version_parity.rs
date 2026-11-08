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

#[test]
fn version_palette_and_supported_arguments() {
    for (version, teal) in [(5, 0x0089_7BFF), (6, 0x0899_81FF)] {
        let source = format!(
            "//@version={version}\nindicator(\"palette\")\nplot(close, color=color.red)\nplot(close, color=color.teal)\nplot(close, color=color.yellow)\n"
        );
        let compiled = compile(&source, "palette").unwrap_or_else(|e| panic!("{e:?}"));
        let expected = [0xF236_45FF, teal, 0xFDD8_35FF];
        for (plot, expected) in compiled.plots.iter().zip(expected) {
            assert_eq!(
                plot.base_color,
                quantick_indicators::Rgba8::from_u32(expected)
            );
        }
    }
    for version in [5, 6] {
        let source = format!(
            "//@version={version}\nindicator(\"casts\")\nplot(bool(close) ? 1 : 0)\nplot(5 / input.int(2))\nplot(close, color=color.new(color.red, 50), linewidth=1)\n"
        );
        assert!(compile(&source, "casts").is_ok());
    }
    for body in [
        "plot(close, style=na)",
        "plot(close, linewidth=0)",
        "plot(close, title=\"a\", title=\"b\")",
        "plot(1[1])",
        "bool x = na",
    ] {
        let source = format!("//@version=5\nindicator(\"legacy\")\n{body}\n");
        assert!(compile(&source, "legacy").is_ok(), "{source}");
    }
}

#[test]
fn version_dynamic_loop_budget() {
    let source = "//@version=6\nindicator(\"budget\")\nlimit = 1\nfor i = 0 to limit\n    limit := limit + 1\nplot(close)\n";
    let mut script = ScriptIndicator::new(
        compile(source, "budget").unwrap_or_else(|e| panic!("{e:?}")),
        source,
    );
    let bar = IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: 1.0,
        high: 1.0,
        low: 1.0,
        close: 1.0,
        buy_volume: 0.0,
        sell_volume: 0.0,
        trade_count: 0.0,
    };
    let mut ctx = Ctx {
        bar_index: 0,
        cvd: &[0.0],
    };
    assert!(script.preview(&bar, &mut ctx).is_err());
}

#[test]
fn version_numeric_markers_preserve_absolute_values() {
    for version in [5, 6] {
        let source = format!(
            "//@version={version}\nindicator(\"markers\")\nplotshape(close, location=location.absolute)\nplotchar(close, location=location.absolute)\nplotshape(close)\nplotchar(close)\n"
        );
        let compiled = compile(&source, "markers").unwrap_or_else(|e| panic!("{e:?}"));
        let mut script = ScriptIndicator::new(compiled, &source);
        for (index, close) in [12.5, -3.0].into_iter().enumerate() {
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
                cvd: &[0.0; 2],
            };
            let expected = [close, close, 1.0, 1.0];
            for _ in 0..2 {
                assert_eq!(script.preview(&bar, &mut ctx).unwrap().values, expected);
            }
            script.on_close(&bar, &mut ctx).unwrap();
            for (column, expected) in expected.into_iter().enumerate() {
                assert_eq!(script.plots().column(PlotId::new(column))[index], expected);
            }
        }
        let alert = format!("//@version={version}\nindicator(\"alert\")\nalertcondition(close)\n");
        assert_eq!(compile(&alert, "alert").is_ok(), version == 5);
    }
}
