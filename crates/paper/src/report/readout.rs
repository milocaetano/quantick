//! The report's numbers worded in the figure the trader reads P&L by.
//!
//! [`ReportView::cut`] aggregates in stored points, the journal's own unit.
//! [`ReportView::with_pnl`] words that cut through a [`PnlReader`]: each
//! row, the net and the equity walk lead with money, points per contract or
//! the grouped figure, so the window paints a finished string and cannot
//! disagree with the ticket. [`LedgerTotals`] is the ledger strip's sum, in
//! all three figures at once, so a mode switch never re-reads the folder.
//!
//! A walk is never plotted across currencies, nor in money a trade cannot
//! express: it falls back to the grouped points and says why — or, where
//! those points are worth different things, plots nothing and says so.

use quantick_trading::ClosedTrade;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::{HistoryRow, ReportView};
use crate::pnl::{PnlKey, PnlPrimary, PnlReader, PnlTotal, SymbolPnl, fmt_amount};

/// Why a walk is in grouped points when money was asked for.
const UNSET_FALLBACK: &str = "pt value unset";

/// The realized-equity walk `E_0..E_n` (`E_0 = 0` before the first trade),
/// in the two shapes its two readers need: exact values for the trade
/// list's running total, and plot-ready `f32` with its bounds for the
/// curve. Computed once when the view is cut — a window holding a year of
/// trades must not walk them again sixty times a second.
pub struct EquityWalk {
    /// `n + 1` exact running totals, in [`Self::primary`].
    pub points: Vec<Decimal>,
    /// The same walk as the curve plots it.
    pub plot: Vec<f32>,
    pub low: f32,
    pub high: f32,
    /// The figure the walk is in.
    pub primary: PnlPrimary,
    /// Its unit, for the axis: `pts`, `pts/ct`, a currency code.
    pub unit: String,
    /// Why the walk is not in money although money was asked for.
    pub fallback: Option<&'static str>,
    /// The deepest drawdown of this walk: peak index, trough index, depth.
    pub drawdown: Option<(usize, usize, Decimal)>,
    /// The instrument a points walk measures, named when other walks
    /// share the report; empty otherwise.
    pub label: String,
}

/// The report's walks: one per unit that adds up — one currency in money,
/// one instrument in points — and which walk each trade belongs to. Never
/// empty.
pub struct EquityWalks {
    walks: Vec<EquityWalk>,
    /// Per row, the index of its walk.
    of_row: Vec<usize>,
}

/// What one row's figure adds up with.
#[derive(PartialEq, Eq)]
struct UnitKey {
    primary: PnlPrimary,
    fallback: Option<&'static str>,
    /// The currency code in money; the price unit's suffix in points.
    unit: String,
    /// The instrument a points figure measures; empty in money.
    instrument: String,
}

impl UnitKey {
    fn of(row: &HistoryRow, pnl: PnlReader<'_>, chosen: PnlPrimary) -> Self {
        let units = pnl.book.units(&row.symbol);
        match (chosen, units.money) {
            (PnlPrimary::Money, Some(money)) => Self {
                primary: PnlPrimary::Money,
                fallback: None,
                unit: money.currency.code().to_string(),
                instrument: String::new(),
            },
            _ => Self {
                primary: match chosen {
                    PnlPrimary::Money => PnlPrimary::PointsTimesQty,
                    points => points,
                },
                fallback: (chosen == PnlPrimary::Money).then_some(UNSET_FALLBACK),
                unit: units.unit_suffix(),
                instrument: units.family.unwrap_or(&row.symbol).to_owned(),
            },
        }
    }
}

impl EquityWalks {
    /// One walk over every row.
    #[must_use]
    pub fn single(walk: EquityWalk) -> Self {
        Self {
            of_row: vec![0; walk.points.len().saturating_sub(1)],
            walks: vec![walk],
        }
    }

