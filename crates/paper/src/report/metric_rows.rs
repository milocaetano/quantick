//! The report window's grids as words: the metric grid, one metric per row
//! with its explanation, and the long-versus-short grid, one metric per row
//! with a value per side. Honest blanks (`—`) where a ratio has no
//! denominator. Worded here, beside the report's arithmetic, so a test reads
//! the window's tables without a window; the window only lays them out.

use quantick_sim::{PerformanceReport, SideReport};
use rust_decimal::Decimal;

use crate::format::{fmt_duration_ms, fmt_points, fmt_signed_points};

/// One metric grid row: its label, its value and what it means.
pub type MetricRow = (&'static str, String, &'static str);

/// One long-versus-short row's value, computed per side.
pub type SideValue = Box<dyn Fn(&SideReport) -> String>;

/// The metric grid's rows, top to bottom.
#[must_use]
pub fn metric_rows(report: &PerformanceReport) -> Vec<MetricRow> {
    let blank = || "—".to_owned();
    let pts = |value: Decimal| format!("{} pts", fmt_points(value));
    let opt_pts = |value: Option<Decimal>| {
        value.map_or_else(blank, |value| format!("{} pts", fmt_points(value)))
    };
    let opt_plain = |value: Option<Decimal>| value.map_or_else(blank, fmt_points);
    let duration = |value: Option<i64>| value.map_or_else(blank, fmt_duration_ms);
    let mut rows = Vec::new();
    let mut row = |label, value, explain| rows.push((label, value, explain));
    row(
        "trades",
        format!(
            "{} ({} long / {} short)",
            report.trades, report.long_trades, report.short_trades
        ),
        "closed round trips under the current filter",
    );
    row(
        "max drawdown",
        pts(report.max_drawdown_points),
        "deepest drop of realized equity below its running peak, in closing order",
    );
    row(
        "max run-up",
        pts(report.max_runup_points),
        "highest rise of realized equity above its running trough — the drawdown's mirror",
    );
    row(
        "recovery factor",
        opt_plain(report.recovery_factor),
        "net profit divided by the max drawdown; blank while nothing drew down",
    );
    row(
        "gross profit / loss",
        format!(
            "{} / {} pts",
            fmt_points(report.gross_profit),
            fmt_points(report.gross_loss)
        ),
        "winners' points and losers' magnitude, before netting",
    );
    row(
        "avg win / loss",
        format!(
            "{} / {} pts",
            opt_plain(report.avg_win),
            opt_plain(report.avg_loss),
        ),
        "mean winner and mean loser magnitude",
    );
    row(
        "payoff ratio",
        opt_plain(report.payoff_ratio),
        "average win divided by average loss — how much a winner pays for a loser",
    );
    row(
        "expectancy",
        report
            .expectancy_points
            .map_or_else(blank, |value| format!("{} pts/trade", fmt_points(value))),
        "net profit per trade: what one average trade pays",
    );
    row(
        "std deviation",
        opt_pts(report.stddev_points),
        "sample standard deviation of trade points (N−1); blank below two trades",
    );
    row(
        "longest streaks",
        format!(
            "{} wins / {} losses",
            report.max_consecutive_wins, report.max_consecutive_losses
        ),
        "longest consecutive runs, in closing order; a scratch breaks both",
    );
    row(
        "avg / median duration",
        format!(
            "{} / {}",
            duration(report.avg_duration_ms),
            duration(report.median_duration_ms),
        ),
        "trade lifetimes in venue time",
    );
    row(
        "winner vs loser duration",
        format!(
            "{} / {}",
            duration(report.avg_win_duration_ms),
            duration(report.avg_loss_duration_ms),
        ),
        "how long winners run against how long losers are held",
    );
    row(
        "largest win / loss",
        format!(
            "{} / {} pts",
            fmt_points(report.largest_win),
            fmt_points(report.largest_loss)
        ),
        "best single trade and worst single trade magnitude",
    );
    row(
        "avg winner MAE",
        report.avg_winner_mae_points.map_or_else(blank, |value| {
            format!(
                "{} pts (over {} of {})",
                fmt_points(value),
                report.winners_with_mae,
                report.wins
            )
        }),
        "how far the average winner first ran against you; only trades that \
         recorded an excursion count, and the denominator says how many did",
    );
    row(
        "avg loser MFE",
        report.avg_loser_mfe_points.map_or_else(blank, |value| {
            format!(
                "{} pts (over {} of {})",
                fmt_points(value),
                report.losers_with_mfe,
                report.losses
            )
        }),
        "how far the average loser was in profit before it lost; same disclosure",
    );
    rows
}

/// The long-versus-short grid's rows: each label and how a side's value is
/// worded.
#[must_use]
pub fn side_rows() -> [(&'static str, SideValue); 7] {
    fn opt_plain(value: Option<Decimal>) -> String {
        value.map_or_else(|| "—".to_owned(), fmt_points)
    }
    [
        ("trades", Box::new(|side| side.trades.to_string())),
        (
            "net",
            Box::new(|side| format!("{} pts", fmt_signed_points(side.net_points))),
        ),
        (
            "win rate",
            Box::new(|side| {
                side.win_rate_pct
                    .map_or_else(|| "—".to_owned(), |rate| format!("{}%", fmt_points(rate)))
            }),
        ),
        (
            "profit factor",
            Box::new(|side| opt_plain(side.profit_factor)),
        ),
        ("avg win", Box::new(|side| opt_plain(side.avg_win))),
        ("avg loss", Box::new(|side| opt_plain(side.avg_loss))),
        (
            "expectancy",
            Box::new(|side| opt_plain(side.expectancy_points)),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_report_reads_blank_where_a_ratio_has_no_denominator() {
        let report = PerformanceReport::from_trades(&[]);
        let rows = metric_rows(&report);
        let value = |label: &str| {
            let row = rows.iter().find(|row| row.0 == label).expect(label);
            row.1.clone()
        };
        assert_eq!(value("trades"), "0 (0 long / 0 short)");
        assert_eq!(value("recovery factor"), "—");
        assert_eq!(value("payoff ratio"), "—");
        assert!(rows.iter().all(|(_, _, explain)| !explain.is_empty()));
        let sides = side_rows();
        assert_eq!(sides[0].0, "trades");
        assert_eq!((sides[0].1)(&report.long), "0");
        assert_eq!((sides[2].1)(&report.long), "—", "0/0 is no win rate");
    }
}
