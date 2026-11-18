use super::*;
use crate::report::readout::LedgerTotals;
use quantick_engine::Side;
use quantick_sim::ExitReason;

#[test]
fn a_hook_scope_reads_chart_all_or_a_symbol_and_blank_is_none() {
    assert_eq!(LedgerScope::from_hook(" chart "), Some(LedgerScope::Chart));
    assert_eq!(LedgerScope::from_hook("all"), Some(LedgerScope::All));
    let symbol = LedgerScope::Symbol("WINZ25".to_owned());
    assert_eq!(LedgerScope::from_hook("WINZ25"), Some(symbol));
    assert_eq!(LedgerScope::from_hook("  "), None);
}

fn trade_at(closed_ms: i64, pnl: i64) -> ClosedTrade {
    ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(100 + pnl),
        opened_ms: closed_ms - 1000,
        closed_ms,
        pnl_points: Decimal::from(pnl),
        exit_reason: ExitReason::Manual,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: None,
        mfe_points: None,
    }
}
fn plain_items<'a>(items: &[(&'a str, &'a ClosedTrade)]) -> Vec<Item<'a>> {
    items
        .iter()
        .map(|&(symbol, trade)| Item {
            trade,
            symbol,
            live: None,
            meta: TradeMeta {
                session: None,
                trade_ref: Default::default(),
                source: None,
                tags: &[],
                pnl: PnlReader::plain().of(symbol),
            },
        })
        .collect()
}
fn push_plain<'a>(
    rows: &mut Vec<LedgerRow<'a>>,
    items: &'a [Item<'a>],
    tz: TzOffset,
    collapsed_days: &std::collections::BTreeSet<i64>,
) {
    let sessions = std::collections::BTreeSet::new();
    let labels = std::collections::BTreeMap::new();
    let view = GroupView {
        collapsed_days,
        collapsed_sessions: &sessions,
        focused: None,
        editing: None,
        labels: &labels,
        live: &sessions,
        tz,
    };
    push_groups(rows, items, &view, &mut false);
}
#[test]
fn the_ledger_builds_a_bounded_number_of_rows_however_deep_the_history() {
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 8, 17);
    // Five thousand saved trades over fifty days.
    let saved: Vec<ClosedTrade> = (0..5_000)
        .map(|index| {
            trade_at(
                day.offset_days(-(index / 100)).start_ms(tz) + (index % 100) * 60_000,
                1,
            )
        })
        .collect();
    let items: Vec<(&str, &ClosedTrade)> = saved.iter().map(|trade| ("WINV26", trade)).collect();

    let page = LedgerPage::of(items.len(), 1);
    assert_eq!(page.shown, LEDGER_PAGE_TRADES);
    assert_eq!(page.remaining, 4_950, "and the control says so out loud");
    let mut rows = Vec::new();
    let plain = plain_items(&items[..page.shown]);
    push_plain(&mut rows, &plain, tz, &std::collections::BTreeSet::new());
    // Fifty trades plus at most one day caption each — nowhere near
    // the five thousand rows the pre-paging ledger would have built.
    assert!(
        rows.len() <= 2 * LEDGER_PAGE_TRADES,
        "{} rows for a page of {LEDGER_PAGE_TRADES}",
        rows.len()
    );
    // Revealing pages grows the list by one page at a time, never all
    // at once.
    let deeper = LedgerPage::of(items.len(), 2);
    assert_eq!(deeper.shown - page.shown, LEDGER_PAGE_TRADES);

    // And the strip under the list is summed with the load, not on the
    // frame: the totals are a stored value, so the frame reads them
    // rather than walking five thousand trades to print one line.
    let totals = LedgerTotals::of_trades(PnlReader::plain(), "X", saved.iter());
    assert_eq!(totals.trades, 5_000);
    assert_eq!(totals.wins, 5_000, "every fixture trade is a winner");
    assert_eq!(totals.net.grouped(), Decimal::from(5_000));
    assert_eq!(totals.win_rate(), Some(100));
    // Nothing saved plus this session's own trades still adds up.
    assert_eq!(
        LedgerTotals::default().plus(&totals),
        totals,
        "an empty half must be the identity"
    );
    assert_eq!(LedgerTotals::default().win_rate(), None, "0/0 is not 0%");
}
#[test]
fn ledger_rows_open_a_new_day_caption_when_the_day_changes() {
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 8, 17);
    // Newest first, the order the ledger cuts in.
    let items = [
        (
            "WINV26",
            trade_at(day.offset_days(1).start_ms(tz) + 3_600_000, 8),
        ),
        ("WINV26", trade_at(day.end_ms(tz) - 1, -25)),
        ("WINV26", trade_at(day.start_ms(tz) + 60_000, 139)),
    ];
    let items: Vec<(&str, &ClosedTrade)> = items
        .iter()
        .map(|(symbol, trade)| (*symbol, trade))
        .collect();
    let mut rows = Vec::new();
    let plain = plain_items(&items);
    push_plain(&mut rows, &plain, tz, &std::collections::BTreeSet::new());
    assert_eq!(rows.len(), 5, "two day captions over three trades");
    match &rows[0] {
        LedgerRow::Day(date, count, net, folded) => {
            assert_eq!(*date, day.offset_days(1));
            assert_eq!(*count, 1);
            assert_eq!(net.0, Decimal::from(8));
            assert!(!*folded);
        }
        _ => panic!("the list opens on a day caption"),
    }
    assert!(matches!(rows[1], LedgerRow::Earlier(..)));
    match &rows[2] {
        LedgerRow::Day(date, count, net, _) => {
            assert_eq!(*date, day);
            assert_eq!(*count, 2, "both of the 17th's trades");
            assert_eq!(net.0, Decimal::from(114), "and what the day netted");
        }
        _ => panic!("the next day opens its own caption"),
    }
    assert!(matches!(rows[3], LedgerRow::Earlier(..)));
    assert!(matches!(rows[4], LedgerRow::Earlier(..)));

    // Folded, the 17th keeps its caption — with its count and its net
    // intact — and contributes no trade rows at all.
    let mut folded_days = std::collections::BTreeSet::new();
    folded_days.insert(day.day_number());
    let mut rows = Vec::new();
    push_plain(&mut rows, &plain, tz, &folded_days);
    assert_eq!(rows.len(), 3, "two captions and only the open day's row");
    match &rows[2] {
        LedgerRow::Day(date, count, net, folded) => {
            assert_eq!(*date, day);
            assert!(*folded, "and it says it is folded");
            assert_eq!(*count, 2, "a folded day still counts its trades");
            assert_eq!(net.0, Decimal::from(114), "and still states its net");
        }
        _ => panic!("a folded day keeps its caption"),
    }
}
#[test]
fn an_inline_edit_row_stands_once_however_many_days_its_session_spans() {
    use crate::report::sessions::SessionKey;
    let tz = TzOffset::new(0);
    let day = 86_400_000_i64;
    let a = SessionKey::parse("X/a.csv").unwrap();
    let b = SessionKey::parse("X/b.csv").unwrap();
    let trades = [
        trade_at(2 * day + 10, 1),
        trade_at(2 * day + 5, 2),
        trade_at(day + 10, 3),
        trade_at(day + 5, 4),
    ];
    let items: Vec<Item<'_>> = trades
        .iter()
        .zip([&a, &b, &a, &b])
        .map(|(trade, key)| Item {
            trade,
            symbol: "X",
            live: None,
            meta: TradeMeta {
                session: Some(key),
                trade_ref: Default::default(),
                source: None,
                tags: &[],
                pnl: PnlReader::plain().of("X"),
            },
        })
        .collect();
    let none = std::collections::BTreeSet::new();
    let labels = std::collections::BTreeMap::new();
    let view = GroupView {
        collapsed_days: &none,
        collapsed_sessions: &std::collections::BTreeSet::new(),
        focused: None,
        editing: Some(&a),
        labels: &labels,
        live: &std::collections::BTreeSet::new(),
        tz,
    };
    let mut rows = Vec::new();
    push_groups(&mut rows, &items, &view, &mut false);
    let count = |pick: fn(&LedgerRow<'_>) -> bool| rows.iter().filter(|row| pick(row)).count();
    assert_eq!(count(|row| matches!(row, LedgerRow::Inline)), 1);
    assert_eq!(count(|row| matches!(row, LedgerRow::SessionHead(_))), 3);
    assert_eq!(count(|row| matches!(row, LedgerRow::Day(..))), 2);
}

/// The trader's day: two Live sessions in an open day, then a folded
/// day. Each session's header names its span from first entry to last
/// exit and its leading figure; every row knows its card and its box.
#[test]
fn two_sessions_nest_in_their_day_card_and_a_folded_day_closes_its_own() {
    use crate::report::sessions::SessionKey;
    let tz = TzOffset::new(0);
    let day = CivilDate::from_ymd(2026, 11, 9).start_ms(tz);
    let minute = 60_000;
    let at = |opened: i64, closed: i64, pnl| ClosedTrade {
        opened_ms: day + opened * minute,
        ..trade_at(day + closed * minute, pnl)
    };
    let early = SessionKey::parse("WINZ26/early.csv").unwrap();
    let late = SessionKey::parse("WINZ26/late.csv").unwrap();
    let older = SessionKey::parse("WINZ26/older.csv").unwrap();
    // Newest first: the late session, the early one, then a day before.
    let trades = [
        at(14 * 60 + 57, 15 * 60 + 7, 44),
        at(13 * 60 + 17, 13 * 60 + 56, -277),
        at(12 * 60 + 16, 12 * 60 + 19, -55),
        at(-600, -590, 5),
    ];
    let keys = [&late, &early, &early, &older];
    let items: Vec<Item<'_>> = trades
        .iter()
        .zip(keys)
        .map(|(trade, key)| Item {
            trade,
            symbol: "WINZ26",
            live: None,
            meta: TradeMeta {
                session: Some(key),
                trade_ref: Default::default(),
                source: Some(SessionSource::Live),
                tags: &[],
                pnl: PnlReader::plain().of("WINZ26"),
            },
        })
        .collect();
    // The labels the cut builds: one per session, from its trades.
    let labels = crate::report::sessions::session_labels(
        None,
        items
            .iter()
            .map(|item| (item.meta.session.unwrap(), item.meta.source, item.trade)),
        tz,
    );
    let folded: std::collections::BTreeSet<i64> =
        [CivilDate::from_ms(day - 1, tz).day_number()].into();
    let none = std::collections::BTreeSet::new();
    let view = GroupView {
        collapsed_days: &folded,
        collapsed_sessions: &none,
        focused: None,
        editing: None,
        labels: &labels,
        live: &none,
        tz,
    };
    let mut rows = vec![LedgerRow::Header("EARLIER SESSIONS", 4)];
    push_groups(&mut rows, &items, &view, &mut false);
    rows.push(LedgerRow::More(3));

    let titles: Vec<(&str, &str)> = rows
        .iter()
        .filter_map(|row| match row {
            LedgerRow::SessionHead(head) => Some((head.title.as_str(), head.net.1.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        titles,
        [
            ("Live · 14:57–15:07", "+44 pts/ct"),
            ("Live · 12:16–13:56", "-332 pts/ct")
        ],
        "start to last exit, and the leading figure alone"
    );
    assert_eq!(
        titles[1].0, labels[&early].text,
        "the header writes the label the search and the delete confirm read"
    );

    let ends = |opens, closes| Some([opens, closes]);
    let row = |day, session, spaced| RowNest {
        day,
        session,
        spaced,
    };
    let nests = nesting(&rows);
    assert_eq!(nests.len(), rows.len());
    let expected = [
        row(None, None, true),                             // caption
        row(ends(true, false), None, false),               // Mon 09
        row(ends(false, false), ends(true, false), false), // late
        row(ends(false, false), ends(false, true), false),
        row(ends(false, false), ends(true, false), false), // early
        row(ends(false, false), ends(false, false), false),
        row(ends(false, true), ends(false, true), false),
        row(ends(true, true), None, true), // folded Sun 08, the last card
        row(None, None, true),             // show older
    ];
    assert_eq!(nests, expected);
}

/// A trade row writes its side, round trip and clock; its figures
/// apart, one per line; no date, which its day's card carries.
#[test]
fn a_trade_row_writes_its_figures_apart_and_leaves_the_date_to_its_card() {
    let tz = TzOffset::new(0);
    let mut trade = trade_at(
        CivilDate::from_ymd(2026, 11, 9).start_ms(tz) + 47_863_000,
        -277,
    );
    trade.side = Side::Sell;
    trade.quantity = Decimal::TWO;
    trade.opened_ms = trade.closed_ms - 2_314_000;
    let tags = ["fade".to_owned()];
    let meta = TradeMeta {
        session: None,
        trade_ref: Default::default(),
        source: None,
        tags: &tags,
        pnl: PnlReader::plain().of("WINZ26"),
    };
    let words = TradeWords::closed(&trade, meta, ("WINZ26", "WINZ26"), tz);
    assert_eq!(words.head, "SHORT 2");
    assert_eq!(words.route, "100 → -177");
    assert_eq!(words.symbol, None);
    assert_eq!(words.detail, ["13:17:43", "38m 34s", "manual · #fade"]);
    assert_eq!(
        words.pnl,
        ("-138.5 pts ×2".to_owned(), "— set pt value".to_owned()),
        "the row's own form: the quantity tight against its sign"
    );
    assert_eq!(
        meta.pnl.closed(&trade),
        "-138.5 pts × 2 · — set pt value",
        "the one-line readout is cut from the same split"
    );
}

/// A row names its market only where it is not the chart's, compared
/// as the journal's folders spell them, whatever the list's scope.
#[test]
fn a_trade_row_names_its_instrument_only_where_it_is_not_the_charts() {
    assert_eq!(shown_symbol("WINZ26", "WINZ26"), None);
    assert_eq!(shown_symbol("WDOZ26", "WINZ26"), Some("WDOZ26"));
    assert_eq!(
        shown_symbol("WIN_N", "WIN@N"),
        None,
        "the folder's spelling of the chart's own market"
    );
    assert_eq!(shown_symbol("WINZ26", ""), Some("WINZ26"));
}

/// The pinned position is this session's: it says Live or Replay, never
/// the unrecorded `?`, and lays its figures out like a closed row.
#[test]
fn the_open_row_names_its_source_and_splits_its_figures() {
    use crate::format::PositionSummary;
    let open = OpenRow {
        summary: PositionSummary {
            side: Side::Buy,
            quantity: Decimal::TWO,
            avg_price: Decimal::from(100),
            open_points: Some(Decimal::from(30)),
        },
        mark_price: Some(Decimal::from(115)),
        held_ms: Some(90_000),
        source: SessionSource::Replay,
    };
    let pnl = PnlReader::plain().of("WINZ26");
    let words = TradeWords::open(&open, pnl, "WINZ26");
    assert_eq!(words.source, Some(SessionSource::Replay));
    assert_eq!(words.symbol, None);
    assert_eq!(words.route, "100 → 115");
    assert_eq!(words.pnl, pnl.row_split(Decimal::from(30), Decimal::TWO));
    assert_eq!(words.pnl.0, "+15 pts ×2");

    // Before a mark there is no figure to write.
    let unmarked = OpenRow {
        summary: PositionSummary {
            open_points: None,
            ..open.summary
        },
        ..open
    };
    let words = TradeWords::open(&unmarked, pnl, "WDOZ26");
    assert_eq!(words.pnl, (String::new(), String::new()));
    assert_eq!(
        words.symbol,
        Some("WINZ26"),
        "another chart's position names it"
    );
}

#[test]
fn a_trade_outside_any_session_sits_loose_in_its_card() {
    let tz = TzOffset::new(0);
    let trades = [trade_at(10 * 60_000, 3), trade_at(5 * 60_000, -1)];
    let items: Vec<(&str, &ClosedTrade)> = trades.iter().map(|trade| ("X", trade)).collect();
    let plain = plain_items(&items);
    let mut rows = Vec::new();
    push_plain(&mut rows, &plain, tz, &std::collections::BTreeSet::new());
    let nests = nesting(&rows);
    assert_eq!(nests[1].session, None, "no file, no box");
    assert_eq!(nests[2].day, Some([false, true]));
    assert!(
        nests[2].spaced,
        "the list's last card leaves a gap under it"
    );
    assert_eq!(session_title("Replay", None, tz), "Replay");
}

#[test]
fn the_ledger_reveals_saved_history_one_page_at_a_time() {
    let page = LedgerPage::of(120, 1);
    assert_eq!(page.shown, LEDGER_PAGE_TRADES);
    assert_eq!(page.remaining, 120 - LEDGER_PAGE_TRADES);
    let page = LedgerPage::of(120, 2);
    assert_eq!(page.shown, 2 * LEDGER_PAGE_TRADES);
    assert_eq!(page.remaining, 120 - 2 * LEDGER_PAGE_TRADES);
    // The last page shows the tail and offers nothing more.
    let page = LedgerPage::of(120, 3);
    assert_eq!(page.shown, 120);
    assert_eq!(page.remaining, 0);
    let page = LedgerPage::of(120, 99);
    assert_eq!(page.shown, 120, "extra pages cannot invent trades");
    assert_eq!(page.remaining, 0);
    // A short history fits in one page and never offers "show older".
    let page = LedgerPage::of(7, 1);
    assert_eq!((page.shown, page.remaining), (7, 0));
    // Page zero is treated as one: the ledger always shows something.
    assert_eq!(LedgerPage::of(7, 0), LedgerPage::of(7, 1));
    assert_eq!(
        LedgerPage::of(0, 1),
        LedgerPage {
            shown: 0,
            remaining: 0
        }
    );
}
