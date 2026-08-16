//! Golden / snapshot harness for indicators.
//!
//! The determinism rule extends from bars to plots: same bars in, same plot
//! values out, always, bit-exact. This harness guards it the same way the
//! engine's [`golden`](quantick_engine::golden) module guards bar building —
//! replay a committed trade fixture, compare against a committed expected
//! output, and additionally run everything twice to catch non-determinism
//! directly.
//!
//! Plot values are compared as **formatted strings** (`{:?}` on f64, which
//! round-trips exactly and prints NaN as `NaN`): bit-exactness is the point,
//! so the fixture format *is* the comparison format.

use std::fmt::Write as _;

use quantick_engine::{BarBuilder, fixture, golden};

use crate::bar::IndicatorBar;
use crate::indicator::{Ctx, EvalError, Indicator};

/// Replay a trade fixture through a bar builder and project the closed bars
/// for indicator consumption. The trailing in-progress bar is intentionally
/// excluded — golden runs cover committed history; preview behavior has its
/// own tests.
///
/// # Panics
///
/// Panics if the fixture does not parse: a broken committed fixture is a bug
/// in the repository, and the panic message is the diagnosis.
#[must_use]
pub fn bars_from_trades_csv<B: BarBuilder>(builder: &mut B, trades_csv: &str) -> Vec<IndicatorBar> {
    let trades = fixture::parse_trades(trades_csv).expect("trade fixture should parse");
    golden::replay(builder, &trades)
        .iter()
        .map(IndicatorBar::from)
        .collect()
}

/// Run an indicator over closed bars the way the host will: cvd accumulated
/// across bars, one commit run per bar.
///
/// # Errors
///
/// Returns the first [`EvalError`] the indicator raises; bars after it are
/// not evaluated (matching the host's disable-on-error rule).
pub fn run_indicator(
    indicator: &mut dyn Indicator,
    bars: &[IndicatorBar],
) -> Result<(), EvalError> {
    let mut cvd = Vec::with_capacity(bars.len());
    let mut sum = 0.0;
    for (bar_index, bar) in bars.iter().enumerate() {
        sum += bar.delta();
        cvd.push(sum);
        let mut ctx = Ctx {
            bar_index,
            cvd: &cvd,
        };
        indicator.on_close(bar, &mut ctx)?;
    }
    Ok(())
}

/// Serialise an indicator's committed plot columns as CSV: a header of plot
/// titles, then one row per bar of `{:?}`-formatted f64 values.
///
/// An indicator that painted at least one candle gets a trailing `bar_paint`
/// column — `#RRGGBBAA` where it painted, `na` where it did not. The column
/// is conditional so that the fixtures of the indicators that never paint
/// stay exactly as they are: a colour channel nobody uses does not belong in
/// their golden output. It also means a regression that stops the painting
/// entirely removes the whole column, and the header diff says so.
///
/// Deterministic by construction (fixed order, exact float formatting, upper
/// -case hex), so it doubles as the byte-identical representation two runs are
/// diffed against.
#[must_use]
pub fn plot_csv(indicator: &dyn Indicator) -> String {
    let descriptor = indicator.descriptor();
    let plots = indicator.plots();
    let paints = plots.paints_any();
    let mut out = String::new();
    out.push_str("bar_index");
    for spec in &descriptor.plots {
        out.push(',');
        out.push_str(&spec.title);
    }
    if paints {
        out.push_str(",bar_paint");
    }
    out.push('\n');
    for row in 0..plots.len() {
        let _ = write!(out, "{row}");
        for spec in &descriptor.plots {
            let _ = write!(out, ",{:?}", plots.value(spec.id, row));
        }
        if paints {
            match plots.bar_paint(row) {
                Some(color) => {
                    let _ = write!(out, ",#{:08X}", color.to_u32());
                }
                None => out.push_str(",na"),
            }
        }
        out.push('\n');
    }
    out
}

