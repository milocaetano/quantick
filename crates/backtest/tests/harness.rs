//! The harness end to end, driven by a strategy the crate has never heard of.
//!
//! `ScriptedOrders` lives here, outside the crate, and implements the same
//! port `EmaCross` and `PlotSignal` do. That is the point: if a test double
//! written against nothing but the public trait can dock, drive the loop and
//! read indicator columns, so can WP-03's operational rules — the port is a
//! port and not a shape the built-in strategies happen to have.

use std::path::Path;

use quantick_backtest::bars::BarSpec;
use quantick_backtest::report::{NOT_AVAILABLE, render_run, render_session};
use quantick_backtest::run::{RunOutcome, run_session};
use quantick_backtest::strategies::{EmaCross, ForceRegion, PlotSignal, Protection};
use quantick_backtest::strategy::{BarView, Strategy};
use quantick_engine::{Side, Trade};
use quantick_indicators::{
    Indicator, PlotId, SourceId,
    native::{Cvd, Ema},
};
use quantick_replay::format::{self, ParseOptions, UtcOffset, WriteHeader};
use quantick_replay::session::Session;
use quantick_sim::{Command, ExitReason};
use rust_decimal::Decimal;

/// The 34-print opening auction the replay crate keeps as its own fixture.
/// Included rather than copied: one recording, one copy of the truth.
const OPENING_AUCTION: &str = include_str!("../../replay/tests/fixtures/WINQ26-2026-08-12.csv");

const VALUE: PlotId = PlotId::new(0);

/// A strategy that issues pre-arranged commands at pre-arranged bars, and
/// records what it was shown.
struct ScriptedOrders {
    steps: Vec<(usize, Command)>,
    /// Bar indices seen, in order.
    seen: Vec<usize>,
    /// The fast-EMA reading at each of those bars.
    readings: Vec<f64>,
    /// Whether an indicator was declared at all.
    with_indicators: bool,
    /// Bumped by `end_of_session`, to prove the hook fires once per session.
    sessions_ended: usize,
}

impl ScriptedOrders {
    fn new(steps: Vec<(usize, Command)>) -> Self {
        Self {
            steps,
            seen: Vec::new(),
            readings: Vec::new(),
            with_indicators: true,
            sessions_ended: 0,
        }
    }
}

impl Strategy for ScriptedOrders {
    fn name(&self) -> &str {
        "scripted-orders"
    }

    fn indicators(&mut self) -> Vec<Box<dyn Indicator>> {
        if self.with_indicators {
            vec![Box::new(Ema::new(3, SourceId::Close)), Box::new(Cvd::new())]
        } else {
            Vec::new()
        }
    }

    fn on_bar(&mut self, view: &BarView<'_>) -> Vec<Command> {
        self.seen.push(view.index);
        self.readings.push(view.signals.latest(0, VALUE));
        self.steps
            .iter()
            .filter(|(at, _)| *at == view.index)
            .map(|(_, command)| *command)
            .collect()
    }

    fn end_of_session(&mut self) {
        self.sessions_ended += 1;
    }
}

/// A rising tape: `count` prints, one unit each, price stepping up by one
/// from `first`, alternating aggressor so cumulative delta stays near zero.
fn rising_tape(first: i64, count: i64) -> String {
    let mut text = format::write_header(&WriteHeader {
        symbol: "WINQ26".to_string(),
        timezone: UtcOffset::UTC,
        side_source: "flags".to_string(),
        source: Some("synthetic".to_string()),
    });
    for step in 0..count {
        format::write_trade(
            &mut text,
            &Trade {
                agg_id: (step + 1) as u64,
                // One print per second, starting at 2026-08-12 00:00:00Z.
                timestamp_ms: 1_786_233_600_000 + step * 1_000,
                price: Decimal::from(first + step),
                quantity: Decimal::ONE,
                side: if step % 2 == 0 { Side::Buy } else { Side::Sell },
            },
            None,
            UtcOffset::UTC,
        );
    }
    text
}

