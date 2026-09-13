// The account's own tests: a round trip with no ticket in sight, the risk
// lock's refusal, the symbol switch, and the journal golden - the money
// path's byte-for-byte record, moved here from `app` with the account.

use quantick_sim::{Currency, InstrumentMoney, MoneySource};

use super::*;
use crate::report::HistoryRow;
use crate::risk_sizing::RiskBasis;
use crate::scratch::ScratchDir;

/// A form with a quantity typed and no protective offsets — the state a
/// ticket is in when the trader has touched nothing but the size box.
fn plain_form() -> TicketForm {
    TicketForm {
        quantity: Ok(Decimal::ONE),
        offsets: Some((None, None)),
    }
}

fn plain_env() -> AccountEnv {
    AccountEnv {
        ruler_levels: None,
        form: plain_form(),
    }
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

/// An account with nothing but a scratch folder and a symbol: no ticket,
/// no window, no drawing type. That this compiles and trades at all is
/// the point of the split, and this module is where it is proved.
fn account(dir: &ScratchDir, symbol: &str) -> PaperAccount {
    let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());
    account.set_symbol(symbol);
    account
}

/// A round trip driven entirely through the account, and journaled.
///
/// The ticket is not constructed anywhere in this test. That is the whole
/// claim of the extraction: the money path runs without one.
#[test]
fn the_account_trades_and_journals_without_a_ticket() {
    let dir = ScratchDir::new("account-round-trip");
    let mut account = account(&dir, "ACCTX");

    account.seed(&print(0, 100));
    account.market(Side::Buy, Decimal::from(100), Bracket::none(), &plain_env());
    account.on_trade(&print(1, 100));
    let events = account.dispatch(Command::ClosePosition);
    account.handle_events(events);
    account.on_trade(&print(2, 105));

    assert!(account.is_flat(), "the position closed");
    assert_eq!(account.session_trades().len(), 1, "one round trip");
    assert_eq!(
        account.session_trades()[0].pnl_points,
        Decimal::from(5),
        "100 to 105 is five points"
    );

    let folder = dir.path().join("ACCTX");
    let files: Vec<_> = std::fs::read_dir(&folder)
        .expect("the symbol folder exists")
        .flatten()
        .collect();
    assert_eq!(files.len(), 1, "one session, one file");
    let text = std::fs::read_to_string(files[0].path()).expect("readable");
    assert!(
        text.contains("# symbol=ACCTX"),
        "the journal names the instrument: {text}"
    );
}

/// The risk lock refuses an oversized entry, and says so through the
/// outbox rather than by reaching for a toast lane it does not own.
#[test]
fn the_lock_refuses_through_the_outbox() {
    let dir = ScratchDir::new("account-risk-refusal");
    let mut account = account(&dir, "WIN$N");
    account.seed(&print(0, 100));

    // The lock measures money, so the instrument's has to be declared -
    // WIN$N's, twenty centavos a point.
    let mut book = InstrumentBook::new();
    book.insert(
        "WIN$N".to_owned(),
        InstrumentMoney {
            point_value: Decimal::new(20, 2),
            size_step: Decimal::ONE,
            min_size: Decimal::ONE,
            max_size: None,
            currency: Currency::new("BRL").expect("BRL"),
            source: MoneySource::Declared,
        },
    );
    account.set_instrument_money(book);
    let mut risk = account.risk_settings().clone();
    risk.lock = true;
    risk.basis = RiskBasis::Amount;
    risk.amount = Decimal::from(1);
    account.set_risk_settings(risk);

    // 99 points of stop x 0.20 x 1000 contracts is far past a 1 BRL budget.
    let intent = OrderIntent::market(Side::Buy, Decimal::from(1000))
        .with_bracket(Bracket::whole(Some(Decimal::from(1)), None));
    let events = account.place_intent(intent);

    assert!(events.is_empty(), "nothing reached the venue");
    assert!(
        account.peek_toast().is_some(),
        "and the refusal is waiting in the outbox, not on a lane"
    );
}

/// Leaving an instrument is a switch; arriving at the first one is not.
/// The caller needs the two told apart, because one forgets the ruler
/// and the other must not.
#[test]
fn set_symbol_tells_arriving_apart_from_switching() {
    let dir = ScratchDir::new("account-symbol");
    let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());

    assert_eq!(account.set_symbol("FIRST"), Some(true), "arriving");
    assert_eq!(account.set_symbol("SECOND"), Some(false), "a real switch");
    assert_eq!(account.set_symbol("SECOND"), None, "no change at all");
}

