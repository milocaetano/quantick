// The report's own tests: the golden numbers and the pure arithmetic under
// them. Moved with the cut from `app`'s `paper_report` tests, whose window
// and ledger tests stay there with the surfaces they drive.

use quantick_engine::Side;
use quantick_sim::{ExitReason, history};
use rust_decimal::Decimal;

use super::*;

/// A closed trade that netted `pnl` points, closing at `closed_ms`.
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

fn row(symbol: &str, source: Option<history::SessionSource>, trade: ClosedTrade) -> HistoryRow {
    HistoryRow {
        symbol: symbol.to_owned(),
        source,
        trade,
    }
}

#[test]
fn report_periods_anchor_to_the_given_trade_never_a_clock() {
    let utc = TzOffset::new(0);
    // 2026-03-16 13:01:08 UTC.
    let anchor = 1_773_666_068_000_i64;
    assert_eq!(ReportPeriod::All.cutoff_ms(anchor, utc), None);
    assert_eq!(
        ReportPeriod::Today.cutoff_ms(anchor, utc),
        Some(1_773_619_200_000),
        "the anchor's own UTC midnight"
    );
    assert_eq!(
        ReportPeriod::Week.cutoff_ms(anchor, utc),
        Some(anchor - 7 * 86_400_000)
    );
    assert_eq!(
        ReportPeriod::Custom(2 * 86_400_000).cutoff_ms(anchor, utc),
        Some(anchor - 2 * 86_400_000),
        "a typed 2d reaches back exactly two days"
    );
}

#[test]
fn today_breaks_at_the_displayed_midnight_not_utc() {
    let day = 86_400_000_i64;
    // 03 Jan 01:00 UTC is 02 Jan 22:00 in UTC-03:00.
    let anchor = 2 * day + 3_600_000;
    let sao_paulo = TzOffset::new(-180);
    assert_eq!(
        ReportPeriod::Today.cutoff_ms(anchor, sao_paulo),
        Some(day + 10_800_000),
        "midnight of the anchor's displayed day, expressed in UTC"
    );
    assert_eq!(
        ReportPeriod::Today.cutoff_ms(anchor, TzOffset::new(0)),
        Some(2 * day),
        "UTC viewers keep the UTC midnight"
    );
}

#[test]
fn typed_periods_parse_strictly_and_format_back() {
    assert_eq!(parse_period("2d"), Some(2 * 86_400_000));
    assert_eq!(parse_period(" 3D "), Some(3 * 86_400_000));
    assert_eq!(parse_period("12h"), Some(12 * 3_600_000));
    assert_eq!(parse_period("45m"), Some(45 * 60_000));
    assert_eq!(parse_period("1w"), Some(7 * 86_400_000));
    for refused in ["", "d", "2", "2x", "0d", "-2d", "2.5d", "d2"] {
        assert_eq!(parse_period(refused), None, "{refused:?} must be refused");
    }
    assert_eq!(fmt_period_ms(2 * 86_400_000), "2d");
    assert_eq!(fmt_period_ms(36 * 3_600_000), "36h");
    assert_eq!(fmt_period_ms(45 * 60_000), "45m");
    assert_eq!(fmt_period_ms(14 * 86_400_000), "2w");
}

/// The equity walk is cut with the view, not re-walked per frame, so
/// the curve above the list and the running total beside each trade
/// are literally the same numbers.
#[test]
fn the_equity_walk_is_cut_once_and_both_readers_share_it() {
    let rows = vec![
        row("X", None, trade_at(1_000, 5)),
        row("X", None, trade_at(2_000, -12)),
        row("X", None, trade_at(3_000, 4)),
    ];
    let walk = EquityWalk::of(&rows);
    assert_eq!(walk.points.len(), rows.len() + 1, "E_0 rides in front");
    assert_eq!(walk.points[0], Decimal::ZERO, "flat before the first trade");
    assert_eq!(walk.points[1], Decimal::from(5));
    assert_eq!(walk.points[2], Decimal::from(-7));
    assert_eq!(walk.points[3], Decimal::from(-3));
    assert_eq!(walk.plot.len(), walk.points.len());
    assert!((walk.low - (-7.0)).abs() < f32::EPSILON, "the trough");
    assert!((walk.high - 5.0).abs() < f32::EPSILON, "the peak");
    // A view with no trades still has a walk, and it is the flat one.
    let empty = EquityWalk::of(&[]);
    assert_eq!(empty.points, vec![Decimal::ZERO]);
    assert_eq!((empty.low, empty.high), (0.0, 0.0));
}

