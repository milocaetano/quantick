//! Golden test harness for bar builders: replays a committed trade fixture through a
//! [`BarBuilder`] and diffs the bars against a committed expected-bars fixture.
//! [`assert_golden`] also runs the builder twice and requires byte-identical output.

use std::fmt::Write as _;

use crate::{Bar, BarBuilder, Trade, fixture};

/// Feed every trade through `builder` in order, collecting the closed bars.
/// The trailing in-progress bar is not emitted (it is not closed); use [`BarBuilder::partial`].
#[must_use]
pub fn replay<B: BarBuilder + ?Sized>(builder: &mut B, trades: &[Trade]) -> Vec<Bar> {
    let mut bars = Vec::with_capacity(trades.len());
    for trade in trades {
        builder.push_into(trade, &mut bars);
    }
    bars
}

/// Compare produced bars against expected bars.
/// Returns `None` on an exact match, or `Some(report)` pinpointing every differing bar and field:
///
/// ```text
/// golden mismatch (expected 2 bars, got 2):
///   bar[1]: close expected 36000.30, got 36000.00
///           trade_count expected 3, got 2
/// ```
#[must_use]
pub fn diff_bars(expected: &[Bar], got: &[Bar]) -> Option<String> {
    if expected == got {
        return None;
    }
    let mut body = String::new();
    for i in 0..expected.len().max(got.len()) {
        match (expected.get(i), got.get(i)) {
            (Some(e), Some(g)) if e == g => {}
            (Some(e), Some(g)) => {
                let diffs = field_diffs(e, g);
                let _ = writeln!(body, "  bar[{i}]: {}", diffs.join("\n          "));
            }
            (Some(_), None) => {
                let _ = writeln!(body, "  bar[{i}]: missing (builder produced no such bar)");
            }
            (None, Some(_)) => {
                let _ = writeln!(
                    body,
                    "  bar[{i}]: unexpected (builder produced an extra bar)"
                );
            }
            (None, None) => unreachable!("index is below the longer length"),
        }
    }
    Some(format!(
        "golden mismatch (expected {} bars, got {}):\n{}",
        expected.len(),
        got.len(),
        body.trim_end(),
    ))
}

fn field_diffs(e: &Bar, g: &Bar) -> Vec<String> {
    let mut diffs = Vec::new();
    macro_rules! cmp {
        ($field:ident) => {
            if e.$field != g.$field {
                diffs.push(format!(
                    "{} expected {}, got {}",
                    stringify!($field),
                    e.$field,
                    g.$field
                ));
            }
        };
    }
    cmp!(open_time);
    cmp!(close_time);
    cmp!(open);
    cmp!(high);
    cmp!(low);
    cmp!(close);
    cmp!(buy_volume);
    cmp!(sell_volume);
    cmp!(trade_count);
    diffs
}

/// Assert replaying `trades_csv` yields `expected_bars_csv` twice over (`make_builder` is a factory).
/// # Panics
/// On a fixture parse failure, non-determinism, or bars differing (with the [`diff_bars`] report).
pub fn assert_golden<B, F>(make_builder: F, trades_csv: &str, expected_bars_csv: &str)
where
    B: BarBuilder,
    F: Fn() -> B,
{
    let trades = fixture::parse_trades(trades_csv).expect("trade fixture should parse");
    let expected =
        fixture::parse_bars(expected_bars_csv).expect("expected-bars fixture should parse");

    let run1 = replay(&mut make_builder(), &trades);
    let run2 = replay(&mut make_builder(), &trades);

    assert_eq!(
        fixture::write_bars(&run1),
        fixture::write_bars(&run2),
        "non-deterministic builder: replaying the same {} trades twice produced different bars",
        trades.len(),
    );

    if let Some(report) = diff_bars(&expected, &run1) {
        panic!("{report}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr as _;

    fn bar(close: &str, trade_count: u64) -> Bar {
        let c = Decimal::from_str(close).unwrap();
        Bar {
            open_time: 0,
            close_time: 0,
            open: c,
            high: c,
            low: c,
            close: c,
            buy_volume: Decimal::ZERO,
            sell_volume: Decimal::ZERO,
            trade_count,
        }
    }

    #[test]
    fn identical_bars_have_no_diff() {
        let bars = vec![bar("1.0", 1)];
        assert!(diff_bars(&bars, &bars).is_none());
    }

    #[test]
    fn diff_names_the_bar_and_the_fields() {
        let expected = vec![bar("36000.30", 3)];
        let got = vec![bar("36000.00", 2)];
        let report = diff_bars(&expected, &got).expect("bars differ");
        assert!(report.contains("bar[0]"), "{report}");
        assert!(
            report.contains("close expected 36000.30, got 36000.00"),
            "{report}"
        );
        assert!(report.contains("trade_count expected 3, got 2"), "{report}");
    }

    #[test]
    fn diff_flags_a_missing_bar() {
        let expected = vec![bar("1.0", 1), bar("2.0", 1)];
        let got = vec![bar("1.0", 1)];
        let report = diff_bars(&expected, &got).expect("lengths differ");
        assert!(report.contains("bar[1]: missing"), "{report}");
    }

    #[test]
    fn diff_flags_an_extra_bar() {
        let expected = vec![bar("1.0", 1)];
        let got = vec![bar("1.0", 1), bar("2.0", 1)];
        let report = diff_bars(&expected, &got).expect("lengths differ");
        assert!(report.contains("bar[1]: unexpected"), "{report}");
    }
}
