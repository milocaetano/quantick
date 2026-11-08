//! C71: independently authored candle and bar output goldens.
use quantick_indicators::{CandleSample, Ctx, Indicator, IndicatorBar, Rgba8};
use quantick_pine::{ScriptIndicator, compile};

fn load(source: &str) -> ScriptIndicator {
    ScriptIndicator::new(
        compile(source, "candles").unwrap_or_else(|e| panic!("{e:?}")),
        source,
    )
}
fn bar(values: &str) -> IndicatorBar {
    let v: Vec<f64> = values.split(',').map(|s| s.parse().unwrap()).collect();
    IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: v[0],
        high: v[1],
        low: v[2],
        close: v[3],
        buy_volume: 2.0,
        sell_volume: 1.0,
        trade_count: 3.0,
    }
}
fn color(c: Option<Rgba8>) -> String {
    c.map_or_else(|| "na".into(), |c| format!("{:08x}", c.to_u32()))
}
fn sample(c: Option<CandleSample>, is_bar: bool) -> String {
    let Some(c) = c else {
        return if is_bar {
            "na,na,na,na,na"
        } else {
            "na,na,na,na,na,na,na"
        }
        .into();
    };
    let mut s = format!(
        "{},{},{},{},{}",
        c.open,
        c.high,
        c.low,
        c.close,
        color(c.color)
    );
    if !is_bar {
        s.push_str(&format!(
            ",{},{}",
            color(c.wick_color),
            color(c.border_color)
        ));
    }
    s
}
#[test]
fn candle_and_bar_plot_parity() {
    let source = include_str!("fixtures/candle_plots/plots.pine");
    let input: Vec<_> = include_str!("fixtures/candle_plots/bars.csv")
        .lines()
        .skip(1)
        .collect();
    let golden: Vec<_> = include_str!("golden/candle_plots/plots.csv")
        .lines()
        .skip(1)
        .collect();
    assert_eq!(input.len(), golden.len());
    let mut script = load(source);
    assert_eq!(
        script
            .plots()
            .candle_specs()
            .iter()
            .map(|s| (s.title.as_str(), s.is_bar))
            .collect::<Vec<_>>(),
        [("candles", false), ("bars", true)]
    );
    assert_eq!(script.plots().plot_count(), 0);
    for replay in 0..2 {
        for (i, (input, expected)) in input.iter().zip(&golden).enumerate() {
            let b = bar(input);
            let mut ctx = Ctx {
                bar_index: i,
                cvd: &[],
            };
            let before = format!("{:?}", script.plots());
            let first = script.preview(&b, &mut ctx).unwrap();
            let mut changed = b;
            changed.close += 10.0;
            script.preview(&changed, &mut ctx).unwrap();
            assert_eq!(first, script.preview(&b, &mut ctx).unwrap());
            assert_eq!(before, format!("{:?}", script.plots()));
            assert_eq!(
                format!(
                    "{},{}",
                    sample(first.candles[0], false),
                    sample(first.candles[1], true)
                ),
                *expected,
                "preview {replay}/{i}"
            );
            script.on_close(&b, &mut ctx).unwrap();
            assert_eq!(script.plots().len(), i + 1);
            assert_eq!(
                format!(
                    "{},{}",
                    sample(script.plots().candle_column(0).unwrap()[i], false),
                    sample(script.plots().candle_column(1).unwrap()[i], true)
                ),
                *expected,
                "commit {replay}/{i}"
            );
        }
        script.reset();
        assert!(script.plots().candle_column(0).unwrap().is_empty());
    }
}
#[test]
fn candle_validation_and_top_level_scope() {
    for body in [
        "plotcandle(open, high, low)",
        "plotbar(open, high, low, close, wickcolor=#ffffff)",
        "plotcandle(open, high, low, close, color=#ffffff, color=na)",
        "plotcandle(open, high, low, close, show_last=1)",
        "if close > open\n    plotbar(open, high, low, close)",
        "f() =>\n    plotcandle(open, high, low, close)",
        "a = plotcandle(open, high, low, close)\nb = plot(close)\nfill(a,b)",
    ] {
        assert!(
            compile(&format!("//@version=5\n{body}\n"), "invalid").is_err(),
            "{body}"
        );
    }
}
#[test]
fn candle_normalization_missing_prices_and_failed_runs() {
    let mut script = load(
        "//@version=5\nplot(close)\nplotcandle(close=2, low=9, high=1, open=5, color=na)\nplotbar(na, 1, 2, 3)\nplotbar(1, na, 2, 3)\nplotbar(1, 2, na, 3)\n",
    );
    let mut ctx = Ctx {
        bar_index: 0,
        cvd: &[],
    };
    let b = bar("1,2,0,1");
    let frame = script.preview(&b, &mut ctx).unwrap();
    let c = frame.candles[0].unwrap();
    assert_eq!((c.open, c.high, c.low, c.close), (5.0, 9.0, 1.0, 2.0));
    assert_eq!(c.color, None);
    assert_eq!(&frame.candles[1..], &[None, None, None]);
    assert_eq!(frame.values, &[1.0]);
    let mut bad = load("//@version=5\nplotcandle(open, high, low, close, color=close)\n");
    assert!(bad.on_close(&b, &mut ctx).is_err());
    assert!(bad.plots().is_empty());
    assert!(bad.preview(&b, &mut ctx).is_err());
    assert!(bad.plots().candle_column(0).unwrap().is_empty());
}

#[test]
fn candle_kernel_preview_rollback_and_rebind() {
    let source = "//@version=5\nlength = input.int(2)\nplotcandle(ta.cum(close * length), high, low, close)\n";
    let mut script = load(source)
        .rebind(&[quantick_indicators::InputValue::Int(2)])
        .unwrap();
    let mut ctx = Ctx {
        bar_index: 0,
        cvd: &[],
    };
    script.on_close(&bar("1,2,0,1"), &mut ctx).unwrap();
    ctx.bar_index = 1;
    let b = bar("1,8,0,7");
    let first = script.preview(&b, &mut ctx).unwrap();
    assert_eq!(first.candles[0].unwrap().open, 16.0);
    assert_eq!(first, script.preview(&b, &mut ctx).unwrap());
    assert_eq!(script.plots().candle_column(0).unwrap().len(), 1);
    script.on_close(&b, &mut ctx).unwrap();
    assert_eq!(
        first.candles[0],
        script.plots().candle_column(0).unwrap()[1]
    );
}

#[test]
fn candle_nested_expressions_are_rejected() {
    for body in [
        "plot(plotbar())",
        "plot(plotbar(open, high, low, close))",
        "a = 1 + plotcandle(open, high, low, close)",
        "a = true ? plotbar(open, high, low, close) : na",
        "plotcandle(open, high, low, plotbar(open, high, low, close))",
    ] {
        let errors = compile(&format!("//@version=5\n{body}\n"), "nested")
            .err()
            .expect("nested candle output must not compile");
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("direct top-level")),
            "{body}: {errors:?}"
        );
    }
}
