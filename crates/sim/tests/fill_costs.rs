//! Golden fills with costs on: slippage in ticks on market-style fills, a
//! commission on both sides of each round trip, and conservative limits
//! that need a print one tick through. The tick is 5, as on the mini index.
//! Free costs are the fill model every other test in this crate pins.

use quantick_engine::{Side, Trade};
use quantick_sim::{Command, Simulator};
use quantick_trading::{
    Bracket, ClosedTrade, Commission, ExitPart, ExitReason, Fill, FillCosts, FillRole,
    TradingVenue, VenueEvent,
};
use rust_decimal::Decimal;

fn dec(value: i64) -> Decimal {
    Decimal::from(value)
}

fn decimal(text: &str) -> Decimal {
    text.parse().expect("test decimal")
}

/// A print: the id doubles as a readable timestamp (`ts = id * 1000`).
fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("test ids are small") * 1000,
        price: dec(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn seeded(costs: FillCosts, price: i64) -> Simulator {
    let mut sim = Simulator::with_costs(costs);
    sim.seed(&print(0, price));
    sim
}

/// Every fill's (role, price, quantity), in order.
fn fills(events: &[VenueEvent]) -> Vec<(FillRole, Decimal, Decimal)> {
    events
        .iter()
        .filter_map(|event| match event {
            VenueEvent::Filled(Fill {
                role,
                price,
                quantity,
                ..
            }) => Some((*role, *price, *quantity)),
            _ => None,
        })
        .collect()
}

fn entry_fill(events: &[VenueEvent]) -> Decimal {
    match fills(events).as_slice() {
        [(FillRole::Entry(_), price, _)] => *price,
        other => panic!("expected one entry fill, got {other:?}"),
    }
}

/// A closed trade's (exit reason, exit price, gross, fees, net).
fn booked(trade: &ClosedTrade) -> (ExitReason, Decimal, Decimal, Decimal, Decimal) {
    (
        trade.exit_reason,
        trade.exit_price,
        trade.pnl_points,
        trade.fees_points,
        trade.net_points(),
    )
}

fn costs(commission: Commission, slippage_ticks: u32, conservative: bool) -> FillCosts {
    FillCosts::new(commission, slippage_ticks, Some(dec(5)), conservative).expect("valid costs")
}

#[test]
fn a_market_entry_and_its_stop_each_slip_a_tick_and_both_sides_pay() {
    let mut sim = seeded(
        costs(Commission::PerContract(decimal("1.25")), 1, false),
        1000,
    );
    sim.apply(Command::PlaceMarket {
        side: Side::Buy,
        quantity: dec(2),
        bracket: Bracket::whole(Some(dec(980)), Some(dec(1050))),
    });

    assert_eq!(
        entry_fill(&sim.on_trade(&print(1, 1000))),
        dec(1005),
        "a buy pays up"
    );
    let exit = sim.on_trade(&print(2, 975));
    assert_eq!(
        fills(&exit),
        [(FillRole::StopLoss, dec(970), dec(2))],
        "a sell gives up"
    );

    // (970 - 1005) x 2 = -70 gross; 1.25 x 2 contracts x 2 sides = 5 fees.
    let trade = &sim.closed_trades()[0];
    assert_eq!(trade.entry_price, dec(1005));
    assert_eq!(
        booked(trade),
        (ExitReason::StopLoss, dec(970), dec(-70), dec(5), dec(-75))
    );
    assert_eq!(sim.realized_points(), dec(-75), "the account keeps the net");
}

#[test]
fn a_conservative_limit_and_take_profit_need_a_print_one_tick_through() {
    let mut sim = seeded(
        costs(Commission::PerNotional(decimal("0.001")), 0, true),
        1000,
    );
    sim.apply(Command::PlaceLimit {
        side: Side::Buy,
        quantity: dec(1),
        price: dec(990),
        bracket: Bracket::whole(None, Some(dec(1020))),
        cancel_at: None,
        flat_only: false,
    });

    assert!(
        fills(&sim.on_trade(&print(1, 990))).is_empty(),
        "a touch is not a fill"
    );
    assert!(
        fills(&sim.on_trade(&print(2, 986))).is_empty(),
        "less than a tick through"
    );
    assert_eq!(
        entry_fill(&sim.on_trade(&print(3, 985))),
        dec(990),
        "fills at its own price"
    );

    assert!(
        fills(&sim.on_trade(&print(4, 1020))).is_empty(),
        "the target is touched only"
    );
    assert_eq!(
        fills(&sim.on_trade(&print(5, 1025))),
        [(FillRole::TakeProfit, dec(1020), dec(1))]
    );

    // 0.001 x 990 + 0.001 x 1020: each side pays on its own notional.
    assert_eq!(
        booked(&sim.closed_trades()[0]),
        (
            ExitReason::TakeProfit,
            dec(1020),
            dec(30),
            decimal("2.01"),
            decimal("27.99")
        )
    );
}

#[test]
fn a_ladders_stop_leg_slips_and_its_take_profit_leg_needs_a_tick_through() {
    let mut sim = seeded(costs(Commission::Free, 1, true), 1000);
    let ladder = Bracket::ladder(&[
        ExitPart {
            quantity: Some(dec(1)),
            take_profit: Some(dec(1010)),
            stop_loss: Some(dec(990)),
        },
        ExitPart {
            quantity: None,
            take_profit: Some(dec(1030)),
            stop_loss: Some(dec(990)),
        },
    ])
    .expect("a valid ladder");
    sim.apply(Command::PlaceMarket {
        side: Side::Buy,
        quantity: dec(2),
        bracket: ladder,
    });
    assert_eq!(entry_fill(&sim.on_trade(&print(1, 1000))), dec(1005));

    assert!(
        fills(&sim.on_trade(&print(2, 1010))).is_empty(),
        "the first target is touched"
    );
    assert_eq!(
        fills(&sim.on_trade(&print(3, 1015))),
        [(FillRole::TakeProfit, dec(1010), dec(1))]
    );
    assert_eq!(
        fills(&sim.on_trade(&print(4, 990))),
        [(FillRole::StopLoss, dec(985), dec(1))],
        "the runner's stop fills a tick under the print"
    );
    let booked: Vec<_> = sim.closed_trades().iter().map(booked).collect();
    assert_eq!(
        booked,
        [
            (ExitReason::TakeProfit, dec(1010), dec(5), dec(0), dec(5)),
            (ExitReason::StopLoss, dec(985), dec(-20), dec(0), dec(-20)),
        ]
    );
}

#[test]
fn a_short_stop_entry_and_a_hand_close_slip_against_the_short() {
    let mut sim = seeded(costs(Commission::PerContract(dec(1)), 1, false), 1000);
    sim.apply(Command::PlaceStop {
        side: Side::Sell,
        quantity: dec(1),
        trigger: dec(990),
        bracket: Bracket::none(),
    });
    assert_eq!(
        entry_fill(&sim.on_trade(&print(1, 985))),
        dec(980),
        "a sell gives up"
    );

    sim.apply(Command::ClosePosition);
    assert_eq!(
        fills(&sim.on_trade(&print(2, 970))),
        [(FillRole::Close, dec(975), dec(1))],
        "a buy to cover pays up"
    );
    assert_eq!(
        booked(&sim.closed_trades()[0]),
        (ExitReason::Manual, dec(975), dec(5), dec(2), dec(3))
    );
}

#[test]
fn a_reset_flattens_at_the_mark_less_its_slippage() {
    let mut sim = seeded(costs(Commission::Free, 2, false), 1000);
    sim.apply(Command::PlaceMarket {
        side: Side::Buy,
        quantity: dec(1),
        bracket: Bracket::none(),
    });
    assert_eq!(entry_fill(&sim.on_trade(&print(1, 1000))), dec(1010));
    sim.on_trade(&print(2, 1040));
    assert_eq!(fills(&sim.reset()), [(FillRole::Reset, dec(1030), dec(1))]);
    assert_eq!(sim.realized_points(), dec(20));
}

#[test]
fn costs_set_on_a_running_simulator_price_the_next_fill() {
    let mut sim = Simulator::new();
    sim.seed(&print(0, 1000));
    sim.apply(Command::PlaceMarket {
        side: Side::Buy,
        quantity: dec(1),
        bracket: Bracket::none(),
    });
    assert_eq!(
        entry_fill(&sim.on_trade(&print(1, 1000))),
        dec(1000),
        "free so far"
    );
    TradingVenue::set_fill_costs(&mut sim, costs(Commission::PerContract(dec(3)), 1, false));
    sim.apply(Command::ClosePosition);
    assert_eq!(
        fills(&sim.on_trade(&print(2, 1000))),
        [(FillRole::Close, dec(995), dec(1))]
    );
    assert_eq!(
        booked(&sim.closed_trades()[0]),
        (ExitReason::Manual, dec(995), dec(-5), dec(6), dec(-11))
    );
}
