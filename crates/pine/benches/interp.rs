//! Interpreter benchmark: real starter-shape scripts over a large
//! deterministic bar burst, measured per commit run against the §5 budget
//! (typical script ≤ 50 µs; hard review fail at 200 µs).
//!
//! Dependency-free like the other benches; run with
//! `cargo bench -p quantick-pine`. Numbers go in PR descriptions.

use std::hint::black_box;
use std::time::Instant;

use quantick_indicators::{Ctx, Indicator, IndicatorBar};
use quantick_pine::{ScriptIndicator, compile};

const EMA_SCRIPT: &str =
    "//@version=5\nindicator(\"EMA\")\nlen = input.int(21, \"Length\")\nplot(ta.ema(close, len))\n";

const FLOW_SCRIPT: &str = "//@version=5\nindicator(\"flow\")\nfast = ta.ema(close, 9)\nslow = ta.ema(close, 21)\nup = ta.crossover(fast, slow)\nstrength = math.abs(delta) / math.max(volume, 1)\nscore = if up\n    strength * 2\nelse\n    strength\nplot(fast)\nplot(slow)\nplot(score)\nplot(cvd)\n";

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
    println!(
        "{name:12} {nodes:>4} AST nodes  {n:>7} bars in {elapsed:>9.2?}  =>  {ns_per:8.1} ns/bar",
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
}
