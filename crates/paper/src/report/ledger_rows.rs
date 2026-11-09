//! Virtual ledger rows resolved before a renderer lays them out.
use super::sessions::{
    SessionKey, SessionLabel, SessionRow, TradeRef, clock_minute, clock_second,
    group_by_day_and_session, place_session_rows, source_word,
};
use super::snapshot::OpenRow;
use crate::format::{fmt_decimal, fmt_duration_ms, position_word};
use crate::pnl::SymbolPnl;
use quantick_civil::{CivilDate, TzOffset};
use quantick_engine::Side;
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

/// What a trade row writes, decided here so a test reads it: the side and
/// size, the round trip, the instrument where the list mixes them, the
/// source, the P&L pair and the detail line. The day's card above carries
/// the date, so no row repeats it.
pub struct TradeWords<'a> {
    pub side: Side,
    pub head: String,
    pub route: String,
    pub symbol: Option<&'a str>,
    pub source: Option<SessionSource>,
    /// The leading figure, written on line one, and the one that keeps it
    /// honest, on line two.
    pub pnl: (String, String),
    pub detail: String,
}

impl<'a> TradeWords<'a> {
    /// A closed trade's row, its tags closing the detail line.
    #[must_use]
    pub fn closed(
        trade: &ClosedTrade,
        meta: TradeMeta<'_>,
        symbol: Option<&'a str>,
        tz: TzOffset,
    ) -> Self {
        let mut detail = trade_detail(trade, tz);
        if !meta.tags.is_empty() {
            detail.push_str(&format!(" · #{}", meta.tags.join(" #")));
        }
        Self {
            side: trade.side,
            head: format!(
                "{} {}",
                position_word(trade.side),
                fmt_decimal(trade.quantity)
            ),
            route: format!(
                "{} → {}",
                fmt_decimal(trade.entry_price),
                fmt_decimal(trade.exit_price)
            ),
            symbol,
            source: meta.source,
            pnl: meta.pnl.closed_split(trade),
            detail,
        }
    }

    /// The open position's row: the current mark as its provisional exit,
    /// and the open profit as the ticket words it.
    #[must_use]
    pub fn open(open: &OpenRow, symbol: Option<&'a str>) -> Self {
        let summary = &open.summary;
        let exit = open.mark_price.map_or_else(|| "…".to_owned(), fmt_decimal);
        let held = open
            .held_ms
            .map_or_else(String::new, |ms| format!("open {}", fmt_duration_ms(ms)));
        Self {
            side: summary.side,
            head: format!(
                "{} {}",
                position_word(summary.side),
                fmt_decimal(summary.quantity)
            ),
            route: format!("{} → {}", fmt_decimal(summary.avg_price), exit),
            symbol,
            source: None,
            pnl: (open.pnl.clone().unwrap_or_default(), String::new()),
            detail: format!("{held} · at the last print"),
        }
    }
}