/// The expected report, byte for byte. Regenerated only when a
/// *behaviour* change is intended and argued for.
const GOLDEN_REPORT: &str = r#"== ALL ==
rows: 8
hidden_outside: 0
hidden_by_source: 1
anchor_ms: Some(1728000000)
equity.points: [0, 12, 7, 4, 4, 13, 5, 9, 15]
equity.plot: [0.0, 12.0, 7.0, 4.0, 4.0, 13.0, 5.0, 9.0, 15.0]
equity.low: 0.0  equity.high: 15.0
  report: PerformanceReport {
      trades: 8,
      wins: 4,
      losses: 3,
      scratches: 1,
      long_trades: 4,
      short_trades: 4,
      net_points: 15,
      gross_profit: 31,
      gross_loss: 16,
      win_rate_pct: Some(
          50,
      ),
      loss_rate_pct: Some(
          37.50,
      ),
      profit_factor: Some(
          1.9375,
      ),
      payoff_ratio: Some(
          1.453125000000000000000,
      ),
      expectancy_points: Some(
          1.875,
      ),
      max_drawdown_points: 8,
      max_drawdown_span: Some(
          (
              1,
              3,
          ),
      ),
      max_runup_points: 15,
      recovery_factor: Some(
          1.875,
      ),
      max_consecutive_wins: 2,
      max_consecutive_losses: 2,
      stddev_points: Some(
          7.0394,
      ),
      largest_win: 12,
      largest_loss: 8,
      avg_win: Some(
          7.75,
      ),
      avg_loss: Some(
          5.3333333333333333333333333333,
      ),
      avg_duration_ms: Some(
          2700000,
      ),
      median_duration_ms: Some(
          2100000,
      ),
      avg_win_duration_ms: Some(
          4500000,
      ),
      avg_loss_duration_ms: Some(
          1000000,
      ),
      avg_winner_mae_points: Some(
          2.50,
      ),
      winners_with_mae: 4,
      avg_loser_mfe_points: Some(
          1.50,
      ),
      losers_with_mfe: 2,
      long: SideReport {
          trades: 4,
          net_points: 3,
          win_rate_pct: Some(
              50,
          ),
          profit_factor: Some(
              1.2307692307692307692307692308,
          ),
          avg_win: Some(
              8,
          ),
          avg_loss: Some(
              6.50,
          ),
          expectancy_points: Some(
              0.75,
          ),
      },
      short: SideReport {
          trades: 4,
          net_points: 12,
          win_rate_pct: Some(
              50,
          ),
          profit_factor: Some(
              5,
          ),
          avg_win: Some(
              7.50,
          ),
          avg_loss: Some(
              3,
          ),
          expectancy_points: Some(
              3,
          ),
      },
      by_exit_reason: [
          ReasonReport {
              reason: StopLoss,
              trades: 3,
              net_points: -16,
          },
          ReasonReport {
              reason: TakeProfit,
              trades: 3,
              net_points: 27,
          },
          ReasonReport {
              reason: Manual,
              trades: 2,
              net_points: 4,
          },
      ],
  }
