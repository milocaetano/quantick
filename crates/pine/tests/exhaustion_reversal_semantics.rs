//! Semantic proof for the embedded `exhaustion_reversal.pine`.
//!
//! Every clause of the signal is tested as a **pair**: a tape that must mark
//! and a near-identical one that must not. A test that only proves the
//! triangle appears would pass on a script that marks every bar, and the
//! whole value of this indicator is the bars it stays silent on.
//!
//! One test does not read rows at all: `the_declared_marks_are_the_ones_the_
//! renderer_draws` pins shape, location and colour. Those three fold at load
//! time with silent fallbacks, so a swapped `location.abovebar` — the sell
//! triangle drawn under the low — is invisible to every row assertion here.
//!
//! The tapes are written as raw `(open, high, low, close)` rows because those
//! four numbers are the entire input surface of this script — no volume, no
//! delta, no CVD. What each row is for is stated beside it.

use quantick_indicators::{
    Ctx, Indicator, IndicatorBar, InputSpec, InputValue, MarkerLocation, MarkerShape, PlotId, Rgba8,
};
use quantick_pine::{ScriptIndicator, compile};

const SCRIPT: &str = include_str!("corpus/ok/exhaustion_reversal.pine");

const SELL_PLOT: &str = "Exhaustion reversal: sell";
const BUY_PLOT: &str = "Exhaustion reversal: buy";

/// (open, high, low, close).
type Ohlc = (f64, f64, f64, f64);

/// Windows shrunk to 4 bars so a fixture stays readable; the rules are
/// identical at any length. Positional — the script's `input.*` order.
fn inputs() -> Vec<InputValue> {
    vec![
        InputValue::Int(4),      // 0  1 Force bar: body average window
        InputValue::Float(1.5),  // 1  1 Force bar: min body (×average)
        InputValue::Int(4),      // 2  1 Force bar: reaches the extreme of
        InputValue::Bool(true),  // 3  1 Force bar: must close with the push
        InputValue::Bool(true),  // 4  2 Run: on
        InputValue::Int(3),      // 5  2 Run: opposite candles in a row
        InputValue::Int(5),      // 6  2 Run: window after the force bar
        InputValue::Float(0.7),  // 7  2 Run: min give-back
        InputValue::Bool(true),  // 8  3 Cover: on
        InputValue::Int(3),      // 9  3 Cover: window after the force bar
        InputValue::Float(0.7),  // 10 3 Cover: min overlap
        InputValue::Bool(true),  // 11 4 Display: mark top reversals
        InputValue::Bool(true),  // 12 4 Display: mark bottom reversals
        InputValue::Float(0.0),  // 13 5 Calibration: points floor, off
        InputValue::Bool(false), // 14 5 Calibration: run reads the body
        InputValue::Bool(false), // 15 5 Calibration: cover reads the body
        InputValue::Bool(false), // 16 5 Calibration: paint what armed
    ]
}

// Positions into the hand-ordered `Vec` above. Naming them keeps a test from
// reading as `off[10] = false` — but note this is legibility only: the bind
// path checks the input *count*, never a name, so reordering two same-typed
// inputs would leave these pointing at the wrong knob with the suite green.
// There is no by-name input API to reach for; the behavioural assertions are
// what actually catch a reorder.
const NEED_DIRECTION: usize = 3;
const USE_RUN: usize = 4;
const RUN_WINDOW: usize = 6;
const USE_COVER: usize = 8;
const SHOW_TOP: usize = 11;
const MIN_BODY_POINTS: usize = 13;
const RUN_BODY_ONLY: usize = 14;
const COVER_BODY_ONLY: usize = 15;
const PAINT_FORCE: usize = 16;

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
/// the extreme it must reach is exactly 101.0.
fn context_up() -> Vec<Ohlc> {
    vec![(100.0, 101.0, 100.0, 101.0); 5]
}

/// The mirror: flat bearish bars, extreme to reach exactly 100.0.
fn context_down() -> Vec<Ohlc> {
    vec![(101.0, 101.0, 100.0, 100.0); 5]
}

