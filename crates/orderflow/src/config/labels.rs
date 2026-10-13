//! The words the bubble panel shows for its settings: plain formatting,
//! kept beside the settings they name rather than in the UI crate.

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::{BubbleRenderMode, BubbleSizeReference, ConsumptionMark};

/// The cluster window, or raw.
#[must_use]
pub fn cluster_label(milliseconds: i64) -> String {
    if milliseconds == 0 {
        "Raw".to_owned()
    } else {
        format!("{milliseconds} ms")
    }
}

/// The tape's cluster window, which may follow history's.
#[must_use]
pub fn lane_cluster_label(window: Option<i64>, inherited: i64) -> String {
    match window {
        None => format!("Same as history · {}", dust_label(inherited)),
        Some(0) => "Raw · one bubble per print".to_owned(),
        Some(milliseconds) => dust_label(milliseconds),
    }
}

/// A region's height in rows, or off.
#[must_use]
pub fn region_label(rows: u32) -> String {
    if rows <= 1 {
        "Off · one mark per row".to_owned()
    } else {
        format!("{rows} rows")
    }
}

/// A region's time window.
#[must_use]
pub fn region_window_label(milliseconds: i64) -> String {
    if milliseconds % 1_000 == 0 {
        format!("{} s", milliseconds / 1_000)
    } else {
        format!("{milliseconds} ms")
    }
}

/// The dust fold's window, or off.
#[must_use]
pub fn dust_label(milliseconds: i64) -> String {
    if milliseconds == 0 {
        "Off · draw every print".to_owned()
    } else if milliseconds % 1_000 == 0 {
        format!("{} s", milliseconds / 1_000)
    } else {
        format!("{milliseconds} ms")
    }
}

/// How the full-size reference is chosen.
#[must_use]
pub const fn size_reference_label(reference: BubbleSizeReference) -> &'static str {
    match reference {
        BubbleSizeReference::VisibleP99 => "Auto · session P99",
        BubbleSizeReference::VisibleMax => "Auto · largest in session",
        BubbleSizeReference::Fixed => "Fixed quantity",
    }
}

/// Flat or sphere.
#[must_use]
pub const fn render_mode_label(mode: BubbleRenderMode) -> &'static str {
    match mode {
        BubbleRenderMode::Flat => "Flat · 2D disc",
        BubbleRenderMode::Sphere => "Sphere · 3D shaded",
    }
}

/// How a print that ate liquidity says so.
#[must_use]
pub const fn consumption_mark_label(mark: ConsumptionMark) -> &'static str {
    match mark {
        ConsumptionMark::Crown => "Crown · arc outside the rim",
        ConsumptionMark::Front => "Front · line through the bubble",
        ConsumptionMark::None => "None",
    }
}

/// A quantity as a bubble labels it: three significant figures at most,
/// thousands as `K`, millions as `M`, billions as `B`.
#[must_use]
pub fn format_quantity(quantity: Decimal) -> String {
    let value = quantity.to_f64().unwrap_or(0.0);
    let absolute = value.abs();
    let (scaled, suffix) = if absolute >= 1_000_000_000.0 {
        (value / 1_000_000_000.0, "B")
    } else if absolute >= 1_000_000.0 {
        (value / 1_000_000.0, "M")
    } else if absolute >= 1_000.0 {
        (value / 1_000.0, "K")
    } else {
        (value, "")
    };
    let decimals = if scaled.abs() >= 100.0 {
        0
    } else if scaled.abs() >= 10.0 {
        1
    } else {
        2
    };
    let formatted = format!("{scaled:.decimals$}");
    format!("{}{suffix}", trim_decimal_zeros(&formatted))
}

fn trim_decimal_zeros(value: &str) -> &str {
    if value.contains('.') {
        value.trim_end_matches('0').trim_end_matches('.')
    } else {
        value
    }
}

/// Quantity and execution count inside a bubble. A cluster uses `×`; a
/// canvas-budget fold uses the renderer's supplied circled-plus glyph, so
/// visual aggregation cannot masquerade as executions at one price.
#[must_use]
pub fn bubble_label(
    quantity: Decimal,
    trade_count: usize,
    folded: u32,
    show_quantity: bool,
    show_count: bool,
    folded_glyph: &str,
) -> Option<String> {
    let mark = if folded > 1 { folded_glyph } else { "×" };
    match (show_quantity, show_count && trade_count > 1) {
        (false, false) => None,
        (true, false) => Some(format_quantity(quantity)),
        (false, true) => Some(format!("{mark}{trade_count}")),
        (true, true) => Some(format!(
            "{} · {mark}{trade_count}",
            format_quantity(quantity)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_honest_and_compact() {
        assert_eq!(
            bubble_label(Decimal::from(1_250), 4, 0, true, true, "⊕"),
            Some("1.25K · ×4".to_owned())
        );
        assert_eq!(format_quantity(Decimal::from(100)), "100");
        assert_eq!(format_quantity(Decimal::ZERO), "0");
        assert_eq!(bubble_label(Decimal::ONE, 1, 0, false, true, "⊕"), None);
    }

    #[test]
    fn a_fold_and_a_cluster_do_not_share_a_glyph() {
        assert_eq!(
            bubble_label(Decimal::from(20), 4, 0, true, true, "⊕"),
            Some("20 · ×4".to_owned())
        );
        assert_eq!(
            bubble_label(Decimal::from(20), 4, 4, true, true, "⊕"),
            Some("20 · ⊕4".to_owned())
        );
    }
}
