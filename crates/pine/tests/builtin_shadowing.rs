//! A script's own names win over builtins of the same name, so a builtin
//! added later never breaks a script that already used the name: a
//! volume-profile script naming its helper `poc(...)` predates `poc`.

use quantick_indicators::{IndicatorBar, PlotId, golden};
use quantick_pine::{ScriptIndicator, compile};

fn bar(close: f64) -> IndicatorBar {
    IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: close,
        high: close,
        low: close,
        close,
        buy_volume: 1.0,
        sell_volume: 1.0,
        trade_count: 1.0,
        poc: Some(50.0),
    }
}

fn first_plot(source: &str) -> Vec<f64> {
    let compiled = match compile(source, "shadowing.pine") {
        Ok(compiled) => compiled,
        Err(errors) => panic!("the script must compile: {errors:?}"),
    };
    let mut script = ScriptIndicator::new(compiled, source);
    golden::run_indicator(&mut script, &[bar(10.0), bar(20.0)]).expect("runs");
    use quantick_indicators::Indicator as _;
    script.plots().column(PlotId::new(0)).to_vec()
}

#[test]
fn a_user_function_named_like_a_builtin_series_is_called() {
    let source = "//@version=5\nindicator(\"x\")\npoc(src) => src * 2\nplot(poc(close))\n";
    assert_eq!(first_plot(source), [20.0, 40.0]);
}

#[test]
fn a_user_variable_named_like_a_builtin_series_is_read() {
    let source = "//@version=5\nindicator(\"x\")\npoc = close + 1\nplot(poc)\n";
    assert_eq!(first_plot(source), [11.0, 21.0]);
}

#[test]
fn the_builtin_series_reads_the_bar_poc() {
    let source = "//@version=5\nindicator(\"x\")\nplot(poc)\n";
    assert_eq!(first_plot(source), [50.0, 50.0]);
}
