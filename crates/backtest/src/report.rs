//! The written report — the harness's own, because no renderer for
//! [`PerformanceReport`] exists outside the desktop app.
//!
//! # The one rule
//!
//! A ratio whose denominator does not exist prints [`NOT_AVAILABLE`], never
//! zero. `sim::report` is careful to hand these out as `None` — profit factor
//! with no losing trade, expectancy with no trade at all — and a renderer
//! that turned `None` into `0` would undo that care and tell a trader a
//! strategy broke even when it never traded.
//!
//! The same rule covers the whole-session facts: a day with no closed trade
//! says so instead of printing a page of zeros, and a position still open at
//! the last print is named rather than folded into the numbers.

use std::fmt::Write as _;

use quantick_engine::Side;
use quantick_sim::{ExitReason, PerformanceReport};
use rust_decimal::Decimal;

use crate::run::{Anomalies, RunOutcome, SessionRun};

/// What an absent ratio prints. Not `0`, not `-`, not an empty cell.
pub const NOT_AVAILABLE: &str = "n/a";

/// Decimal places every figure is shown at. Points on B3 index futures are
/// whole numbers, so two places is enough for the ratios and never turns a
/// price into a wall of digits.
const DISPLAY_PLACES: u32 = 2;
/// Column widths of the two-metrics-per-line block: label, value, label,
/// value. Kept as constants because eight `writeln!` calls have to agree for
/// the block to read as a table at all.
const LABEL_WIDTH: usize = 18;
const VALUE_WIDTH: usize = 16;

/// The full run: a header, one block per session, then the aggregate.
#[must_use]
pub fn render_run(outcome: &RunOutcome) -> String {
    let mut out = String::with_capacity(1024 + outcome.sessions.len() * 512);
    let _ = writeln!(
        out,
        "strategy {} · bars {} · {} session(s)",
        outcome.strategy,
        outcome.spec.summary(),
        outcome.sessions.len()
    );
    for session in &outcome.sessions {
        out.push('\n');
        out.push_str(&render_session(session));
    }
    out.push('\n');
    out.push_str(&render_aggregate(outcome));
    out
}

/// One session's block.
#[must_use]
pub fn render_session(run: &SessionRun) -> String {
    let mut out = String::with_capacity(512);
    let _ = writeln!(
        out,
        "{}  —  {} prints · {} bars · {} trades",
        run.label, run.prints, run.bars, run.report.trades
    );
    if run.report.trades == 0 {
        let _ = writeln!(
            out,
            "  no closed trade — nothing to measure (the metrics are omitted, not zero)"
        );
    } else {
        push_metrics(&mut out, &run.report);
    }
    push_anomalies(&mut out, &run.anomalies);
    if let Some((side, quantity)) = run.open_at_end {
        let held = match side {
            Side::Buy => "long",
            Side::Sell => "short",
        };
        let _ = writeln!(
            out,
            "  {held} {} still open at the last print — excluded from the metrics \
             above, because no print proves its exit",
            quantity.normalize()
        );
    }
    out
}

/// The aggregate over every session, plus the spread the aggregate hides.
#[must_use]
pub fn render_aggregate(outcome: &RunOutcome) -> String {
    let spread = &outcome.distribution;
    let mut out = String::with_capacity(512);
    let _ = writeln!(
        out,
        "all sessions  —  {} session(s) · {} trades",
        spread.sessions, outcome.aggregate.trades
    );
    if outcome.aggregate.trades == 0 {
        let _ = writeln!(
            out,
            "  no closed trade in the whole run — nothing to measure (metrics omitted, not zero)"
        );
    } else {
        push_metrics(&mut out, &outcome.aggregate);
        let _ = writeln!(
            out,
            "  {:<LABEL_WIDTH$}{} up · {} down · {} flat",
            "sessions", spread.up, spread.down, spread.flat,
        );
        if spread.without_trades > 0 {
            let _ = writeln!(
                out,
                "  {} session(s) produced no closed trade",
                spread.without_trades
            );
        }
        push_extreme(&mut out, "best ", spread.best.as_ref());
        push_extreme(&mut out, "worst", spread.worst.as_ref());
        let _ = writeln!(
            out,
            "  {:<LABEL_WIDTH$}{}",
            "median session",
            maybe_signed(spread.median_net)
        );
    }
    push_anomalies(&mut out, &outcome.anomalies);
    if spread.open_at_end > 0 {
        let _ = writeln!(
            out,
            "  {} session(s) ended holding a position — those trades are in no report",
            spread.open_at_end
        );
    }
    out
}

