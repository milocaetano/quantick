// The account's own tests: a round trip with no ticket in sight, the risk
// lock's refusal, the symbol switch, and the journal golden - the money
// path's byte-for-byte record, moved here from `app` with the account.

use quantick_trading::{Currency, InstrumentMoney, Money, MoneySource};

use super::*;
use crate::report::HistoryRow;
use crate::risk_sizing::{BudgetRefusal, RiskBasis};
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

/// The refusal sentence the trader reads carries no run of stray spaces.
///
/// A lost line continuation once left eighteen of them before "turn", and the
/// move out of `app` kept them byte for byte rather than change a sentence
/// mid-move. The whole wording is pinned here so they cannot come back.
#[test]
fn the_refusal_sentence_has_no_stray_spaces() {
    let brl = Currency::new("BRL").expect("BRL");
    let refusal = RiskRefusal {
        risk: Money::new(Decimal::new(1980, 2), brl.clone()),
        budget: Money::new(Decimal::ONE, brl),
    };

    assert_eq!(
        refusal.sentence(),
        "this order risks 19.8 BRL - over your 1 BRL risk per trade. Raise the risk, or turn the lock off."
    );
    assert!(
        !refusal.sentence().contains("  "),
        "no run of two spaces survives: {}",
        refusal.sentence()
    );
}

/// One of every [`BudgetRefusal`], so a sentence walk covers the class.
///
/// Listed by hand and then checked: the `match` below stops compiling the
/// moment a variant is added, so the list cannot quietly fall behind the
/// enum it claims to cover.
fn every_budget_refusal(amount: &Currency, instrument: &Currency) -> Vec<BudgetRefusal> {
    let all = vec![
        BudgetRefusal::NotSet,
        BudgetRefusal::AmountNotPositive,
        BudgetRefusal::PercentNotPositive,
        BudgetRefusal::NoCapital(amount.clone()),
        BudgetRefusal::AmountInAnotherCurrency {
            amount: amount.clone(),
            instrument: instrument.clone(),
        },
    ];
    for refusal in &all {
        match refusal {
            BudgetRefusal::NotSet
            | BudgetRefusal::AmountNotPositive
            | BudgetRefusal::PercentNotPositive
            | BudgetRefusal::NoCapital(_)
            | BudgetRefusal::AmountInAnotherCurrency { .. } => {}
        }
    }
    all
}

/// No refusal sentence this crate hands a reader carries a run of spaces.
///
/// The same lost line continuation that left eighteen spaces in the risk
/// lock's sentence left a run in `BudgetRefusal::AmountInAnotherCurrency`
/// too, and both reach the same two readers - the ticket's toast and the
/// control plane's error. So the guard is per class and not per literal: it
/// walks every `BudgetRefusal` variant beside [`RiskRefusal::sentence`], and
/// a variant added later is covered by this test instead of needing a
/// hand-written literal beside it.
#[test]
fn no_refusal_sentence_carries_a_run_of_spaces() {
    let brl = Currency::new("BRL").expect("BRL");
    let usd = Currency::new("USD").expect("USD");

    let risk_refusal = RiskRefusal {
        risk: Money::new(Decimal::new(1980, 2), brl.clone()),
        budget: Money::new(Decimal::ONE, brl.clone()),
    };

    let mismatch = BudgetRefusal::AmountInAnotherCurrency {
        amount: brl.clone(),
        instrument: usd.clone(),
    };
    assert_eq!(
        mismatch.sentence(),
        "your risk per trade is BRL and this instrument trades in USD - nothing here converts \
         between currencies. Set a risk in USD to size on it.",
        "the currency mismatch wording is pinned, tidied"
    );

    let budget_refusals = every_budget_refusal(&brl, &usd);
    let sentences = std::iter::once(("RiskRefusal", risk_refusal.sentence())).chain(
        budget_refusals
            .iter()
            .map(|refusal| ("BudgetRefusal", refusal.sentence())),
    );
    for (owner, sentence) in sentences {
        assert!(
            !sentence.contains("  "),
            "{owner} hands the reader a run of two spaces: {sentence}"
        );
    }
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
        exit_reason: quantick_trading::ExitReason::Manual,
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
            session: None,
            trade_ref: history::TradeRef::default(),
            tags: Vec::new(),
        },
        HistoryRow {
            symbol: "WINQ26".to_owned(),
            source: None,
            trade: trade(1_773_666_368_000, -2, None),
            session: None,
            trade_ref: history::TradeRef::default(),
            tags: Vec::new(),
        },
    ];
    let declared = crate::risk_sizing::InstrumentBook::new();
    let catalog = crate::instruments::InstrumentCatalog::default();
    let text = export_csv(
        &rows,
        &crate::pnl::PnlBook {
            declared: &declared,
            catalog: &catalog,
            feed: "",
        },
    );
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "header plus two rows");
    assert!(lines[0].starts_with("symbol,side,quantity,opened_ms,opened_utc"));
    assert!(
        lines[0].ends_with(",source,pnl_per_unit,pnl_money,currency"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].contains("2026-03-16T13:01:08Z"),
        "human-readable UTC beside the epoch: {}",
        lines[1]
    );
    assert!(
        lines[1].ends_with(",manual,1,2,2,2,live,5,,"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].contains(",3,"),
        "running equity 5 + (-2): {}",
        lines[2]
    );
    assert!(
        lines[2].ends_with(",manual,,,,,,-2,,"),
        "unknown v1 fields and an unrecorded source stay empty: {}",
        lines[2]
    );
}