/// The detail line under a ledger row: the closing clock, how long the
/// trade was held, and why it ended - every character spent on the reason.
#[must_use]
pub fn trade_detail(trade: &ClosedTrade, tz: TzOffset) -> String {
    format!(
        "{} · {} · {}",
        clock_second(trade.closed_ms, tz),
        fmt_duration_ms(trade.closed_ms.saturating_sub(trade.opened_ms)),
        trade.exit_reason.as_str().replace('_', " "),
    )
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
    /// What the row writes: `Live · 12:16–13:56`, from its first entry to
    /// its last exit that day ([`session_title`]).
    pub title: String,
    pub source: Option<SessionSource>,
    pub count: usize,
    /// The leading figure alone; the day above carries the pair.
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
                    let label = view.labels.get(key);
                    let stem = label.map_or(source_word(first.meta.source), |label| &label.stem);
                    let trades = session.items.iter().map(|&index| items[index].trade);
                    let primary = first.meta.pnl.reader.word_split(&session.pnl).0;
                    rows.push(LedgerRow::SessionHead(SessionHead {
                        key: key.clone(),
                        label: label.map_or_else(|| stem.to_owned(), |label| label.text.clone()),
                        title: session_title(stem, trades, view.tz),
                        source: first.meta.source,
                        count: session.count,
                        net: (session.tint, primary),
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

/// `Live · 12:16–13:56`: a session's name, then its first entry and its
/// last exit among `trades`, in the display timezone.
pub fn session_title<'t>(
    stem: &str,
    trades: impl IntoIterator<Item = &'t ClosedTrade>,
    tz: TzOffset,
) -> String {
    let span = trades
        .into_iter()
        .fold(None, |span: Option<(i64, i64)>, trade| {
            Some(
                span.map_or((trade.opened_ms, trade.closed_ms), |(from, to)| {
                    (from.min(trade.opened_ms), to.max(trade.closed_ms))
                }),
            )
        });
    match span {
        Some((from, to)) => format!(
            "{stem} · {}\u{2013}{}",
            clock_minute(from, tz),
            clock_minute(to, tz)
        ),
        None => stem.to_owned(),
    }
}

/// Where a row stands in one container, a day's card or a session's box:
/// its first row opens the container, its last closes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub first: bool,
    pub last: bool,
}

/// The containers that hold one row, and where it stands in each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowNest {
    pub day: Option<Span>,
    pub session: Option<Span>,
}

/// What a row is to the containers around it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// A caption or control between days.
    Outside,
    /// A day's header: it opens the day's card.
    Day,
    /// A session's header or its inline edit: it opens the session's box.
    Session,
    /// A trade inside its session's box.
    InSession,
    /// A trade no session file holds, loose in its day's card.
    Loose,
}

impl Place {
    fn of(row: &LedgerRow<'_>) -> Self {
        match row {
            LedgerRow::Header(..) | LedgerRow::HiddenLive(_) | LedgerRow::More(_) => Self::Outside,
            LedgerRow::Day(..) => Self::Day,
            LedgerRow::SessionHead(_) | LedgerRow::Inline => Self::Session,
            LedgerRow::Session(_, _, meta) | LedgerRow::Earlier(_, _, meta) => {
                if meta.session.is_some() {
                    Self::InSession
                } else {
                    Self::Loose
                }
            }
        }
    }
}

/// Each row's place in its day's card and its session's box: a day runs
/// from its header to the next caption or day, a session from its header
/// to the next session, loose trade or day. Pure, so the painter and a test
/// read the same nesting.
pub fn nesting(rows: &[LedgerRow<'_>]) -> Vec<RowNest> {
    let places: Vec<Place> = rows.iter().map(Place::of).collect();
    places
        .iter()
        .enumerate()
        .map(|(index, place)| {
            let next = places.get(index + 1).copied().unwrap_or(Place::Outside);
            let day = Span {
                first: *place == Place::Day,
                last: matches!(next, Place::Outside | Place::Day),
            };
            let session = Span {
                first: *place == Place::Session,
                last: next != Place::InSession,
            };
            match place {
                Place::Outside => RowNest::default(),
                Place::Day | Place::Loose => RowNest {
                    day: Some(day),
                    session: None,
                },
                Place::Session | Place::InSession => RowNest {
                    day: Some(day),
                    session: Some(session),
                },
            }
        })
        .collect()
}

/// Which saved history the ledger lists. Three cases, not two: following
/// the chart is what the panel opens on, but a trader reviewing yesterday
/// wants to name an instrument without retuning the chart to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerScope {
    /// Whatever the chart is showing — the panel's default, and the only
    /// one that moves when the chart does.
    Chart,
    /// One named instrument, whatever the chart shows.
    Symbol(String),
    /// Every instrument in the folder, mixed into one timeline.
    All,
}

impl LedgerScope {
    /// The symbol folder to read, or `None` for the whole folder.
    #[must_use]
    pub fn folder<'a>(&'a self, chart: &'a str) -> Option<&'a str> {
        match self {
            Self::Chart => Some(chart),
            Self::Symbol(symbol) => Some(symbol.as_str()),
            Self::All => None,
        }
    }

