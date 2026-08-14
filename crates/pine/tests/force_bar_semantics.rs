//! Semantic proof for the embedded `force_bar.pine`.
//!
//! Every classification is tested as a **pair**: one tape that must paint and
//! a near-identical one that must not. A test that only proves the colour
//! appears would pass on a script that paints every bar.
//!
//! The colours fed in are synthetic and all different (1,0,0 / 2,0,0 / …), so
//! each assertion also proves that *that particular* `input.color` reached the
//! paint channel — a script that wired bullish to the bearish slot would still
//! paint, and would still be wrong.

use quantick_indicators::{Ctx, Indicator, IndicatorBar, InputValue, Rgba8};
use quantick_pine::{ScriptIndicator, compile};

const FORCE_BAR: &str = include_str!("corpus/ok/force_bar.pine");

const FORCE_BULL: Rgba8 = Rgba8::opaque(1, 0, 0);
const FORCE_BEAR: Rgba8 = Rgba8::opaque(2, 0, 0);
const EXHAUSTION_BULL: Rgba8 = Rgba8::opaque(3, 0, 0);
const EXHAUSTION_BEAR: Rgba8 = Rgba8::opaque(4, 0, 0);
const BIGGEST_BULL: Rgba8 = Rgba8::opaque(5, 0, 0);
const BIGGEST_BEAR: Rgba8 = Rgba8::opaque(6, 0, 0);

/// Windows shrunk to 4 bars so a fixture stays readable; the rules are the
/// same at any length. Positional — the script's `input.*` order.
fn inputs() -> Vec<InputValue> {
    vec![
        InputValue::Bool(true),             // 0  1 Force: paint
        InputValue::Int(4),                 // 1  1 Force: window
        InputValue::Float(1.5),             // 2  1 Force: min ×average
        InputValue::Float(2.5),             // 3  1 Force: max ×average
        InputValue::Color(FORCE_BULL),      // 4
        InputValue::Color(FORCE_BEAR),      // 5
        InputValue::Bool(true),             // 6  2 Exhaustion: paint
        InputValue::Int(4),                 // 7  2 Exhaustion: window
        InputValue::Float(2.5),             // 8  2 Exhaustion: min ×average
        InputValue::Color(EXHAUSTION_BULL), // 9
        InputValue::Color(EXHAUSTION_BEAR), // 10
        InputValue::Bool(true),             // 11 3 Biggest: paint
        InputValue::Int(4),                 // 12 3 Biggest: window
        InputValue::Color(BIGGEST_BULL),    // 13
        InputValue::Color(BIGGEST_BEAR),    // 14
    ]
}

fn make_bar(index: usize, open: f64, close: f64, high: f64, low: f64) -> IndicatorBar {
    let t = 1_700_000_000_000 + index as i64 * 1_000;
    IndicatorBar {
        open_time: t,
        close_time: t + 999,
        open,
        high,
        low,
        close,
        buy_volume: 2.0,
        sell_volume: 1.0,
        trade_count: 3.0,
    }
}

/// Context bar: body 1.0, range strictly shrinking with the index.
///
/// The shrinking range is deliberate. With a flat range every bar ties the
/// window's widest, and `>=` would paint the whole tape biggest-yellow —
/// drowning the two tests that are actually about the body.
fn context(index: usize) -> IndicatorBar {
    let range = 10.0 - index as f64;
    let pad = (range - 1.0) / 2.0;
    make_bar(index, 100.0, 101.0, 101.0 + pad, 100.0 - pad)
}

/// The bar under test: exact body, exact range, chosen direction.
fn probe(index: usize, body: f64, range: f64, bullish: bool) -> IndicatorBar {
    let open = 100.0;
    let close = if bullish { open + body } else { open - body };
    let pad = (range - body) / 2.0;
    make_bar(
        index,
        open,
        close,
        open.max(close) + pad,
        open.min(close) - pad,
    )
}