== WEEK ==
rows: 5
hidden_outside: 3
hidden_by_source: 1
anchor_ms: Some(1728000000)
equity.points: [0, 0, 9, 1, 5, 11]
equity.plot: [0.0, 0.0, 9.0, 1.0, 5.0, 11.0]
equity.low: 0.0  equity.high: 11.0
  report: PerformanceReport {
      trades: 5,
      wins: 3,
      losses: 1,
      scratches: 1,
      long_trades: 2,
      short_trades: 3,
      net_points: 11,
      gross_profit: 19,
      gross_loss: 8,
      win_rate_pct: Some(
          60,
      ),
      loss_rate_pct: Some(
          20,
      ),
      profit_factor: Some(
          2.375,
      ),
      payoff_ratio: Some(
          0.7916666666666666666666666667,
      ),
      expectancy_points: Some(
          2.20,
      ),
      max_drawdown_points: 8,
      max_drawdown_span: Some(
          (
              2,
              3,
          ),
      ),
      max_runup_points: 11,
      recovery_factor: Some(
          1.375,
      ),
      max_consecutive_wins: 2,
      max_consecutive_losses: 1,
      stddev_points: Some(
          6.5727,
      ),
      largest_win: 9,
      largest_loss: 8,
      avg_win: Some(
          6.3333333333333333333333333333,
      ),
      avg_loss: Some(
          8,
      ),
      avg_duration_ms: Some(
          3060000,
      ),
      median_duration_ms: Some(
          2400000,
      ),
      avg_win_duration_ms: Some(
          4800000,
      ),
      avg_loss_duration_ms: Some(
          300000,
      ),
      avg_winner_mae_points: Some(
          2,
      ),
      winners_with_mae: 3,
      avg_loser_mfe_points: None,
      losers_with_mfe: 0,
      long: SideReport {
          trades: 2,
          net_points: -4,
          win_rate_pct: Some(
              50,
          ),
          profit_factor: Some(
              0.50,
          ),
          avg_win: Some(
              4,
          ),
          avg_loss: Some(
              8,
          ),
          expectancy_points: Some(
              -2,
          ),
      },
      short: SideReport {
          trades: 3,
          net_points: 15,
          win_rate_pct: Some(
              66.666666666666666666666666667,
          ),
          profit_factor: None,
          avg_win: Some(
              7.50,
          ),
          avg_loss: None,
          expectancy_points: Some(
              5,
          ),
      },
      by_exit_reason: [
          ReasonReport {
              reason: StopLoss,
              trades: 1,
              net_points: -8,
          },
          ReasonReport {
              reason: TakeProfit,
              trades: 2,
              net_points: 15,
          },
          ReasonReport {
              reason: Manual,
              trades: 2,
              net_points: 4,
          },
      ],
  }
"#;

/// The fixture the golden report is computed from: nine closed trades
/// over eleven days, chosen so that every branch of the arithmetic has
/// something to say. Both sides trade; there are wins, losses and a
/// scratch; the equity curve draws down and runs up again; two exit
/// reasons appear and three do not; and two rows carry no MAE/MFE, the
/// way a version-1 history file loads, so the disclosed denominators
/// are not simply the trade count.
///
/// It is fixed on purpose. The numbers below are the trader's own
/// results as this repository computes them, and a move that shifts one
/// of them is the code telling them a lie about their trading. Nothing
/// here may be edited to make a failing assertion pass: a change to this
/// fixture invalidates the golden, and a change to the golden has to be
/// argued for as a change in behaviour.
fn golden_history() -> LoadedHistory {
    let day = 86_400_000_i64;
    let t = |closed_ms: i64,
             side: Side,
             pnl: i64,
             held_ms: i64,
             reason: quantick_sim::ExitReason,
             excursions: Option<(i64, i64)>| ClosedTrade {
        side,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(100 + pnl),
        opened_ms: closed_ms - held_ms,
        closed_ms,
        pnl_points: Decimal::from(pnl),
        exit_reason: reason,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: excursions.map(|(mae, _)| Decimal::from(mae)),
        mfe_points: excursions.map(|(_, mfe)| Decimal::from(mfe)),
    };
    use quantick_sim::ExitReason::{Manual, StopLoss, TakeProfit};
    LoadedHistory {
        rows: vec![
            row(
                "AAAUSDT",
                Some(history::SessionSource::Live),
                t(
                    10 * day,
                    Side::Buy,
                    12,
                    3_600_000,
                    TakeProfit,
                    Some((4, 15)),
                ),
            ),
            row(
                "AAAUSDT",
                Some(history::SessionSource::Live),
                t(11 * day, Side::Buy, -5, 900_000, StopLoss, Some((7, 2))),
            ),
            row(
                "AAAUSDT",
                Some(history::SessionSource::Live),
                t(12 * day, Side::Sell, -3, 1_800_000, StopLoss, Some((6, 1))),
            ),
            row(
                "AAAUSDT",
                None,
                t(13 * day, Side::Sell, 0, 600_000, Manual, None),
            ),
            row(
                "BBBUSDT",
                Some(history::SessionSource::Live),
                t(
                    14 * day,
                    Side::Sell,
                    9,
                    7_200_000,
                    TakeProfit,
                    Some((2, 11)),
                ),
            ),
            row(
                "BBBUSDT",
                None,
                t(15 * day, Side::Buy, -8, 300_000, StopLoss, None),
            ),
            // A practice run. The default Source filter keeps it out
            // of the real track record, so the golden covers that
            // filter too rather than assuming it never fires.
            row(
                "BBBUSDT",
                Some(history::SessionSource::Replay),
                t(
                    16 * day,
                    Side::Buy,
                    40,
                    1_200_000,
                    TakeProfit,
                    Some((1, 44)),
                ),
            ),
            row(
                "BBBUSDT",
                Some(history::SessionSource::Live),
                t(19 * day, Side::Buy, 4, 2_400_000, Manual, Some((3, 6))),
            ),
            row(
                "BBBUSDT",
                Some(history::SessionSource::Live),
                t(20 * day, Side::Sell, 6, 4_800_000, TakeProfit, Some((1, 8))),
            ),
        ],
        files: 3,
        unreadable_files: 0,
        problem_rows: 0,
    }
}

