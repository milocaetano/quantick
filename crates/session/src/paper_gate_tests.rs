//! New entries wait while the host says the chart is out of sync with its
//! feed, on the simulator and on a real broker route alike; an order that
//! only reduces the position, and every exit, still goes.

use std::path::PathBuf;

use quantick_engine::{Side, Trade};
use quantick_paper::PaperAccount;
use quantick_paper::desk::ChartCommand;
use quantick_paper::fake_broker::{TradeMode, broker};
use quantick_sim::{Bracket, Command, EntryKind, OrderId, OrderIntent, RejectReason, VenueEvent};
use quantick_sources::sync::{Desync, LinkState, STRATEGY_ENTRY_PAUSED, SyncLimits, SyncReading};
use quantick_strategy::runner::StrategyPort;
use rust_decimal::Decimal;

use crate::paper::{TabPaper, TradeCommand, TradeRefusal};

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn late() -> Desync {
    Desync {
        reading: SyncReading::Latency,
        link: LinkState::Up,
        latency_ms: Some(900),
        latency_hop: Some("MT5"),
        tape_age_ms: Some(40),
        limits: SyncLimits::default(),
        notice: None,
    }
}

fn sim() -> TabPaper {
    TabPaper::new(PaperAccount::with_trades_dir(PathBuf::new()))
}

fn real() -> TabPaper {
    let mut account = PaperAccount::with_trades_dir(PathBuf::new());
    account.attach_broker(broker(TradeMode::Real).0);
    let paper = TabPaper::new(account);
    assert_eq!(paper.account.route_kind().word(), "REAL");
    paper
}

/// A long of one filled at 100 and a resting buy limit at 90 — an order
/// that would grow the long — both placed before the feed fell behind.
fn exposed(paper: &mut TabPaper) -> OrderId {
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    paper.working_orders()[0].id
}

fn refused(paper: &mut TabPaper, command: TradeCommand) -> String {
    let outcome = paper.execute(command.clone());
    assert!(outcome.events.is_empty(), "{command:?} reached the venue");
    let refusal = outcome.refusal.expect("an entry is refused");
    assert_eq!(
        refusal,
        TradeRefusal::Paused(late()),
        "a caller can tell why"
    );
    let refusal = refusal.sentence();
    let toast = paper.account.peek_toast().cloned().unwrap_or_default();
    assert!(toast.ends_with(&refusal), "toasted as refused: {toast}");
    refusal
}

/// Every door an order that grows exposure comes through, with the gate
/// set: nothing reaches the venue, on any route.
fn growing_orders_wait(paper: &mut TabPaper, resting: OrderId) {
    paper.set_trade_gate(Some(late()));
    let buy = OrderIntent::market(Side::Buy, Decimal::ONE);
    let sell_two = OrderIntent::market(Side::Sell, Decimal::from(2));
    for command in [
        TradeCommand::Market { side: Side::Buy },
        TradeCommand::Place {
            intent: Box::new(buy),
        },
        TradeCommand::Place {
            intent: Box::new(sell_two),
        },
        TradeCommand::Reverse,
    ] {
        let refusal = refused(paper, command);
        assert!(refusal.contains("prints arrive 900 ms late"), "{refusal}");
    }
    paper.desk.ticket.qty_text = "2".to_owned();
    refused(paper, TradeCommand::Market { side: Side::Sell });
    paper.desk.ticket.qty_text = "1".to_owned();

    let orders = paper.working_orders().to_vec();
    assert!(!paper.place_resting(Side::Buy, EntryKind::Limit, 80.0));
    let place = ChartCommand::PlaceResting {
        side: Side::Buy,
        kind: EntryKind::Limit,
        raw_price: 85.0,
    };
    assert_eq!(paper.carry_out(place), Ok(()));
    let reprice = ChartCommand::AmendOrder {
        id: resting,
        price: Decimal::from(99),
    };
    assert_eq!(paper.carry_out(reprice), Ok(()));
    assert_eq!(paper.working_orders(), orders, "no placement, no reprice");
    assert_eq!(paper.account.venue().in_flight(), 0, "nothing in flight");
}

fn goes(paper: &mut TabPaper, command: TradeCommand) {
    let outcome = paper.execute(command.clone());
    assert_eq!(outcome.refusal, None, "{command:?} was refused");
    assert!(!outcome.events.is_empty(), "{command:?} reached the venue");
}