/// The file name [`the_journal_bytes_are_fixed`] must produce: named
/// from the first close's own timestamp.
const JOURNAL_GOLDEN_FILE: &str = "19700101-000002.csv";

/// Every byte [`the_journal_bytes_are_fixed`] must write. Recorded from
/// a run against `app`'s ticket *before* the policy half moved out of it,
/// and not touched since - not when the account left the ticket, and not
/// when it left `app` for this crate.
///
/// SHA-256 of these bytes:
/// `ab74859479f2f1e471dfb5a1556a15d2891d440c7c119db49c0e2ad64be094d6`.
/// The hash is written down so that the "before" and the "after" of the
/// extraction can be compared by someone who is reading neither this
/// file's history nor the diff — a reviewer, or the trader.
const JOURNAL_GOLDEN: &str = concat!(
    "# quantick-trades 2\n",
    "# symbol=GOLDEN\n",
    "# source=live\n",
    "opened_ms,closed_ms,side,quantity,entry_price,exit_price,pnl_points,",
    "exit_reason,entry_agg_id,exit_agg_id,mae_points,mfe_points\n",
    // A long taken at the market and closed by hand: 100 to 105.
    "1000,2000,long,1,100,105,5,manual,1,2,0,5\n",
    // A short, the same way: 105 down to 103.
    "3000,4000,short,1,105,103,2,manual,3,4,0,2\n",
    // A long stopped out. The entry is 103 and the ticket's stop offset
    // is 2, so the stop sits at 101 and the tape reaches it.
    "5000,6000,long,1,103,101,-2,stop_loss,5,6,2,0\n",
    // A long taken at its target: entry 101, offset 6, filled at 107.
    "7000,8000,long,1,101,107,6,take_profit,7,8,0,6\n",
);

/// A market entry the way the ticket places one: priced against the
/// mark, protected by the form's offsets around it, and handed the form
/// itself so the account can size it. No ruler is wound, which is the
/// ticket's own state when nobody has touched the wheel.
fn ticket_market(account: &mut PaperAccount, side: Side, form: &TicketForm) {
    let reference = account.mark_price().unwrap_or_default();
    let env = AccountEnv {
        ruler_levels: None,
        form: form.clone(),
    };
    account.market(side, reference, form.bracket(side, reference), &env);
}

/// One fixed tape, one journal, asserted byte for byte.
///
/// This is the money path's golden. It was written against `app`'s
/// ticket *before* the policy half moved into an account of its own, and
/// its expected bytes did not change when that account moved into this
/// crate — that is the whole point. An extraction that alters a fill
/// rule, a bracket price, the risk lock's arithmetic, a rounding or the
/// journal's own format fails here rather than in front of the trader,
/// and it fails naming the byte.
///
/// The tape is fixed in every respect the writer reads: prices and
/// quantities are exact decimals, every timestamp is derived from the
/// print's own `agg_id` rather than a clock, and the session file's name
/// comes from the first close's `closed_ms`. So the file name is
/// asserted too — a session that opened a differently named file would
/// still hold the right rows, and the trader would still have lost the
/// trade in a folder nobody reads.
///
/// Four round trips, chosen to cover the four ways a position ends:
/// a long closed by hand, a short closed by hand, a long stopped out,
/// and a long taken at its target. The last two carry the ticket's two
/// offsets, so the bracket arithmetic is under the golden and not only
/// the flat manual close. In `app` those offsets were typed into the
/// ticket's boxes; here they arrive as the [`TicketForm`] the ticket
/// resolves its boxes into, which is the only part of it the account
/// ever read.
#[test]
fn the_journal_bytes_are_fixed() {
    // Its own scratch folder, carrying a run token and removed with the
    // value: a reused process id would otherwise hand this run the last
    // one's journal, and the golden would fail on a file it never wrote.
    let dir = ScratchDir::new("paper-journal-golden");
    let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());
    account.set_symbol("GOLDEN");
    account.seed(&print(0, 100));
    let bare = plain_form();

    // 1. A long, entered at the market and closed by hand: +5.
    ticket_market(&mut account, Side::Buy, &bare);
    account.on_trade(&print(1, 100));
    let events = account.dispatch(Command::ClosePosition);
    account.handle_events(events);
    account.on_trade(&print(2, 105));

    // 2. A short, the same way: 105 down to 103 is +2.
    ticket_market(&mut account, Side::Sell, &bare);
    account.on_trade(&print(3, 105));
    let events = account.dispatch(Command::ClosePosition);
    account.handle_events(events);
    account.on_trade(&print(4, 103));

    // 3. A long with protection, stopped out: a stop offset of 2 and a
    //    profit offset of 6, exactly what the ticket's boxes held.
    let protected = TicketForm {
        quantity: Ok(Decimal::ONE),
        offsets: Some((Some(Decimal::from(2)), Some(Decimal::from(6)))),
    };
    ticket_market(&mut account, Side::Buy, &protected);
    account.on_trade(&print(5, 103));
    account.on_trade(&print(6, 101));

    // 4. A long with the same protection, taken at its target.
    ticket_market(&mut account, Side::Buy, &protected);
    account.on_trade(&print(7, 101));
    account.on_trade(&print(8, 107));

    let folder = dir.path().join("GOLDEN");
    let mut files: Vec<_> = std::fs::read_dir(&folder)
        .expect("the symbol folder exists")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    files.sort();
    assert_eq!(files.len(), 1, "one session, one file: {files:?}");
    assert_eq!(
        files[0].file_name().and_then(|name| name.to_str()),
        Some(JOURNAL_GOLDEN_FILE),
        "the session file is named from the first close, not from a clock"
    );

    let text = std::fs::read_to_string(&files[0]).expect("readable");
    assert_eq!(
        text, JOURNAL_GOLDEN,
        "the journal's bytes moved; the money path is not what it was"
    );
}

