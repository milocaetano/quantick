//! Virtual ledger rows resolved before a renderer lays them out.
use super::sessions::{
    SessionKey, SessionLabel, SessionRow, TradeRef, group_by_day_and_session, place_session_rows,
    source_word,
};
use crate::pnl::SymbolPnl;
use quantick_civil::{CivilDate, TzOffset};
use quantick_sim::{ClosedTrade, history::SessionSource};
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet};
/// One virtualised ledger line.
pub enum LedgerRow<'a> {
    /// A group caption and its count.
    Header(&'static str, usize),
    /// A civil day's caption: the date, how many of the rows under it
    /// closed on that day, what they netted, and whether it is folded
    /// shut. A ledger of bare clock times cannot answer "which session was
    /// that" — the day header is where the answer lives, it carries the
    /// day's result for free, and clicking it folds the day away.
    Day(CivilDate, usize, (Decimal, String), bool),
    /// A session's own row inside its day — every session a file holds.
    SessionHead(SessionHead),
    /// The rename or delete row standing where its session's row was.
    Inline,
    /// A closed trade from this session's simulator: selectable, and its
    /// round trip is on the current tape.
    Session(usize, &'a ClosedTrade, TradeMeta<'a>),
    /// A row loaded from an earlier session's journal. Selectable for its
    /// tags; its tape is not the one on screen, so it never navigates.
    Earlier(&'a str, &'a ClosedTrade, TradeMeta<'a>),
    /// This session's trades the filter hides — said, never dropped.
    HiddenLive(usize),
    /// The control that reveals the next page of saved history, carrying
    /// how many trades are still held back.
    More(usize),
}

/// What the session layer knows of one trade row.
#[derive(Clone, Copy)]
pub struct TradeMeta<'a> {
    /// The session file it is in; `None` when no file under the trades
    /// folder took it.
    pub session: Option<&'a SessionKey>,
    /// Which trade of that file it is.
    pub trade_ref: TradeRef,
    pub source: Option<SessionSource>,
    pub tags: &'a [String],
    /// How its P&L is worded, bound to its own symbol.
    pub pnl: SymbolPnl<'a>,
}

/// One trade before it is cut into rows.
pub struct Item<'a> {
    pub trade: &'a ClosedTrade,
    pub symbol: &'a str,
    /// Its index among this session's closed trades; `None` for a saved row.
    pub live: Option<usize>,
    pub meta: TradeMeta<'a>,
}

/// How the list folds and shows sessions while rows are cut.
pub struct GroupView<'s> {
    pub collapsed_days: &'s BTreeSet<i64>,
    pub collapsed_sessions: &'s BTreeSet<SessionKey>,
    /// The session F2 and Del act on.
    pub focused: Option<&'s SessionKey>,
    /// The session whose row an inline rename or delete replaces.
    pub editing: Option<&'s SessionKey>,
    pub labels: &'s BTreeMap<SessionKey, SessionLabel>,
    /// Sessions a tab still journals to.
    pub live: &'s BTreeSet<SessionKey>,
    pub tz: TzOffset,
}

/// A session's row as the list paints it.
pub struct SessionHead {
    pub key: SessionKey,
    pub label: String,
    pub source: Option<SessionSource>,
    pub count: usize,
    pub net: (Decimal, String),
    pub folded: bool,
    pub focused: bool,
    /// A tab is still journaling to it: delete is refused.
    pub live: bool,
}

/// Cut `items` (newest first) into rows: a header per civil day, then each
/// session's own row where [`place_session_rows`] gives it one, then the
/// trades. `inline_placed` carries the one inline editor across the lists
/// of one ledger.
pub fn push_groups<'a>(
    rows: &mut Vec<LedgerRow<'a>>,
    items: &'a [Item<'a>],
    view: &GroupView<'_>,
    inline_placed: &mut bool,
) {
    let mut days = group_by_day_and_session(
        items
            .iter()
            .map(|item| (item.meta.session, item.meta.pnl, item.trade)),
        view.tz,
    );
    place_session_rows(&mut days, view.editing, view.collapsed_days, inline_placed);
    for day in days {
        let folded = view.collapsed_days.contains(&day.day.day_number());
        rows.push(LedgerRow::Day(
            day.day,
            day.count,
            (day.tint, day.net_text),
            folded,
        ));
        // A folded day builds no trade rows at all — the header keeps the
        // date, the count and the net, so the day is summarised rather
        // than merely hidden, and the frame does not pay for what it does
        // not show.
        if folded {
            continue;
        }
        for session in day.sessions {
            let first = &items[session.items[0]];
            if session.row != SessionRow::Bare
                && let Some(key) = &session.key
            {
                if session.row == SessionRow::Inline {
                    rows.push(LedgerRow::Inline);
                } else {
                    rows.push(LedgerRow::SessionHead(SessionHead {
                        key: key.clone(),
                        label: view.labels.get(key).map_or_else(
                            || source_word(first.meta.source).to_owned(),
                            |label| label.text.clone(),
                        ),
                        source: first.meta.source,
                        count: session.count,
                        net: (session.tint, session.net_text.clone()),
                        folded: view.collapsed_sessions.contains(key),
                        focused: view.focused == Some(key),
                        live: view.live.contains(key),
                    }));
                }
                if view.collapsed_sessions.contains(key) {
                    continue;
                }
            }
            rows.extend(session.items.iter().map(|&index| {
                let item = &items[index];
                match item.live {
                    Some(live) => LedgerRow::Session(live, item.trade, item.meta),
                    None => LedgerRow::Earlier(item.symbol, item.trade, item.meta),
                }
            }));
        }
    }
}

pub const LEDGER_PAGE_TRADES: usize = 50;
/// How much saved history the ledger is showing and how much it is
/// holding back. Pure so the count printed on the "show older" control and
/// the rows above it can never disagree — a button promising trades that
/// are not there is worse than no button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerPage {
    /// Saved trades rendered, newest first.
    pub shown: usize,
    /// Saved trades loaded but not yet revealed.
    pub remaining: usize,
}

impl LedgerPage {
    /// `pages` pages of [`LEDGER_PAGE_TRADES`] each, clamped to `total`.
    /// Page zero is treated as one: the ledger always shows something.
    pub fn of(total: usize, pages: usize) -> Self {
        let shown = LEDGER_PAGE_TRADES.saturating_mul(pages.max(1)).min(total);
        Self {
            shown,
            remaining: total.saturating_sub(shown),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pnl::PnlReader;
    use crate::report::readout::LedgerTotals;
    use quantick_engine::Side;
    use quantick_sim::ExitReason;
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
        let items: Vec<(&str, &ClosedTrade)> =
            saved.iter().map(|trade| ("WINV26", trade)).collect();

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
}