/// Orders that only reduce the long, and every exit, with the gate set. A
/// reducing order counts against the long while it works or is in flight,
/// so the second one that would outgrow it waits.
fn reducing_orders_go(paper: &mut TabPaper, resting: OrderId) {
    let sell_limit = || TradeCommand::Place {
        intent: Box::new(OrderIntent::limit(
            Side::Sell,
            Decimal::ONE,
            Decimal::from(110),
        )),
    };
    goes(paper, sell_limit());
    refused(paper, sell_limit());
    refused(paper, TradeCommand::Market { side: Side::Sell });
    let mut working = paper.working_orders().iter();
    let sell = working.rfind(|order| order.side == Side::Sell);
    let sell = sell.expect("the sell limit works").id;
    goes(paper, TradeCommand::Cancel { id: sell });

    goes(paper, TradeCommand::Market { side: Side::Sell });
    refused(paper, TradeCommand::Market { side: Side::Sell });
    assert_eq!(
        paper.account.venue().in_flight(),
        1,
        "one sell before a fill"
    );

    let protect = Bracket::whole(Some(Decimal::from(80)), None);
    goes(
        paper,
        TradeCommand::Amend {
            id: resting,
            bracket: Box::new(protect),
        },
    );
    goes(paper, TradeCommand::Cancel { id: resting });
    for exit in [
        TradeCommand::CancelAll,
        TradeCommand::Close,
        TradeCommand::Flatten,
    ] {
        assert_eq!(paper.execute(exit.clone()).refusal, None, "{exit:?}");
    }
    paper.on_trade(&print(2, 101));
    assert!(paper.is_flat(), "the long closed");
    assert!(paper.working_orders().is_empty(), "nothing works");
}

#[test]
fn a_paused_simulator_refuses_what_grows_exposure_and_takes_what_reduces_it() {
    let mut paper = sim();
    let resting = exposed(&mut paper);
    growing_orders_wait(&mut paper, resting);
    let toast = paper.account.peek_toast().cloned().unwrap_or_default();
    assert!(toast.starts_with("SIM: new entries paused"), "{toast}");
    reducing_orders_go(&mut paper, resting);
}

#[test]
fn a_paused_real_route_refuses_what_grows_exposure_and_takes_what_reduces_it() {
    let mut paper = real();
    let resting = exposed(&mut paper);
    growing_orders_wait(&mut paper, resting);
    let toast = paper.account.peek_toast().cloned().unwrap_or_default();
    assert!(toast.starts_with("REAL: new entries paused"), "{toast}");
    reducing_orders_go(&mut paper, resting);
}

/// An armed strategy is asked the same question, with no toast: its entry
/// that would grow the long and its reprice of one are refused, and its
/// entry against the long goes.
#[test]
fn a_paused_strategy_may_reduce_but_not_grow_exposure() {
    let mut paper = sim();
    let resting = exposed(&mut paper);
    let _ = paper.account.take_toast();
    paper.set_trade_gate(Some(late()));
    let blocked = vec![VenueEvent::Rejected(RejectReason::Blocked(
        STRATEGY_ENTRY_PAUSED,
    ))];
    let entry = |side| Command::PlaceMarket {
        side,
        quantity: Decimal::ONE,
        bracket: Bracket::default(),
    };
    assert_eq!(paper.apply(entry(Side::Buy)), blocked);
    let reprice = Command::ModifyOrder {
        id: resting,
        price: Decimal::from(95),
    };
    assert_eq!(paper.apply(reprice), blocked);
    assert_ne!(paper.apply(entry(Side::Sell)), blocked, "it reduces");
    assert_eq!(paper.account.peek_toast(), None, "no toast per print");
}

#[test]
fn entries_resume_once_the_gate_lifts() {
    let mut paper = sim();
    paper.seed(&print(0, 100));
    paper.set_trade_gate(Some(late()));
    assert_eq!(paper.trade_gate(), Some(&late()));
    paper.set_trade_gate(None);
    let outcome = paper.execute(TradeCommand::Market { side: Side::Buy });
    assert_eq!(outcome.refusal, None);
    assert!(!outcome.events.is_empty(), "the entry reached the venue");
}

/// A print reaching the desk ends a pause for the tape's age at once, so a
/// strategy that print fires is not refused on the silence it ended; a
/// pause for latency waits for the next judgement.
#[test]
fn a_print_ends_a_pause_for_the_tapes_age() {
    let mut paper = sim();
    paper.seed(&print(0, 100));
    let stale = Desync {
        reading: SyncReading::TapeAge,
        ..late()
    };
    paper.set_trade_gate(Some(stale));
    paper.on_trade(&print(1, 100));
    assert_eq!(paper.trade_gate(), None);
    paper.set_trade_gate(Some(late()));
    paper.on_trade(&print(2, 100));
    assert_eq!(paper.trade_gate(), Some(&late()), "latency is not age");
}