fn synthetic(text: &str) -> Session {
    Session::from_text(
        Path::new("synthetic/WINQ26/20260812.csv"),
        text,
        ParseOptions::default(),
    )
    .expect("the synthetic session parses")
}

/// A tape of exactly these prices, one print per second, unit quantity.
fn tape_of(prices: &[&str]) -> String {
    let mut text = format::write_header(&WriteHeader {
        symbol: "WINQ26".to_string(),
        timezone: UtcOffset::UTC,
        side_source: "flags".to_string(),
        source: Some("synthetic".to_string()),
    });
    for (step, price) in prices.iter().enumerate() {
        format::write_trade(
            &mut text,
            &Trade {
                agg_id: (step + 1) as u64,
                timestamp_ms: 1_786_233_600_000 + step as i64 * 1_000,
                price: price.parse().expect("fixture price"),
                quantity: Decimal::ONE,
                side: if step % 2 == 0 { Side::Buy } else { Side::Sell },
            },
            None,
            UtcOffset::UTC,
        );
    }
    text
}

/// The chart's armed-rectangle kernel runs under the harness unchanged:
/// same trigger, same gates, same projected bracket — one brain, two
/// consumers. The tape mirrors the strategy crate's own golden walk
/// (`crates/strategy/tests/full_operation.rs`), so any divergence between
/// the live chart and this harness has nowhere to hide.
#[test]
fn the_force_region_kernel_walks_a_full_operation_under_the_harness() {
    use quantick_strategy::{ForceParams, Rearm, Region, StrategyParams};

    // Tick(2) bars: three body-1 warmup bars, a body-4 force bar closing
    // at 107 inside the region, the fill print, the take-profit print.
    let session = synthetic(&tape_of(&[
        "100", "101", "101", "102", "102", "103", "103", "107", "107.5", "111",
    ]));
    let mut strategy = ForceRegion::new(
        Region::new(Decimal::from(100), Decimal::from(110)),
        StrategyParams {
            side: Side::Buy,
            quantity: Decimal::ONE,
            tp_mult: Decimal::ONE,
            sl_mult: Decimal::ONE,
            rearm: Rearm::OneShot,
            on_break: quantick_strategy::BreakPolicy::Ignore,
        },
        ForceParams {
            window: 3,
            min_factor: "1.5".parse().expect("fixture factor"),
            max_factor: "2.5".parse().expect("fixture factor"),
            min_body: Decimal::ZERO,
        },
    );
    let run = run_session(&session, BarSpec::Tick(2), &mut strategy);

    assert_eq!(run.trades.len(), 1, "one operation round-tripped");
    let trade = &run.trades[0];
    assert_eq!(
        trade.entry_price,
        "107.5".parse::<Decimal>().expect("price"),
        "the market entry met the print after the trigger bar"
    );
    assert_eq!(trade.exit_price, Decimal::from(111));
    assert_eq!(trade.exit_reason, ExitReason::TakeProfit);
    assert_eq!(
        trade.pnl_points,
        "3.5".parse::<Decimal>().expect("points"),
        "close 107 + 1x range 4 = take profit 111, entered from 107.5"
    );
    assert_eq!(
        run.open_at_end, None,
        "the take profit closed it before the recording ended"
    );
}