#[test]
fn utc_compact_matches_known_timestamps() {
    // 2026-03-16 13:01:08 UTC.
    assert_eq!(utc_compact(1_773_666_068_000), "20260316-130108");
    // The epoch itself.
    assert_eq!(utc_compact(0), "19700101-000000");
}

#[test]
fn the_export_csv_carries_readable_stamps_and_running_equity() {
    let trade = |closed_ms: i64, pnl: i64, mae: Option<i64>| ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(100 + pnl),
        opened_ms: closed_ms - 60_000,
        closed_ms,
        pnl_points: Decimal::from(pnl),
        exit_reason: quantick_sim::ExitReason::Manual,
        entry_agg_id: mae.map(|_| 1),
        exit_agg_id: mae.map(|_| 2),
        mae_points: mae.map(Decimal::from),
        mfe_points: mae.map(Decimal::from),
    };
    let rows = vec![
        HistoryRow {
            symbol: "BTCUSDT".to_owned(),
            source: Some(history::SessionSource::Live),
            trade: trade(1_773_666_068_000, 5, Some(2)),
        },
        HistoryRow {
            symbol: "WINQ26".to_owned(),
            source: None,
            trade: trade(1_773_666_368_000, -2, None),
        },
    ];
    let text = export_csv(&rows);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "header plus two rows");
    assert!(lines[0].starts_with("symbol,side,quantity,opened_ms,opened_utc"));
    assert!(lines[0].ends_with(",source"), "{}", lines[0]);
    assert!(
        lines[1].contains("2026-03-16T13:01:08Z"),
        "human-readable UTC beside the epoch: {}",
        lines[1]
    );
    assert!(lines[1].ends_with(",manual,1,2,2,2,live"), "{}", lines[1]);
    assert!(
        lines[2].contains(",3,"),
        "running equity 5 + (-2): {}",
        lines[2]
    );
    assert!(
        lines[2].ends_with(",manual,,,,,"),
        "unknown v1 fields and an unrecorded source stay empty: {}",
        lines[2]
    );
}

#[test]
fn long_export_paths_elide_to_the_file_name() {
    let short = std::path::Path::new("paper-trades/export-1.csv");
    assert_eq!(elide_path(short), short.display().to_string());
    let long = std::path::Path::new(
        "C:/some/extremely/long/path/that/never/ends/and/keeps/going/paper-trades/export-20260805-141233.csv",
    );
    let elided = elide_path(long);
    assert!(elided.starts_with('…'), "{elided}");
    assert!(elided.ends_with("export-20260805-141233.csv"), "{elided}");
}

#[test]
fn export_rows_remember_the_source_each_trade_closed_under() {
    let dir = ScratchDir::new("account-export-source");
    let mut account = account(&dir, "SRCX");
    account.seed(&print(0, 100));
    ticket_market(&mut account, Side::Buy, &plain_form());
    account.on_trade(&print(1, 100));
    let events = account.dispatch(Command::ClosePosition);
    account.handle_events(events);
    account.on_trade(&print(2, 105));
    account.set_session_source(history::SessionSource::Replay);
    let sources: Vec<_> = account
        .session_history_rows()
        .into_iter()
        .map(|row| row.source)
        .collect();
    assert_eq!(
        sources,
        vec![Some(history::SessionSource::Live)],
        "a trade keeps the source it closed under, not the current one"
    );
}
