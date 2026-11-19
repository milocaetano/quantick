//! `alertcondition` semantics: an alarm is raised by a commit run whose
//! condition holds, once, and never by a preview run — a forming bar may
//! still change its mind.

use quantick_indicators::{Ctx, FiredAlert, Indicator, IndicatorBar};
use quantick_pine::{ScriptIndicator, compile};

const RENKO_POC_FLIP: &str = include_str!("../../app/scripts/renko_poc_flip.pine");

/// A brick from `open` to `close` whose footprint POC is `poc`.
fn brick(index: usize, open: f64, close: f64, poc: Option<f64>) -> IndicatorBar {
    let t = 1_700_000_000_000 + index as i64 * 1_000;
    IndicatorBar {
        open_time: t,
        close_time: t + 999,
        open,
        high: open.max(close),
        low: open.min(close),
        close,
        buy_volume: 2.0,
        sell_volume: 1.0,
        trade_count: 3.0,
        poc,
    }
}

fn load(source: &str) -> ScriptIndicator {
    let compiled = compile(source, "test.pine").unwrap_or_else(|errors| {
        panic!("script must compile: {errors:?}");
    });
    ScriptIndicator::new(compiled, source)
}

fn ctx(index: usize, cvd: &[f64]) -> Ctx<'_> {
    Ctx {
        bar_index: index,
        cvd: &cvd[..=index],
    }
}

fn alert(title: &str, message: &str) -> FiredAlert {
    FiredAlert {
        title: title.to_owned(),
        message: message.to_owned(),
    }
}

const UP_DOWN: &str = "//@version=5\n\
    indicator(\"t\")\n\
    alertcondition(close > open, title=\"up\", message=\"went up\")\n\
    alertcondition(close < open, \"down\")\n\
    plot(close)\n";

#[test]
fn a_held_condition_fires_once_on_the_commit_run() {
    let mut script = load(UP_DOWN);
    let cvd = [0.0; 2];
    script.on_close(&brick(0, 10.0, 11.0, None), &mut ctx(0, &cvd)).unwrap();
    assert_eq!(script.take_fired_alerts(), vec![alert("up", "went up")]);
    assert!(script.take_fired_alerts().is_empty(), "taken alarms are gone");

    // A message left out reads as the title.
    script.on_close(&brick(1, 11.0, 10.0, None), &mut ctx(1, &cvd)).unwrap();
    assert_eq!(script.take_fired_alerts(), vec![alert("down", "down")]);
}

#[test]
fn a_preview_never_fires_and_never_drops_a_pending_alarm() {
    let mut script = load(UP_DOWN);
    let cvd = [0.0; 2];
    for _ in 0..3 {
        script.preview(&brick(0, 10.0, 11.0, None), &mut ctx(0, &cvd)).unwrap();
    }
    assert!(script.take_fired_alerts().is_empty(), "a forming bar fires nothing");

    script.on_close(&brick(0, 10.0, 11.0, None), &mut ctx(0, &cvd)).unwrap();
    script.preview(&brick(1, 11.0, 12.0, None), &mut ctx(1, &cvd)).unwrap();
    assert_eq!(
        script.take_fired_alerts(),
        vec![alert("up", "went up")],
        "the closed bar's alarm survives the next bar's preview"
    );
}

#[test]
fn each_commit_run_replaces_the_alarms_left_untaken() {
    let mut script = load(UP_DOWN);
    let cvd = [0.0; 3];
    script.on_close(&brick(0, 10.0, 11.0, None), &mut ctx(0, &cvd)).unwrap();
    script.on_close(&brick(1, 11.0, 11.0, None), &mut ctx(1, &cvd)).unwrap();
    assert!(
        script.take_fired_alerts().is_empty(),
        "a replayed bar's alarm does not ride into the next close"
    );
    script.on_close(&brick(2, 11.0, 12.0, None), &mut ctx(2, &cvd)).unwrap();
    script.reset();
    assert!(script.take_fired_alerts().is_empty(), "reset drops them");
}

#[test]
fn a_failed_commit_run_raises_nothing() {
    let source = "//@version=5\n\
        indicator(\"t\")\n\
        alertcondition(close > open, \"up\")\n\
        a = array.new_float(0)\n\
        plot(array.get(a, 3))\n";
    let mut script = load(source);
    let cvd = [0.0];
    assert!(script.on_close(&brick(0, 10.0, 11.0, None), &mut ctx(0, &cvd)).is_err());
    assert!(script.take_fired_alerts().is_empty());
}

#[test]
fn alertcondition_no_longer_warns() {
    let compiled = compile(UP_DOWN, "t.pine").unwrap();
    assert!(compiled.warnings.is_empty(), "{:?}", compiled.warnings);
}

#[test]
fn renko_poc_flip_loads_without_warnings() {
    let compiled = compile(RENKO_POC_FLIP, "renko_poc_flip.pine").unwrap();
    assert!(compiled.warnings.is_empty(), "{:?}", compiled.warnings);
}

#[test]
fn renko_poc_flip_fires_on_a_colour_flip_the_poc_confirms() {
    let mut script = load(RENKO_POC_FLIP);
    // down, up with POC higher (bull), up again (no flip), down with POC
    // lower (bear), up with POC lower (flip the POC denies), down with no
    // footprint (no POC, no flip).
    let bricks = [
        brick(0, 11.0, 10.0, Some(10.5)),
        brick(1, 10.0, 11.0, Some(10.75)),
        brick(2, 11.0, 12.0, Some(11.5)),
        brick(3, 12.0, 11.0, Some(11.25)),
        brick(4, 11.0, 12.0, Some(11.0)),
        brick(5, 12.0, 11.0, None),
    ];
    let cvd = [0.0; 6];
    let mut fired = Vec::new();
    for (index, bar) in bricks.iter().enumerate() {
        script.preview(bar, &mut ctx(index, &cvd)).unwrap();
        script.on_close(bar, &mut ctx(index, &cvd)).unwrap();
        for alarm in script.take_fired_alerts() {
            fired.push((index, alarm.title));
        }
    }
    assert_eq!(
        fired,
        vec![(1, "Bull flip".to_owned()), (3, "Bear flip".to_owned())]
    );
}
