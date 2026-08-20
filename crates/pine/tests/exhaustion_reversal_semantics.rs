//! Semantic proof for the embedded `exhaustion_reversal.pine`.
//!
//! Every clause of the signal is tested as a **pair**: a tape that must mark
//! and a near-identical one that must not. A test that only proves the
//! triangle appears would pass on a script that marks every bar, and the
//! whole value of this indicator is the bars it stays silent on.
//!
//! The tapes are written as raw `(open, high, low, close)` rows because those
//! four numbers are the entire input surface of this script — no volume, no
//! delta, no CVD. What each row is for is stated beside it.

use quantick_indicators::{Ctx, Indicator, IndicatorBar, InputValue, PlotId, Rgba8};
use quantick_pine::{ScriptIndicator, compile};

const SCRIPT: &str = include_str!("corpus/ok/exhaustion_reversal.pine");

const SELL_PLOT: &str = "Exhaustion reversal: sell";
const BUY_PLOT: &str = "Exhaustion reversal: buy";

const TOP_COLOUR: Rgba8 = Rgba8::opaque(1, 0, 0);
const BOTTOM_COLOUR: Rgba8 = Rgba8::opaque(2, 0, 0);

/// (open, high, low, close).
type Ohlc = (f64, f64, f64, f64);

/// Windows shrunk to 4 bars so a fixture stays readable; the rules are
/// identical at any length. Positional — the script's `input.*` order.
fn inputs() -> Vec<InputValue> {
    vec![
        InputValue::Int(4),               // 0  1 Force bar: body average window
        InputValue::Float(1.5),           // 1  1 Force bar: min body (×average)
        InputValue::Int(4),               // 2  1 Force bar: new extreme over (bars)
        InputValue::Bool(true),           // 3  1 Force bar: must close with the push
        InputValue::Bool(true),           // 4  2 Run: on
        InputValue::Int(3),               // 5  2 Run: opposite candles in a row
        InputValue::Int(5),               // 6  2 Run: window after the force bar
        InputValue::Float(0.7),           // 7  2 Run: min give-back
        InputValue::Bool(true),           // 8  3 Engulf: on
        InputValue::Int(3),               // 9  3 Engulf: window after the force bar
        InputValue::Float(0.7),           // 10 3 Engulf: min overlap
        InputValue::Bool(true),           // 11 4 Display: mark top reversals
        InputValue::Bool(true),           // 12 4 Display: mark bottom reversals
        InputValue::Color(TOP_COLOUR),    // 13
        InputValue::Color(BOTTOM_COLOUR), // 14
    ]
}

/// Input slots the tests reach for by name, so a reordering of the script's
/// inputs breaks compilation here instead of silently testing the wrong knob.
const NEED_DIRECTION: usize = 3;
const USE_RUN: usize = 4;
const RUN_WINDOW: usize = 6;
const USE_ENGULF: usize = 8;
const SHOW_TOP: usize = 11;

