//! Performance metrics computed from closed trades.
//!
//! Pure aggregation: the same trades always produce the same report, and a
//! ratio whose denominator does not exist (`profit_factor` with no losses,
//! `win_rate_pct` with no trades) is `None`, never a made-up number.

use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::simulator::ClosedTrade;

/// The report the UI renders. All point values follow the crate's honesty
/// rule: points (price units × quantity), not currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceReport {
    pub trades: u64,
    /// Trades with positive points.
    pub wins: u64,
    /// Trades with negative points.
    pub losses: u64,
    /// Trades that closed at exactly zero.
    pub scratches: u64,
    pub long_trades: u64,
    pub short_trades: u64,
    /// Sum of every trade's points, signed.
    pub net_points: Decimal,
    /// Sum of winning trades' points (≥ 0).
    pub gross_profit: Decimal,
    /// Magnitude of losing trades' points (≥ 0).
    pub gross_loss: Decimal,
    /// `wins / trades × 100`; `None` when there are no trades.
    pub win_rate_pct: Option<Decimal>,
    /// `gross_profit / gross_loss`; `None` when there are no losses — an
    /// undefined ratio is not infinity.
    pub profit_factor: Option<Decimal>,
    /// Deepest drop of the realized equity curve below its running peak,
    /// walking the trades in the order they closed (≥ 0).
    pub max_drawdown_points: Decimal,
    /// Best single trade's points (0 when there are no wins).
    pub largest_win: Decimal,
    /// Worst single trade's magnitude (0 when there are no losses).
    pub largest_loss: Decimal,
    /// `gross_profit / wins`; `None` when there are no wins.
    pub avg_win: Option<Decimal>,
    /// `gross_loss / losses` (magnitude); `None` when there are no losses.
    pub avg_loss: Option<Decimal>,
}

impl PerformanceReport {
    /// Aggregate `trades` in the order given — drawdown depends on it, and
    /// the caller keeps history in closing order.
    #[must_use]
    pub fn from_trades(trades: &[ClosedTrade]) -> Self {
        let mut wins = 0u64;
        let mut losses = 0u64;
        let mut scratches = 0u64;
        let mut long_trades = 0u64;
        let mut short_trades = 0u64;
        let mut gross_profit = Decimal::ZERO;
        let mut gross_loss = Decimal::ZERO;
        let mut largest_win = Decimal::ZERO;
        let mut largest_loss = Decimal::ZERO;
        let mut equity = Decimal::ZERO;
        let mut peak = Decimal::ZERO;
        let mut max_drawdown = Decimal::ZERO;

        for trade in trades {
            match trade.side {
                Side::Buy => long_trades += 1,
                Side::Sell => short_trades += 1,
            }
            let points = trade.pnl_points;
            if points > Decimal::ZERO {
                wins += 1;
                gross_profit = gross_profit.saturating_add(points);
                largest_win = largest_win.max(points);
            } else if points < Decimal::ZERO {
                losses += 1;
                let magnitude = -points;
                gross_loss = gross_loss.saturating_add(magnitude);
                largest_loss = largest_loss.max(magnitude);
            } else {
                scratches += 1;
            }
            equity = equity.saturating_add(points);
            peak = peak.max(equity);
            max_drawdown = max_drawdown.max(peak.saturating_sub(equity));
        }

        let total = wins + losses + scratches;
        let ratio = |numerator: Decimal, denominator: Decimal| {
            (denominator > Decimal::ZERO).then(|| numerator / denominator)
        };
        Self {
            trades: total,
            wins,
            losses,
            scratches,
            long_trades,
            short_trades,
            net_points: gross_profit.saturating_sub(gross_loss),
            gross_profit,
            gross_loss,
            win_rate_pct: ratio(
                Decimal::from(wins).saturating_mul(Decimal::ONE_HUNDRED),
                Decimal::from(total),
            ),
            profit_factor: ratio(gross_profit, gross_loss),
            max_drawdown_points: max_drawdown,
            largest_win,
            largest_loss,
            avg_win: ratio(gross_profit, Decimal::from(wins)),
            avg_loss: ratio(gross_loss, Decimal::from(losses)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::ExitReason;

    fn trade(side: Side, pnl: i64) -> ClosedTrade {
        ClosedTrade {
            side,
            quantity: Decimal::ONE,
            entry_price: Decimal::ONE_HUNDRED,
            exit_price: Decimal::ONE_HUNDRED + Decimal::from(pnl),
            opened_ms: 0,
            closed_ms: 1,
            pnl_points: Decimal::from(pnl),
            exit_reason: ExitReason::Manual,
        }
    }

    #[test]
    fn an_empty_history_reports_zeros_and_no_fake_ratios() {
        let report = PerformanceReport::from_trades(&[]);
        assert_eq!(report.trades, 0);
        assert_eq!(report.net_points, Decimal::ZERO);
        assert_eq!(report.win_rate_pct, None);
        assert_eq!(report.profit_factor, None);
        assert_eq!(report.avg_win, None);
        assert_eq!(report.avg_loss, None);
        assert_eq!(report.max_drawdown_points, Decimal::ZERO);
    }

    #[test]
    fn a_known_sequence_produces_the_hand_computed_metrics() {
        let trades = [
            trade(Side::Buy, 10),
            trade(Side::Sell, -5),
            trade(Side::Buy, 2),
            trade(Side::Sell, -12),
        ];
        let report = PerformanceReport::from_trades(&trades);
        assert_eq!(report.trades, 4);
        assert_eq!(report.wins, 2);
        assert_eq!(report.losses, 2);
        assert_eq!(report.scratches, 0);
        assert_eq!(report.long_trades, 2);
        assert_eq!(report.short_trades, 2);
        assert_eq!(report.net_points, Decimal::from(-5));
        assert_eq!(report.gross_profit, Decimal::from(12));
        assert_eq!(report.gross_loss, Decimal::from(17));
        assert_eq!(report.win_rate_pct, Some(Decimal::from(50)));
        assert_eq!(
            report.profit_factor,
            Some(Decimal::from(12) / Decimal::from(17))
        );
        // Equity walks 10, 5, 7, -5; the peak of 10 to the trough of -5.
        assert_eq!(report.max_drawdown_points, Decimal::from(15));
        assert_eq!(report.largest_win, Decimal::from(10));
        assert_eq!(report.largest_loss, Decimal::from(12));
        assert_eq!(report.avg_win, Some(Decimal::from(6)));
        assert_eq!(report.avg_loss, Some(Decimal::from(17) / Decimal::from(2)));
    }

    #[test]
    fn all_winners_leave_profit_factor_undefined_not_infinite() {
        let trades = [trade(Side::Buy, 3), trade(Side::Buy, 4)];
        let report = PerformanceReport::from_trades(&trades);
        assert_eq!(report.profit_factor, None);
        assert_eq!(report.win_rate_pct, Some(Decimal::ONE_HUNDRED));
        assert_eq!(report.max_drawdown_points, Decimal::ZERO);
    }

    #[test]
    fn a_scratch_counts_as_a_trade_but_not_as_a_win() {
        let trades = [trade(Side::Buy, 0), trade(Side::Buy, 4)];
        let report = PerformanceReport::from_trades(&trades);
        assert_eq!(report.trades, 2);
        assert_eq!(report.wins, 1);
        assert_eq!(report.scratches, 1);
        assert_eq!(report.win_rate_pct, Some(Decimal::from(50)));
    }
}