/// Body 10 against an average of 1.0, high 111 against an extreme of 101,
/// closing up: a buying push at a new high. Range 10, so a close at or below
/// 104.0 gives back the required 70%.
const FORCE_UP: Ohlc = (101.0, 111.0, 101.0, 111.0);

/// The mirror: a selling push at a new low, range 90..100.
const FORCE_DOWN: Ohlc = (100.0, 100.0, 90.0, 90.0);

/// The canonical run tape: force bar, then three bearish candles closing at
/// 103 — 80% of the force bar handed back on the third. Its deepest single
/// body overlaps only 40%, so the cover shape cannot see this tape.
fn sell_tape() -> Vec<Ohlc> {
    let mut t = context_up();
    t.push(FORCE_UP); //                      5  force bar
    t.push((111.0, 111.0, 109.0, 109.0)); //  6  give-back 1 of 3 (age 1)
    t.push((109.0, 109.0, 107.0, 107.0)); //  7  give-back 2 of 3 (age 2)
    t.push((107.0, 107.0, 103.0, 103.0)); //  8  give-back 3 of 3 -> 80%, marks
    t
}

/// The mirror of `sell_tape`.
fn buy_tape() -> Vec<Ohlc> {
    let mut t = context_down();
    t.push(FORCE_DOWN); //                5  selling push at a new low
    t.push((90.0, 92.0, 90.0, 92.0)); //  6  give-back 1 of 3
    t.push((92.0, 94.0, 92.0, 94.0)); //  7  give-back 2 of 3
    t.push((94.0, 98.0, 94.0, 98.0)); //  8  give-back 3 of 3 -> 80%, marks
    t
}

/// Force bar, then one bearish candle whose body covers 80% of its range —
/// the shape the run cannot see, because one candle is not three.
fn cover_tape() -> Vec<Ohlc> {
    let mut t = context_up();
    t.push(FORCE_UP); //                      5  force bar, range 101..111
    t.push((111.0, 111.0, 103.0, 103.0)); //  6  body 111->103 = 80% overlap
    t
}

fn load() -> ScriptIndicator {
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
    ScriptIndicator::new(compiled, SCRIPT)
}

