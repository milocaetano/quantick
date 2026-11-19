//! What a strategy reads off a script besides its plots: the
//! `alertcondition` titles it declares, before any bar runs, and the
//! structured form of a runtime failure — the code and the line, not a
//! sentence to parse them back out of.

use quantick_indicators::{Ctx, Indicator, IndicatorBar};
use quantick_pine::{AlertTitle, ErrorCode, ScriptIndicator, alert_titles, compile};

const RENKO_POC_FLIP: &str = include_str!("../../app/scripts/renko_poc_flip.pine");

fn bar(index: usize, close: f64) -> IndicatorBar {
    let t = 1_700_000_000_000 + index as i64 * 1_000;
    IndicatorBar {
        open_time: t,
        close_time: t + 999,
        open: 1.0,
        high: close.max(1.0),
        low: close.min(1.0),
        close,
        buy_volume: 1.0,
        sell_volume: 1.0,
        trade_count: 2.0,
        poc: None,
    }
}

#[test]
fn the_fixture_declares_its_two_literal_titles_in_call_order() {
    let compiled = compile(RENKO_POC_FLIP, "renko_poc_flip.pine").expect("the fixture compiles");
    assert_eq!(
        alert_titles(&compiled, RENKO_POC_FLIP),
        vec![
            AlertTitle::Literal("Bull flip".to_owned()),
            AlertTitle::Literal("Bear flip".to_owned()),
        ]
    );
}

#[test]
fn a_missing_title_and_a_computed_one_are_named_apart() {
    let source = "//@version=5\n\
        indicator(\"t\")\n\
        alertcondition(close > open)\n\
        name = close > open ? \"up\" : \"down\"\n\
        alertcondition(close > open, name)\n\
        alertcondition(close < open, message=\"m\", title=\"Sell dip\")\n\
        plot(close)\n";
    let compiled = compile(source, "t.pine").expect("compiles");
    assert_eq!(
        alert_titles(&compiled, source),
        vec![
            AlertTitle::Default,
            AlertTitle::Computed { line: 5 },
            AlertTitle::Literal("Sell dip".to_owned()),
        ]
    );
}

#[test]
fn a_runtime_failure_keeps_its_code_and_line() {
    let source = "//@version=5\n\
        indicator(\"t\")\n\
        x = 0.0\n\
        if bar_index == 1\n\
        \x20   while x >= 0\n\
        \x20       x := x + 1\n\
        plot(x)\n";
    let compiled = compile(source, "t.pine").expect("compiles");
    let mut script = ScriptIndicator::new(compiled, source);
    let cvd = [0.0, 0.0];
    script
        .on_close(
            &bar(0, 2.0),
            &mut Ctx {
                bar_index: 0,
                cvd: &cvd[..1],
            },
        )
        .expect("bar 0 never enters the loop");
    assert_eq!(script.runtime_fault(), None, "a clean run leaves no fault");

    script
        .on_close(
            &bar(1, 2.0),
            &mut Ctx {
                bar_index: 1,
                cvd: &cvd,
            },
        )
        .expect_err("bar 1 spends the loop budget");
    let fault = script.runtime_fault().expect("the failure is kept as data");
    assert_eq!(fault.code, ErrorCode::PineLoopBudget);
    assert_eq!(fault.line, 5, "the `while` that ran away");
    assert!(!fault.message.is_empty());

    script.reset();
    assert_eq!(script.runtime_fault(), None, "a reset forgets the failure");
}
