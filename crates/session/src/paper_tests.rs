//! A tab's paper trading, driven without a window: the desk's text and the
//! account's money path joined by [`TabPaper`].
//!
//! The account journals to an empty folder path and is never named a
//! symbol unless a test says so, and a close on an unnamed account writes
//! nothing to disk — so these tests own no files. The journal's bytes are
//! `quantick_paper`'s own tests.

use std::path::PathBuf;

use quantick_engine::{Side, Trade};
use quantick_paper::PaperAccount;
use quantick_sim::{Command, VenueEvent};
use quantick_strategy::runner::StrategyPort;
use rust_decimal::Decimal;

use crate::paper::TabPaper;

fn flat() -> TabPaper {
    TabPaper::new(PaperAccount::with_trades_dir(PathBuf::new()))
}

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// C4: a reversal is one market order for twice the size — the long closes
/// as a journaled trade and a short of the same quantity opens, protected
/// by the ticket's offsets.
#[test]
fn reverse_flips_a_filled_market_position() {
    let mut paper = flat();
    paper.desk.ticket.qty_text = "2".to_owned();
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let long = paper.account.venue().position().expect("the buy filled");
    assert_eq!((long.side, long.quantity), (Side::Buy, Decimal::from(2)));
    assert!(paper.session_trades().is_empty(), "nothing closed yet");
    let _ = paper.account.take_journal_changed();

    paper.desk.ticket.stop_offset_text = "5".to_owned();
    paper.reverse_position();
    paper.on_trade(&print(2, 100));

    let short = paper
        .account
        .venue()
        .position()
        .expect("reversed, not flat");
    assert_eq!(short.side, Side::Sell);
    assert_eq!(short.quantity, Decimal::from(2), "the same quantity");
    assert_eq!(
        short.stop_loss,
        Some(Decimal::from(105)),
        "the new short is protected by the ticket's offset"
    );
    assert_eq!(paper.session_trades().len(), 1, "the long closed once");
    assert!(
        paper.account.take_journal_changed(),
        "and the close went through the journal"
    );
}

#[test]
fn reversing_a_flat_account_places_nothing() {
    let mut paper = flat();
    paper.seed(&print(0, 100));
    paper.reverse_position();
    assert!(paper.is_flat());
    assert!(paper.working_orders().is_empty());
    assert!(paper.account.peek_toast().is_none(), "nothing to say");
}

#[test]
fn a_bad_offset_toasts_and_blocks_the_order() {
    let mut paper = flat();
    paper.seed(&print(0, 100));
    paper.desk.ticket.stop_offset_text = "abc".to_owned();
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    assert!(paper.is_flat(), "no order reached the venue");
    assert!(
        paper.account.peek_toast().is_some(),
        "the refusal teaches, never silent"
    );
}

#[test]
fn an_accepted_market_entry_puts_the_ruler_away() {
    let mut paper = flat();
    paper.seed(&print(0, 100));
    assert_eq!(paper.set_ruler_ticks(3), 3);
    paper.market(Side::Buy);
    assert_eq!(paper.ruler_ticks(), 0, "the distance was for this trade");
}

/// The opening instrument is arrival, not departure: a ruler standing
/// before the first symbol survives it, and only leaving one forgets.
#[test]
fn the_first_symbol_of_a_session_keeps_a_standing_ruler() {
    let mut paper = flat();
    paper.desk.ruler.notches = 6;
    assert!(paper.set_symbol("BTCUSDT"));
    assert_eq!(paper.ruler_ticks(), 6, "arriving is not a switch");
    assert!(
        !paper.set_symbol("BTCUSDT"),
        "the same symbol changes nothing"
    );
    assert!(paper.set_symbol("WIN$N"));
    assert_eq!(paper.ruler_ticks(), 0, "leaving one still forgets");
}

#[test]
fn a_timeline_reset_flattens_and_empties_the_hand() {
    let mut paper = flat();
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let _ = paper.account.take_toast();
    paper.desk.armed = Some(quantick_paper::desk::ArmedPlacement {
        side: Side::Buy,
        kind: quantick_sim::EntryKind::Limit,
    });

    paper.on_timeline_reset();

    assert!(paper.is_flat());
    assert!(paper.desk.armed.is_none(), "an armed click dies with it");
    assert_eq!(paper.session_trades().len(), 1, "the flatten is a trade");
}

#[test]
fn a_timeline_reset_that_sweeps_orders_says_so() {
    let mut paper = flat();
    paper.seed(&print(0, 100));
    let events = paper
        .account
        .venue_mut()
        .submit(quantick_sim::OrderIntent::limit(
            Side::Buy,
            Decimal::ONE,
            Decimal::from(90),
        ));
    paper.account.handle_events(events);
    assert_eq!(paper.working_orders().len(), 1);
    let _ = paper.account.take_toast();

    paper.on_timeline_reset();

    assert!(paper.working_orders().is_empty());
    assert_eq!(
        paper.account.peek_toast().map(String::as_str),
        Some(super::paper::RESET_ORDERS_TOAST),
        "a sweep is never silent"
    );
}

#[test]
fn a_quiet_timeline_reset_says_nothing() {
    let mut paper = flat();
    paper.on_timeline_reset();
    assert!(paper.account.peek_toast().is_none());
}

#[test]
fn the_strategy_port_reaches_the_account() {
    let mut paper = flat();
    paper.seed(&print(0, 100));
    assert!(StrategyPort::is_flat(&paper));
    let events = paper.apply(Command::PlaceMarket {
        side: Side::Buy,
        quantity: Decimal::ONE,
        bracket: quantick_sim::Bracket::none(),
    });
    assert!(
        matches!(events.as_slice(), [VenueEvent::Placed(_)]),
        "the command reached the venue: {events:?}"
    );
    paper.on_trade(&print(1, 100));
    assert!(!StrategyPort::is_flat(&paper), "and it filled");
}