/// Borrowed inputs on purpose: a test that flips two switches would otherwise
/// have to build the vector twice, and the second copy silently drifts.
fn run(inputs: &[InputValue], rows: &[Ohlc]) -> ScriptIndicator {
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
    let mut indicator = ScriptIndicator::with_inputs(compiled, SCRIPT, inputs.to_vec());

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

/// `color.orange` and `color.blue` as this dialect folds them: the two paints
/// the section-1 diagnostic uses. Constants in the script, constants here —
/// asserting on the exact colour is what catches the two sides being wired to
/// the same one, which is a bug the arrows already shipped once.
const PAINT_TOP: Rgba8 = Rgba8::opaque(0xFF, 0x98, 0x00);
const PAINT_BOTTOM: Rgba8 = Rgba8::opaque(0x29, 0x62, 0xFF);

/// The bar paint of each row, `None` where the script asked for none. The row
/// count is passed in rather than read off the channel: the channel is kept
/// only as far as the last painted bar, so its length would silently shorten
/// the assertion to exactly the rows that happen to be painted.
fn paints(indicator: &ScriptIndicator, rows: usize) -> Vec<Option<Rgba8>> {
    (0..rows)
        .map(|row| indicator.plots().bar_paint(row))
        .collect()
}

#[test]
fn the_declared_marks_are_the_ones_the_renderer_draws() {
    // Shape, location and colour all fold at load time and all fall back
    // SILENTLY when they do not fold — `plotshape` has no unfoldable-argument
    // warning. So every row assertion in this file would survive a sell
    // triangle pointing up, drawn under the bar, in the wrong colour.
    let indicator = load();
    let plots = &indicator.descriptor().plots;
    assert_eq!(plots.len(), 2, "one plot per side, and nothing else");

    let sell = plots
        .iter()
        .find(|p| p.title == SELL_PLOT)
        .expect("sell plot");
    let sell_marker = sell.marker.as_ref().expect("the sell plot is a marker");
    assert_eq!(sell_marker.shape, MarkerShape::TriangleDown);
    assert_eq!(
        sell_marker.location,
        MarkerLocation::AboveBar,
        "a sell mark sits above the bar; below it would point at the wrong \
         price and cover the candle it is about"
    );
    assert_eq!(
        sell.base_color,
        Rgba8::new(0xF2, 0x36, 0x45, 0xFF),
        "color.red — an unfoldable colour argument lands as amber instead, \
         which is how both sides once shipped the same colour"
    );

    let buy = plots
        .iter()
        .find(|p| p.title == BUY_PLOT)
        .expect("buy plot");
    let buy_marker = buy.marker.as_ref().expect("the buy plot is a marker");
    assert_eq!(buy_marker.shape, MarkerShape::TriangleUp);
    assert_eq!(buy_marker.location, MarkerLocation::BelowBar);
    assert_eq!(
        buy.base_color,
        Rgba8::new(0x00, 0x89, 0x7B, 0xFF),
        "color.teal, and distinct from the sell colour"
    );
}

/// The thirteen inputs the indicator shipped with in #212, in order. This is
/// not documentation — it is the binding contract for every preset a trader
/// has already saved. Titles rather than the derived `name()`, because they
/// are what a reader can check against the script in one glance; `name()` is
/// a slug of the title, so pinning one pins the other.
const SHIPPED_ORDER: [&str; 13] = [
    "1 Force bar: body average window (bars)",
    "1 Force bar: min body (×average)",
    "1 Force bar: reaches the extreme of (bars)",
    "1 Force bar: must close in the direction of the push",
    "2 Run: mark a give-back by consecutive candles",
    "2 Run: opposite candles in a row",
    "2 Run: window after the force bar (bars)",
    "2 Run: min give-back of the force bar",
    "3 Cover: mark a give-back by one opposite candle",
    "3 Cover: window after the force bar (bars)",
    "3 Cover: min overlap with the force bar",
    "4 Display: mark top reversals (sell side)",
    "4 Display: mark bottom reversals (buy side)",
];

#[test]
fn the_inputs_this_script_shipped_with_keep_their_positions() {
    // A saved preset is a list of values matched to inputs BY POSITION, with
    // only a type check to object (`App::load_indicator_preset`): the loader
    // walks the declared inputs and takes the saved value at the same index
    // when the discriminant matches. Insert an input in the middle and every
    // preset saved before it shifts one place — silently, because a `bool`
    // matches a `bool`. That is not hypothetical: dropping "paint the bars
    // that arm a push" at index 4 would have handed it the value saved for
    // "2 Run: on" — `true` by default — and a trader's candles would have
    // started repainting themselves from a preset written before the switch
    // existed. So new inputs go at the END, in section 5, and this test is
    // what says so out loud.
    //
    // Two titles here lost "(fraction of range)" in this same change, because
    // the fraction can now be read against the body instead; that is a
    // rename, not a reorder, and the loader never reads a title. The test
    // still catches it, deliberately: `InputSpec::name()` is documented as a
    // persistence key, and the day something starts persisting by it, a
    // rename becomes a silent data loss.
    let indicator = load();
    let declared: Vec<&str> = indicator
        .descriptor()
        .inputs
        .iter()
        .map(|spec| spec.title())
        .collect();
    assert!(
        declared.len() >= SHIPPED_ORDER.len(),
        "an input was removed: presets bind by position, so removing one shifts every value saved after it"
    );
    assert_eq!(
        &declared[..SHIPPED_ORDER.len()],
        &SHIPPED_ORDER[..],
        "the inputs this script shipped with must keep their indices —          append new ones instead of inserting them"
    );
}

#[test]
fn no_declared_input_is_a_control_that_does_nothing() {
    // `input.color` does not fold in this dialect, so a colour handed to a
    // `plotshape` is dropped for the default amber — silently, because
    // `plotshape` has no unfoldable-argument warning. The settings dialog
    // still renders the picker, so the trader gets a control that moves
    // nothing, which the repo treats as worse than an absent one. This
    // script therefore declares no colour input at all and takes its colours
    // from the Style tab; the assertion is here so nobody adds one back.
    //
    // `barcolor` is the one site an `input.color` *would* fold through
    // (force_bar.pine proves it), so this is a ban on the picker, not on
    // paint: the diagnostic below paints with folded constants instead.
    let indicator = load();
    let inputs = &indicator.descriptor().inputs;
    assert_eq!(
        inputs.len(),
        17,
        "four force-bar knobs, four run, three cover, two display, four calibration"
    );
    for spec in inputs {
        assert!(
            !matches!(spec, InputSpec::Color { .. }),
            "colour inputs cannot reach a plot in this dialect: {spec:?}"
        );
    }
}

#[test]
fn three_bearish_candles_give_back_the_push_and_a_shallower_run_does_not() {
    let hit = run(&inputs(), &sell_tape());
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
        sells(&run(&inputs(), &shallow)),
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
        sells(&run(&inputs(), &paused)),
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
        sells(&run(&inputs(), &late)),
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
        sells(&run(&inputs(), &broken)),
        none(),
        "the push was given back, but not by a run — two in a row is not three"
    );
}

