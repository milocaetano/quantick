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

use quantick_sim::ClosedTrade;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::{HistoryRow, ReportView};
use crate::pnl::{PnlKey, PnlPrimary, PnlReader, PnlTotal, SymbolPnl, fmt_amount};

/// Why a walk is in grouped points when money was asked for.
const UNSET_FALLBACK: &str = "pt value unset";
const MIXED_FALLBACK: &str = "mixed currencies";
/// Why there is no walk at all: no money to plot, and points that add up
/// to nothing.
const NO_CURVE: &str = "mixed units — no curve";

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
}

impl EquityWalk {
    /// The walk in stored points, the journal's own figure.
    #[must_use]
    pub fn of(rows: &[HistoryRow]) -> Self {
        Self::walk(
            rows.iter().map(|row| row.trade.pnl_points),
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
        }
    }

    /// The walk of `rows` in the figure `pnl` leads a total of them with —
    /// grouped points, labelled, where that is money no single currency
    /// can hold. `total` is `pnl`'s sum of the same rows.
    #[must_use]
    pub fn read(rows: &[HistoryRow], pnl: PnlReader<'_>, total: &PnlTotal) -> Self {
        let chosen = pnl.total_primary(total);
        let fallback = match (chosen, total.money()) {
            (PnlPrimary::Money, None) => Some(UNSET_FALLBACK),
            (PnlPrimary::Money, Some(money)) if money.len() > 1 => Some(MIXED_FALLBACK),
            _ => None,
        };
        if fallback.is_some() && total.mixed_units() {
            // Grouped points of instruments worth different things are no
            // figure at all: no curve beats a sum across units.
            return Self::unplotted();
        }
        let primary = if fallback.is_some() {
            PnlPrimary::PointsTimesQty
        } else {
            chosen
        };
        let values = rows.iter().map(|row| {
            pnl.value(&row.symbol, &row.trade, primary)
                .unwrap_or_default()
        });
        let mut walk = Self::walk(values, primary, total.unit_label(primary));
        walk.fallback = fallback;
        walk
    }

    /// The walk that is not plotted: trades whose money cannot be read and
    /// whose points are worth different things. No values, no drawdown —
    /// only the caption saying why.
    fn unplotted() -> Self {
        Self {
            points: Vec::new(),
            plot: Vec::new(),
            low: 0.0,
            high: 0.0,
            primary: PnlPrimary::PointsTimesQty,
            unit: String::new(),
            fallback: Some(NO_CURVE),
            drawdown: None,
        }
    }

    /// Whether there is a curve to paint; see [`Self::unplotted`].
    #[must_use]
    pub fn plotted(&self) -> bool {
        !self.plot.is_empty()
    }

    /// `E_k` in the walk's figure and unit; flat past the end. Empty for a
    /// walk that is not plotted.
    #[must_use]
    pub fn text(&self, k: usize) -> String {
        if !self.plotted() {
            return String::new();
        }
        let value = self.points.get(k).copied().unwrap_or_default();
        fmt_amount(value, self.primary, &self.unit)
    }

    /// The curve's caption: what it plots, in which unit, and why when that
    /// is not the figure the trader chose.
    #[must_use]
    pub fn caption(&self) -> String {
        let mut caption = "REALIZED EQUITY".to_owned();
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
        self.equity = EquityWalk::read(&self.rows, pnl, &total);
        self.pnl_rows = self
            .rows
            .iter()
            .map(|row| pnl.trade(&row.symbol, row.trade.pnl_points, row.trade.quantity))
            .collect();
        self.net_text = pnl.word_split(&total);
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
            if trade.pnl_points > Decimal::ZERO {
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
}

#[cfg(test)]
#[path = "readout_tests.rs"]
mod readout_tests;
