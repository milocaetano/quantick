//! Performance metrics computed from closed trades.
//!
//! Pure aggregation: the same trades always produce the same report, and a
//! ratio whose denominator does not exist (`profit_factor` with no losses,
//! `win_rate_pct` with no trades) is `None`, never a made-up number.

use quantick_engine::Side;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};

use quantick_trading::{ClosedTrade, ExitReason};

/// Every exit reason, in the fixed order the report presents them.
const EXIT_REASONS: [ExitReason; 5] = [
    ExitReason::StopLoss,
    ExitReason::TakeProfit,
    ExitReason::Manual,
    ExitReason::Reversal,
    ExitReason::Reset,
];

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
    /// `wins / trades × 100`; `None` when there are no trades. A scratch
    /// counts in the denominator.
    pub win_rate_pct: Option<Decimal>,
    /// `losses / trades × 100`; `None` when there are no trades.
    pub loss_rate_pct: Option<Decimal>,
    /// `gross_profit / gross_loss`; `None` when there are no losses — an
    /// undefined ratio is not infinity.
    pub profit_factor: Option<Decimal>,
    /// `avg_win / avg_loss`; `None` when either side is empty.
    pub payoff_ratio: Option<Decimal>,
    /// `net_points / trades`; `None` when there are no trades. Equivalent
    /// to `win_rate × avg_win − loss_rate × avg_loss`.
    pub expectancy_points: Option<Decimal>,
    /// Deepest drop of the realized equity curve below its running peak,
    /// walking the trades in the order they closed (≥ 0).
    pub max_drawdown_points: Decimal,
    /// The deepest drawdown's bounding points on the equity curve
    /// `E_0..E_n` (`E_0 = 0` before the first trade): indices of the peak
    /// and the trough, `None` when the curve never drew down.
    pub max_drawdown_span: Option<(u64, u64)>,
    /// Highest rise of the equity curve above its running trough (≥ 0) —
    /// the mirror of the drawdown.
    pub max_runup_points: Decimal,
    /// `net_points / max_drawdown_points`; `None` when there was no
    /// drawdown.
    pub recovery_factor: Option<Decimal>,
    /// Longest run of consecutive wins; a scratch breaks both runs.
    pub max_consecutive_wins: u64,
    /// Longest run of consecutive losses; a scratch breaks both runs.
    pub max_consecutive_losses: u64,
    /// Sample standard deviation of trade points (`N − 1` denominator);
    /// `None` below two trades. A display metric: the square root is taken
    /// in `f64` (IEEE-754 sqrt is exactly rounded, so this stays
    /// deterministic) and rounded to four decimal places.
    pub stddev_points: Option<Decimal>,
    /// Best single trade's points (0 when there are no wins).
    pub largest_win: Decimal,
    /// Worst single trade's magnitude (0 when there are no losses).
    pub largest_loss: Decimal,
    /// `gross_profit / wins`; `None` when there are no wins.
    pub avg_win: Option<Decimal>,
    /// `gross_loss / losses` (magnitude); `None` when there are no losses.
    pub avg_loss: Option<Decimal>,
    /// Mean of `closed_ms − opened_ms` in milliseconds, rounded toward
    /// zero; `None` when there are no trades.
    pub avg_duration_ms: Option<i64>,
    /// Median trade duration in milliseconds (mean of the middle two on an
    /// even count); `None` when there are no trades.
    pub median_duration_ms: Option<i64>,
    /// Mean duration of winning trades; `None` when there are no wins.
    pub avg_win_duration_ms: Option<i64>,
    /// Mean duration of losing trades; `None` when there are no losses.
    pub avg_loss_duration_ms: Option<i64>,
    /// Mean MAE of winning trades, over the winners that recorded one
    /// (version-1 history rows did not); `None` when none did.
    pub avg_winner_mae_points: Option<Decimal>,
    /// Winners with a recorded MAE — the denominator the report discloses.
    pub winners_with_mae: u64,
    /// Mean MFE of losing trades, over the losers that recorded one;
    /// `None` when none did.
    pub avg_loser_mfe_points: Option<Decimal>,
    /// Losers with a recorded MFE.
    pub losers_with_mfe: u64,
    /// The long side on its own.
    pub long: SideReport,
    /// The short side on its own.
    pub short: SideReport,
    /// Per-exit-reason totals in a fixed order, empty reasons skipped.
    pub by_exit_reason: Vec<ReasonReport>,
}