#[test]
fn the_streak_and_the_give_back_are_independent_tests() {
    // Documented behaviour, pinned so nobody "fixes" it by accident: the run
    // asks (a) did the last three candles close against the push, and (b) is
    // price now back off the extreme. It does NOT ask that the three candles
    // did the travelling. Here a single bullish gap hands back 80% and the
    // three bearish candles that follow move 1% between them.
    let mut cosmetic = context_up();
    cosmetic.push(FORCE_UP); //                        5  force bar, range 10
    cosmetic.push((102.0, 103.2, 101.8, 103.0)); //    6  bullish gap — gives back 80%
    cosmetic.push((103.0, 103.0, 102.9, 102.9)); //    7  bearish by 0.1
    cosmetic.push((102.9, 102.9, 102.8, 102.8)); //    8  bearish by 0.1
    cosmetic.push((102.8, 102.8, 102.7, 102.7)); //    9  bearish by 0.1 -> marks
    assert_eq!(
        sells(&run(&inputs(), &cosmetic)),
        vec![9],
        "price is back and the tape is still leaning down, which is what the \
         two conditions actually say — the header says so too"
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
        sells(&run(&inputs(), &weak)),
        none(),
        "a breakout the tape did not have to work for is not exhaustion"
    );
}

#[test]
fn a_ratio_without_a_size_is_not_an_elephant() {
    // The force bar's body is 10 points against an average of 1.0, so the
    // ratio says force at any factor this fixture could carry. The floor is
    // the other half of the question, and it is the half a quiet tape needs:
    // a collapsed average makes 1.5× of almost nothing arm a push nobody
    // would call one (measured on WINV26: 247 of 1 355 bars by ratio alone,
    // 7 with a 100-point floor — docs/ux/strategy-anchors.md).
    let mut floored = inputs();
    floored[MIN_BODY_POINTS] = InputValue::Float(11.0);
    assert_eq!(
        sells(&run(&floored, &sell_tape())),
        none(),
        "a 10-point body under an 11-point floor never arms, so nothing the bars after it do can mark"
    );

    // And the floor is a floor, not a filter on the mark: one point lower and
    // the identical tape marks where it always did.
    let mut cleared = inputs();
    cleared[MIN_BODY_POINTS] = InputValue::Float(10.0);
    assert_eq!(
        sells(&run(&cleared, &sell_tape())),
        vec![8],
        "the floor is `>=`: a body exactly at it still counts"
    );

    // Off is off — the shipped default changes nothing about the old ruler.
    assert_eq!(sells(&run(&inputs(), &sell_tape())), vec![8]);
}

