//! Golden walk of one full operation: fixed trades through the engine's
//! bar builder, the kernel and the simulator, twice — and the two runs must
//! be identical, or the kernel broke the determinism rule everything else
//! in the workspace keeps.

use quantick_engine::{BarBuilder as _, Side, TickBarBuilder, Trade};
use quantick_sim::{ClosedTrade, Command, ExitReason, Simulator};
use quantick_strategy::{
    ArmedState, ArmedStrategy, ForceParams, ForceTrigger, Rearm, Region, StrategyParams,
};
use rust_decimal::Decimal;

fn dec(s: &str) -> Decimal {
    use core::str::FromStr as _;
    Decimal::from_str(s).unwrap()
}

fn trade(agg_id: u64, price: &str) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: 1_700_000_000_000 + agg_id as i64 * 1_000,
        price: dec(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// The fixed tape. Two prints per tick bar (threshold 2): three body-1
/// warmup bars, then a body-4 force bar closing at 107 inside the region,
/// then the fill print and the take-profit print forming the final bar.
fn tape() -> Vec<Trade> {
    [
        "100", "101", // bar 1, body 1
        "101", "102", // bar 2, body 1
        "102", "103", // bar 3, body 1
        "103", "107",   // bar 4: body 4 vs average (1+1+4)/3 = 2 → force 2×
        "107.5", // entry market order fills here
        "111",   // take profit (107 + range 4 = 111) trades at its price
    ]
    .iter()
    .enumerate()
    .map(|(i, price)| trade(i as u64, price))
    .collect()
}

struct RunOutcome {
    commands: Vec<Command>,
    closed: Vec<ClosedTrade>,
    final_state: ArmedState,
    status: String,
}

/// The same interleaving the backtest run loop and the chart use: the print
/// meets the simulator first, then the bar builder; a closed bar consults
/// the kernel and its commands queue for the next print.
fn run() -> RunOutcome {
    let region = Region::new(dec("100"), dec("110"));
    let mut instance = ArmedStrategy::new(
        StrategyParams {
            side: Side::Buy,
            quantity: Decimal::ONE,
            tp_mult: Decimal::ONE,
            sl_mult: Decimal::ONE,
            rearm: Rearm::OneShot,
            on_break: quantick_strategy::BreakPolicy::Ignore,
        },
        Box::new(ForceTrigger::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_body: Decimal::ZERO,
        })),
    );
    let mut sim = Simulator::new();
    let mut builder = TickBarBuilder::new(2);
    let mut commands = Vec::new();

    for print in tape() {
        let events = sim.on_trade(&print);
        let _ = instance.on_sim_events(&events);
        if let Some(bar) = builder.push(&print) {
            let flat = sim.position().is_none();
            for command in instance.on_closed_bar(&bar, &region, true, flat) {
                commands.push(command);
                let events = sim.apply(command);
                let _ = instance.on_sim_events(&events);
            }
        }
    }

    RunOutcome {
        commands,
        closed: sim.closed_trades().to_vec(),
        final_state: instance.state().clone(),
        status: instance.status_line(),
    }
}

#[test]
fn one_operation_walks_the_whole_pipeline() {
    let outcome = run();

    assert_eq!(outcome.commands.len(), 1, "exactly one entry was fired");
    let Command::PlaceMarket {
        side,
        quantity,
        bracket,
    } = outcome.commands[0]
    else {
        panic!(
            "the kernel places market entries, got {:?}",
            outcome.commands[0]
        );
    };
    assert_eq!(side, Side::Buy);
    assert_eq!(quantity, Decimal::ONE);
    assert_eq!(bracket.take_profit, Some(dec("111")));
    assert_eq!(bracket.stop_loss, Some(dec("103")));

    assert_eq!(outcome.closed.len(), 1, "the operation round-tripped");
    let trade = &outcome.closed[0];
    assert_eq!(
        trade.entry_price,
        dec("107.5"),
        "market entry met the next print"
    );
    assert_eq!(trade.exit_price, dec("111"));
    assert_eq!(trade.exit_reason, ExitReason::TakeProfit);
    assert_eq!(trade.pnl_points, dec("3.5"));

    assert_eq!(outcome.final_state, ArmedState::Done, "one shot, then done");
    assert_eq!(outcome.status, "done · one shot");
}

#[test]
fn the_same_tape_produces_the_same_run_twice() {
    let first = run();
    let second = run();
    assert_eq!(first.commands, second.commands);
    assert_eq!(first.closed, second.closed);
    assert_eq!(first.final_state, second.final_state);
    assert_eq!(first.status, second.status);
}

