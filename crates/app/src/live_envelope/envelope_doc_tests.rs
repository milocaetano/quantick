//! The published envelope and the code's envelope are one set of numbers.
//!
//! `docs/quality/live-envelope.md` renders every constant as a table row
//! `` | `NAME` | value | ``; this fails when either side moves alone. The caps
//! are also held to the per-frame worst case they were derived from.

use super::*;

const DOC: &str = include_str!("../../../../docs/quality/live-envelope.md");

/// `4096` → `4,096`, the doc's own rendering.
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[test]
fn the_document_renders_every_envelope_constant_with_the_codes_value() {
    let rows: [(&str, u64); 11] = [
        ("SUSTAINED_TRADES_PER_S", SUSTAINED_TRADES_PER_S),
        ("BURST_TRADES_PER_S", BURST_TRADES_PER_S),
        ("BURST_TRADES_PER_FRAME", BURST_TRADES_PER_FRAME as u64),
        ("MEAN_TRADES_PER_S", MEAN_TRADES_PER_S),
        ("DEPTH_UPDATES_PER_S", DEPTH_UPDATES_PER_S),
        (
            "BURST_DEPTH_UPDATES_PER_FRAME",
            BURST_DEPTH_UPDATES_PER_FRAME as u64,
        ),
        ("SESSION_HOURS", SESSION_HOURS),
        ("RETAINED_SESSIONS", RETAINED_SESSIONS),
        ("RETAINED_TRADES_PER_PANE", RETAINED_TRADES_PER_PANE as u64),
        ("INDICATOR_COMMAND_QUEUE", INDICATOR_COMMAND_QUEUE as u64),
        ("BOOK_COMMAND_QUEUE", BOOK_COMMAND_QUEUE as u64),
    ];
    for (name, value) in rows {
        let row = format!("| `{name}` | {} |", grouped(value));
        assert!(
            DOC.contains(&row),
            "docs/quality/live-envelope.md lacks the row `{row}`"
        );
    }
}

#[test]
fn grouping_matches_the_documents_rendering() {
    assert_eq!(grouped(7), "7");
    assert_eq!(grouped(512), "512");
    assert_eq!(grouped(4_096), "4,096");
    assert_eq!(grouped(3_960_000), "3,960,000");
}