/// The core metrics for one side of the book (long-only or short-only).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SideReport {
    pub trades: u64,
    pub net_points: Decimal,
    pub win_rate_pct: Option<Decimal>,
    pub profit_factor: Option<Decimal>,
    pub avg_win: Option<Decimal>,
    pub avg_loss: Option<Decimal>,
    pub expectancy_points: Option<Decimal>,
}

/// How trades that left one way performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasonReport {
    pub reason: ExitReason,
    pub trades: u64,
    pub net_points: Decimal,
}

/// Win/loss/net accumulator shared by the whole-book and per-side walks.
#[derive(Default)]
struct Tally {
    trades: u64,
    wins: u64,
    losses: u64,
    net: Decimal,
    gross_profit: Decimal,
    gross_loss: Decimal,
}

impl Tally {
    fn add(&mut self, points: Decimal) {
        self.trades += 1;
        self.net = self.net.saturating_add(points);
        if points > Decimal::ZERO {
            self.wins += 1;
            self.gross_profit = self.gross_profit.saturating_add(points);
        } else if points < Decimal::ZERO {
            self.losses += 1;
            self.gross_loss = self.gross_loss.saturating_add(-points);
        }
    }

    fn side_report(&self) -> SideReport {
        SideReport {
            trades: self.trades,
            net_points: self.net,
            win_rate_pct: ratio(
                Decimal::from(self.wins).saturating_mul(Decimal::ONE_HUNDRED),
                Decimal::from(self.trades),
            ),
            profit_factor: ratio(self.gross_profit, self.gross_loss),
            avg_win: ratio(self.gross_profit, Decimal::from(self.wins)),
            avg_loss: ratio(self.gross_loss, Decimal::from(self.losses)),
            expectancy_points: ratio(self.net, Decimal::from(self.trades)),
        }
    }
}

/// `numerator / denominator` when the denominator is positive, else `None`.
/// The numerator may be signed (expectancy, recovery factor).
fn ratio(numerator: Decimal, denominator: Decimal) -> Option<Decimal> {
    (denominator > Decimal::ZERO).then(|| numerator / denominator)
}

/// Mean of `sum` over `count` in integer milliseconds, rounded toward zero.
fn mean_ms(sum: i128, count: u64) -> Option<i64> {
    (count > 0).then(|| i64::try_from(sum / i128::from(count)).unwrap_or(i64::MAX))
}

