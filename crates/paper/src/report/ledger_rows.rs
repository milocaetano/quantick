//! Virtual ledger rows resolved before a renderer lays them out.
use super::LoadedHistory;
use super::fit::TradeRowText;
use super::ledger_state::LiveTrade;
use super::sessions::{
    SessionKey, SessionLabel, SessionRow, TradeRef, clock_second, group_by_day_and_session,
    place_session_rows, session_title, source_word, span_of,
};
use super::snapshot::OpenRow;
use crate::format::{fmt_decimal, fmt_duration_ms, position_word, sanitize_symbol};
use crate::pnl::{PnlReader, SymbolPnl};
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
/// size, the round trip, the instrument where it is not the chart's
/// ([`shown_symbol`]), the source, the P&L pair and the detail line's
/// facts. The day's card above carries the date, so no row repeats it.
pub struct TradeWords<'a> {
    pub side: Side,
    pub head: String,
    pub route: String,
    pub symbol: Option<&'a str>,
    pub source: Option<SessionSource>,
    /// The leading figure, written on line one, and the one that keeps it
    /// honest, on line two ([`SymbolPnl::row_split`]).
    pub pnl: (String, String),
    /// Whether the trade won, lost or came out flat: the sign of its move;
    /// `None` while the position is open, its move not settled.
    pub result: Option<std::cmp::Ordering>,
    /// The detail line's facts: the closing clock, how long it was held,
    /// and why it ended with its tags ([`trade_detail_parts`]).
    pub detail: [String; 3],
}

impl<'a> TradeWords<'a> {
    /// A closed trade of `instrument`'s row on the `chart`'s list, its tags
    /// closing the detail line.
    #[must_use]
    pub fn closed(
        trade: &ClosedTrade,
        meta: TradeMeta<'_>,
        (instrument, chart): (&'a str, &str),
        tz: TzOffset,
    ) -> Self {
        let mut detail = trade_detail_parts(trade, tz);
        if !meta.tags.is_empty() {
            detail[2].push_str(&format!(" · #{}", meta.tags.join(" #")));
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
            symbol: shown_symbol(instrument, chart),
            source: meta.source,
            pnl: meta.pnl.closed_split(trade),
            result: Some(trade.pnl_points.cmp(&Decimal::ZERO)),
            detail,
        }
    }

    /// The open position's row on the `chart`'s list: the current mark as
    /// its provisional exit, its open profit worded by `pnl` (the account's
    /// instrument) as a closed row's is, and no figure before a mark.
    #[must_use]
    pub fn open(open: &OpenRow, pnl: SymbolPnl<'a>, chart: &str) -> Self {
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
            symbol: shown_symbol(pnl.symbol, chart),
            source: Some(open.source),
            pnl: summary
                .open_points
                .map(|points| pnl.row_split(points, summary.quantity))
                .unwrap_or_default(),
            result: None,
            detail: [String::new(), held, "at the last print".to_owned()],
        }
    }

    /// The row's words as [`super::fit::fit_trade_row`] fits them, after the source
    /// `mark` the painter draws.
    #[must_use]
    pub fn row_text<'s>(&'s self, mark: &'s str) -> TradeRowText<'s> {
        let [clock, held, reason] = &self.detail;
        TradeRowText {
            head: &self.head,
            route: &self.route,
            lead: &self.pnl.0,
            mark,
            symbol: self.symbol.unwrap_or_default(),
            clock,
            held,
            reason,
            behind: &self.pnl.1,
        }
    }
}

/// The instrument a row of `instrument` names on the `chart`'s list: none
/// where it is the chart's own, compared as the journal's folders spell
/// them ([`sanitize_symbol`]), so a row of another market always says
/// which and a row of this one never repeats it.
#[must_use]
pub fn shown_symbol<'a>(instrument: &'a str, chart: &str) -> Option<&'a str> {
    (sanitize_symbol(instrument) != sanitize_symbol(chart)).then_some(instrument)
}

/// The detail line under a ledger row: the closing clock, how long the
/// trade was held, and why it ended - every character spent on the reason.
#[must_use]
pub fn trade_detail(trade: &ClosedTrade, tz: TzOffset) -> String {
    trade_detail_parts(trade, tz).join(" · ")
}

/// [`trade_detail`]'s facts apart, so a narrow row cuts the reason and
/// drops the duration while the clock stays.
#[must_use]
pub fn trade_detail_parts(trade: &ClosedTrade, tz: TzOffset) -> [String; 3] {
    [
        clock_second(trade.closed_ms, tz),
        fmt_duration_ms(trade.closed_ms.saturating_sub(trade.opened_ms)),
        trade.exit_reason.as_str().replace('_', " "),
    ]
}

/// One trade before it is cut into rows.
pub struct Item<'a> {
    pub trade: &'a ClosedTrade,
    pub symbol: &'a str,
    /// Its index among this session's closed trades; `None` for a saved row.
    pub live: Option<usize>,
    pub meta: TradeMeta<'a>,
}