    /// The walks of `rows` in the figure the trader reads P&L by. Each
    /// spans every row, flat where another unit's trade closes, so trade
    /// `k` sits at the same x on every curve; a sum across units is never
    /// drawn. Money asked of a trade with no point value walks its points
    /// and says why. `total` is `pnl`'s sum of the same rows.
    #[must_use]
    pub fn read(rows: &[HistoryRow], pnl: PnlReader<'_>, total: &PnlTotal) -> Self {
        let chosen = pnl.settings.primary(total.money().is_some());
        let mut keys: Vec<UnitKey> = Vec::new();
        let of_row: Vec<usize> = rows
            .iter()
            .map(|row| {
                let key = UnitKey::of(row, pnl, chosen);
                keys.iter()
                    .position(|seen| *seen == key)
                    .unwrap_or_else(|| {
                        keys.push(key);
                        keys.len() - 1
                    })
            })
            .collect();
        if keys.is_empty() {
            let walk = EquityWalk::walk([], chosen, total.unit_label(chosen));
            return Self::single(walk);
        }
        let several = keys.len() > 1;
        let walks = keys
            .iter()
            .enumerate()
            .map(|(index, key)| {
                let mine = |at: &usize| *at == index;
                let unit = pnl
                    .sum(
                        rows.iter()
                            .zip(&of_row)
                            .filter(|(_, at)| mine(at))
                            .map(|(row, _)| (row.symbol.as_str(), &row.trade)),
                    )
                    .unit_label(key.primary);
                let values = rows.iter().zip(&of_row).map(|(row, at)| {
                    mine(at)
                        .then(|| pnl.value(&row.symbol, &row.trade, key.primary))
                        .flatten()
                        .unwrap_or_default()
                });
                let mut walk = EquityWalk::walk(values, key.primary, unit);
                walk.fallback = key.fallback;
                if several {
                    walk.label.clone_from(&key.instrument);
                }
                walk
            })
            .collect();
        Self { walks, of_row }
    }

    /// Every walk, the first trade's unit first.
    #[must_use]
    pub fn walks(&self) -> &[EquityWalk] {
        &self.walks
    }

    /// The walk `E_k` is read on: trade `k`'s own, the first before any.
    #[must_use]
    pub fn walk_at(&self, k: usize) -> &EquityWalk {
        let index = k
            .checked_sub(1)
            .and_then(|row| self.of_row.get(row))
            .copied()
            .unwrap_or(0);
        &self.walks[index]
    }

    /// `E_k` on trade `k`'s own walk, as the EQUITY cell reads it.
    #[must_use]
    pub fn text(&self, k: usize) -> String {
        self.walk_at(k).text(k)
    }

    /// `E_k` on trade `k`'s own walk, for the cell's colour.
    #[must_use]
    pub fn value(&self, k: usize) -> Decimal {
        self.walk_at(k).points.get(k).copied().unwrap_or_default()
    }