impl PerformanceReport {
    /// Aggregate `trades` in the order given — drawdown, run-up and streaks
    /// depend on it, and the caller keeps history in closing order.
    #[must_use]
    pub fn from_trades(trades: &[ClosedTrade]) -> Self {
        let mut all = Tally::default();
        let mut long = Tally::default();
        let mut short = Tally::default();
        let mut scratches = 0u64;
        let mut largest_win = Decimal::ZERO;
        let mut largest_loss = Decimal::ZERO;

        let mut equity = Decimal::ZERO;
        let mut peak = Decimal::ZERO;
        let mut peak_index = 0u64;
        let mut trough = Decimal::ZERO;
        let mut max_drawdown = Decimal::ZERO;
        let mut max_drawdown_span = None;
        let mut max_runup = Decimal::ZERO;

        let mut consecutive_wins = 0u64;
        let mut consecutive_losses = 0u64;
        let mut max_consecutive_wins = 0u64;
        let mut max_consecutive_losses = 0u64;

        let mut durations: Vec<i64> = Vec::with_capacity(trades.len());
        let mut duration_sum = 0i128;
        let mut win_duration_sum = 0i128;
        let mut loss_duration_sum = 0i128;

        let mut winner_mae_sum = Decimal::ZERO;
        let mut winners_with_mae = 0u64;
        let mut loser_mfe_sum = Decimal::ZERO;
        let mut losers_with_mfe = 0u64;

        let mut reason_tallies: [(u64, Decimal); EXIT_REASONS.len()] = Default::default();

        for (index, trade) in trades.iter().enumerate() {
            let points = trade.pnl_points;
            all.add(points);
            match trade.side {
                Side::Buy => long.add(points),
                Side::Sell => short.add(points),
            }

            let duration = trade.closed_ms.saturating_sub(trade.opened_ms).max(0);
            durations.push(duration);
            duration_sum += i128::from(duration);

            if points > Decimal::ZERO {
                largest_win = largest_win.max(points);
                win_duration_sum += i128::from(duration);
                consecutive_wins += 1;
                consecutive_losses = 0;
                max_consecutive_wins = max_consecutive_wins.max(consecutive_wins);
                if let Some(mae) = trade.mae_points {
                    winner_mae_sum = winner_mae_sum.saturating_add(mae);
                    winners_with_mae += 1;
                }
            } else if points < Decimal::ZERO {
                largest_loss = largest_loss.max(-points);
                loss_duration_sum += i128::from(duration);
                consecutive_losses += 1;
                consecutive_wins = 0;
                max_consecutive_losses = max_consecutive_losses.max(consecutive_losses);
                if let Some(mfe) = trade.mfe_points {
                    loser_mfe_sum = loser_mfe_sum.saturating_add(mfe);
                    losers_with_mfe += 1;
                }
            } else {
                scratches += 1;
                // A scratch breaks both runs: it is neither.
                consecutive_wins = 0;
                consecutive_losses = 0;
            }

            // The equity curve E_0..E_n, where this trade lands at k.
            let k = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
            equity = equity.saturating_add(points);
            if equity > peak {
                peak = equity;
                peak_index = k;
            }
            let drawdown = peak.saturating_sub(equity);
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
                max_drawdown_span = Some((peak_index, k));
            }
            trough = trough.min(equity);
            max_runup = max_runup.max(equity.saturating_sub(trough));

            if let Some(slot) = EXIT_REASONS
                .iter()
                .position(|reason| *reason == trade.exit_reason)
            {
                reason_tallies[slot].0 += 1;
                reason_tallies[slot].1 = reason_tallies[slot].1.saturating_add(points);
            }
        }

        let total = Decimal::from(all.trades);
        let avg_win = ratio(all.gross_profit, Decimal::from(all.wins));
        let avg_loss = ratio(all.gross_loss, Decimal::from(all.losses));

        durations.sort_unstable();
        let median_duration_ms = match durations.len() {
            0 => None,
            n if n % 2 == 1 => Some(durations[n / 2]),
            n => Some(durations[n / 2 - 1].midpoint(durations[n / 2])),
        };

        let by_exit_reason = EXIT_REASONS
            .iter()
            .zip(reason_tallies)
            .filter(|(_, (count, _))| *count > 0)
            .map(|(reason, (count, net))| ReasonReport {
                reason: *reason,
                trades: count,
                net_points: net,
            })
            .collect();