fn push_extreme(out: &mut String, label: &str, extreme: Option<&(String, Decimal)>) {
    let _ = match extreme {
        Some((session, net)) => writeln!(
            out,
            "  {label:<LABEL_WIDTH$}{:<VALUE_WIDTH$}{session}",
            signed(*net)
        ),
        None => writeln!(out, "  {label:<LABEL_WIDTH$}{NOT_AVAILABLE}"),
    };
}

/// The metric block shared by a session and the aggregate.
fn push_metrics(out: &mut String, report: &PerformanceReport) {
    let mut row = |left_label: &str, left: String, right_label: &str, right: String| {
        let _ = writeln!(
            out,
            "  {left_label:<LABEL_WIDTH$}{left:<VALUE_WIDTH$}{right_label:<LABEL_WIDTH$}{right}"
        );
    };
    row(
        "net points",
        signed(report.net_points),
        "max drawdown",
        decimal(report.max_drawdown_points),
    );
    row(
        "expectancy",
        maybe_signed(report.expectancy_points),
        "profit factor",
        maybe(report.profit_factor),
    );
    row(
        "win rate",
        percent(report.win_rate_pct),
        "payoff ratio",
        maybe(report.payoff_ratio),
    );
    // `sim::report` hands the losing side out as a *magnitude* — `avg_loss`
    // is `gross_loss / losses`, `largest_loss` is the worst trade's size,
    // both ≥ 0. Printing those through `signed` would put a `+` in front of
    // a loss, which is the most misleading thing this renderer could do, so
    // the four sizes print unsigned and let their labels carry direction.
    row(
        "avg win",
        maybe(report.avg_win),
        "avg loss",
        maybe(report.avg_loss),
    );
    row(
        "largest win",
        decimal(report.largest_win),
        "largest loss",
        decimal(report.largest_loss),
    );
    row(
        "wins / losses",
        format!("{} / {}", report.wins, report.losses),
        "scratches",
        report.scratches.to_string(),
    );
    row(
        "longs",
        format!(
            "{} ({})",
            report.long.trades,
            signed(report.long.net_points)
        ),
        "shorts",
        format!(
            "{} ({})",
            report.short.trades,
            signed(report.short.net_points)
        ),
    );
    row(
        "avg duration",
        duration(report.avg_duration_ms),
        "recovery factor",
        maybe(report.recovery_factor),
    );
    if !report.by_exit_reason.is_empty() {
        let exits: Vec<String> = report
            .by_exit_reason
            .iter()
            .map(|reason| {
                format!(
                    "{} {} ({})",
                    exit_label(reason.reason),
                    reason.trades,
                    signed(reason.net_points)
                )
            })
            .collect();
        let _ = writeln!(out, "  {:<LABEL_WIDTH$}{}", "exits", exits.join(" · "));
    }
}

fn push_anomalies(out: &mut String, anomalies: &Anomalies) {
    if anomalies.is_clean() {
        return;
    }
    let detail = |counts: &std::collections::BTreeMap<&'static str, u64>| {
        counts
            .iter()
            .map(|(code, count)| format!("{code} {count}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !anomalies.rejected.is_empty() {
        let _ = writeln!(
            out,
            "  {:<LABEL_WIDTH$}{} ({})",
            "rejections",
            anomalies.rejections(),
            detail(&anomalies.rejected)
        );
    }
    if !anomalies.brackets_dropped.is_empty() {
        let _ = writeln!(
            out,
            "  {:<LABEL_WIDTH$}{} ({})",
            "brackets dropped",
            anomalies.dropped_brackets(),
            detail(&anomalies.brackets_dropped)
        );
    }
}

fn exit_label(reason: ExitReason) -> &'static str {
    reason.as_str()
}

/// A decimal at two places, trailing zeros trimmed.
fn decimal(value: Decimal) -> String {
    value.round_dp(DISPLAY_PLACES).normalize().to_string()
}

/// As [`decimal`], with an explicit `+` on positives so a column of results
/// reads at a glance.
fn signed(value: Decimal) -> String {
    let text = decimal(value);
    if value > Decimal::ZERO {
        format!("+{text}")
    } else {
        text
    }
}