#[test]
fn a_foreign_strategy_docks_reads_indicators_and_reaches_the_tape() {
    // 60 prints at 100..159, ten per bar: bar 2 closes on print 29 (price
    // 129) and bar 4 on print 49 (price 149).
    let session = synthetic(&rising_tape(100, 60));
    let mut strategy = ScriptedOrders::new(vec![
        (
            2,
            Command::PlaceMarket {
                side: Side::Buy,
                quantity: Decimal::ONE,
                bracket: quantick_sim::Bracket::none(),
            },
        ),
        (4, Command::ClosePosition),
    ]);

    let run = run_session(&session, BarSpec::Tick(10), &mut strategy);

    assert_eq!(run.prints, 60);
    assert_eq!(run.bars, 6);
    assert_eq!(
        strategy.seen,
        vec![0, 1, 2, 3, 4, 5],
        "every close is offered"
    );
    assert_eq!(strategy.sessions_ended, 1, "the end hook fires once");

    // Warmup is `na`, and a strategy must be able to tell that apart from a
    // reading of zero.
    assert!(strategy.readings[0].is_nan(), "EMA(3) is na on bar 0");
    assert!(
        strategy.readings[2].is_finite(),
        "EMA(3) has a value by bar 2: {:?}",
        strategy.readings
    );

    // The order placed on bar 2 filled on the print *after* the one that
    // closed it (130, not 129), and the close on bar 4 filled on the print
    // after that bar's (150, not 149). That gap is the no-look-ahead rule.
    assert_eq!(run.report.trades, 1, "{:?}", run.trades);
    let trade = &run.trades[0];
    assert_eq!(trade.entry_price, Decimal::from(130));
    assert_eq!(trade.exit_price, Decimal::from(150));
    assert_eq!(trade.pnl_points, Decimal::from(20));
    assert_eq!(trade.exit_reason, ExitReason::Manual);
    assert_eq!(run.report.net_points, Decimal::from(20));
    assert!(run.open_at_end.is_none());
    assert!(run.anomalies.is_clean(), "{:?}", run.anomalies);
}

#[test]
fn a_position_left_open_is_reported_rather_than_flattened() {
    // Buying on the last bar leaves nothing to fill against: the recording
    // ends. The honest answer is "still open", not an invented exit at the
    // last mark.
    let session = synthetic(&rising_tape(100, 60));
    let mut strategy = ScriptedOrders::new(vec![(
        5,
        Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: quantick_sim::Bracket::none(),
        },
    )]);

    let run = run_session(&session, BarSpec::Tick(10), &mut strategy);
    assert_eq!(run.report.trades, 0, "an unfilled entry is not a trade");
    assert!(
        run.open_at_end.is_none(),
        "the entry never filled, so nothing is open either"
    );

    // Filling it needs one more print after the bar closes.
    let session = synthetic(&rising_tape(100, 61));
    let mut strategy = ScriptedOrders::new(vec![(
        5,
        Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: quantick_sim::Bracket::none(),
        },
    )]);
    let run = run_session(&session, BarSpec::Tick(10), &mut strategy);
    assert!(run.open_at_end.is_some(), "the fill happened on print 61");
    assert_eq!(run.report.trades, 0, "an open position is not a round trip");
    let text = render_session(&run);
    assert!(
        text.contains("still open at the last print"),
        "the report must say so:\n{text}"
    );
}

#[test]
fn refusals_are_counted_not_swallowed() {
    // Closing nothing, and a limit on the wrong side of the market: both are
    // refused by the simulator, and a harness that hid them would let a
    // broken rule look like a quiet one.
    let session = synthetic(&rising_tape(100, 30));
    let mut strategy = ScriptedOrders::new(vec![
        (0, Command::ClosePosition),
        (
            1,
            Command::PlaceLimit {
                side: Side::Buy,
                quantity: Decimal::ONE,
                // Far above the market: a buy limit there fills instantly.
                price: Decimal::from(10_000),
                bracket: quantick_sim::Bracket::none(),
                cancel_at: None,
            },
        ),
    ]);

    let run = run_session(&session, BarSpec::Tick(10), &mut strategy);
    assert_eq!(run.anomalies.rejections(), 2, "{:?}", run.anomalies);
    assert_eq!(run.anomalies.rejected.get("no_position"), Some(&1));
    assert_eq!(run.anomalies.rejected.get("limit_on_wrong_side"), Some(&1));
    let text = render_session(&run);
    assert!(text.contains("rejections"), "{text}");
    assert!(text.contains("no_position"), "{text}");
}

