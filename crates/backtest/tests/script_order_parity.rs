//! A script placing orders, run by the harness and by the chart's own path
//! over one fixed tape: the two runs fill alike, and their fills are pinned
//! in a golden file.
//!
//! The chart's path is the one a tab steps a script instance through: the
//! account consumes the print, the instance hears its events, and a closed
//! bar is judged with the account lent to the runner.

use std::fmt::Write as _;
use std::path::Path;

use quantick_backtest::bars::BarSpec;
use quantick_backtest::run::run_session;
use quantick_backtest::script_orders::ScriptOrders;
use quantick_engine::{BarBuilder as _, Side, TickBarBuilder, Trade};
use quantick_replay::format::{self, ParseOptions, UtcOffset, WriteHeader};
use quantick_replay::session::Session;
use quantick_sim::{Command, Simulator};
use quantick_strategy::runner::{self, StrategyPort};
use quantick_strategy::script::{Script, ScriptInstance, ScriptPlacement};
use quantick_strategy::{OrderQuantity, PriceBracket, SignalAction, SignalOrder};
use quantick_trading::{ClosedTrade, VenueEvent};
use rust_decimal::Decimal;

/// The harness run's fills, recorded when script orders landed.
const GOLDEN: &str = include_str!("fixtures/script_order_parity.txt");

/// Buys every up bar a flat account can take.
const UP_CLOSE: &str = "//@version=5\n\
    indicator(\"Up close\")\n\
    alertcondition(close > open, title=\"Buy up\")\n\
    plot(close)\n";

const PRINTS: usize = 1_200;

/// Tick bars of four prints, as the chart's tick builder cuts them.
const TICKS: u64 = 4;

/// A deterministic walk pulled toward 1000, a burst every so often: a fixed
/// linear congruential sequence, so the fixture is the seed.
fn tape() -> Vec<Trade> {
    let mut state: u64 = 23;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        state >> 33
    };
    let mut price: i64 = 1_000;
    (0..PRINTS)
        .map(|step| {
            let pull = if price > 1_000 { -1 } else { 1 };
            price += match next() % 100 {
                0..=2 => pull * 6,
                3..=5 => -pull * 5,
                6..=40 => 1,
                41..=75 => -1,
                76..=85 => pull,
                _ => 0,
            };
            Trade {
                agg_id: step as u64 + 1,
                timestamp_ms: 1_786_233_600_000 + step as i64 * 1_000,
                price: Decimal::from(price),
                quantity: Decimal::ONE,
                side: if step % 2 == 0 { Side::Buy } else { Side::Sell },
            }
        })
        .collect()
}

fn session(trades: &[Trade]) -> Session {
    let mut text = format::write_header(&WriteHeader {
        symbol: "WINQ26".to_string(),
        timezone: UtcOffset::UTC,
        side_source: "flags".to_string(),
        source: Some("synthetic".to_string()),
    });
    for trade in trades {
        format::write_trade(&mut text, trade, None, UtcOffset::UTC);
    }
    let path = Path::new("synthetic/WINQ26/20260812.csv");
    Session::from_text(path, &text, ParseOptions::default()).expect("the synthetic session parses")
}

/// Wider than the walk's largest single step, so no fill outruns a leg.
fn bracket() -> PriceBracket {
    PriceBracket {
        stop: Decimal::from(8),
        gain: Decimal::from(10),
    }
}

fn script() -> Script {
    Script::load("up_close.pine", UP_CLOSE).expect("the script arms")
}

/// The simulator lent to the runner, with the harness's flat gate: no
/// position and nothing resting. One print closes at most one tick bar, so
/// no market entry is still queued when a bar is judged.
struct Venue<'a>(&'a mut Simulator);

impl StrategyPort for Venue<'_> {
    fn apply(&mut self, command: Command) -> Vec<VenueEvent> {
        self.0.apply(command)
    }

    fn is_flat(&self) -> bool {
        self.0.position().is_none() && self.0.orders().is_empty()
    }
}

/// The chart's path: the account consumes each print, the instance hears
/// its events, and a closed bar is judged through the runner.
fn chart_run(trades: &[Trade]) -> Vec<ClosedTrade> {
    let action = SignalAction::Order(SignalOrder {
        quantity: OrderQuantity::Fixed(Decimal::TWO),
        stop: bracket().stop,
        gain: bracket().gain,
    });
    let placement = ScriptPlacement::<()>::Anywhere;
    let mut instance = ScriptInstance::arm(0, &script(), Side::Buy, placement, &[], action);
    let mut sim = Simulator::new();
    let mut builder = TickBarBuilder::new(TICKS);
    for trade in trades {
        let events = sim.on_trade(trade);
        runner::deliver_print_events(
            std::slice::from_mut(&mut instance),
            &events,
            &mut Venue(&mut sim),
        );
        if let Some(bar) = builder.push(trade) {
            let resolved = placement.resolve(|()| None);
            let _ = instance.on_closed_bar(&bar, None, resolved.as_ref(), &mut Venue(&mut sim), 0);
        }
    }
    sim.closed_trades().to_vec()
}

#[test]
fn script_order_backtest_matches_the_chart_run() {
    let trades = tape();
    let mut strategy = ScriptOrders::new(script(), Side::Buy, Decimal::TWO, bracket());
    let run = run_session(&session(&trades), BarSpec::Tick(TICKS), &mut strategy);

    assert_eq!(
        run.trades,
        chart_run(&trades),
        "the harness fills as the chart does"
    );
    let mut rendered = String::new();
    for trade in &run.trades {
        writeln!(rendered, "{trade:?}").expect("a String takes every write");
    }
    writeln!(rendered, "open at end: {:?}", run.open_at_end).expect("a String takes every write");
    let golden = GOLDEN.replace("\r\n", "\n");
    assert!(
        rendered == golden,
        "the script-order backtest drifted from its recorded output:\n{rendered}"
    );
    for needle in ["StopLoss", "TakeProfit", "quantity: 2"] {
        assert!(golden.contains(needle), "the tape never shows {needle:?}");
    }
}