/// The export carries what the report shows: the move per contract and the
/// money it made, in the instrument's own currency - empty where no point
/// value is known, never a guessed zero.
#[test]
fn the_export_csv_adds_per_contract_points_and_money() {
    let trade = |pnl: i64, quantity: i64| ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::from(quantity),
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(100 + pnl / quantity),
        opened_ms: 1_773_666_000_000,
        closed_ms: 1_773_666_060_000,
        pnl_points: Decimal::from(pnl),
        exit_reason: quantick_trading::ExitReason::Manual,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: None,
        mfe_points: None,
    };
    let row = |symbol: &str, trade: ClosedTrade| HistoryRow {
        symbol: symbol.to_owned(),
        source: None,
        trade,
        session: None,
        trade_ref: history::TradeRef::default(),
        tags: Vec::new(),
    };
    let rows = vec![row("WINV26", trade(400, 2)), row("XAUUSD", trade(30, 3))];
    let catalog = crate::instruments::InstrumentCatalog::parse(
        "version = 1
[instruments.\"WIN*\"]
currency = \"BRL\"
point_value = \"0.20\"
size_step = \"1\"
",
    )
    .expect("catalog");
    let declared = crate::risk_sizing::InstrumentBook::new();
    let text = export_csv(
        &rows,
        &crate::pnl::PnlBook {
            declared: &declared,
            catalog: &catalog,
            feed: "",
        },
    );
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[1].ends_with(",400,400,60000,manual,,,,,,200,80,BRL"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].ends_with(",10,,"),
        "unknown money stays empty: {}",
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

/// A journaled close raises the flag the host re-reads its report on, once:
/// the host takes it, and a print that closes nothing does not raise it
/// again.
#[test]
fn a_journaled_close_tells_the_host_once() {
    let dir = ScratchDir::new("account-journal-flag");
    let mut account = account(&dir, "FLAG");
    account.seed(&print(0, 100));
    ticket_market(&mut account, Side::Buy, &plain_form());
    account.on_trade(&print(1, 100));
    assert!(!account.take_journal_changed(), "an entry journals nothing");
    let events = account.dispatch(Command::ClosePosition);
    account.handle_events(events);
    account.on_trade(&print(2, 101));
    assert!(
        account.take_journal_changed(),
        "the close reached the journal"
    );
    assert!(!account.take_journal_changed(), "and the flag was taken");
    account.on_trade(&print(3, 102));
    assert!(
        !account.take_journal_changed(),
        "a quiet print raises nothing"
    );
}

/// A launch hook's risk stands from construction and says so, and the
/// money it declared lands on the first symbol the host names.
#[test]
fn a_risk_hook_is_told_rather_than_read() {
    let dir = ScratchDir::new("account-risk-hook");
    let hook = crate::risk_sizing::parse_hook("100:0.20:1:BRL").expect("a valid hook spec");
    let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf()).with_risk_hook(hook);
    assert!(
        account.risk_from_hook(),
        "the host must not restore over it"
    );
    assert_eq!(account.risk_settings().amount, Decimal::from(100));
    assert!(
        account.instrument_money().is_empty(),
        "no symbol to key it by yet"
    );
    account.set_symbol("WIN$N");
    assert_eq!(
        account
            .instrument_money()
            .get("WIN$N")
            .map(|money| money.point_value),
        Some(Decimal::new(20, 2)),
        "the hook's money landed on the first symbol"
    );
    account.set_symbol("WDO$N");
    assert!(
        !account.instrument_money().contains_key("WDO$N"),
        "and only on the first"
    );
}

/// A timeline reset flattens at the last mark, journals the forced close,
/// ends the session file and says what it swept.
#[test]
fn a_timeline_reset_journals_the_forced_close_and_ends_the_file() {
    let dir = ScratchDir::new("account-reset");
    let mut account = account(&dir, "RESET");
    account.seed(&print(0, 100));
    ticket_market(&mut account, Side::Buy, &plain_form());
    account.on_trade(&print(1, 100));
    account.on_trade(&print(2, 103));
    assert!(account.journal_path().is_none(), "nothing closed yet");

    let reset = account.reset_timeline();

    assert_eq!(
        reset,
        TimelineReset {
            had_position: true,
            had_orders: false,
            all_saved: true,
        }
    );
    assert!(account.is_flat(), "the position flattened");
    assert_eq!(account.session_trades().len(), 1, "at the last mark");
    assert!(
        account.take_journal_changed(),
        "a reset close invalidates the disk report just like another Closed event"
    );
    assert!(
        !account.take_journal_changed(),
        "the reset hint is consumed once"
    );
    assert!(
        account.journal_path().is_none(),
        "the reset ended the session file, so the next close opens a new one"
    );
    assert_eq!(
        account.session_journal_paths().len(),
        1,
        "the close was journaled"
    );
}

