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
        },
        Box::new(ForceTrigger::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
        })),
    );
    let mut sim = Simulator::new();
    let mut builder = TickBarBuilder::new(2);
    let mut commands = Vec::new();

    for print in tape() {
        let events = sim.on_trade(&print);
        instance.on_sim_events(&events);
        if let Some(bar) = builder.push(&print) {
            let flat = sim.position().is_none();
            for command in instance.on_closed_bar(&bar, &region, true, flat) {
                commands.push(command);
                let events = sim.apply(command);
                instance.on_sim_events(&events);
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