#[test]
fn the_floor_and_the_paint_agree_about_what_armed() {
    // The diagnostic is only useful if it reports the same decision the
    // signal path made. A floor that silenced the arrow while leaving the bar
    // painted would send the trader hunting a bug that is not there.
    let mut floored = inputs();
    floored[MIN_BODY_POINTS] = InputValue::Float(11.0);
    floored[PAINT_FORCE] = InputValue::Bool(true);
    let tape = sell_tape();
    assert_eq!(
        paints(&run(&floored, &tape), tape.len()),
        vec![None; tape.len()],
        "no bar armed, so no bar is painted"
    );
}

#[test]
fn a_force_bar_that_reaches_no_extreme_is_not_a_force_bar() {
    // Identical to the canonical tape except bar 3, which leaves a 120 high
    // standing above everything the force bar can reach.
    let mut buried = sell_tape();
    buried[3] = (100.0, 120.0, 100.0, 101.0); // tall wick, body still 1.0
    assert_eq!(
        sells(&run(&inputs(), &buried)),
        none(),
        "a big body in the middle of a move is not a move ending"
    );
    // The pair: the same tape without that overhead high does mark.
    assert_eq!(sells(&run(&inputs(), &sell_tape())), vec![8]);
}

#[test]
fn touching_the_extreme_is_enough_to_anchor() {
    // Equalling the last N highs anchors: on futures the offer parks at a
    // price and highs tie constantly, and a push that shoves into a level the
    // tape touched N bars ago is the same event as one that clears it.
    let mut tie = context_up();
    // A bullish body of 10 whose high is exactly the window's high of 101 —
    // it ties the extreme rather than clearing it.
    tie.push((91.0, 101.0, 91.0, 101.0)); //  5  force bar, high ties 101
    tie.push((101.0, 101.0, 99.0, 99.0)); //  6  give-back 1 of 3
    tie.push((99.0, 99.0, 97.0, 97.0)); //    7  give-back 2 of 3
    tie.push((97.0, 97.0, 93.0, 93.0)); //    8  give-back 3 of 3 -> 80%
    assert_eq!(
        sells(&run(&inputs(), &tie)),
        vec![8],
        "a tied high still anchors; the body test is what keeps a flat, quiet \
         stretch of tied highs from ever getting here"
    );
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
        sells(&run(&inputs(), &wick)),
        none(),
        "with the direction rule on, only a bar closing up can be a buying push"
    );

    let mut loose = inputs();
    loose[NEED_DIRECTION] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(&loose, &wick)),
        vec![8],
        "with it off, any big bar at a new extreme anchors — the input is \
         wired to the clause it names"
    );
}

#[test]
fn the_buy_side_is_the_exact_mirror() {
    let hit = run(&inputs(), &buy_tape());
    assert_eq!(buys(&hit), vec![8]);
    assert_eq!(sells(&hit), none(), "a faded selling push marks no short");
}

#[test]
fn a_push_is_spent_by_its_own_signal() {
    // A fourth bearish candle, still inside the window, giving back even more.
    let mut long_run = sell_tape();
    long_run.push((103.0, 103.0, 101.0, 101.0)); // 9  age 4, run 4, 100% back
    assert_eq!(long_run.len(), 10, "the tape really is ten bars long");
    assert_eq!(
        sells(&run(&inputs(), &long_run)),
        vec![8],
        "one arrow per push: the same reversal must not be marked twice"
    );
}