/// A bot or the backtest branches on a refusal without reading English:
/// `try_place_intent` answers with the typed refusal and posts nothing,
/// while `place_intent` posts the same sentence the refusal carries.
#[test]
fn a_refusal_is_typed_for_a_caller_and_worded_for_the_trader() {
    let dir = ScratchDir::new("account-typed-refusal");
    let mut account = account(&dir, "WIN$N");
    account.seed(&print(0, 100));
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
    let intent = || {
        OrderIntent::market(Side::Buy, Decimal::from(1000))
            .with_bracket(Bracket::whole(Some(Decimal::from(1)), None))
    };

    let refusal = account
        .try_place_intent(intent())
        .expect_err("99 points x 0.20 x 1000 is past a 1 BRL budget");
    assert_eq!(refusal.budget.amount, Decimal::from(1));
    assert_eq!(refusal.risk.amount, Decimal::from(19_800));
    assert_eq!(refusal.risk.currency.code(), "BRL");
    assert!(
        account.peek_toast().is_none(),
        "a typed refusal posts nothing"
    );
    assert!(account.is_flat(), "and nothing reached the venue");
    assert!(
        account.try_place_intent(intent()).is_err(),
        "asking twice is safe: still refused, still nothing placed"
    );

    assert!(account.place_intent(intent()).is_empty());
    assert_eq!(
        account.peek_toast().map(String::as_str),
        Some(format!("SIM: {}", refusal.sentence()).as_str()),
        "the toast is the refusal's own sentence"
    );
}

/// Removing a kept strategy keeps the selection on the strategy it named,
/// not on the slot that shifted under it - and removing the selected one
/// selects nothing rather than its neighbour.
#[test]
fn removing_a_strategy_keeps_the_selection_on_its_name() {
    let dir = ScratchDir::new("account-strategies");
    let mut account = account(&dir, "LADDER");
    let named = |name: &str| OrderStrategy {
        name: name.to_owned(),
        rows: Vec::new(),
    };
    for name in ["first", "second", "third"] {
        account.add_order_strategy(named(name));
    }
    account.select_strategy(Some(2));

    assert_eq!(
        account.remove_order_strategy(0).map(|s| s.name),
        Some("first".to_owned())
    );
    assert_eq!(
        account.selected_order_strategy().map(|s| s.name.as_str()),
        Some("third"),
        "the selection followed its strategy down a slot"
    );
    assert_eq!(
        account.remove_order_strategy(1).map(|s| s.name),
        Some("third".to_owned())
    );
    assert_eq!(account.selected_strategy(), None, "no neighbour is armed");
    assert!(
        account.remove_order_strategy(5).is_none(),
        "an index past the end is nothing"
    );
}

/// The venue the host hands over is the one the account trades on.
#[test]
fn the_account_trades_on_the_venue_it_is_handed() {
    let dir = ScratchDir::new("account-with-venue");
    let mut seeded = Simulator::new();
    seeded.seed(&print(0, 250));
    let account = PaperAccount::with_venue(dir.path().to_path_buf(), Box::new(seeded));
    assert_eq!(
        account.mark_price(),
        Some(Decimal::from(250)),
        "the mark came from the handed simulator, not a fresh one"
    );
}

#[test]
fn reset_without_a_closed_event_does_not_create_or_erase_a_journal_hint() {
    let dir = ScratchDir::new("reset-no-close-hint");
    let mut account = account(&dir, "RESET");
    account.seed(&print(0, 100));
    let empty = account.reset_timeline();
    assert!(!empty.had_position && !empty.had_orders && empty.all_saved);
    assert!(!account.take_journal_changed());
    account.seed(&print(0, 100));
    ticket_market(&mut account, Side::Buy, &plain_form());
    let orders = account.reset_timeline();
    assert!(!orders.had_position && orders.had_orders && orders.all_saved);
    assert!(!account.take_journal_changed());
    account.seed(&print(0, 100));
    ticket_market(&mut account, Side::Buy, &plain_form());
    account.on_trade(&print(1, 100));
    let events = account.dispatch(Command::ClosePosition);
    account.handle_events(events);
    account.on_trade(&print(2, 105));
    let no_close = account.reset_timeline();
    assert!(!no_close.had_position && no_close.all_saved);
    assert!(
        account.take_journal_changed(),
        "the earlier close remains pending"
    );
    assert!(!account.take_journal_changed());
}

#[test]
fn a_failed_reset_journal_invalidates_but_never_claims_a_saved_row() {
    let dir = ScratchDir::new("reset-failed-journal");
    let mut account = account(&dir, "RESET");
    // A file where the per-symbol directory must go fails before any append.
    std::fs::write(dir.path().join("RESET"), b"not a directory").unwrap();
    account.seed(&print(0, 100));
    ticket_market(&mut account, Side::Buy, &plain_form());
    account.on_trade(&print(1, 100));
    account.on_trade(&print(2, 105));
    let reset = account.reset_timeline();
    assert!(reset.had_position && !reset.all_saved);
    assert!(account.is_flat());
    assert_eq!(account.session_trades().len(), 1, "the venue really closed");
    assert!(
        crate::report::load_history(dir.path(), Some("RESET"), &[])
            .rows
            .is_empty()
    );
    assert!(
        account.take_journal_changed(),
        "attempted close invalidates uncertain disk state"
    );
    assert!(!account.take_journal_changed());
    assert!(account.peek_toast().unwrap().contains("could not save"));
    assert!(account.journal_path().is_none());
}

#[test]
fn an_unnamed_reset_close_invalidates_without_inventing_persistence_or_io_warning() {
    let dir = ScratchDir::new("reset-unnamed");
    let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());
    account.seed(&print(0, 100));
    ticket_market(&mut account, Side::Buy, &plain_form());
    account.on_trade(&print(1, 100));
    account.take_toast();
    let reset = account.reset_timeline();
    assert!(reset.had_position && !reset.all_saved);
    assert!(account.take_journal_changed());
    assert!(!account.take_journal_changed());
    assert!(
        account.peek_toast().is_none(),
        "missing symbol is not an IO warning"
    );
    assert!(
        crate::report::load_history(dir.path(), None, &[])
            .rows
            .is_empty()
    );
}