        Self {
            trades: all.trades,
            wins: all.wins,
            losses: all.losses,
            scratches,
            long_trades: long.trades,
            short_trades: short.trades,
            net_points: all.net,
            gross_profit: all.gross_profit,
            gross_loss: all.gross_loss,
            win_rate_pct: ratio(
                Decimal::from(all.wins).saturating_mul(Decimal::ONE_HUNDRED),
                total,
            ),
            loss_rate_pct: ratio(
                Decimal::from(all.losses).saturating_mul(Decimal::ONE_HUNDRED),
                total,
            ),
            profit_factor: ratio(all.gross_profit, all.gross_loss),
            payoff_ratio: match (avg_win, avg_loss) {
                (Some(win), Some(loss)) => ratio(win, loss),
                _ => None,
            },
            expectancy_points: ratio(all.net, total),
            max_drawdown_points: max_drawdown,
            max_drawdown_span,
            max_runup_points: max_runup,
            recovery_factor: ratio(all.net, max_drawdown),
            max_consecutive_wins,
            max_consecutive_losses,
            stddev_points: stddev(trades, all.net, total),
            largest_win,
            largest_loss,
            avg_win,
            avg_loss,
            avg_duration_ms: mean_ms(duration_sum, all.trades),
            median_duration_ms,
            avg_win_duration_ms: mean_ms(win_duration_sum, all.wins),
            avg_loss_duration_ms: mean_ms(loss_duration_sum, all.losses),
            avg_winner_mae_points: ratio(winner_mae_sum, Decimal::from(winners_with_mae)),
            winners_with_mae,
            avg_loser_mfe_points: ratio(loser_mfe_sum, Decimal::from(losers_with_mfe)),
            losers_with_mfe,
            long: long.side_report(),
            short: short.side_report(),
            by_exit_reason,
        }
    }
}