fn make(index: usize, row: Ohlc) -> IndicatorBar {
    let (open, high, low, close) = row;
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

/// Five flat bullish bars: body 1.0, range 1.0, every high identical. The
/// averages the force bar is judged against are therefore exactly 1.0, and
/// the extreme it must clear is exactly 101.0.
fn context_up() -> Vec<Ohlc> {
    vec![(100.0, 101.0, 100.0, 101.0); 5]
}

/// The mirror: flat bearish bars, extreme to clear exactly 100.0.
fn context_down() -> Vec<Ohlc> {
    vec![(101.0, 101.0, 100.0, 100.0); 5]
}

/// Body 10 against an average of 1.0, high 111 against an extreme of 101,
/// closing up: a buying push at a new high. Range 10, so a close at or below
/// 104.0 gives back the required 70%.
const FORCE_UP: Ohlc = (101.0, 111.0, 101.0, 111.0);

/// The canonical tape: force bar, then three bearish candles closing at 103
/// — 80% of the force bar handed back on the third.
fn sell_tape() -> Vec<Ohlc> {
    let mut t = context_up();
    t.push(FORCE_UP); //                      5  force bar
    t.push((111.0, 111.0, 109.0, 109.0)); //  6  give-back 1 of 3 (age 1)
    t.push((109.0, 109.0, 107.0, 107.0)); //  7  give-back 2 of 3 (age 2)
    t.push((107.0, 107.0, 103.0, 103.0)); //  8  give-back 3 of 3 -> 80%, marks
    t
}

fn run(inputs: Vec<InputValue>, rows: &[Ohlc]) -> ScriptIndicator {
    let compiled = match compile(SCRIPT, "exhaustion_reversal.pine") {
        Ok(c) => c,
        Err(errors) => panic!(
            "exhaustion_reversal.pine must compile:\n{}",
            errors
                .iter()
                .map(|e| e.render("exhaustion_reversal.pine", SCRIPT))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    let mut indicator = ScriptIndicator::with_inputs(compiled, SCRIPT, inputs);

    let mut cvd = Vec::new();
    let mut sum = 0.0;
    for (index, row) in rows.iter().enumerate() {
        let bar = make(index, *row);
        sum += bar.delta();
        cvd.push(sum);
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &cvd,
        };
        indicator.on_close(&bar, &mut ctx).expect("commit run");
    }
    indicator
}

/// The bar indices carrying a mark on the named plot.
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

fn sells(indicator: &ScriptIndicator) -> Vec<usize> {
    marker_rows(indicator, SELL_PLOT)
}

fn buys(indicator: &ScriptIndicator) -> Vec<usize> {
    marker_rows(indicator, BUY_PLOT)
}

fn none() -> Vec<usize> {
    Vec::new()
}

#[test]
fn three_bearish_candles_give_back_the_push_and_a_shallower_run_does_not() {
    let hit = run(inputs(), &sell_tape());
    assert_eq!(
        sells(&hit),
        vec![8],
        "the mark lands on the bar that closes the run, not at the extreme"
    );
    assert_eq!(buys(&hit), none(), "a faded buying push marks no long");

    // Same tape, third candle closing at 105: 60% given back, short of 70%.
    let mut shallow = sell_tape();
    shallow[8] = (107.0, 107.0, 105.0, 105.0);
    assert_eq!(
        sells(&run(inputs(), &shallow)),
        none(),
        "60% is not 70% — the threshold is real, not decorative"
    );
}

#[test]
fn two_pause_bars_still_count_and_three_pause_bars_are_too_late() {
    // The trader's second example: force bar, two bars of pause, then the
    // three-candle give-back landing exactly on the window's last bar.
    let mut paused = context_up();
    paused.push(FORCE_UP); //                      5  force bar
    paused.push((111.0, 111.6, 110.9, 111.5)); //  6  pause (body 0.5)
    paused.push((111.5, 112.1, 111.4, 112.0)); //  7  pause (body 0.5)
    paused.push((112.0, 112.0, 110.0, 110.0)); //  8  give-back 1 of 3
    paused.push((110.0, 110.0, 107.0, 107.0)); //  9  give-back 2 of 3
    paused.push((107.0, 107.0, 103.0, 103.0)); // 10  give-back 3 of 3, age 5
    assert_eq!(
        sells(&run(inputs(), &paused)),
        vec![10],
        "age 5 is the last bar the window covers, and it still marks"
    );

    // One more bar of pause pushes the same give-back to age 6.
    let mut late = context_up();
    late.push(FORCE_UP); //                      5  force bar
    late.push((111.0, 111.6, 110.9, 111.5)); //  6  pause
    late.push((111.5, 112.1, 111.4, 112.0)); //  7  pause
    late.push((112.0, 112.6, 111.9, 112.5)); //  8  pause
    late.push((112.5, 112.5, 110.0, 110.0)); //  9  give-back 1 of 3
    late.push((110.0, 110.0, 107.0, 107.0)); // 10  give-back 2 of 3
    late.push((107.0, 107.0, 103.0, 103.0)); // 11  give-back 3 of 3, age 6
    assert_eq!(
        sells(&run(inputs(), &late)),
        none(),
        "the same give-back one bar later is a micro range, not a reversal — \
         this pair is the entire reason the window exists"
    );
}

#[test]
fn the_run_must_be_consecutive() {
    // Four bearish closes, but a bullish bar in the middle: the run restarts,
    // and the last two bars are only two in a row.
    let mut broken = context_up();
    broken.push(FORCE_UP); //                      5  force bar
    broken.push((111.0, 111.0, 109.0, 109.0)); //  6  bearish
    broken.push((109.0, 109.0, 107.0, 107.0)); //  7  bearish
    broken.push((107.0, 108.5, 107.0, 108.0)); //  8  bullish — breaks the run
    broken.push((108.0, 108.0, 105.0, 105.0)); //  9  bearish (1 of a new run)
    broken.push((105.0, 105.0, 103.0, 103.0)); // 10  bearish (2), 80% given back
    assert_eq!(
        sells(&run(inputs(), &broken)),
        none(),
        "the push was given back, but not by a run — two in a row is not three"
    );
}

#[test]
fn an_ordinary_bar_at_a_new_high_is_not_a_force_bar() {
    let mut weak = context_up();
    weak.push((101.0, 102.0, 101.0, 102.0)); // 5  new high, body 1.0 = average
    weak.push((102.0, 102.0, 101.5, 101.5)); // 6
    weak.push((101.5, 101.5, 101.0, 101.0)); // 7
    weak.push((101.0, 101.0, 100.5, 100.5)); // 8  gives back far more than 70%
    assert_eq!(
        sells(&run(inputs(), &weak)),
        none(),
        "a breakout the tape did not have to work for is not exhaustion"
    );
}

#[test]
fn a_force_bar_that_takes_out_no_extreme_is_not_a_force_bar() {
    // Identical to the canonical tape except bar 3, which leaves a 120 high
    // standing above everything the force bar can reach.
    let mut buried = sell_tape();
    buried[3] = (100.0, 120.0, 100.0, 101.0); // tall wick, body still 1.0
    assert_eq!(
        sells(&run(inputs(), &buried)),
        none(),
        "a big body in the middle of a move is not a move ending"
    );
    // The pair: the same tape without that overhead high does mark.
    assert_eq!(sells(&run(inputs(), &sell_tape())), vec![8]);
}

#[test]
fn the_direction_rule_decides_whether_a_reversal_bar_can_anchor() {
    // Body 10, high 112 clears the extreme, but the bar closes DOWN: a
    // rejection bar, not a buying push.
    let mut wick = context_up();
    wick.push((111.0, 112.0, 101.0, 101.0)); // 5  big bearish bar at a new high
    wick.push((101.0, 101.0, 100.0, 100.0)); // 6  bearish
    wick.push((100.0, 100.0, 99.0, 99.0)); //   7  bearish
    wick.push((99.0, 99.0, 98.0, 98.0)); //     8  bearish, 127% given back

    assert_eq!(
        sells(&run(inputs(), &wick)),
        none(),
        "with the direction rule on, only a bar closing up can be a buying push"
    );

    let mut loose = inputs();
    loose[NEED_DIRECTION] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(loose, &wick)),
        vec![8],
        "with it off, any big bar at a new extreme anchors — the input is \
         wired to the clause it names"
    );
}