impl<'a> Item<'a> {
    /// This session's close as a list item on the chart's `symbol`.
    #[must_use]
    pub fn live(live: LiveTrade<'a>, symbol: &'a str, pnl: SymbolPnl<'a>) -> Self {
        Self {
            trade: live.trade,
            symbol,
            live: Some(live.index),
            meta: TradeMeta {
                session: live.at.map(|at| &at.session),
                trade_ref: live.at.map_or_else(TradeRef::default, |at| at.trade),
                source: live.source,
                tags: live.tags,
                pnl,
            },
        }
    }
}

/// The revealed page of saved rows, newest first, as list items: only the
/// `shown` newest of the `kept` rows are walked, so a year of sessions costs
/// the same per frame as a week of them.
#[must_use]
pub fn saved_items<'a>(
    cache: Option<&'a LoadedHistory>,
    kept: &[usize],
    shown: usize,
    pnl: PnlReader<'a>,
) -> Vec<Item<'a>> {
    let Some(cache) = cache else {
        return Vec::new();
    };
    let item = |row: &'a super::HistoryRow| Item {
        trade: &row.trade,
        symbol: &row.symbol,
        live: None,
        meta: TradeMeta {
            session: row.session.as_ref(),
            trade_ref: row.trade_ref,
            source: row.source,
            tags: &row.tags,
            pnl: pnl.of(&row.symbol),
        },
    };
    kept.iter()
        .rev()
        .take(shown)
        .map(|&index| item(&cache.rows[index]))
        .collect()
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
    pub day: CivilDate,
    pub key: SessionKey,
    /// What the row writes, and what the search, the delete confirm and the
    /// control plane call it: its label, `Live · 12:16–13:56`, from its
    /// first entry to its last exit ([`session_title`]).
    pub title: String,
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
                    // The label the cut gave it; a session no cut named
                    // yet is titled the same way from what is listed.
                    let title = view.labels.get(key).map_or_else(
                        || {
                            let trades = session.items.iter().map(|&index| items[index].trade);
                            let stem = source_word(first.meta.source);
                            session_title(stem, span_of(trades), view.tz)
                        },
                        |label| label.text.clone(),
                    );
                    rows.push(LedgerRow::SessionHead(SessionHead {
                        day: day.day,
                        key: key.clone(),
                        title,
                        count: session.count,
                        net: (session.tint, session.lead.clone()),
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

/// The list's rows, newest first, and whether an inline edit row was placed.
#[must_use]
pub fn ledger_rows<'a>(
    live: &'a [Item<'a>],
    hidden_live: usize,
    saved: &'a [Item<'a>],
    kept: usize,
    remaining: usize,
    view: &GroupView<'_>,
) -> (Vec<LedgerRow<'a>>, bool) {
    let mut rows = Vec::new();
    let mut inline_placed = false;
    if !live.is_empty() || hidden_live > 0 {
        rows.push(LedgerRow::Header("THIS SESSION", live.len()));
        if hidden_live > 0 {
            rows.push(LedgerRow::HiddenLive(hidden_live));
        }
        push_groups(&mut rows, live, view, &mut inline_placed);
    }
    if !saved.is_empty() {
        rows.push(LedgerRow::Header("EARLIER SESSIONS", kept));
        push_groups(&mut rows, saved, view, &mut inline_placed);
    }
    if remaining > 0 {
        rows.push(LedgerRow::More(remaining));
    }
    (rows, inline_placed)
}

/// The containers that hold one row, and where it stands in each as
/// `[opens, closes]`: a container's first row opens it, its last closes it
/// (the shape `appearance::paper::ledger_frame` paints from).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowNest {
    pub day: Option<[bool; 2]>,
    pub session: Option<[bool; 2]>,
    /// The list's ordinary spacing follows the row: it stands outside every
    /// card, or it closes the last card before a caption or the list's end.
    pub spaced: bool,
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
            let day = [
                *place == Place::Day,
                matches!(next, Place::Outside | Place::Day),
            ];
            let session = [*place == Place::Session, next != Place::InSession];
            let spaced = next == Place::Outside;
            match place {
                Place::Outside => RowNest {
                    spaced: true,
                    ..RowNest::default()
                },
                Place::Day | Place::Loose => RowNest {
                    day: Some(day),
                    session: None,
                    spaced,
                },
                Place::Session | Place::InSession => RowNest {
                    day: Some(day),
                    session: Some(session),
                    spaced,
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
    /// A scripted launch's scope: `chart`, `all`, or a symbol folder name;
    /// blank names none.
    #[must_use]
    pub fn from_hook(value: &str) -> Option<Self> {
        match value.trim() {
            "chart" => Some(Self::Chart),
            "all" => Some(Self::All),
            "" => None,
            symbol => Some(Self::Symbol(symbol.to_owned())),
        }
    }

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
#[path = "ledger_rows_tests.rs"]
mod tests;