/// Rest a buy limit at `price` under a market marked at 100.
fn rest_limit(account: &mut PaperAccount, price: i64) -> quantick_trading::OrderId {
    let events = account
        .try_place_intent(OrderIntent::limit(
            Side::Buy,
            Decimal::ONE,
            Decimal::from(price),
        ))
        .expect("the lock lets a one-lot limit through");
    match events.as_slice() {
        [VenueEvent::Placed(order)] => order.id,
        other => panic!("expected one placement, got {other:?}"),
    }
}

/// A cancel removes the named order and says so with a user cancellation.
#[test]
fn cancelling_an_order_removes_it_and_tells_the_host_why() {
    let dir = ScratchDir::new("account-cancel-order");
    let mut account = account(&dir, "CNCLX");
    account.seed(&print(0, 100));
    let first = rest_limit(&mut account, 95);
    let second = rest_limit(&mut account, 90);

    let events = account.cancel_order(first);

    assert!(
        matches!(
            events.as_slice(),
            [VenueEvent::Cancelled { order, reason: quantick_trading::CancelReason::User }]
                if order.id == first
        ),
        "one user cancellation of the named order: {events:?}"
    );
    let ids: Vec<_> = account.working_orders().iter().map(|o| o.id).collect();
    assert_eq!(ids, vec![second], "only the other order is left");
}

/// A reprice moves the price and nothing else: same id, same size, and the
/// venue says it was updated rather than replaced.
#[test]
fn amending_an_order_price_moves_it_in_place() {
    let dir = ScratchDir::new("account-amend-order");
    let mut account = account(&dir, "AMNDX");
    account.seed(&print(0, 100));
    let first = rest_limit(&mut account, 95);
    let second = rest_limit(&mut account, 90);
    let before = account.working_orders().to_vec();

    let events = account.amend_order_price(first, Decimal::from(97));

    assert!(
        matches!(events.as_slice(), [VenueEvent::Updated(order)] if order.id == first),
        "one update of the named order: {events:?}"
    );
    let after = account.working_orders();
    assert_eq!(after.len(), 2, "no order was added or removed");
    let moved = after.iter().find(|o| o.id == first).expect("same id");
    assert_eq!(moved.price, Some(Decimal::from(97)), "the price moved");
    assert_eq!(moved.quantity, before[0].quantity, "the size did not");
    let other = after.iter().find(|o| o.id == second).expect("neighbour");
    assert_eq!(*other, before[1], "the other order is untouched");
}

/// A reprice the venue refuses leaves the order where it was.
#[test]
fn a_refused_reprice_leaves_the_order_resting() {
    let dir = ScratchDir::new("account-amend-refused");
    let mut account = account(&dir, "AMNRX");
    account.seed(&print(0, 100));
    let id = rest_limit(&mut account, 95);

    // A buy limit above the market would fill at once, so it is not a
    // resting price.
    let events = account.amend_order_price(id, Decimal::from(105));

    assert!(
        matches!(events.as_slice(), [VenueEvent::Rejected(_)]),
        "the venue refuses: {events:?}"
    );
    assert_eq!(
        account.working_orders()[0].price,
        Some(Decimal::from(95)),
        "and the order stays at its price"
    );
}

/// The same tape traded three times — each run ended by the timeline reset a
/// replay restart sends — lands in three session files, never one: the venue
/// stamp repeats, so the second and third runs take `.rerun-N` names.
#[test]
fn three_runs_of_one_tape_are_three_session_files() {
    let dir = ScratchDir::new("account-three-runs");
    let mut account = account(&dir, "RERUN");
    account.set_session_source(history::SessionSource::Replay);
    for _ in 0..3 {
        account.seed(&print(0, 100));
        ticket_market(&mut account, Side::Buy, &plain_form());
        account.on_trade(&print(1, 100));
        account.on_trade(&print(2, 103));
        account.reset_timeline();
    }
    let paths = account.session_journal_paths().to_vec();
    assert_eq!(paths.len(), 3, "{paths:?}");
    let names: Vec<String> = paths
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(names[1].ends_with(".rerun-1.csv"), "{names:?}");
    assert!(names[2].ends_with(".rerun-2.csv"), "{names:?}");
    let files: Vec<_> = account
        .session_trade_at()
        .iter()
        .map(|at| at.as_ref().map(|at| at.session.path(account.trades_dir())))
        .collect();
    assert_eq!(
        files,
        paths.iter().cloned().map(Some).collect::<Vec<_>>(),
        "one trade per file"
    );
    let rows = account.session_history_rows();
    let keys: Vec<_> = rows.iter().map(|row| row.session.clone()).collect();
    assert!(keys.iter().all(Option::is_some));
    assert_eq!(
        keys.iter().collect::<std::collections::BTreeSet<_>>().len(),
        3
    );
}