    /// What the picker shows for this scope.
    #[must_use]
    pub fn label(&self, chart: &str) -> String {
        match self {
            Self::Chart if chart.is_empty() => "This chart".to_owned(),
            Self::Chart => format!("This chart · {chart}"),
            Self::Symbol(symbol) => symbol.clone(),
            Self::All => "All symbols".to_owned(),
        }
    }

    /// Whether a trade row names its instrument: only where the list mixes
    /// them - every symbol, or a named one beside this session's trades on
    /// the `chart`'s. Under "This chart" the picker already says which.
    #[must_use]
    pub fn names_symbol(&self, chart: &str) -> bool {
        match self {
            Self::Chart => false,
            Self::Symbol(symbol) => symbol != chart,
            Self::All => true,
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

    /// The trader's day: two Live sessions in an open day, then a folded
    /// day. Each session's header names its span from first entry to last
    /// exit and its leading figure; every row knows its card and its box.
    #[test]
    fn two_sessions_nest_in_their_day_card_and_a_folded_day_closes_its_own() {
        use crate::report::sessions::{SessionKey, SessionLabel};
        let tz = TzOffset::new(0);
        let day = CivilDate::from_ymd(2026, 11, 9).start_ms(tz);
        let minute = 60_000;
        let at = |opened: i64, closed: i64, pnl| ClosedTrade {
            opened_ms: day + opened * minute,
            ..trade_at(day + closed * minute, pnl)
        };
        let early = SessionKey::parse("WINZ26/early.csv").unwrap();
        let late = SessionKey::parse("WINZ26/late.csv").unwrap();
        // Newest first: the late session, the early one, then a day before.
        let trades = [
            at(14 * 60 + 57, 15 * 60 + 7, 44),
            at(13 * 60 + 17, 13 * 60 + 56, -277),
            at(12 * 60 + 16, 12 * 60 + 19, -55),
            at(-600, -590, 5),
        ];
        let keys = [&late, &early, &early, &early];
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
        let mut labels = std::collections::BTreeMap::new();
        for key in [&early, &late] {
            let label = SessionLabel {
                stem: "Live".to_owned(),
                ..SessionLabel::new("Live · 12:19".to_owned())
            };
            labels.insert(key.clone(), label);
        }
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

        let span = |first, last| Some(Span { first, last });
        let nests = nesting(&rows);
        assert_eq!(nests.len(), rows.len());
        let expected = [
            RowNest::default(), // caption
            RowNest {
                day: span(true, false),
                session: None,
            }, // Mon 09
            RowNest {
                day: span(false, false),
                session: span(true, false),
            }, // late
            RowNest {
                day: span(false, false),
                session: span(false, true),
            },
            RowNest {
                day: span(false, false),
                session: span(true, false),
            }, // early
            RowNest {
                day: span(false, false),
                session: span(false, false),
            },
            RowNest {
                day: span(false, true),
                session: span(false, true),
            },
            RowNest {
                day: span(true, true),
                session: None,
            }, // folded Sun 08
            RowNest::default(), // show older
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
        let words = TradeWords::closed(&trade, meta, None, tz);
        assert_eq!(words.head, "SHORT 2");
        assert_eq!(words.route, "100 → -177");
        assert_eq!(words.symbol, None);
        assert_eq!(words.detail, "13:17:43 · 38m 34s · manual · #fade");
        assert_eq!(words.pnl, meta.pnl.closed_split(&trade));
        assert_eq!(
            format!("{} · {}", words.pnl.0, words.pnl.1),
            meta.pnl.closed(&trade),
            "the same pair the row used to write on one line"
        );
    }

    #[test]
    fn a_trade_row_names_its_instrument_only_where_the_list_mixes_them() {
        assert!(!LedgerScope::Chart.names_symbol("WINZ26"));
        assert!(!LedgerScope::Symbol("WINZ26".to_owned()).names_symbol("WINZ26"));
        assert!(LedgerScope::Symbol("WDOZ26".to_owned()).names_symbol("WINZ26"));
        assert!(LedgerScope::All.names_symbol("WINZ26"));
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
        assert_eq!(
            nests[2].day,
            Some(Span {
                first: false,
                last: true
            })
        );
        assert_eq!(session_title("Replay", std::iter::empty(), tz), "Replay");
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
