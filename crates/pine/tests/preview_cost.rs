//! Diagnostic, not a gate: measures what one live-preview tick costs — a
//! `rebind` (construct anew) plus a full-history replay of the copilot,
//! which is exactly what the settings dialog's slider drag asks of the
//! indicator worker per coalesced batch. `#[ignore]`d because wall-clock
//! numbers belong in a PR body, not in CI's pass/fail:
//!
//! ```sh
//! cargo test -p quantick-pine --release --test preview_cost -- --ignored --nocapture
//! ```

use std::time::Instant;

use quantick_indicators::{Ctx, Indicator, IndicatorBar, InputValue};
use quantick_pine::{ScriptIndicator, compile};

const COPILOT: &str = include_str!("corpus/ok/copilot.pine");
/// A realistic retained history: the chart's default backfill is 1,000
/// trades and dense sessions run a few thousand bars.
const BARS: usize = 2_000;
/// Replays averaged over, so one scheduler hiccup cannot write the number.
const REPLAYS: usize = 30;

#[test]
#[ignore = "diagnostic: prints per-replay cost for the live-preview budget"]
fn preview_tick_cost_on_two_thousand_bars() {
    let compiled = match compile(COPILOT, "copilot.pine") {
        Ok(c) => c,
        Err(errors) => panic!("copilot.pine must compile: {errors:?}"),
    };
    let base = ScriptIndicator::new(compiled, COPILOT.to_owned());
    let inputs: Vec<InputValue> = base.input_values();

    // A wavy tape so pivots and structures actually form and the scoring
    // pipeline does real work instead of short-circuiting on na.
    let mut bars = Vec::with_capacity(BARS);
    let mut cvd = Vec::with_capacity(BARS);
    let mut sum = 0.0;
    for i in 0..BARS {
        let mid = 100.0 + 5.0 * ((i as f64) * 0.35).sin();
        let buy = 3.0 + ((i as f64) * 0.7).sin();
        let sell = 3.0 + ((i as f64) * 0.7).cos();
        let ms = 1_700_000_000_000 + i as i64 * 1_000;
        bars.push(IndicatorBar {
            open_time: ms,
            close_time: ms + 999,
            open: mid,
            high: mid + 1.0,
            low: mid - 1.0,
            close: mid + 0.3,
            buy_volume: buy,
            sell_volume: sell,
            trade_count: 3.0,
        });
        sum += buy - sell;
        cvd.push(sum);
    }

    let start = Instant::now();
    for _ in 0..REPLAYS {
        let mut indicator = base
            .rebind(&inputs)
            .expect("input value set matches the script's input list");
        for (i, bar) in bars.iter().enumerate() {
            let mut ctx = Ctx {
                bar_index: i,
                cvd: &cvd,
            };
            indicator
                .on_close(bar, &mut ctx)
                .expect("commit run succeeds");
        }
    }
    let per_replay = start.elapsed() / REPLAYS as u32;
    println!(
        "preview tick (rebind + {BARS}-bar replay): {per_replay:?} averaged over {REPLAYS} replays"
    );
}