#[test]
fn the_buy_side_is_the_exact_mirror() {
    let mut t = context_down();
    t.push((100.0, 100.0, 90.0, 90.0)); //  5  selling push at a new low
    t.push((90.0, 92.0, 90.0, 92.0)); //    6  give-back 1 of 3
    t.push((92.0, 94.0, 92.0, 94.0)); //    7  give-back 2 of 3
    t.push((94.0, 98.0, 94.0, 98.0)); //    8  give-back 3 of 3 -> 80%

    let hit = run(inputs(), &t);
    assert_eq!(buys(&hit), vec![8]);
    assert_eq!(sells(&hit), none(), "a faded selling push marks no short");
}

#[test]
fn a_push_is_spent_by_its_own_signal() {
    // A fourth bearish candle, still inside the window, giving back even more.
    let mut long_run = sell_tape();
    long_run.push((103.0, 103.0, 101.0, 101.0)); // 9  age 4, run 4, 100% back
    assert_eq!(
        run(inputs(), &long_run)
            .plots()
            .column(PlotId::new(0))
            .len(),
        10,
        "the tape really is ten bars long"
    );
    assert_eq!(
        sells(&run(inputs(), &long_run)),
        vec![8],
        "one arrow per push: the same reversal must not be marked twice"
    );
}

