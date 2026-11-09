//! Independent, hand-calculated policy fixtures, separate from proxy data.
use quantick_indicators::{Ctx, Indicator, IndicatorBar, PlotId};
use quantick_pine::{ScriptIndicator, compile};

#[test]
fn conditional_state_and_history_parity() {
    let source = include_str!("fixtures/semantics/conditional.pine");
    let mut script = ScriptIndicator::new(compile(source, "semantics").unwrap(), source);
    for (index, line) in include_str!("golden/semantics/conditional.csv")
        .lines()
        .skip(1)
        .enumerate()
    {
        let bar = IndicatorBar {
            close: (index + 1) as f64,
            ..bar()
        };
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &[],
        };
        let first = format!("{:?}", script.preview(&bar, &mut ctx).unwrap());
        assert_eq!(
            first,
            format!("{:?}", script.preview(&bar, &mut ctx).unwrap())
        );
        script.on_close(&bar, &mut ctx).unwrap();
        for (column, cell) in line.split(',').enumerate() {
            let actual = script.plots().column(PlotId::new(column))[index];
            if cell == "na" {
                assert!(actual.is_nan(), "{index}:{column} = {actual}");
            } else {
                assert_eq!(actual, cell.parse::<f64>().unwrap(), "{index}:{column}");
            }
        }
    }
}

#[test]
fn varying_loop_sources_replace_from_pre_bar_state() {
    let source = include_str!("fixtures/semantics/loop.pine");
    let mut script = ScriptIndicator::new(compile(source, "loop").unwrap(), source);
    for (index, line) in include_str!("golden/semantics/loop.csv")
        .lines()
        .skip(2)
        .enumerate()
    {
        let bar = IndicatorBar {
            close: (index + 1) as f64,
            ..bar()
        };
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &[],
        };
        script.preview(&bar, &mut ctx).unwrap();
        script.preview(&bar, &mut ctx).unwrap();
        script.on_close(&bar, &mut ctx).unwrap();
        for (column, cell) in line.split(',').enumerate() {
            let actual = script.plots().column(PlotId::new(column))[index];
            if cell == "na" {
                assert!(actual.is_nan());
            } else {
                assert_eq!(actual, cell.parse::<f64>().unwrap());
            }
        }
    }
}

fn bar() -> IndicatorBar {
    IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: 1.0,
        high: 1.0,
        low: 1.0,
        close: 1.0,
        buy_volume: 1.0,
        sell_volume: 0.0,
        trade_count: 1.0,
    }
}

#[test]
fn skipped_function_calls_advance_without_leaking_effects() {
    let source = "//@version=5\nindicator(\"paths\")\nf(x) =>\n    line.new(0, 0, 1, 1)\n    ta.cum(x)\na = if bar_index % 2 == 0\n    f(close)\nelse\n    0\nb = if bar_index % 2 == 1\n    f(close * 10)\nelse\n    0\nplot(a)\nplot(b)\n";
    let mut script = ScriptIndicator::new(compile(source, "paths").unwrap(), source);
    for index in 0..5 {
        let bar = IndicatorBar {
            close: (index + 1) as f64,
            ..bar()
        };
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &[],
        };
        let wild = IndicatorBar {
            close: 999.0,
            ..bar
        };
        script.preview(&wild, &mut ctx).unwrap();
        script.preview(&bar, &mut ctx).unwrap();
        script.on_close(&bar, &mut ctx).unwrap();
    }
    assert_eq!(
        script.plots().column(PlotId::new(0)),
        &[1.0, 0.0, 6.0, 0.0, 15.0]
    );
    assert_eq!(
        script.plots().column(PlotId::new(1)),
        &[0.0, 30.0, 0.0, 100.0, 0.0]
    );
    assert_eq!(script.objects().unwrap().snapshot().lines.len(), 5);
}

#[test]
fn empty_loops_advance_and_later_visits_use_their_bar_position() {
    let source = "//@version=5\nindicator(\"empty\")\ns = 0.0\nfor i = 0 to (bar_index % 2 == 0 ? 0 : -1)\n    s := ta.cum(close)\nplot(s)\n";
    let mut script = ScriptIndicator::new(compile(source, "empty").unwrap(), source);
    for index in 0..5 {
        let bar = IndicatorBar {
            close: (index + 1) as f64,
            ..bar()
        };
        script
            .on_close(
                &bar,
                &mut Ctx {
                    bar_index: index,
                    cvd: &[],
                },
            )
            .unwrap();
    }
    assert_eq!(
        script.plots().column(PlotId::new(0)),
        &[1.0, 0.0, 6.0, 0.0, 15.0]
    );
}

#[test]
fn skipped_drawing_arguments_advance_without_creating_or_mutating_objects() {
    let source = "//@version=5\nindicator(\"draw args\")\nvar l = line.new(0, 0, 1, 0)\nif bar_index % 2 == 0\n    line.new(0, ta.cum(close), 1, 0)\n    l.set_y1(ta.cum(close * 10))\n";
    let mut script = ScriptIndicator::new(compile(source, "draw args").unwrap(), source);
    for index in 0..5 {
        let bar = IndicatorBar {
            close: (index + 1) as f64,
            ..bar()
        };
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &[],
        };
        let before = format!("{:?}", script.objects().map(|objects| objects.snapshot()));
        script
            .preview(
                &IndicatorBar {
                    close: 999.0,
                    ..bar
                },
                &mut ctx,
            )
            .unwrap();
        assert_eq!(
            before,
            format!("{:?}", script.objects().map(|objects| objects.snapshot()))
        );
        script.on_close(&bar, &mut ctx).unwrap();
    }
    let snapshot = script.objects().unwrap().snapshot();
    assert_eq!(
        snapshot
            .lines
            .iter()
            .map(|line| line.y1)
            .collect::<Vec<_>>(),
        vec![150.0, 1.0, 6.0, 15.0]
    );
}