/// The retest walk: a sell force bar cuts below the region, the kernel
/// rests a limit at the cut edge, and the tape decides — return to the edge
/// (fill, bracket, take profit) or reach the target first (the order goes,
/// no trade). Two prints per tick bar, exactly like [`tape`].
///
/// Bars: 110→109, 109→108 (warmup), then 108→104 — body 4 over average
/// (1+1+4)/3 = 2, force 2×, closing below the 105 edge. That bar's range is
/// 4 (high 108, low 104): SL 108, TP 100, entry edge 105, cancel-at 100.
fn run_retest(after_cut: &[&str]) -> RunOutcome {
    let region = Region::new(dec("105"), dec("115"));
    let mut instance = ArmedStrategy::new(
        StrategyParams {
            side: Side::Sell,
            quantity: Decimal::ONE,
            tp_mult: Decimal::ONE,
            sl_mult: Decimal::ONE,
            rearm: Rearm::OneShot,
            on_break: quantick_strategy::BreakPolicy::RetestLimit,
        },
        Box::new(ForceTrigger::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_body: Decimal::ZERO,
        })),
    );
    let mut sim = Simulator::new();
    let mut builder = TickBarBuilder::new(2);
    let mut commands = Vec::new();

    let prints: Vec<Trade> = ["110", "109", "109", "108", "108", "104"]
        .iter()
        .chain(after_cut)
        .enumerate()
        .map(|(i, price)| trade(i as u64, price))
        .collect();
    for print in prints {
        let events = sim.on_trade(&print);
        let _ = instance.on_sim_events(&events);
        if let Some(bar) = builder.push(&print) {
            let flat = sim.position().is_none();
            for command in instance.on_closed_bar(&bar, &region, true, flat) {
                commands.push(command);
                let events = sim.apply(command);
                let _ = instance.on_sim_events(&events);
            }
        }
    }

    RunOutcome {
        commands,
        closed: sim.closed_trades().to_vec(),
        final_state: instance.state().clone(),
        status: instance.status_line(),
    }
}

#[test]
fn the_retest_fills_at_the_edge_and_rides_to_the_target() {
    // The tape returns to the edge (105 fills the resting sell limit),
    // then walks down through the target (100 = the position's take
    // profit).
    let outcome = run_retest(&["104.5", "105", "101", "100"]);

    assert_eq!(
        outcome.commands,
        vec![Command::PlaceLimit {
            side: Side::Sell,
            quantity: Decimal::ONE,
            price: dec("105"),
            bracket: quantick_sim::Bracket {
                stop_loss: Some(dec("108")),
                take_profit: Some(dec("100")),
            },
            cancel_at: Some(dec("100")),
            flat_only: true,
        }]
    );
    assert_eq!(outcome.closed.len(), 1, "the retest round-tripped");
    let trade = &outcome.closed[0];
    assert_eq!(trade.entry_price, dec("105"), "filled at the edge itself");
    assert_eq!(trade.exit_price, dec("100"));
    assert_eq!(trade.exit_reason, ExitReason::TakeProfit);
    assert_eq!(trade.pnl_points, dec("5"));
    assert_eq!(outcome.final_state, ArmedState::Done, "one shot, then done");
}

#[test]
fn the_target_first_removes_the_order_and_no_trade_happens() {
    // The tape never returns: it walks straight down through the target.
    // The print at 100 cancels the resting limit by price.
    let outcome = run_retest(&["102", "100"]);

    assert_eq!(outcome.commands.len(), 1, "the limit was placed");
    assert!(
        outcome.closed.is_empty(),
        "no fill, no trade: {:?}",
        outcome.closed
    );
    assert_eq!(
        outcome.final_state,
        ArmedState::Disarmed {
            reason: quantick_strategy::DisarmReason::TargetBeforeRetest
        }
    );
    assert_eq!(outcome.status, "target hit before retest");
}

#[test]
fn the_retest_tape_produces_the_same_run_twice() {
    for after_cut in [&["104.5", "105", "101", "100"][..], &["102", "100"][..]] {
        let first = run_retest(after_cut);
        let second = run_retest(after_cut);
        assert_eq!(first.commands, second.commands);
        assert_eq!(first.closed, second.closed);
        assert_eq!(first.final_state, second.final_state);
        assert_eq!(first.status, second.status);
    }
}