#[test]
fn nothing_is_marked_while_the_averages_are_warming_up() {
    let warm = run(inputs(), &context_up());
    assert_eq!(sells(&warm), none());
    assert_eq!(buys(&warm), none());
}

#[test]
fn a_window_shorter_than_the_run_still_means_the_run() {
    // window 1 with a run of 3 would be "never" read literally. The script
    // lifts the window to the run instead of silently never firing.
    let mut impossible = inputs();
    impossible[RUN_WINDOW] = InputValue::Int(1);
    assert_eq!(
        sells(&run(impossible, &sell_tape())),
        vec![8],
        "an input that cannot fire is a trap, not a setting"
    );
}

#[test]
fn a_display_toggle_silences_its_own_side_and_nothing_else() {
    let mut top_off = inputs();
    top_off[SHOW_TOP] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(top_off, &sell_tape())),
        none(),
        "the sell side goes quiet"
    );

    let mut t = context_down();
    t.push((100.0, 100.0, 90.0, 90.0));
    t.push((90.0, 92.0, 90.0, 92.0));
    t.push((92.0, 94.0, 92.0, 94.0));
    t.push((94.0, 98.0, 94.0, 98.0));
    let mut top_off_again = inputs();
    top_off_again[SHOW_TOP] = InputValue::Bool(false);
    assert_eq!(
        buys(&run(top_off_again, &t)),
        vec![8],
        "…while the other side keeps marking: the toggle gates one draw site"
    );
}

#[test]
fn a_forming_bar_never_shows_the_mark_and_never_consumes_the_push() {
    let compiled = compile(SCRIPT, "exhaustion_reversal.pine").expect("compiles");
    let mut indicator = ScriptIndicator::with_inputs(compiled, SCRIPT, inputs());

    let rows = sell_tape();
    let mut cvd = Vec::new();
    let mut sum = 0.0;
    // Commit every bar up to, but not including, the one that signals.
    for (index, row) in rows.iter().enumerate().take(8) {
        let bar = make(index, *row);
        sum += bar.delta();
        cvd.push(sum);
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &cvd,
        };
        indicator.on_close(&bar, &mut ctx).expect("commit run");
    }

    // Now preview the signalling bar, twice, as a live tape would.
    let forming = make(8, rows[8]);
    for _ in 0..2 {
        cvd.push(sum + forming.delta());
        let mut ctx = Ctx {
            bar_index: 8,
            cvd: &cvd,
        };
        let frame = indicator.preview(&forming, &mut ctx).expect("preview run");
        cvd.pop();
        assert!(
            frame.values.iter().all(|v| v.is_nan()),
            "a forming bar carries no mark: {:?}",
            frame.values
        );
    }

    // The previews must not have spent the armed force bar.
    sum += forming.delta();
    cvd.push(sum);
    let mut ctx = Ctx {
        bar_index: 8,
        cvd: &cvd,
    };
    indicator.on_close(&forming, &mut ctx).expect("commit run");
    assert_eq!(
        sells(&indicator),
        vec![8],
        "the mark appears when the bar closes — previews rolled back cleanly"
    );
}

// ---------------------------------------------------------------------------
// The engulf: one opposite candle covering the force bar by itself.
//
// Measured as OVERLAP, not as give-back: the two shapes answer different
// questions, and these tapes are chosen so that each is invisible to the
// other. `engulf_tape` has no run (one candle is not three); `sell_tape` has
// no engulf (its deepest single body overlaps 40%). Anything that mixed the
// two measures up would light both tapes.
// ---------------------------------------------------------------------------

/// Force bar, then one bearish candle whose body covers 80% of its range.
fn engulf_tape() -> Vec<Ohlc> {
    let mut t = context_up();
    t.push(FORCE_UP); //                      5  force bar, range 101..111
    t.push((111.0, 111.0, 103.0, 103.0)); //  6  body 111->103 = 80% overlap
    t
}

#[test]
fn one_opposite_candle_covering_the_force_bar_marks_on_its_own() {
    assert_eq!(
        sells(&run(inputs(), &engulf_tape())),
        vec![6],
        "the engulf answers the push the bar after it, long before three \
         candles could line up"
    );

    let mut run_only = inputs();
    run_only[USE_ENGULF] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(run_only, &engulf_tape())),
        none(),
        "with the engulf switched off the same tape goes quiet: the checkbox \
         is wired to the shape it names"
    );
}