#[test]
fn a_dropped_bracket_is_counted() {
    // A protective price is validated against the mark that closed the bar,
    // but the entry fills on the *next* print. On a tape that only rises it
    // is the target the tape outruns: bar 1 closes at 119, a long target at
    // 119.5 is legitimately above that mark, and by the time the entry fills
    // — at 120 — the target sits below the position it is supposed to take
    // profit on. The simulator drops it rather than exit on the next print
    // wearing a lying label, and the harness has to count that.
    let session = synthetic(&rising_tape(100, 40));
    let mut strategy = ScriptedOrders::new(vec![(
        1,
        Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: quantick_sim::Bracket {
                stop_loss: None,
                take_profit: Some(Decimal::new(1_195, 1)),
            },
        },
    )]);

    let run = run_session(&session, BarSpec::Tick(10), &mut strategy);
    assert_eq!(run.anomalies.dropped_brackets(), 1, "{:?}", run.anomalies);
    assert_eq!(
        run.anomalies
            .brackets_dropped
            .get("take_profit_on_wrong_side"),
        Some(&1)
    );
    assert!(
        run.anomalies.rejected.is_empty(),
        "the command itself was valid: {:?}",
        run.anomalies
    );
    let text = render_session(&run);
    assert!(text.contains("brackets dropped"), "{text}");
}

#[test]
fn the_same_session_run_twice_produces_identical_reports() {
    // The workspace's standard guard shape: run it twice, demand byte
    // equality. Anything that leaked a clock, a hash order or an address
    // into the numbers shows up here.
    let session = synthetic(&rising_tape(1_000, 400));
    let spec = BarSpec::Tick(7);
    let render = || {
        let mut strategy = EmaCross::new(
            3,
            9,
            Decimal::ONE,
            Protection {
                stop_points: Some(Decimal::from(15)),
                target_points: Some(Decimal::from(25)),
            },
            false,
        );
        let run = run_session(&session, spec, &mut strategy);
        let outcome = RunOutcome::from_sessions("ema-cross(3/9)", spec, vec![run]);
        render_run(&outcome)
    };

    let first = render();
    let second = render();
    assert_eq!(first, second, "two runs of one session disagreed");
    assert!(
        first.contains("trades"),
        "the run must have produced a report:\n{first}"
    );
}

#[test]
fn the_recorded_opening_auction_is_a_smoke_test_and_says_so() {
    // 34 prints in 75 ms of a crossed opening auction. It proves the pipe
    // loads, builds bars and reports — it measures nothing. No strategy has
    // a signal in six bars, and the report is required to say "no closed
    // trade" rather than print a page of zeros.
    let session = Session::from_text(
        Path::new("WINQ26-2026-08-12.csv"),
        OPENING_AUCTION,
        ParseOptions::default(),
    )
    .expect("the replay crate's fixture parses");
    assert_eq!(session.trades.len(), 34, "the fixture is 34 prints");

    let spec = BarSpec::Tick(5);
    let mut strategy = EmaCross::new(9, 21, Decimal::ONE, Protection::default(), false);
    let run = run_session(&session, spec, &mut strategy);

    assert_eq!(run.prints, 34);
    assert_eq!(run.report.trades, 0, "no rule signals inside an auction");
    assert!(run.anomalies.is_clean());
    let outcome = RunOutcome::from_sessions("ema-cross(9/21)", spec, vec![run]);
    let text = render_run(&outcome);
    assert!(text.contains("no closed trade"), "{text}");
    assert!(
        !text.contains(&format!("profit factor     {NOT_AVAILABLE}")),
        "with no trades the metric block is omitted entirely, not filled with n/a:\n{text}"
    );
}