/// Two closes with the same times — the partial exits of one position —
/// are two trades of the file. The live record and a fresh read of the file
/// must name them alike, or a tag set on one lands on the other.
#[test]
fn same_time_closes_are_journaled_as_distinct_trades_of_their_file() {
    let dir = ScratchDir::new("account-same-time-closes");
    let mut account = account(&dir, "PARTIAL");
    let close = take_profit_close();
    assert!(account.journal(&close));
    assert!(account.journal(&close));
    let at: Vec<_> = account
        .session_trade_at()
        .iter()
        .map(|at| at.clone().expect("journaled"))
        .collect();
    assert_eq!((at[0].trade.nth, at[1].trade.nth), (0, 1));
    assert_eq!(at[0].session, at[1].session);
    let loaded = crate::report::load_history(account.trades_dir(), None, &[]);
    let refs: Vec<_> = loaded.rows.iter().map(|row| row.trade_ref).collect();
    assert_eq!(refs, vec![at[0].trade, at[1].trade]);
}

/// One take-profit close at fixed venue times.
fn take_profit_close() -> quantick_trading::ClosedTrade {
    quantick_trading::ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(103),
        opened_ms: 1_773_666_000_000,
        closed_ms: 1_773_666_060_000,
        pnl_points: Decimal::from(3),
        exit_reason: quantick_trading::ExitReason::TakeProfit,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: None,
        mfe_points: None,
    }
}

/// A close appended into a file that already holds rows — where the
/// rerun-limit fallback lands, here with a row that does not parse — is
/// counted the way a reader counts that file, not from the account's own
/// writes alone, or its tags land on another trade.
#[test]
fn an_append_into_an_existing_file_counts_its_trades_like_the_reader() {
    let dir = ScratchDir::new("account-append-existing");
    let mut account = account(&dir, "APPEND");
    let close = take_profit_close();
    let folder = dir.path().join("APPEND");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("20260316-130100.csv");
    let mut text = history::write_header("APPEND", history::SessionSource::Live);
    text.push_str(&history::write_trade(&close));
    text.push_str("not,a,trade\n");
    std::fs::write(&path, text).unwrap();
    account.journal_path = Some(path);
    assert!(account.journal(&close));
    let at = account.session_trade_at()[0].clone().expect("journaled");
    let loaded = crate::report::load_history(account.trades_dir(), None, &[]);
    assert_eq!(loaded.problem_rows, 1, "the bad row is counted, not parsed");
    let refs: Vec<_> = loaded.rows.iter().map(|row| row.trade_ref).collect();
    assert_eq!(refs.len(), 2);
    assert_eq!(at.trade, refs[1], "the writer counts the reader's way");
    assert_eq!(at.trade.nth, 1);
}

/// An MT5 suffix symbol journals under a folder ending in a dot (`US30.`,
/// as `sanitize_symbol` keeps it). Its trades still name their session, and
/// the ledger scoped to that symbol names the same one.
#[test]
fn a_suffix_symbols_trades_are_keyed_to_their_session() {
    let dir = ScratchDir::new("account-suffix-symbol");
    let mut account = account(&dir, "US30.");
    assert!(account.journal(&take_profit_close()));
    let at = account.session_trade_at()[0]
        .clone()
        .expect("keyed to its file");
    assert!(at.session.as_str().starts_with("US30./"), "{}", at.session);
    let loaded = crate::report::load_history(account.trades_dir(), Some("US30."), &[]);
    assert_eq!(loaded.rows[0].session.as_ref(), Some(&at.session));
    assert_eq!(loaded.rows[0].trade_ref, at.trade);
}

/// A reverse opens a position, so the risk lock measures the leg it opens
/// like any entry: refused with the entry's own sentence while the lock
/// stands, nothing reaching the venue, and let through once it is lifted.
#[test]
fn the_risk_lock_refuses_a_reverse_it_would_refuse_as_an_entry() {
    let dir = ScratchDir::new("account-reverse-lock");
    let mut account = account(&dir, "WIN$N");
    account.seed(&print(0, 100));
    account.place_intent(OrderIntent::market(Side::Buy, Decimal::from(1000)));
    account.on_trade(&print(1, 100));
    assert_eq!(account.venue().position().map(|p| p.side), Some(Side::Buy));
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
    account.set_risk_settings(risk.clone());
    // A short of 1000 stopped 99 points above is far past a 1 BRL budget.
    let stop = Bracket::whole(Some(Decimal::from(199)), None);

    account.reverse_position(stop);
    account.on_trade(&print(2, 100));
    let position = account.venue().position().expect("still open");
    assert_eq!(
        (position.side, position.quantity),
        (Side::Buy, Decimal::from(1000)),
        "the refused flip never reached the venue"
    );
    let toast = account.peek_toast().expect("the refusal is said");
    assert!(toast.starts_with("SIM: "), "{toast}");

    risk.lock = false;
    account.set_risk_settings(risk);
    account.reverse_position(stop);
    account.on_trade(&print(3, 100));
    let position = account.venue().position().expect("flipped");
    assert_eq!(
        (position.side, position.quantity),
        (Side::Sell, Decimal::from(1000)),
        "unlocked, the same flip goes through"
    );
}