#[test]
fn an_engulf_short_of_the_overlap_threshold_does_not_mark() {
    let mut shallow = engulf_tape();
    shallow[6] = (111.0, 111.0, 105.0, 105.0); // covers 60%, not 70%
    assert_eq!(
        sells(&run(inputs(), &shallow)),
        none(),
        "60% of the force bar is not enough, and nothing else on this tape \
         can mark it"
    );
}

#[test]
fn the_engulf_has_its_own_shorter_window() {
    // Two pause bars: the covering candle lands at age 3, the last bar the
    // engulf window covers.
    let mut in_time = context_up();
    in_time.push(FORCE_UP); //                      5  force bar
    in_time.push((111.0, 111.6, 110.9, 111.5)); //  6  pause
    in_time.push((111.5, 112.1, 111.4, 112.0)); //  7  pause
    in_time.push((112.0, 112.0, 103.0, 103.0)); //  8  covers 80%, age 3
    assert_eq!(sells(&run(inputs(), &in_time)), vec![8]);

    // One more pause and the identical candle is a bar too late — even though
    // the run's own window (5) would still have the force bar armed.
    let mut late = context_up();
    late.push(FORCE_UP); //                      5  force bar
    late.push((111.0, 111.6, 110.9, 111.5)); //  6  pause
    late.push((111.5, 112.1, 111.4, 112.0)); //  7  pause
    late.push((112.0, 112.6, 111.9, 112.5)); //  8  pause
    late.push((112.5, 112.5, 103.0, 103.0)); //  9  covers 80%, age 4
    assert_eq!(
        sells(&run(inputs(), &late)),
        none(),
        "a bar reacting four bars later is reacting to something else"
    );
}

#[test]
fn a_candle_closing_with_the_push_never_engulfs_it() {
    let mut wrong_way = context_up();
    wrong_way.push(FORCE_UP); //                     5  force bar
    wrong_way.push((103.0, 111.5, 102.5, 111.0)); // 6  wide, but BULLISH
    assert_eq!(
        sells(&run(inputs(), &wrong_way)),
        none(),
        "a body spanning the force bar in the push's own direction is the \
         push continuing, not a reversal of it"
    );
}

#[test]
fn the_engulf_marks_the_buy_side_too() {
    let mut t = context_down();
    t.push((100.0, 100.0, 90.0, 90.0)); // 5  selling push at a new low
    t.push((90.0, 98.0, 90.0, 98.0)); //   6  bullish body covering 80%
    let hit = run(inputs(), &t);
    assert_eq!(buys(&hit), vec![6]);
    assert_eq!(sells(&hit), none());
}

#[test]
fn the_two_shapes_share_one_arrow_per_push() {
    // The engulf fires at bar 6; bars 7 and 8 then complete a three-candle
    // run that gives back 100% and would fire on its own.
    let mut both = engulf_tape();
    both.push((103.0, 103.0, 102.0, 102.0)); // 7  bearish
    both.push((102.0, 102.0, 101.0, 101.0)); // 8  bearish — run of 3, 100% back
    assert_eq!(
        sells(&run(inputs(), &both)),
        vec![6],
        "the push was spent by the engulf; the run must not mark it again"
    );
}

#[test]
fn switching_the_run_off_leaves_the_engulf_alone_and_vice_versa() {
    let mut engulf_only = inputs();
    engulf_only[USE_RUN] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(engulf_only, &sell_tape())),
        none(),
        "the canonical run tape has no single candle covering 70% — its \
         deepest body overlaps 40%"
    );

    let mut engulf_only_again = inputs();
    engulf_only_again[USE_RUN] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(engulf_only_again, &engulf_tape())),
        vec![6],
        "…while the engulf tape still marks with the run switched off"
    );

    let mut neither = inputs();
    neither[USE_RUN] = InputValue::Bool(false);
    neither[USE_ENGULF] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(neither, &engulf_tape())),
        none(),
        "both shapes off is an indicator that marks nothing at all"
    );
}