#[test]
fn the_aggregate_keeps_the_spread_the_average_hides() {
    // Two sessions, one winning and one losing, run over the same rule. The
    // aggregate is the concatenated equity curve; the distribution is what
    // says the two days did not look alike.
    let rising = synthetic(&rising_tape(100, 60));
    let falling_text = {
        // The same tape read backwards: a strategy long into it loses.
        let mut text = format::write_header(&WriteHeader {
            symbol: "WINQ26".to_string(),
            timezone: UtcOffset::UTC,
            side_source: "flags".to_string(),
            source: Some("synthetic".to_string()),
        });
        for step in 0..60i64 {
            format::write_trade(
                &mut text,
                &Trade {
                    agg_id: (step + 1) as u64,
                    timestamp_ms: 1_786_320_000_000 + step * 1_000,
                    price: Decimal::from(200 - step),
                    quantity: Decimal::ONE,
                    side: if step % 2 == 0 { Side::Sell } else { Side::Buy },
                },
                None,
                UtcOffset::UTC,
            );
        }
        text
    };
    let falling = synthetic(&falling_text);

    let buy_then_close = || {
        ScriptedOrders::new(vec![
            (
                2,
                Command::PlaceMarket {
                    side: Side::Buy,
                    quantity: Decimal::ONE,
                    bracket: quantick_sim::Bracket::none(),
                },
            ),
            (4, Command::ClosePosition),
        ])
    };
    let mut up = buy_then_close();
    let mut down = buy_then_close();
    let runs = vec![
        run_session(&rising, BarSpec::Tick(10), &mut up),
        run_session(&falling, BarSpec::Tick(10), &mut down),
    ];

    let outcome = RunOutcome::from_sessions("scripted-orders", BarSpec::Tick(10), runs);
    assert_eq!(outcome.aggregate.trades, 2);
    assert_eq!(outcome.distribution.sessions, 2);
    assert_eq!(outcome.distribution.up, 1);
    assert_eq!(outcome.distribution.down, 1);
    assert_eq!(outcome.aggregate.net_points, Decimal::ZERO, "+20 and -20");
    assert_eq!(
        outcome.distribution.median_net,
        Some(Decimal::ZERO),
        "the mean of +20 and -20"
    );
    let (best_label, best_net) = outcome.distribution.best.clone().expect("a best session");
    assert_eq!(best_net, Decimal::from(20));
    assert!(best_label.contains("WINQ26"), "{best_label}");

    // The aggregate is not a zero-drawdown flat line: the losing session is
    // inside the same equity curve.
    assert_eq!(outcome.aggregate.max_drawdown_points, Decimal::from(20));
}

#[test]
fn a_pine_script_supplies_the_signal_and_rust_places_the_orders() {
    // The division of labour the dialect enforces: `strategy.*` is rejected
    // by the compiler, so a script can only plot a number. `PlotSignal`
    // reads that column and Rust decides size, protection and reversals.
    // On a tape that only rises, a "close above its own EMA" script is long
    // from warmup onwards — one entry, held to the end, never reversed.
    let source = "//@version=5\n\
                  indicator(\"Signal\", overlay=false)\n\
                  plot(close > ta.ema(close, 5) ? 1 : -1, title=\"signal\")\n";
    let mut strategy =
        PlotSignal::compile("rising", source, 0, Decimal::ONE, Protection::default())
            .expect("the script compiles");

    let session = synthetic(&rising_tape(100, 200));
    let run = run_session(&session, BarSpec::Tick(10), &mut strategy);

    assert_eq!(run.bars, 20);
    assert_eq!(
        run.open_at_end.map(|(side, _)| side),
        Some(Side::Buy),
        "a rising tape leaves the script long"
    );
    // The script plots `-1` while `ta.ema` is still `na`: a comparison
    // against na is false, so the ternary takes its else branch. The run
    // therefore opens short on warmup and reverses once the EMA exists —
    // one closed round trip, and a standing reminder that handling na is
    // the script author's job, not the harness's.
    assert_eq!(run.report.trades, 1, "{:?}", run.trades);
    assert_eq!(run.trades[0].side, Side::Sell, "the warmup short");
    assert!(run.anomalies.is_clean(), "{:?}", run.anomalies);

    // The mirror: a falling tape puts the same script short. Same code, and
    // the only thing that changed is what the script plotted.
    let mut falling = String::new();
    falling.push_str(&format::write_header(&WriteHeader {
        symbol: "WINQ26".to_string(),
        timezone: UtcOffset::UTC,
        side_source: "flags".to_string(),
        source: Some("synthetic".to_string()),
    }));
    for step in 0..200i64 {
        format::write_trade(
            &mut falling,
            &Trade {
                agg_id: (step + 1) as u64,
                timestamp_ms: 1_786_233_600_000 + step * 1_000,
                price: Decimal::from(400 - step),
                quantity: Decimal::ONE,
                side: Side::Sell,
            },
            None,
            UtcOffset::UTC,
        );
    }
    let mut strategy =
        PlotSignal::compile("falling", source, 0, Decimal::ONE, Protection::default())
            .expect("the script compiles");
    let run = run_session(&synthetic(&falling), BarSpec::Tick(10), &mut strategy);
    assert_eq!(
        run.open_at_end.map(|(side, _)| side),
        Some(Side::Sell),
        "a falling tape leaves the same script short"
    );
}