    /// Each walk's end side by side: the net of units that never add up.
    #[must_use]
    pub fn ends_text(&self) -> String {
        self.walks
            .iter()
            .map(|walk| {
                let end = walk.text(walk.points.len().saturating_sub(1));
                if walk.label.is_empty() {
                    end
                } else {
                    format!("{end} {}", walk.label)
                }
            })
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

impl EquityWalk {
    /// The walk in stored points, the journal's own figure.
    #[must_use]
    pub fn of(rows: &[HistoryRow]) -> Self {
        Self::walk(
            rows.iter().map(|row| row.trade.net_points()),
            PnlPrimary::PointsTimesQty,
            "pts".to_owned(),
        )
    }

    /// The running total of `values`, in `primary` read as `unit`.
    #[must_use]
    pub fn walk(
        values: impl IntoIterator<Item = Decimal>,
        primary: PnlPrimary,
        unit: String,
    ) -> Self {
        let mut points = vec![Decimal::ZERO];
        let mut plot = vec![0.0_f32];
        let (mut low, mut high) = (0.0_f32, 0.0_f32);
        let (mut sum, mut peak, mut peak_at) = (Decimal::ZERO, Decimal::ZERO, 0usize);
        let mut drawdown = None;
        for (index, value) in values.into_iter().enumerate() {
            let k = index + 1;
            sum = sum.saturating_add(value);
            points.push(sum);
            let as_f32 = sum.to_f64().unwrap_or_default() as f32;
            low = low.min(as_f32);
            high = high.max(as_f32);
            plot.push(as_f32);
            // The same rule as the report's own drawdown: a new high moves
            // the peak, the deepest fall below it is the one kept.
            if sum > peak {
                peak = sum;
                peak_at = k;
            }
            let depth = peak.saturating_sub(sum);
            if drawdown.is_none_or(|(_, _, deepest)| depth > deepest) && depth > Decimal::ZERO {
                drawdown = Some((peak_at, k, depth));
            }
        }
        Self {
            points,
            plot,
            low,
            high,
            primary,
            unit,
            fallback: None,
            drawdown,
            label: String::new(),
        }
    }

    /// `E_k` in the walk's figure and unit; flat past the end.
    #[must_use]
    pub fn text(&self, k: usize) -> String {
        let value = self.points.get(k).copied().unwrap_or_default();
        fmt_amount(value, self.primary, &self.unit)
    }

    /// The curve's caption: what it plots, in which unit, and why when that
    /// is not the figure the trader chose.
    #[must_use]
    pub fn caption(&self) -> String {
        let mut caption = "REALIZED EQUITY".to_owned();
        if !self.label.is_empty() {
            caption.push_str(" · ");
            caption.push_str(&self.label);
        }
        if !self.unit.is_empty() {
            caption.push_str(" · ");
            caption.push_str(&self.unit);
        }
        if let Some(why) = self.fallback {
            caption.push_str(&format!(" ({why})"));
        }
        caption
    }

    /// The deepest drawdown's depth, as the chip on the curve reads it.
    #[must_use]
    pub fn drawdown_text(&self) -> String {
        let depth = self.drawdown.map_or(Decimal::ZERO, |(_, _, depth)| depth);
        fmt_amount(-depth, self.primary, &self.unit)
    }
}

impl ReportView {
    /// This cut worded through `pnl`: each row, the net and the equity
    /// walk in the figure the trader reads P&L by.
    #[must_use]
    pub fn with_pnl(mut self, pnl: PnlReader<'_>) -> Self {
        let total = pnl.sum(
            self.rows
                .iter()
                .map(|row| (row.symbol.as_str(), &row.trade)),
        );
        self.equity = EquityWalks::read(&self.rows, pnl, &total);
        self.pnl_rows = self
            .rows
            .iter()
            .map(|row| pnl.trade(&row.symbol, row.trade.net_points(), row.trade.quantity))
            .collect();
        self.net_text = pnl.word_split(&total);
        if self.equity.walks().len() > 1 {
            self.net_text.0 = self.equity.ends_text();
        }
        self.net_tint = pnl.tint(&total);
        self.pnl_key = Some(pnl.key());
        self
    }

    /// Whether this view was worded by a reader that still reads the same.
    #[must_use]
    pub fn worded_by(&self, pnl: PnlReader<'_>) -> bool {
        self.pnl_key.as_ref().is_some_and(|key| pnl.wrote(key))
    }

    /// Row `index` as the trade list reads it; empty before
    /// [`Self::with_pnl`].
    #[must_use]
    pub fn trade_text(&self, index: usize) -> &str {
        self.pnl_rows.get(index).map_or("", String::as_str)
    }
}

/// The ledger's totals over every saved trade in scope. Summed when the
/// folder is read, never on the frame: walking a year of sessions sixty
/// times a second to print one line is work nobody asked for. The net is
/// held in every figure at once, so switching the mode re-reads nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LedgerTotals {
    pub trades: usize,
    pub wins: usize,
    pub net: PnlTotal,
    /// What [`Self::of_rows`] was worded under; `None` for a sum that is
    /// not kept across frames.
    worded: Option<PnlKey>,
}

impl LedgerTotals {
    /// Each trade with the symbol it is worded by.
    pub fn of<'s, 't>(trades: impl IntoIterator<Item = (SymbolPnl<'s>, &'t ClosedTrade)>) -> Self {
        let mut totals = Self::default();
        for (pnl, trade) in trades {
            totals.trades += 1;
            if trade.net_points() > Decimal::ZERO {
                totals.wins += 1;
            }
            pnl.add_to(&mut totals.net, trade);
        }
        totals
    }