/// The report and its equity walk as one block of text, every field
/// named. `{:#?}` over the report rather than a hand-written field list
/// on purpose: a hand-written list can silently omit a metric, and the
/// one metric nobody thought to write down is exactly the one a move
/// would break unnoticed. The equity walk is dumped beside it because
/// the curve and the trade list's running total read it, and it is
/// computed here rather than in `quantick-sim`.
fn dump_report(view: &ReportView) -> String {
    let mut out = String::new();
    out.push_str(&format!("rows: {}\n", view.rows.len()));
    out.push_str(&format!("hidden_outside: {}\n", view.hidden_outside));
    out.push_str(&format!("hidden_by_source: {}\n", view.hidden_by_source));
    out.push_str(&format!("anchor_ms: {:?}\n", view.anchor_ms));
    out.push_str(&format!("equity.points: {:?}\n", view.equity.points));
    out.push_str(&format!("equity.plot: {:?}\n", view.equity.plot));
    out.push_str(&format!(
        "equity.low: {:?}  equity.high: {:?}\n",
        view.equity.low, view.equity.high
    ));
    // Indented, and not merely for looks. `{:#?}` closes its braces in
    // column 0, and the size guard finds the end of a `#[cfg(test)]`
    // module by scanning for a line that is exactly `}` — so an
    // unindented dump inside the golden below walks that scan out of
    // the test module and scores three thousand lines of test code as
    // production. Two spaces keep the ratchet honest, and cost the
    // golden nothing: it is the same text, byte for byte.
    for line in format!("report: {:#?}", view.report).lines() {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The numbers must not move.
///
/// A fixed journal in, the whole report out, asserted byte for byte —
/// across two cuts, so the filter arithmetic is under the golden too
/// and not only `PerformanceReport::from_trades`. `All` covers the
/// unfiltered aggregation; `Week` covers the anchor-relative cutoff,
/// which is the part that lives in this crate rather than in
/// `quantick-sim`.
///
/// This test was written before the report moved out of `app`, and its
/// expected text did not change when it moved into this crate. That is
/// the whole point: an extraction that alters a rounding, a filter boundary or an
/// equity walk fails here rather than in front of the trader.
#[test]
fn the_report_numbers_are_fixed() {
    // The state a report window opens in: the Real source and no picked
    // span, so the period alone cuts - exactly what `app`'s window hands
    // `ReportView::cut` before a trader touches a filter.
    let utc = TzOffset::new(0);
    let history = golden_history();
    let cut = |period| ReportView::cut(&history, period, SourceFilter::Real, None, utc);
    let all = dump_report(&cut(ReportPeriod::All));
    let week = dump_report(&cut(ReportPeriod::Week));

    assert_eq!(
        format!("== ALL ==\n{all}== WEEK ==\n{week}"),
        GOLDEN_REPORT,
        "the report's numbers moved"
    );
}