#[test]
fn the_warm_up_guards_keep_the_opening_stretch_clean() {
    // `ta.sma(body[1], 4)` carries a NaN until bar 4, so a textbook shape one
    // bar earlier must not mark. The two tapes are the same shape, one bar
    // apart — without the `not na` guards the first would mark too.
    let mut too_early = vec![(100.0, 101.0, 100.0, 101.0); 3];
    too_early.push(FORCE_UP); //                      3  force shape, still warming
    too_early.push((111.0, 111.0, 103.0, 103.0)); //  4  would cover 80%
    assert_eq!(
        sells(&run(&inputs(), &too_early)),
        none(),
        "the body average does not exist yet, so nothing can beat it"
    );

    let mut warm = vec![(100.0, 101.0, 100.0, 101.0); 4];
    warm.push(FORCE_UP); //                      4  the same force shape, warm
    warm.push((111.0, 111.0, 103.0, 103.0)); //  5  covers 80% -> marks
    assert_eq!(
        sells(&run(&inputs(), &warm)),
        vec![5],
        "one bar later the average exists and the identical shape marks"
    );
}

#[test]
fn a_window_shorter_than_the_run_still_means_the_run() {
    // window 1 with a run of 3 would be "never" read literally. The script
    // lifts the window to the run instead of silently never firing.
    let mut impossible = inputs();
    impossible[RUN_WINDOW] = InputValue::Int(1);
    assert_eq!(
        sells(&run(&impossible, &sell_tape())),
        vec![8],
        "an input that cannot fire is a trap, not a setting"
    );
}

#[test]
fn the_run_can_measure_give_back_against_the_force_bars_body_instead_of_its_range() {
    // A force bar with wicks on both sides: body 105..111 (width 6), full
    // range 101..115 (width 14, a 4-wide wick on each end). The three-candle
    // run below closes at 106 — 64% of the RANGE given back (short of 70%)
    // but 83% of the BODY (past it).
    let mut wicked = context_up();
    wicked.push((105.0, 115.0, 101.0, 111.0)); // 5  force bar, body 105..111
    wicked.push((111.0, 111.0, 109.0, 109.0)); // 6  give-back 1 of 3
    wicked.push((109.0, 109.0, 107.0, 107.0)); // 7  give-back 2 of 3
    wicked.push((107.0, 107.0, 106.0, 106.0)); // 8  give-back 3 of 3

    assert_eq!(
        sells(&run(&inputs(), &wicked)),
        none(),
        "measured against the full range this run gives back only 64% of \
         it — short of the 70% threshold"
    );

    let mut body_only = inputs();
    body_only[RUN_BODY_ONLY] = InputValue::Bool(true);
    assert_eq!(
        sells(&run(&body_only, &wicked)),
        vec![8],
        "the identical tape marks once the switch says to ignore the wicks \
         and judge the give-back against the body alone"
    );
}

#[test]
fn the_force_bar_diagnostic_paints_the_push_and_leaves_every_other_bar_alone() {
    // Silent unless asked. An indicator that painted candles the moment it
    // was added would repaint the chart of everyone who already runs it.
    let tape = sell_tape();
    assert_eq!(
        paints(&run(&inputs(), &tape), tape.len()),
        vec![None; tape.len()],
        "with the switch off the script asks for no paint at all"
    );

    let mut on = inputs();
    on[PAINT_FORCE] = InputValue::Bool(true);
    let painted = paints(&run(&on, &tape), tape.len());
    assert_eq!(
        painted[5],
        Some(PAINT_TOP),
        "bar 5 is the force bar — the whole point of the switch is that it becomes visible whether or not anything fades it"
    );
    assert_eq!(
        painted[8], None,
        "bar 8 carries the arrow, and the give-back is not a push: painting it would answer a question nobody asked"
    );
    assert_eq!(
        painted.iter().filter(|p| p.is_some()).count(),
        1,
        "one push in this tape, one painted bar"
    );

    // The switch is a diagnostic, not a filter: the arrow is unchanged.
    assert_eq!(sells(&run(&on, &tape)), vec![8]);
}

