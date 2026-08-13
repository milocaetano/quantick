//! Semantic proof for the embedded `copilot.pine`: the SELL marker fires on
//! the confirming bar of a double top whose second touch shows CVD
//! divergence, absorption and sell aggression — and stays silent when the
//! divergence is absent. A flipped comparison anywhere in the setup chain
//! turns one of these tapes green where it must be red, so compile
//! conformance alone cannot stand in for this test.

use quantick_indicators::{Ctx, Indicator, IndicatorBar, InputValue, PlotId};
use quantick_pine::{ScriptIndicator, compile};

const COPILOT: &str = include_str!("corpus/ok/copilot.pine");

/// Shrunken windows so the fixture stays readable: the script's defaults
/// need 100-bar warmups; the logic under test is identical at any length.
fn test_inputs() -> Vec<InputValue> {
    vec![
        InputValue::Int(10),     // regime_len
        InputValue::Float(50.0), // regime_k — wide open: regime is not under test here
        InputValue::Int(3),      // atr_len
        InputValue::Int(2),      // pivot_l
        InputValue::Int(2),      // pivot_r
        InputValue::Float(0.5),  // tol_atr
        InputValue::Int(3),      // min_gap
        InputValue::Int(30),     // max_gap
        InputValue::Int(10),     // z_len
        InputValue::Float(1.5),  // z_thr
        InputValue::Int(10),     // er_len
        InputValue::Float(1.2),  // er_thr
        InputValue::Float(0.1),  // rng_floor
    ]
}

/// A fully explicit bar: the copilot reads highs, lows, volumes and delta,
/// so the script_semantics close-only helper is not enough here.
#[derive(Clone, Copy)]
struct Tape {
    high: f64,
    low: f64,
    close: f64,
    buy: f64,
    sell: f64,
}

fn bar(index: usize, t: Tape) -> IndicatorBar {
    let ms = 1_700_000_000_000 + index as i64 * 1_000;
    IndicatorBar {
        open_time: ms,
        close_time: ms + 999,
        open: t.close,
        high: t.high,
        low: t.low,
        close: t.close,
        buy_volume: t.buy,
        sell_volume: t.sell,
        trade_count: 3.0,
    }
}

/// Drives the rebound copilot exactly like the host: cvd accumulated
/// across bars, one commit run per closed bar.
fn run(tape: &[Tape]) -> Box<dyn Indicator> {
    let compiled = match compile(COPILOT, "copilot.pine") {
        Ok(c) => c,
        Err(errors) => panic!(
            "copilot.pine must compile:\n{}",
            errors
                .iter()
                .map(|e| e.render("copilot.pine", COPILOT))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    let base = ScriptIndicator::new(compiled, COPILOT.to_owned());
    let mut indicator = base
        .rebind(&test_inputs())
        .expect("input value set matches the script's input list");
    let mut cvd = Vec::new();
    let mut sum = 0.0;
    for (i, &t) in tape.iter().enumerate() {
        sum += t.buy - t.sell;
        cvd.push(sum);
        let mut ctx = Ctx {
            bar_index: i,
            cvd: &cvd,
        };
        indicator
            .on_close(&bar(i, t), &mut ctx)
            .expect("commit run succeeds");
    }
    indicator
}

fn marker_rows(indicator: &dyn Indicator, title: &str) -> Vec<usize> {
    let spec = indicator
        .descriptor()
        .plots
        .iter()
        .find(|p| p.title == title)
        .unwrap_or_else(|| panic!("plot {title:?} exists"));
    indicator
        .plots()
        .column(PlotId::new(spec.id.index()))
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.is_nan())
        .map(|(i, _)| i)
        .collect()
}

/// Quiet range bar: two-point range, mild buy pressure.
const CALM: Tape = Tape {
    high: 101.0,
    low: 99.0,
    close: 100.0,
    buy: 2.0,
    sell: 1.0,
};

/// The double-top tape. Touches at bars 12 and 20 print the same 110.0
/// high (pivot strength 2 each side); `between` shapes the CVD path from
/// the first touch to the second; the second touch absorbs (40 lots in a
/// 1.5-point range vs ~1.5 lots/point on calm bars) and bar 21 is the
/// sell aggression (delta −29, far past 1.5 sigmas of the recent tape).
/// With pivot_r = 2, bar 22 confirms the second pivot: the one bar where
/// the marker may print.
fn double_top_tape(between: Tape) -> Vec<Tape> {
    let mut tape = vec![CALM; 23];
    let touch = Tape {
        high: 110.0,
        low: 108.0,
        close: 109.0,
        ..CALM
    };
    tape[12] = touch;
    for slot in tape.iter_mut().take(20).skip(13) {
        *slot = between;
    }
    tape[20] = Tape {
        high: 110.0,
        low: 108.5,
        close: 109.0,
        buy: 20.0,
        sell: 20.0,
    };
    tape[21] = Tape {
        buy: 1.0,
        sell: 30.0,
        ..CALM
    };
    tape
}

/// Sell-heavy drift between the touches: the retest of the high happens on
/// a falling CVD — the divergence the setup requires.
const SELL_DRIFT: Tape = Tape {
    buy: 1.0,
    sell: 4.0,
    ..CALM
};

/// Buy-heavy drift: same double top, but CVD is *higher* on the second
/// touch — no divergence, the marker must stay silent.
const BUY_DRIFT: Tape = Tape {
    buy: 4.0,
    sell: 1.0,
    ..CALM
};

#[test]
fn the_sell_marker_fires_on_the_confirmed_divergent_double_top() {
    let indicator = run(&double_top_tape(SELL_DRIFT));
    assert_eq!(
        marker_rows(indicator.as_ref(), "Short setup"),
        vec![22],
        "exactly the confirmation bar of the second touch"
    );
    assert_eq!(
        marker_rows(indicator.as_ref(), "Long setup"),
        Vec::<usize>::new(),
        "a double top arms no long"
    );
}

#[test]
fn without_cvd_divergence_the_same_double_top_stays_silent() {
    let indicator = run(&double_top_tape(BUY_DRIFT));
    assert_eq!(
        marker_rows(indicator.as_ref(), "Short setup"),
        Vec::<usize>::new(),
        "second touch on rising CVD is not the setup"
    );
}
