//! Interpreter benchmark: real starter-shape scripts over a large
//! deterministic bar burst, measured per commit run against the §5 budget
//! (typical script ≤ 50 µs; hard review fail at 200 µs).
//!
//! Dependency-free like the other benches; run with
//! `cargo bench -p quantick-pine`. Numbers go in PR descriptions.

use std::hint::black_box;
use std::time::Instant;

use quantick_indicators::{Ctx, Indicator, IndicatorBar, PlotId};
use quantick_pine::{ScriptIndicator, compile};

const EMA_SCRIPT: &str =
    "//@version=5\nindicator(\"EMA\")\nlen = input.int(21, \"Length\")\nplot(ta.ema(close, len))\n";

const FLOW_SCRIPT: &str = "//@version=5\nindicator(\"flow\")\nfast = ta.ema(close, 9)\nslow = ta.ema(close, 21)\nup = ta.crossover(fast, slow)\nstrength = math.abs(delta) / math.max(volume, 1)\nscore = if up\n    strength * 2\nelse\n    strength\nplot(fast)\nplot(slow)\nplot(score)\nplot(cvd)\n";

/// The embedded `force_bar.pine`, byte for byte: what the paint channel
/// actually costs on the commit path, measured on the script that ships
/// rather than on a sketch of it.
const FORCE_BAR: &str = include_str!("../tests/corpus/ok/force_bar.pine");

/// The embedded `exhaustion_reversal.pine`, byte for byte. Its per-bar work
/// is a state machine plus three hoisted kernels — no loops, no dynamic
/// history offsets — so this number says what the arm-and-fade shape costs
/// when written that way instead of as a scan back over the window.
///
/// Measured on **both** tapes below, and the pair is the point: this script
/// keeps most of its arithmetic behind `and` guards that only open once a
/// force bar is armed, so the quiet tape measures the cheap path and the
/// active one measures the path that actually does the work.
const EXHAUSTION_REVERSAL: &str = include_str!("../tests/corpus/ok/exhaustion_reversal.pine");

fn make_bars(n: usize) -> Vec<IndicatorBar> {
    (0..n)
        .map(|i| {
            let close = 36_000.0 + ((i % 100) as f64 - 50.0) / 10.0;
            let t = 1_700_000_000_000 + i as i64 * 250;
            IndicatorBar {
                open_time: t,
                close_time: t + 249,
                open: close - 0.3,
                high: close + 0.8,
                low: close - 0.8,
                close,
                buy_volume: ((i % 900) + 1) as f64 / 1000.0,
                sell_volume: ((i % 700) + 1) as f64 / 1000.0,
                trade_count: (i % 50 + 1) as f64,
            }
        })
        .collect()
}

/// A tape whose bars are not all the same size, so a script that asks "is
/// this bar unusually large?" can answer yes.
///
/// `make_bars` cannot: its body is a constant 0.3 on every one of its bars,
/// so every ×average test is false forever and a conditional script is timed
/// entirely on its untaken branches. Here every `CYCLE`th bar carries a body
/// ten times the ordinary one and reaches a new high, and the three bars
/// after it hand most of it back — the shape `exhaustion_reversal.pine` arms
/// on, delivered often enough to be measured and rarely enough to stay
/// realistic.
fn make_active_bars(n: usize) -> Vec<IndicatorBar> {
    /// Bars between one force bar and the next.
    const CYCLE: usize = 37;
    /// Body of the force bar, in price units.
    const PUSH: f64 = 3.0;
    /// Body of each of the three bars that hand it back.
    const GIVE_BACK: f64 = 0.9;
    /// Body of an ordinary bar.
    const CHOP: f64 = 0.3;
    /// Wick beyond the body, each side.
    const WICK: f64 = 0.1;

    let mut bars = Vec::with_capacity(n);
    let mut open = 36_000.0;
    for i in 0..n {
        let close = match i % CYCLE {
            0 => open + PUSH,
            1..=3 => open - GIVE_BACK,
            phase if phase % 2 == 0 => open + CHOP,
            _ => open - CHOP,
        };
        let t = 1_700_000_000_000 + i as i64 * 250;
        bars.push(IndicatorBar {
            open_time: t,
            close_time: t + 249,
            open,
            high: open.max(close) + WICK,
            low: open.min(close) - WICK,
            close,
            buy_volume: ((i % 900) + 1) as f64 / 1000.0,
            sell_volume: ((i % 700) + 1) as f64 / 1000.0,
            trade_count: (i % 50 + 1) as f64,
        });
        open = close;
    }
    bars
}

fn bench(name: &str, source: &str, bars: &[IndicatorBar]) {
    let compiled = compile(source, name).expect("bench script compiles");
    let nodes = compiled.ast.len();
    let mut indicator = ScriptIndicator::new(compiled, source);
    let mut cvd = Vec::with_capacity(bars.len());
    let mut sum = 0.0;
    let start = Instant::now();
    for (bar_index, bar) in bars.iter().enumerate() {
        sum += bar.delta();
        cvd.push(sum);
        let mut ctx = Ctx {
            bar_index,
            cvd: &cvd,
        };
        indicator
            .on_close(black_box(bar), &mut ctx)
            .expect("bench script evaluates");
    }
    let elapsed = start.elapsed();
    let ns_per = elapsed.as_secs_f64() * 1e9 / bars.len() as f64;
    // Cells the script actually drew. A conditional script on the wrong tape
    // draws nothing and times only its untaken branches, which reads as a
    // flattering number rather than as the mistake it is — so the count goes
    // beside the timing instead of being left to be assumed.
    let drawn: usize = (0..indicator.descriptor().plots.len())
        .map(|plot| {
            indicator
                .plots()
                .column(PlotId::new(plot))
                .iter()
                .filter(|value| !value.is_nan())
                .count()
        })
        .sum();
    println!(
        "{name:16} {nodes:>4} AST nodes  {n:>7} bars in {elapsed:>9.2?}  =>  {ns_per:8.1} ns/bar  ({drawn} cells drawn)",
        n = bars.len(),
    );
    black_box(indicator.plots().len());
}

fn main() {
    let n = 100_000;
    println!("interpreter benchmark: {n} deterministic bars (budget: 50 us/commit)\n");
    let bars = make_bars(n);
    bench("ema.pine", EMA_SCRIPT, &bars);
    bench("flow.pine", FLOW_SCRIPT, &bars);
    bench("force_bar", FORCE_BAR, &bars);
    bench("exh_reversal", EXHAUSTION_REVERSAL, &bars);

    // The same script on a tape that arms it. Everything above runs on bars
    // of one constant body, where every "unusually large?" test is false and
    // the conditional half of a script is never entered.
    println!("\non a tape that actually arms the conditional paths:\n");
    let active = make_active_bars(n);
    bench("force_bar", FORCE_BAR, &active);
    bench("exh_reversal", EXHAUSTION_REVERSAL, &active);
}