fn maybe(value: Option<Decimal>) -> String {
    value.map_or_else(|| NOT_AVAILABLE.to_owned(), decimal)
}

fn maybe_signed(value: Option<Decimal>) -> String {
    value.map_or_else(|| NOT_AVAILABLE.to_owned(), signed)
}

fn percent(value: Option<Decimal>) -> String {
    value.map_or_else(|| NOT_AVAILABLE.to_owned(), |v| format!("{}%", decimal(v)))
}

/// `1h 02m`, `3m 07s`, `450ms` — or `n/a` when there is no duration to
/// average.
fn duration(ms: Option<i64>) -> String {
    let Some(ms) = ms else {
        return NOT_AVAILABLE.to_owned();
    };
    let (sign, ms) = if ms < 0 { ("-", -ms) } else { ("", ms) };
    let seconds = ms / 1_000;
    let (hours, minutes, secs) = (seconds / 3_600, (seconds % 3_600) / 60, seconds % 60);
    if hours > 0 {
        format!("{sign}{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{sign}{minutes}m {secs:02}s")
    } else if seconds > 0 {
        format!("{sign}{secs}s")
    } else {
        format!("{sign}{ms}ms")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_ratio_never_prints_as_zero() {
        assert_eq!(maybe(None), NOT_AVAILABLE);
        assert_eq!(maybe_signed(None), NOT_AVAILABLE);
        assert_eq!(percent(None), NOT_AVAILABLE);
        assert_eq!(duration(None), NOT_AVAILABLE);
        assert_eq!(maybe(Some(Decimal::ZERO)), "0", "a real zero still prints");
    }

    #[test]
    fn signs_and_rounding_are_stable() {
        assert_eq!(
            signed(Decimal::from_str_exact("12.500").expect("d")),
            "+12.5"
        );
        assert_eq!(
            signed(Decimal::from_str_exact("-3.14159").expect("d")),
            "-3.14"
        );
        assert_eq!(signed(Decimal::ZERO), "0");
        assert_eq!(
            percent(Some(Decimal::from_str_exact("54.05").expect("d"))),
            "54.05%"
        );
    }

    /// A finished round trip with the given signed result.
    fn trade(points: i64) -> quantick_sim::ClosedTrade {
        quantick_sim::ClosedTrade {
            side: quantick_engine::Side::Buy,
            quantity: Decimal::ONE,
            entry_price: Decimal::from(100),
            exit_price: Decimal::from(100 + points),
            opened_ms: 0,
            closed_ms: 60_000,
            pnl_points: Decimal::from(points),
            exit_reason: ExitReason::Manual,
            entry_agg_id: Some(1),
            exit_agg_id: Some(2),
            mae_points: None,
            mfe_points: None,
        }
    }

    #[test]
    fn a_loss_never_prints_with_a_plus_in_front_of_it() {
        // `sim::report` reports the losing side as a magnitude. Rendering
        // that through the signed formatter would print "avg loss +40.99",
        // which reads as a profit — the one formatting bug in this file that
        // would actually mislead a trader.
        let report = PerformanceReport::from_trades(&[trade(30), trade(-40), trade(-10)]);
        assert_eq!(report.avg_loss, Some(Decimal::from(25)), "a magnitude");
        assert_eq!(report.largest_loss, Decimal::from(40), "a magnitude");

        let text = push_block(&report);
        assert!(text.contains("avg loss          25"), "{text}");
        assert!(text.contains("largest loss      40"), "{text}");
        assert!(!text.contains("avg loss          +"), "{text}");
        assert!(!text.contains("largest loss      +"), "{text}");
        // The genuinely signed figures keep their sign.
        assert!(text.contains("net points        -20"), "{text}");
    }

    fn push_block(report: &PerformanceReport) -> String {
        let mut out = String::new();
        push_metrics(&mut out, report);
        out
    }

    #[test]
    fn durations_read_in_the_coarsest_useful_unit() {
        assert_eq!(duration(Some(450)), "450ms");
        assert_eq!(duration(Some(9_000)), "9s");
        assert_eq!(duration(Some(187_000)), "3m 07s");
        assert_eq!(duration(Some(3_720_000)), "1h 02m");
    }
}