/// The position card's stop and target read what the position would at
/// each level, in the same words as the open P&L beside them.
#[test]
fn the_brackets_read_what_the_position_would_at_each_level() {
    let dir = ScratchDir::new("account-bracket-pnl");
    let mut account = account(&dir, "WINZ26");
    account.seed(&print(0, 100));
    assert_eq!(account.bracket_pnl(), [None, None], "flat: no legs");
    let mut book = InstrumentBook::new();
    book.insert(
        "WINZ26".to_owned(),
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
    let intent = OrderIntent::market(Side::Buy, Decimal::from(2)).with_bracket(Bracket::whole(
        Some(Decimal::from(90)),
        Some(Decimal::from(120)),
    ));
    let events = account.place_intent(intent);
    account.handle_events(events);
    account.on_trade(&print(1, 100));
    let [stop, target] = account.bracket_pnl();
    assert_eq!(
        stop,
        Some((Decimal::from(90), "-4.00 BRL · -10 pts × 2".to_owned())),
        "ten points against two contracts at 0.20 a point"
    );
    assert_eq!(
        target,
        Some((Decimal::from(120), "+8.00 BRL · +20 pts × 2".to_owned()))
    );
}

/// Every feed id a tab could carry, and none.
const TAB_FEEDS: [&str; 4] = ["metatrader-b3", "binance", "hyperliquid", ""];

fn shipped_settings() -> crate::pnl::PnlSettings {
    crate::pnl::PnlSettings::load(
        include_str!("../../../app/config/instruments.toml"),
        None,
        None,
    )
}

#[test]
fn the_account_reads_the_shipped_catalog_whatever_the_tab_feed() {
    let dir = ScratchDir::new("catalog-feed");
    let mut account = account(&dir, "BTCUSDT");
    account.set_pnl_settings(shipped_settings());
    for feed in TAB_FEEDS {
        account.set_feed(feed);
        let money = account.symbol_money().cloned().expect("BTCUSDT");
        assert_eq!(
            money.size_step,
            Decimal::new(1, 5),
            "the ticket steps 0.00001"
        );
        assert_eq!(
            money.currency.code(),
            "USDT",
            "a risk amount is stamped USDT"
        );
        let seeded = account.risk_editor().seeded;
        assert_eq!(seeded.as_ref(), Some(&money));
        assert!(
            crate::risk_sizing::money_note(seeded.as_ref()).contains("from instruments.toml"),
            "the risk block names where the value came from"
        );
    }
    // A WIN replay opened on a tab whose live feed is Binance still reads
    // the mini index's money; Binance's WINUSDT never does.
    account.set_feed("binance");
    account.set_symbol("WINV26");
    let win = account
        .symbol_money()
        .cloned()
        .expect("WINV26 on a binance tab");
    assert_eq!(win.point_value, Decimal::new(20, 2));
    assert_eq!(win.currency.code(), "BRL");
    for symbol in ["WINUSDT", "DOLOUSDT"] {
        account.set_symbol(symbol);
        assert_eq!(account.symbol_money(), None, "{symbol} is no B3 contract");
    }
    assert!(crate::risk_sizing::money_note(None).contains("No feed reports"));
}

/// A saved trade is worth what it is worth: the ledger row, the total and
/// the CSV export read the same money from every tab.
#[test]
fn a_saved_trade_prices_identically_from_any_tab() {
    let dir = ScratchDir::new("catalog-any-tab");
    let mut account = account(&dir, "BTCUSDT");
    account.set_pnl_settings(shipped_settings());
    let trade = ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::from(2),
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(300),
        opened_ms: 1_773_666_000_000,
        closed_ms: 1_773_666_060_000,
        pnl_points: Decimal::from(400),
        exit_reason: quantick_trading::ExitReason::Manual,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: None,
        mfe_points: None,
    };
    let rows = vec![HistoryRow {
        symbol: "WINV26".to_owned(),
        source: None,
        trade: trade.clone(),
        session: None,
        trade_ref: history::TradeRef::default(),
        tags: Vec::new(),
    }];
    let read = |account: &PaperAccount| {
        let reader = account.pnl_reader();
        (
            reader.trade("WINV26", trade.pnl_points, trade.quantity),
            reader.total(rows.iter().map(|row| (row.symbol.as_str(), &row.trade))),
            export_csv(&rows, &account.pnl_book()),
        )
    };
    account.set_feed("metatrader-b3");
    let on_b3 = read(&account);
    assert!(on_b3.0.contains("80.00 BRL"), "{}", on_b3.0);
    assert!(
        on_b3
            .2
            .lines()
            .nth(1)
            .is_some_and(|line| line.ends_with(",200,80,BRL")),
        "{}",
        on_b3.2
    );
    for feed in TAB_FEEDS {
        account.set_feed(feed);
        assert_eq!(read(&account), on_b3, "read from a {feed:?} tab");
    }
}

/// The close toast words the trade in the figure the ticket leads with —
/// the trader's chosen mode, its honest second figure and their units —
/// never a bare points total that ignores the switch.
#[test]
fn the_close_toast_reads_in_the_chosen_figure() {
    let dir = ScratchDir::new("close-toast-pnl");
    let mut account = account(&dir, "WINZ26");
    account.set_pnl_settings(shipped_settings());
    let close_two = |account: &mut PaperAccount, first: u64| {
        account.seed(&print(first, 100));
        let env = AccountEnv {
            ruler_levels: None,
            form: TicketForm {
                quantity: Ok(Decimal::from(2)),
                offsets: Some((None, None)),
            },
        };
        account.market(Side::Buy, Decimal::from(100), Bracket::none(), &env);
        account.on_trade(&print(first + 1, 100));
        let events = account.dispatch(Command::ClosePosition);
        account.handle_events(events);
        account.on_trade(&print(first + 2, 105));
        account.take_toast().expect("the close is said")
    };
    account.set_pnl_primary(crate::pnl::PnlPrimary::PointsPerContract);
    assert_eq!(
        close_two(&mut account, 0),
        "SIM closed: LONG 2 for +5 pts × 2 · +2.00 BRL (manual)",
        "pts/ct leads, money keeps it honest"
    );
    account.set_pnl_primary(crate::pnl::PnlPrimary::Money);
    assert_eq!(
        close_two(&mut account, 10),
        "SIM closed: LONG 2 for +2.00 BRL · +5 pts × 2 (manual)"
    );
}