/// Five context bars then the probe; returns the paint of every bar.
fn run(inputs: Vec<InputValue>, probe_bar: IndicatorBar) -> Vec<Option<Rgba8>> {
    let compiled = match compile(FORCE_BAR, "force_bar.pine") {
        Ok(c) => c,
        Err(errors) => panic!(
            "force_bar.pine must compile:\n{}",
            errors
                .iter()
                .map(|e| e.render("force_bar.pine", FORCE_BAR))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    let mut indicator = ScriptIndicator::with_inputs(compiled, FORCE_BAR, inputs);

    let mut cvd = Vec::new();
    let mut sum = 0.0;
    let mut bars: Vec<IndicatorBar> = (0..5).map(context).collect();
    bars.push(probe_bar);
    for (index, bar) in bars.iter().enumerate() {
        sum += bar.delta();
        cvd.push(sum);
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &cvd,
        };
        indicator.on_close(bar, &mut ctx).expect("commit run");
    }

    (0..bars.len())
        .map(|row| indicator.plots().bar_paint(row))
        .collect()
}

/// The probe sits at index 5.
fn probe_paint(inputs: Vec<InputValue>, probe_bar: IndicatorBar) -> Option<Rgba8> {
    run(inputs, probe_bar)[5]
}

#[test]
fn a_body_inside_the_band_is_force_and_an_ordinary_body_is_nothing() {
    // avg body over the last 4 = (1 + 1 + 1 + body)/4.
    // body 2.0 -> ratio 1.6, inside [1.5, 2.5].
    assert_eq!(
        probe_paint(inputs(), probe(5, 2.0, 5.0, true)),
        Some(FORCE_BULL),
        "2.0 body against a 1.0 average is force"
    );
    // body 1.0 -> ratio 1.0, below the band.
    assert_eq!(
        probe_paint(inputs(), probe(5, 1.0, 5.0, true)),
        None,
        "an average bar is not force, and an unpainted bar is the point"
    );
}

#[test]
fn exhaustion_takes_over_where_the_band_ends() {
    // Exhaustion measures against the bars BEFORE it: average 1.0 flat.
    assert_eq!(
        probe_paint(inputs(), probe(5, 3.0, 5.0, true)),
        Some(EXHAUSTION_BULL),
        "3.0 > 2.5 × the previous average"
    );
    // A hair under the factor: still force, never exhaustion.
    assert_eq!(
        probe_paint(inputs(), probe(5, 2.4, 5.0, true)),
        Some(FORCE_BULL),
        "2.4 is short of 2.5 — the pair that proves the threshold is real"
    );
}

#[test]
fn the_widest_bar_of_the_window_and_the_one_just_short_of_it() {
    // Window over bars 2..5: context ranges 8, 7, 6 plus the probe.
    assert_eq!(
        probe_paint(inputs(), probe(5, 1.0, 9.0, true)),
        Some(BIGGEST_BULL),
        "9 clears the window's 8"
    );
    assert_eq!(
        probe_paint(inputs(), probe(5, 1.0, 5.0, true)),
        None,
        "5 does not, and a near-widest bar wears nothing"
    );
}

#[test]
fn the_biggest_bar_outranks_exhaustion() {
    // Both fire: body 3.0 (exhaustion) inside a 9.0 range (biggest).
    assert_eq!(
        probe_paint(inputs(), probe(5, 3.0, 9.0, true)),
        Some(BIGGEST_BULL),
        "priority is biggest > exhaustion > force, and one bar wears one colour"
    );
}

#[test]
fn a_bearish_bar_wears_the_bearish_colour_of_its_class() {
    assert_eq!(
        probe_paint(inputs(), probe(5, 3.0, 5.0, false)),
        Some(EXHAUSTION_BEAR)
    );
    assert_eq!(
        probe_paint(inputs(), probe(5, 2.0, 5.0, false)),
        Some(FORCE_BEAR)
    );
    assert_eq!(
        probe_paint(inputs(), probe(5, 1.0, 9.0, false)),
        Some(BIGGEST_BEAR)
    );
}

#[test]
fn nothing_is_painted_before_the_rulers_have_history() {
    let paints = run(inputs(), probe(5, 2.0, 5.0, true));
    for (row, paint) in paints.iter().enumerate().take(5) {
        assert_eq!(
            *paint, None,
            "bar {row} is warm-up or an ordinary context bar; it must stay clean"
        );
    }
}

#[test]
fn a_toggle_silences_only_its_own_classification() {
    let mut off = inputs();
    off[6] = InputValue::Bool(false); // 2 Exhaustion: paint
    assert_eq!(
        probe_paint(off, probe(5, 3.0, 5.0, true)),
        Some(FORCE_BULL),
        "with exhaustion off the bar falls through to the next rule, it does \
         not go blank — the classification below it is still true"
    );

    let mut all_off = inputs();
    all_off[0] = InputValue::Bool(false);
    all_off[6] = InputValue::Bool(false);
    all_off[11] = InputValue::Bool(false);
    assert_eq!(
        probe_paint(all_off, probe(5, 3.0, 9.0, true)),
        None,
        "every toggle off paints nothing at all"
    );
}

#[test]
fn a_reversed_multiplier_pair_still_means_the_same_band() {
    // min 2.5 / max 1.5 typed the wrong way round: math.min/max put them back
    // in order instead of silently meaning "never".
    let mut swapped = inputs();
    swapped[2] = InputValue::Float(2.5);
    swapped[3] = InputValue::Float(1.5);
    assert_eq!(
        probe_paint(swapped, probe(5, 2.0, 5.0, true)),
        Some(FORCE_BULL)
    );
}