/// Compare produced plot CSV against expected text, tolerating comments and
/// blank lines in the committed fixture. Returns `None` on a match or a
/// human-readable per-line report.
#[must_use]
pub fn diff_plot_csv(expected: &str, got: &str) -> Option<String> {
    let expected_lines: Vec<&str> = expected
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let got_lines: Vec<&str> = got.lines().collect();
    if expected_lines == got_lines {
        return None;
    }
    let mut body = String::new();
    for i in 0..expected_lines.len().max(got_lines.len()) {
        match (expected_lines.get(i), got_lines.get(i)) {
            (Some(e), Some(g)) if e == g => {}
            (Some(e), Some(g)) => {
                let _ = writeln!(body, "  line {i}: expected `{e}`, got `{g}`");
            }
            (Some(e), None) => {
                let _ = writeln!(body, "  line {i}: missing (expected `{e}`)");
            }
            (None, Some(g)) => {
                let _ = writeln!(body, "  line {i}: unexpected `{g}`");
            }
            (None, None) => unreachable!("index is below the longer length"),
        }
    }
    Some(format!(
        "plot golden mismatch (expected {} lines, got {}):\n{}",
        expected_lines.len(),
        got_lines.len(),
        body.trim_end(),
    ))
}

/// Assert that replaying `trades_csv` through fresh bars and a fresh
/// indicator produces `expected_plot_csv`, and that the whole pipeline is
/// deterministic.
///
/// Both `make_indicator` and `make_builder` are factories so the harness can
/// run the pipeline twice from scratch and require byte-identical output —
/// an indicator reading a wall clock, randomness or map iteration order
/// fails here even if it happens to match the fixture once.
///
/// # Panics
///
/// - if a fixture fails to parse;
/// - if the indicator errors on any bar;
/// - if the two runs differ (non-determinism);
/// - if the produced plots differ from the expected fixture.
pub fn assert_golden<I, FI, B, FB>(
    make_indicator: FI,
    make_builder: FB,
    trades_csv: &str,
    expected_plot_csv: &str,
) where
    I: Indicator,
    FI: Fn() -> I,
    B: BarBuilder,
    FB: Fn() -> B,
{
    let run = || {
        let bars = bars_from_trades_csv(&mut make_builder(), trades_csv);
        let mut indicator = make_indicator();
        run_indicator(&mut indicator, &bars).expect("indicator should evaluate the fixture");
        plot_csv(&indicator)
    };

    let csv1 = run();
    let csv2 = run();
    assert_eq!(
        csv1, csv2,
        "non-deterministic indicator: two runs over the same fixture produced different plots",
    );

    if let Some(report) = diff_plot_csv(expected_plot_csv, &csv1) {
        panic!("{report}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_csv_has_no_diff() {
        let text = "bar_index,close\n0,1.0\n";
        assert!(diff_plot_csv(text, text).is_none());
    }

    #[test]
    fn expected_side_tolerates_comments_and_blanks() {
        let expected = "# fixture note\n\nbar_index,close\n0,1.0\n";
        let got = "bar_index,close\n0,1.0\n";
        assert!(diff_plot_csv(expected, got).is_none());
    }

    #[test]
    fn diff_names_the_line() {
        let expected = "bar_index,close\n0,1.0\n";
        let got = "bar_index,close\n0,2.0\n";
        let report = diff_plot_csv(expected, got).expect("lines differ");
        assert!(report.contains("line 1"), "{report}");
        assert!(report.contains("expected `0,1.0`, got `0,2.0`"), "{report}");
    }

    #[test]
    fn diff_flags_missing_and_extra_lines() {
        let report = diff_plot_csv("a\nb\n", "a\n").expect("shorter");
        assert!(report.contains("missing"), "{report}");
        let report = diff_plot_csv("a\n", "a\nb\n").expect("longer");
        assert!(report.contains("unexpected"), "{report}");
    }
}