/// An account with a scratch folder and no symbol named: the state a host
/// is in before its tab says which market it shows.
fn unnamed(dir: &ScratchDir) -> PaperAccount {
    PaperAccount::with_trades_dir(dir.path().to_path_buf())
}

fn decimal_print(agg_id: u64, price: Decimal) -> Trade {
    Trade {
        price,
        ..print(agg_id, 0)
    }
}

/// A leg on a working order is judged against the order's own price, not
/// the market's: a buy limit at 95 takes no stop at 96, and the refusal
/// names the leg.
#[test]
fn a_working_orders_leg_is_judged_against_the_order_not_the_market() {
    let dir = ScratchDir::new("account-order-leg");
    let mut account = account(&dir, "LEGX");
    account.seed(&print(0, 100));
    let id = rest_limit(&mut account, 95);

    account.amend_leg(
        quantick_trading::BracketTarget::Order(id),
        Leg::StopLoss,
        Some(Decimal::from(96)),
    );
    assert_eq!(account.working_orders()[0].bracket.stop_loss(), None);
    let toast = account.peek_toast().expect("the refusal teaches");
    assert!(toast.contains("stop loss"), "{toast}");
}

/// A selection naming a strategy the list no longer carries arms nothing
/// rather than a different ladder.
#[test]
fn a_selection_naming_a_missing_strategy_selects_nothing() {
    let dir = ScratchDir::new("account-missing-strategy");
    let mut account = account(&dir, "STRATX");
    account.set_order_strategies(
        vec![OrderStrategy::starter(0)],
        Some("a strategy that was deleted"),
    );
    assert!(account.selected_order_strategy().is_none());
    assert_eq!(account.order_strategies().len(), 1, "the list is intact");
}

/// The tick is the finest step the tape has shown: trailing zeros are
/// decoration, a rounder print never coarsens it, and a symbol switch
/// forgets it until the new market prints.
#[test]
fn the_tick_is_the_finest_step_the_tape_has_shown() {
    let dir = ScratchDir::new("account-tick");
    let mut account = account(&dir, "BTCUSDT");
    account.seed(&decimal_print(0, Decimal::new(7_811_257_000_000, 8)));
    assert_eq!(account.tick(), Decimal::new(1, 2), "two places, not eight");
    account.on_trade(&print(1, 78_100));
    assert_eq!(account.tick(), Decimal::new(1, 2), "never coarser");
    account.on_trade(&decimal_print(2, Decimal::new(78_100_123, 3)));
    assert_eq!(
        account.tick(),
        Decimal::new(1, 3),
        "a finer print refines it"
    );

    account.set_symbol("WIN$N");
    assert_eq!(
        account.tick(),
        Decimal::ONE,
        "the old precision stays behind"
    );
    account.on_trade(&decimal_print(9, Decimal::new(1_380_055, 1)));
    assert_eq!(account.tick(), Decimal::new(1, 1));
}

/// Prices snap to the precision the instrument has shown, and a round
/// print does not take cents away.
#[test]
fn snapping_uses_the_instruments_learned_precision() {
    let dir = ScratchDir::new("account-snap");
    let mut cents = unnamed(&dir);
    cents.seed(&decimal_print(1, Decimal::new(10_325, 2)));
    assert_eq!(cents.snap(101.234_56), Decimal::new(10_123, 2));
    cents.on_trade(&print(2, 103));
    assert_eq!(cents.snap(101.234_56), Decimal::new(10_123, 2));

    let mut whole = unnamed(&dir);
    whole.seed(&print(1, 182_035));
    assert_eq!(whole.snap(182_036.7), Decimal::from(182_037));
}

/// The ruler's default notch scales with the instrument, so the first
/// roll feels right with nothing configured.
#[test]
fn the_default_ruler_step_scales_with_the_instrument() {
    let dir = ScratchDir::new("account-ruler-step");
    let mut btc = unnamed(&dir);
    btc.seed(&decimal_print(0, Decimal::new(7_800_057, 2)));
    assert_eq!(btc.tick(), Decimal::new(1, 2));
    assert_eq!(btc.derived_ruler_step(), Decimal::from(5));

    let mut win = unnamed(&dir);
    win.seed(&print(0, 138_000));
    win.on_trade(&print(1, 138_005));
    assert_eq!(win.derived_ruler_step(), Decimal::from(10), "two ticks");
}

/// The ticket's offsets become a bracket around the reference, mirrored
/// for a sell, and one unreadable box stands the whole armed bracket down
/// rather than projecting the other leg alone.
#[test]
fn one_unreadable_offset_stands_the_whole_bracket_down() {
    let dir = ScratchDir::new("account-offsets");
    let mut account = unnamed(&dir);
    account.seed(&print(0, 100));
    let reference = Decimal::from(100);
    let mut ticket = crate::desk::Ticket {
        stop_offset_text: "2".to_owned(),
        profit_offset_text: "5".to_owned(),
        ..crate::desk::Ticket::default()
    };
    let sell = ticket.parse_bracket(Side::Sell, reference).expect("read");
    assert_eq!(sell.stop_loss(), Some(Decimal::from(102)));
    assert_eq!(sell.take_profit(), Some(Decimal::from(95)));
    let env = |ticket: &crate::desk::Ticket| AccountEnv {
        ruler_levels: None,
        form: ticket.form(),
    };
    let both = account.armed_bracket(Side::Buy, reference, Decimal::ONE, &env(&ticket));
    assert_eq!(both.stop_loss(), Some(Decimal::from(98)));
    assert_eq!(both.take_profit(), Some(Decimal::from(105)));

    ticket.stop_offset_text = "abc".to_owned();
    let spoiled = account.armed_bracket(Side::Buy, reference, Decimal::ONE, &env(&ticket));
    assert_eq!(spoiled.stop_loss(), None);
    assert_eq!(spoiled.take_profit(), None, "the readable target goes too");
}