/// Sample standard deviation of trade points; see the field doc for the
/// determinism note on the `f64` square root.
fn stddev(trades: &[ClosedTrade], net: Decimal, total: Decimal) -> Option<Decimal> {
    if trades.len() < 2 {
        return None;
    }
    let mean = net / total;
    let mut sum_squares = Decimal::ZERO;
    for trade in trades {
        let deviation = trade.pnl_points.saturating_sub(mean);
        sum_squares = sum_squares.saturating_add(deviation.saturating_mul(deviation));
    }
    let variance = sum_squares / Decimal::from(trades.len() - 1);
    let root = variance.to_f64()?.sqrt();
    Decimal::from_f64(root).map(|value| value.round_dp(4))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            entry_agg_id: None,
            exit_agg_id: None,
            mae_points: None,
            mfe_points: None,
        }
    }

    /// A trade with everything the extended metrics read.
    fn full(
        side: Side,
        pnl: i64,
        opened_ms: i64,
        closed_ms: i64,
        mae: Option<i64>,
        mfe: Option<i64>,
        reason: ExitReason,
    ) -> ClosedTrade {
        ClosedTrade {
            side,
            quantity: Decimal::ONE,
            entry_price: Decimal::ONE_HUNDRED,
            exit_price: Decimal::ONE_HUNDRED + Decimal::from(pnl),
            opened_ms,
            closed_ms,
            pnl_points: Decimal::from(pnl),
            exit_reason: reason,
            entry_agg_id: Some(1),
            exit_agg_id: Some(2),
            mae_points: mae.map(Decimal::from),
            mfe_points: mfe.map(Decimal::from),
        }
    }

    #[test]
    fn an_empty_history_reports_zeros_and_no_fake_ratios() {
        let report = PerformanceReport::from_trades(&[]);
        assert_eq!(report.trades, 0);
        assert_eq!(report.net_points, Decimal::ZERO);
        assert_eq!(report.win_rate_pct, None);
        assert_eq!(report.loss_rate_pct, None);
        assert_eq!(report.profit_factor, None);
        assert_eq!(report.payoff_ratio, None);
        assert_eq!(report.expectancy_points, None);
        assert_eq!(report.avg_win, None);
        assert_eq!(report.avg_loss, None);
        assert_eq!(report.max_drawdown_points, Decimal::ZERO);
        assert_eq!(report.max_drawdown_span, None);
        assert_eq!(report.max_runup_points, Decimal::ZERO);
        assert_eq!(report.recovery_factor, None);
        assert_eq!(report.stddev_points, None);
        assert_eq!(report.avg_duration_ms, None);
        assert_eq!(report.median_duration_ms, None);
        assert_eq!(report.by_exit_reason, Vec::new());
        assert_eq!(report.long, SideReport::default());
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
        assert_eq!(report.loss_rate_pct, Some(Decimal::from(50)));
        assert_eq!(
            report.profit_factor,
            Some(Decimal::from(12) / Decimal::from(17))
        );
        // Equity walks 10, 5, 7, -5; the peak of 10 to the trough of -5.
        assert_eq!(report.max_drawdown_points, Decimal::from(15));
        assert_eq!(report.max_drawdown_span, Some((1, 4)));
        assert_eq!(report.max_runup_points, Decimal::from(10));
        assert_eq!(
            report.recovery_factor,
            Some(Decimal::from(-5) / Decimal::from(15))
        );
        assert_eq!(report.largest_win, Decimal::from(10));
        assert_eq!(report.largest_loss, Decimal::from(12));
        assert_eq!(report.avg_win, Some(Decimal::from(6)));
        assert_eq!(report.avg_loss, Some(Decimal::from(17) / Decimal::from(2)));
        assert_eq!(
            report.payoff_ratio,
            Some(Decimal::from(6) / (Decimal::from(17) / Decimal::from(2)))
        );
        assert_eq!(
            report.expectancy_points,
            Some(Decimal::from(-5) / Decimal::from(4))
        );
    }

    #[test]
    fn the_extended_fixture_matches_the_hand_computation() {
        // Close order: W +10, W +2, L -5, L -12, scratch, W +6.
        let minute = 60_000i64;
        let trades = [
            full(
                Side::Buy,
                10,
                0,
                minute,
                Some(2),
                Some(12),
                ExitReason::TakeProfit,
            ),
            full(
                Side::Buy,
                2,
                0,
                minute / 2,
                Some(1),
                Some(4),
                ExitReason::Manual,
            ),
            full(
                Side::Sell,
                -5,
                0,
                2 * minute,
                Some(6),
                Some(1),
                ExitReason::StopLoss,
            ),
            full(
                Side::Sell,
                -12,
                0,
                4 * minute,
                Some(14),
                None,
                ExitReason::StopLoss,
            ),
            full(
                Side::Buy,
                0,
                0,
                minute / 6,
                Some(3),
                Some(3),
                ExitReason::Manual,
            ),
            full(
                Side::Buy,
                6,
                0,
                minute / 3,
                None,
                Some(8),
                ExitReason::Reversal,
            ),
        ];
        let report = PerformanceReport::from_trades(&trades);

        assert_eq!(report.trades, 6);
        assert_eq!((report.wins, report.losses, report.scratches), (3, 2, 1));
        assert_eq!(report.net_points, Decimal::ONE);
        assert_eq!(report.max_consecutive_wins, 2, "W W then broken by a loss");
        assert_eq!(report.max_consecutive_losses, 2);

        // Equity: 10, 12, 7, -5, -5, 1 → peak 12 at k=2, trough -5 at k=4.
        assert_eq!(report.max_drawdown_points, Decimal::from(17));
        assert_eq!(report.max_drawdown_span, Some((2, 4)));
        assert_eq!(report.max_runup_points, Decimal::from(12));
        assert_eq!(
            report.recovery_factor,
            Some(Decimal::ONE / Decimal::from(17))
        );

        // Durations: 60, 30, 120, 240, 10, 20 seconds.
        assert_eq!(report.avg_duration_ms, Some(80_000));
        assert_eq!(report.median_duration_ms, Some(45_000), "(30s + 60s) / 2");
        assert_eq!(
            report.avg_win_duration_ms,
            Some(36_666),
            "(60s + 30s + 20s) / 3, rounded toward zero"
        );
        assert_eq!(report.avg_loss_duration_ms, Some(180_000));

        // Winners' MAE: 2 and 1 known, the +6 winner did not record one.
        assert_eq!(report.winners_with_mae, 2);
        assert_eq!(
            report.avg_winner_mae_points,
            Some(Decimal::from(3) / Decimal::from(2))
        );
        // Losers' MFE: only the -5 loser recorded one.
        assert_eq!(report.losers_with_mfe, 1);
        assert_eq!(report.avg_loser_mfe_points, Some(Decimal::ONE));

        // Long side: +10, +2, 0, +6 → 4 trades, 3 wins, 1 scratch.
        assert_eq!(report.long.trades, 4);
        assert_eq!(report.long.net_points, Decimal::from(18));
        assert_eq!(report.long.win_rate_pct, Some(Decimal::from(75)));
        assert_eq!(report.long.profit_factor, None, "no long losses");
        assert_eq!(report.long.avg_win, Some(Decimal::from(6)));
        assert_eq!(report.long.avg_loss, None);
        assert_eq!(
            report.long.expectancy_points,
            Some(Decimal::from(18) / Decimal::from(4))
        );
        // Short side: -5, -12.
        assert_eq!(report.short.trades, 2);
        assert_eq!(report.short.net_points, Decimal::from(-17));
        assert_eq!(report.short.win_rate_pct, Some(Decimal::ZERO));
        assert_eq!(report.short.profit_factor, Some(Decimal::ZERO));
        assert_eq!(
            report.short.expectancy_points,
            Some(Decimal::from(-17) / Decimal::from(2))
        );

        // Exit reasons in fixed order, empty ones skipped.
        assert_eq!(
            report.by_exit_reason,
            vec![
                ReasonReport {
                    reason: ExitReason::StopLoss,
                    trades: 2,
                    net_points: Decimal::from(-17),
                },
                ReasonReport {
                    reason: ExitReason::TakeProfit,
                    trades: 1,
                    net_points: Decimal::from(10),
                },
                ReasonReport {
                    reason: ExitReason::Manual,
                    trades: 2,
                    net_points: Decimal::from(2),
                },
                ReasonReport {
                    reason: ExitReason::Reversal,
                    trades: 1,
                    net_points: Decimal::from(6),
                },
            ]
        );

        // Sample stddev of {10, 2, -5, -12, 0, 6}: variance 11118/180,
        // sqrt ≈ 7.859176, rounded to four places.
        assert_eq!(report.stddev_points, Some(Decimal::new(78_592, 4)));
    }

    #[test]
    fn a_scratch_breaks_both_streaks() {
        let trades = [
            trade(Side::Buy, 4),
            trade(Side::Buy, 0),
            trade(Side::Buy, 5),
            trade(Side::Buy, 6),
        ];
        let report = PerformanceReport::from_trades(&trades);
        assert_eq!(
            report.max_consecutive_wins, 2,
            "the scratch broke the first run"
        );
        assert_eq!(report.max_consecutive_losses, 0);
    }

    #[test]
    fn all_winners_leave_profit_factor_undefined_not_infinite() {
        let trades = [trade(Side::Buy, 3), trade(Side::Buy, 4)];
        let report = PerformanceReport::from_trades(&trades);
        assert_eq!(report.profit_factor, None);
        assert_eq!(report.payoff_ratio, None, "no losses to pay off against");
        assert_eq!(report.win_rate_pct, Some(Decimal::ONE_HUNDRED));
        assert_eq!(report.max_drawdown_points, Decimal::ZERO);
        assert_eq!(report.max_drawdown_span, None);
        assert_eq!(report.recovery_factor, None, "no drawdown to recover from");
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

    #[test]
    fn a_single_trade_has_no_stddev_but_has_durations() {
        let trades = [full(
            Side::Buy,
            7,
            1_000,
            31_000,
            Some(1),
            Some(9),
            ExitReason::Manual,
        )];
        let report = PerformanceReport::from_trades(&trades);
        assert_eq!(report.stddev_points, None, "one sample has no spread");
        assert_eq!(report.avg_duration_ms, Some(30_000));
        assert_eq!(report.median_duration_ms, Some(30_000));
    }
}
