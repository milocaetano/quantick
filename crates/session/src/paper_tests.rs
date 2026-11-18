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

/// C5: a live print reaches a resting limit through the tab, and its
/// bracket's target closes it as a journaled trade - the flag the host
/// reads to re-read an open report.
#[test]
fn a_live_print_fills_a_resting_limit_and_its_target_journals_the_close() {
    let mut paper = flat();
    paper.seed(&print(0, 100));
    let events = paper.account.venue_mut().submit(
        quantick_sim::OrderIntent::limit(Side::Buy, Decimal::ONE, Decimal::from(95))
            .with_bracket(quantick_sim::Bracket::whole(None, Some(Decimal::from(105)))),
    );
    paper.account.handle_events(events);
    assert_eq!(paper.working_orders().len(), 1, "the limit rests");

    paper.on_trade(&print(1, 95));
    assert!(
        !paper.is_flat(),
        "the print crossed the limit and it filled"
    );
    let _ = paper.account.take_journal_changed();

    paper.on_trade(&print(2, 105));
    assert!(paper.is_flat(), "the target closed it");
    assert_eq!(paper.session_trades().len(), 1);
    assert!(paper.account.take_journal_changed(), "through the journal");
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

/// A saved flatten is never silent and outranks the sweep; an unsaved one
/// leaves the could-not-save warning standing.
#[test]
fn a_saved_flatten_says_so_and_an_unsaved_one_leaves_the_warning() {
    use super::paper::{RESET_POSITION_TOAST, reset_toast};
    use quantick_paper::account::TimelineReset;
    let reset = |had_position, all_saved| TimelineReset {
        had_position,
        had_orders: true,
        all_saved,
    };
    assert_eq!(reset_toast(reset(true, true)), Some(RESET_POSITION_TOAST));
    assert_eq!(reset_toast(reset(true, false)), None);
    assert_eq!(reset_toast(reset(false, false)), None);
}

/// The steppers walk the instrument's own size step: a hard-coded 1 moved
/// a crypto size by a hundred thousand steps.
#[test]
fn the_quantity_steppers_walk_the_instruments_own_size_step() {
    let mut paper = flat();
    paper.set_symbol("BTCUSDT");
    let money = quantick_sim::InstrumentMoney {
        point_value: Decimal::ONE,
        size_step: Decimal::new(1, 5),
        min_size: Decimal::new(1, 5),
        max_size: None,
        currency: quantick_sim::Currency::new("USDT").expect("USDT"),
        source: quantick_sim::MoneySource::Declared,
    };
    let book = [("BTCUSDT".to_owned(), money)].into_iter().collect();
    paper.account.set_instrument_money(book);
    paper.desk.ticket.qty_text = "0.00002".to_owned();
    paper.step_quantity(Decimal::ONE);
    assert_eq!(paper.desk.ticket.qty_text, "0.00003");
    paper.step_quantity(-Decimal::ONE);
    assert_eq!(paper.desk.ticket.qty_text, "0.00002");
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