#[test]
fn the_two_sides_of_the_diagnostic_wear_different_colours() {
    // A single colour for both would make the paint useless exactly where a
    // trader needs it — a bar painted at the bottom of a slide is either the
    // selling push it should be, or a buying push read upside down.
    let mut on = inputs();
    on[PAINT_FORCE] = InputValue::Bool(true);

    let tape = buy_tape();
    let painted = paints(&run(&on, &tape), tape.len());
    assert_eq!(
        painted[5],
        Some(PAINT_BOTTOM),
        "the selling push at a new low is the other colour"
    );
    assert_ne!(PAINT_TOP, PAINT_BOTTOM);
    assert_eq!(painted.iter().filter(|p| p.is_some()).count(), 1);
}

#[test]
fn a_display_toggle_silences_its_own_side_and_nothing_else() {
    let mut top_off = inputs();
    top_off[SHOW_TOP] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(&top_off, &sell_tape())),
        none(),
        "the sell side goes quiet"
    );
    assert_eq!(
        buys(&run(&top_off, &buy_tape())),
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
// The cover: one opposite candle sitting on top of the force bar.
//
// Measured as OVERLAP, not as travel: the two shapes answer different
// questions, and these tapes are chosen so each is invisible to the other.
// `cover_tape` has no run (one candle is not three); `sell_tape` has no cover
// (its deepest single body overlaps 40%). Anything that mixed the two
// measures up would light both tapes.
// ---------------------------------------------------------------------------

#[test]
fn one_opposite_candle_covering_the_force_bar_marks_on_its_own() {
    assert_eq!(
        sells(&run(&inputs(), &cover_tape())),
        vec![6],
        "the cover answers the push the bar after it, long before three \
         candles could line up"
    );

    let mut run_only = inputs();
    run_only[USE_COVER] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(&run_only, &cover_tape())),
        none(),
        "with the cover switched off the same tape goes quiet: the checkbox \
         is wired to the shape it names"
    );
}

#[test]
fn a_cover_short_of_the_overlap_threshold_does_not_mark() {
    let mut shallow = cover_tape();
    shallow[6] = (111.0, 111.0, 105.0, 105.0); // covers 60%, not 70%
    assert_eq!(
        sells(&run(&inputs(), &shallow)),
        none(),
        "60% of the force bar is not enough, and nothing else on this tape \
         can mark it"
    );
}

#[test]
fn a_candle_inside_the_force_bar_still_covers_it() {
    // High below the force bar's high, low above its low — the opposite of a
    // classical engulfing candle, and it marks, because its body still erases
    // 80% of the push. Pinned because the name "cover" was chosen over
    // "engulf" precisely to stop this reading as a bug.
    let mut inside = context_up();
    inside.push(FORCE_UP); //                        5  force bar, range 101..111
    inside.push((110.0, 110.5, 101.5, 102.0)); //    6  strictly inside, covers 80%
    assert_eq!(
        sells(&run(&inputs(), &inside)),
        vec![6],
        "a small bar that erases most of a large one is the exhaustion this \
         is looking for, whether or not it pokes out the ends"
    );
}

#[test]
fn the_cover_has_its_own_shorter_window() {
    // Two pause bars: the covering candle lands at age 3, the last bar the
    // cover window covers.
    let mut in_time = context_up();
    in_time.push(FORCE_UP); //                      5  force bar
    in_time.push((111.0, 111.6, 110.9, 111.5)); //  6  pause
    in_time.push((111.5, 112.1, 111.4, 112.0)); //  7  pause
    in_time.push((112.0, 112.0, 103.0, 103.0)); //  8  covers 80%, age 3
    assert_eq!(sells(&run(&inputs(), &in_time)), vec![8]);

    // One more pause and the identical candle is a bar too late — even though
    // the run's own window (5) would still have the force bar armed.
    let mut late = context_up();
    late.push(FORCE_UP); //                      5  force bar
    late.push((111.0, 111.6, 110.9, 111.5)); //  6  pause
    late.push((111.5, 112.1, 111.4, 112.0)); //  7  pause
    late.push((112.0, 112.6, 111.9, 112.5)); //  8  pause
    late.push((112.5, 112.5, 103.0, 103.0)); //  9  covers 80%, age 4
    assert_eq!(
        sells(&run(&inputs(), &late)),
        none(),
        "a bar reacting four bars later is reacting to something else"
    );
}