    /// Saved rows, each worded by its own symbol folder.
    pub fn of_rows<'r>(pnl: PnlReader<'_>, rows: impl IntoIterator<Item = &'r HistoryRow>) -> Self {
        let mut totals = Self::of(
            rows.into_iter()
                .map(|row| (pnl.of(&row.symbol), &row.trade)),
        );
        totals.worded = Some(pnl.key());
        totals
    }

    /// These saved totals, summed again from `rows` when `pnl` no longer
    /// reads as it did when they were summed — a declared point value, the
    /// mode or the feed changed.
    pub fn reworded(&mut self, pnl: PnlReader<'_>, rows: Option<&[HistoryRow]>) -> &Self {
        let stale = self.worded.as_ref().is_some_and(|key| !pnl.wrote(key));
        if let (true, Some(rows)) = (stale, rows) {
            *self = Self::of_rows(pnl, rows);
        }
        self
    }

    /// One symbol's trades — this session's, on the chart's instrument.
    pub fn of_trades<'t>(
        pnl: PnlReader<'_>,
        symbol: &str,
        trades: impl IntoIterator<Item = &'t ClosedTrade>,
    ) -> Self {
        Self::of(trades.into_iter().map(|trade| (pnl.of(symbol), trade)))
    }

    /// The two sets the strip adds up: what is saved on disk and what this
    /// session has closed since.
    #[must_use]
    pub fn plus(&self, other: &Self) -> Self {
        let mut net = self.net.clone();
        net.merge(&other.net);
        Self {
            trades: self.trades + other.trades,
            wins: self.wins + other.wins,
            net,
            worded: self.worded.clone().or_else(|| other.worded.clone()),
        }
    }

    /// Whole-percent win rate; `None` when there is nothing to divide by.
    #[must_use]
    pub fn win_rate(&self) -> Option<usize> {
        (self.wins * 100).checked_div(self.trades)
    }

    /// The strip's count line: trades and win rate over everything in
    /// scope, and how many are listed while saved pages are still held back.
    #[must_use]
    pub fn summary(&self, held_back: bool, rows_listed: usize) -> String {
        let win_rate = self
            .win_rate()
            .map_or_else(String::new, |rate| format!(" · {rate}% win"));
        let scope = if held_back {
            format!(" · {rows_listed} listed")
        } else {
            String::new()
        };
        format!("{} trades{win_rate}{scope}", self.trades)
    }

    /// The strip's totals over everything *in scope*, not everything
    /// listed: the rows above are one revealed page and the strip must not
    /// swing every time the trader reveals another. Unfiltered, these saved
    /// totals (summed when the folder was read, again only if their wording
    /// went stale) plus this session's own closes - a handful. A filter
    /// narrows the scope, and the strip follows it.
    pub fn in_scope<'t>(
        &mut self,
        pnl: PnlReader<'_>,
        rows: Option<&[HistoryRow]>,
        scope: LedgerScopeTotals<'_, 't>,
    ) -> Self {
        match scope {
            LedgerScopeTotals::Whole { symbol, own } => {
                let own = Self::of_trades(pnl, symbol, own);
                self.reworded(pnl, rows).plus(&own)
            }
            LedgerScopeTotals::Cut { kept, live } => {
                let rows = rows.unwrap_or_default();
                let kept = Self::of_rows(pnl, kept.iter().filter_map(|&index| rows.get(index)));
                kept.plus(&Self::of(live))
            }
        }
    }
}

/// What [`LedgerTotals::in_scope`] sums besides the saved totals.
pub enum LedgerScopeTotals<'a, 't> {
    /// No filter: this session's closes, all on the chart's `symbol`.
    Whole {
        symbol: &'a str,
        own: &'t [ClosedTrade],
    },
    /// A filter cut the list: the saved rows it `kept` and the `live`
    /// closes it let through, each with the symbol it is worded by.
    Cut {
        kept: &'a [usize],
        live: Vec<(SymbolPnl<'a>, &'t ClosedTrade)>,
    },
}

#[cfg(test)]
#[path = "readout_tests.rs"]
mod readout_tests;