/// The status cell answers "am I in a trade?", and the close button names
/// the position it exits.
#[test]
fn the_status_cell_and_the_close_button_name_the_position() {
    let dir = ScratchDir::new("account-status");
    let mut account = unnamed(&dir);
    assert!(account.status_cell().is_none(), "untouched owes no line");
    account.seed(&print(0, 100));
    ticket_market(&mut account, Side::Buy, &plain_form());
    account.on_trade(&print(1, 100));
    let (text, _) = account.status_cell().expect("open");
    assert_eq!(text, "SIM LONG 1 · 0 pts × 1 · — set pt value");
    account.on_trade(&print(2, 105));
    let (text, sign) = account.status_cell().expect("open");
    assert_eq!(text, "SIM LONG 1 · +5 pts × 1 · — set pt value");
    assert_eq!(sign, std::cmp::Ordering::Greater);

    account.close_position();
    account.on_trade(&print(3, 107));
    let (text, sign) = account.status_cell().expect("history keeps it");
    assert_eq!(text, "SIM flat · +7 pts/ct · — set pt value");
    assert_eq!(sign, std::cmp::Ordering::Greater);
    assert!(account.close_button_label().is_none(), "flat has no close");

    let three = TicketForm {
        quantity: Ok(Decimal::from(3)),
        offsets: Some((None, None)),
    };
    ticket_market(&mut account, Side::Sell, &three);
    account.on_trade(&print(4, 100));
    assert_eq!(
        account.close_button_label().as_deref(),
        Some("Close 3 SHORT")
    );
    let summary = account.position_summary().expect("open");
    assert_eq!(summary.side, Side::Sell);
    assert_eq!(summary.quantity, Decimal::from(3));
    assert_eq!(summary.avg_price, Decimal::from(100));
}

/// A fresh account - a restart - opens its own file beside every earlier
/// one and never touches them: the same replayed tape takes a rerun name,
/// a later session its own stamp.
#[test]
fn a_fresh_account_never_writes_into_an_earlier_sessions_file() {
    let dir = ScratchDir::new("account-fresh-hosts");
    let round_trip = |first: u64, price: i64| {
        let mut account = account(&dir, "FRESH");
        account.set_session_source(history::SessionSource::Replay);
        account.seed(&print(first, price));
        ticket_market(&mut account, Side::Buy, &plain_form());
        account.on_trade(&print(first + 1, price));
        let events = account.dispatch(Command::ClosePosition);
        account.handle_events(events);
        account.on_trade(&print(first + 2, price + 3));
    };
    let names = || {
        let mut names: Vec<String> = std::fs::read_dir(dir.path().join("FRESH"))
            .expect("the symbol folder exists")
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
            .collect();
        names.sort();
        names
    };

    round_trip(0, 100);
    let first = dir.path().join("FRESH").join(&names()[0]);
    let first_bytes = std::fs::read(&first).expect("readable");
    round_trip(0, 100);
    assert!(names()[1].contains(".rerun-1."), "{:?}", names());
    round_trip(10_000, 200);
    assert_eq!(names().len(), 3, "{:?}", names());
    assert_eq!(std::fs::read(&first).expect("readable"), first_bytes);
    let loaded = crate::report::load_history(dir.path(), Some("FRESH"), &[]);
    assert_eq!(loaded.rows.len(), 3);
    assert!(
        loaded
            .rows
            .iter()
            .all(|row| row.source == Some(history::SessionSource::Replay))
    );
}

/// The saved history is scoped by the symbol folders on disk: the combo
/// lists every traded symbol, and a scope reads only its own folder.
#[test]
fn the_saved_history_scopes_by_symbol_folder() {
    let dir = ScratchDir::new("account-symbol-folders");
    for (symbol, first, price) in [("AAAUSDT", 0, 100), ("BBBUSDT", 100, 200)] {
        let mut account = account(&dir, symbol);
        account.seed(&print(first, price));
        ticket_market(&mut account, Side::Buy, &plain_form());
        account.on_trade(&print(first + 1, price));
        let events = account.dispatch(Command::ClosePosition);
        account.handle_events(events);
        account.on_trade(&print(first + 2, price + 5));
    }
    assert_eq!(
        crate::report::list_symbol_folders(dir.path()),
        vec!["AAAUSDT".to_owned(), "BBBUSDT".to_owned()]
    );
    let all = crate::report::load_history(dir.path(), None, &[]);
    let symbols: Vec<&str> = all.rows.iter().map(|row| row.symbol.as_str()).collect();
    assert_eq!(symbols, vec!["AAAUSDT", "BBBUSDT"]);
    let one = crate::report::load_history(dir.path(), Some("BBBUSDT"), &[]);
    assert_eq!(one.rows.len(), 1);
    assert_eq!(one.rows[0].symbol, "BBBUSDT");
}