#[test]
fn an_ema_cross_turns_a_crossing_into_an_order() {
    // A tape that rises then falls crosses the fast EMA back under the slow
    // one, and the harness has to turn that into a round trip. Without this
    // the only proof `EmaCross` works is that the determinism test does not
    // crash.
    let mut text = format::write_header(&WriteHeader {
        symbol: "WINQ26".to_string(),
        timezone: UtcOffset::UTC,
        side_source: "flags".to_string(),
        source: Some("synthetic".to_string()),
    });
    // Up 100 → 300, back down to 100, up again: 600 prints, 60 bars of 10.
    // Two crossings, because only a reversal closes a round trip — one
    // crossing would leave the position open and the report empty.
    for step in 0..600i64 {
        let price = match step {
            s if s < 200 => 100 + s,
            s if s < 400 => 300 - (s - 200),
            s => 100 + (s - 400),
        };
        format::write_trade(
            &mut text,
            &Trade {
                agg_id: (step + 1) as u64,
                timestamp_ms: 1_786_233_600_000 + step * 1_000,
                price: Decimal::from(price),
                quantity: Decimal::ONE,
                side: if step % 2 == 0 { Side::Buy } else { Side::Sell },
            },
            None,
            UtcOffset::UTC,
        );
    }
    let session = synthetic(&text);
    let mut strategy = EmaCross::new(3, 9, Decimal::ONE, Protection::default(), false);
    let run = run_session(&session, BarSpec::Tick(10), &mut strategy);

    assert!(
        run.report.trades >= 1,
        "the two turns produce at least one round trip: {:?}",
        run.trades
    );
    assert!(
        run.trades.iter().any(|t| t.side == Side::Sell),
        "the fall is taken short: {:?}",
        run.trades
    );
    assert!(
        run.open_at_end.is_some(),
        "the final rise leaves a position open"
    );
    assert!(run.anomalies.is_clean(), "{:?}", run.anomalies);
}

#[test]
fn a_strategy_that_declares_no_indicator_reads_na_instead_of_panicking() {
    // `PlotBuffer::column` panics on an unknown plot id. A strategy asking
    // for a slot it never declared must get `na` and carry on, or one bad
    // index would abort a run halfway through a session library.
    let session = synthetic(&rising_tape(100, 30));
    let mut strategy = ScriptedOrders::new(Vec::new());
    strategy.with_indicators = false;

    let run = run_session(&session, BarSpec::Tick(10), &mut strategy);
    assert_eq!(run.bars, 3);
    assert!(
        strategy.readings.iter().all(|value| value.is_nan()),
        "an undeclared slot reads na: {:?}",
        strategy.readings
    );
}