#[test]
fn a_candle_closing_with_the_push_never_covers_it() {
    let mut wrong_way = context_up();
    wrong_way.push(FORCE_UP); //                     5  force bar
    wrong_way.push((103.0, 111.5, 102.5, 111.0)); // 6  wide, but BULLISH
    assert_eq!(
        sells(&run(&inputs(), &wrong_way)),
        none(),
        "a body spanning the force bar in the push's own direction is the \
         push continuing, not a reversal of it"
    );
}

#[test]
fn the_cover_marks_the_buy_side_too() {
    let mut t = context_down();
    t.push(FORCE_DOWN); //                5  selling push at a new low
    t.push((90.0, 98.0, 90.0, 98.0)); //  6  bullish body covering 80%
    let hit = run(&inputs(), &t);
    assert_eq!(buys(&hit), vec![6]);
    assert_eq!(sells(&hit), none());
}

#[test]
fn the_cover_can_measure_overlap_against_the_force_bars_body_instead_of_its_range() {
    // A force bar with a wick above its body: body 101..111 (width 10), full
    // range 101..115 (width 14, a 4-wide upper wick). The next candle's body
    // overlaps the force bar's body by exactly 70%, but only 50% of its full
    // range — the trader's exact complaint: a candle that visibly erases the
    // body still fails a threshold judged against the whole wick-to-wick range.
    let mut wicked = context_up();
    wicked.push((101.0, 115.0, 101.0, 111.0)); // 5  force bar, body 101..111
    wicked.push((111.0, 111.0, 104.0, 104.0)); // 6  body 111->104: 50% of range, 70% of body

    assert_eq!(
        sells(&run(&inputs(), &wicked)),
        none(),
        "measured against the full range the overlap is only 50%, short of \
         the 70% threshold"
    );

    let mut body_only = inputs();
    body_only[COVER_BODY_ONLY] = InputValue::Bool(true);
    assert_eq!(
        sells(&run(&body_only, &wicked)),
        vec![6],
        "the identical candle marks once the switch says to judge the \
         overlap against the body alone"
    );
}

#[test]
fn the_two_shapes_share_one_arrow_per_push() {
    // The cover fires at bar 6; bars 7 and 8 then complete a three-candle run
    // that gives back 100% and would fire on its own.
    let mut both = cover_tape();
    both.push((103.0, 103.0, 102.0, 102.0)); // 7  bearish
    both.push((102.0, 102.0, 101.0, 101.0)); // 8  bearish — run of 3, 100% back
    assert_eq!(
        sells(&run(&inputs(), &both)),
        vec![6],
        "the push was spent by the cover; the run must not mark it again"
    );

    // Switching a shape off does not only remove marks — it can move one,
    // because a push the cover never spent is still there for the run to
    // find. The script header says so; this is the tape that proves it.
    let mut run_only = inputs();
    run_only[USE_COVER] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(&run_only, &both)),
        vec![8],
        "with the cover off the same push is marked three bars later, where \
         the run completes — a jump, not an inconsistency"
    );
}

#[test]
fn switching_the_run_off_leaves_the_cover_alone_and_vice_versa() {
    let mut cover_only = inputs();
    cover_only[USE_RUN] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(&cover_only, &sell_tape())),
        none(),
        "the canonical run tape has no single candle covering 70% — its \
         deepest body overlaps 40%"
    );
    assert_eq!(
        sells(&run(&cover_only, &cover_tape())),
        vec![6],
        "…while the cover tape still marks with the run switched off"
    );

    let mut neither = inputs();
    neither[USE_RUN] = InputValue::Bool(false);
    neither[USE_COVER] = InputValue::Bool(false);
    assert_eq!(
        sells(&run(&neither, &cover_tape())),
        none(),
        "both shapes off is an indicator that marks nothing at all"
    );
}
