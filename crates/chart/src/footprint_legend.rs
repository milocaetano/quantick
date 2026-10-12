//! The footprint layer's one-line legend, as text: what the layer is drawing,
//! how much further to zoom (wider for numbers, taller for single rows), what
//! a row stands for and every filter in force. The window paints the string;
//! every word of it is decided here, so it reads the same wherever it is
//! asked for and is testable without a frame.

use rust_decimal::Decimal;

use crate::footprint_lod::DetailLevel;
use crate::footprint_projection::fmt_qty;

/// Everything the legend says, measured by the caller for this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendFacts<'a> {
    /// The style the trader asked for, by id.
    pub requested_style: &'a str,
    /// The style actually drawn, by id: a hand-over names both.
    pub drawn_style: &'a str,
    /// What the drawn style's Detailed columns are ("sell|buy", ...).
    pub detailed_legend: &'a str,
    pub level: DetailLevel,
    /// The candle width on screen, in pixels.
    pub candle_width: f32,
    /// The on-screen candle width at which the summary numbers arrive.
    pub numbers_at_px: f32,
    /// The price one display row stands for (capture group times multiple).
    pub rows: Decimal,
    /// How much taller merged rows must draw to split (`height_to_split`).
    pub split_hint: Option<f32>,
    /// The imbalance floor in force.
    pub min_qty: Decimal,
    /// The drawn style is the heat-coloured cluster.
    pub heat_cells: bool,
    /// A cap-coarsened ladder was hidden.
    pub aggregated_any: bool,
    pub side_inferred: bool,
    /// The depth map is shown and the drawn style paints an opaque plate.
    pub plate_over_map: bool,
    /// The cell budget ran out before every row was painted.
    pub capped: bool,
    /// The zone cap kept only this many of the strongest zones.
    pub zone_cap: Option<usize>,
    /// The `QUANTICK_FOOTPRINT_DEBUG` suffix, when asked for.
    pub debug: Option<&'a str>,
}

/// The legend line for `facts`.
#[must_use]
pub fn legend_text(facts: &LegendFacts<'_>) -> String {
    let level = facts.level;
    let mut text = String::from("footprint");
    // A style that handed over says so, naming both: the trader asked for one
    // reading and is looking at another, and a chart that quietly became a
    // different chart is the same defect as a layer that is on and invisible.
    if facts.drawn_style != facts.requested_style {
        text.push_str(&format!(
            " · {} → {}",
            facts.requested_style, facts.drawn_style
        ));
    }
    match level {
        DetailLevel::Off => text.push_str(" · zoom in for detail"),
        DetailLevel::Marks => text.push_str(" · marks"),
        DetailLevel::Profile => text.push_str(" · profile"),
        DetailLevel::Compact => text.push_str(" · delta"),
        // The legend names what the columns actually are — "sell|buy" over
        // a delta ladder would misread every number (data honesty).
        DetailLevel::Detailed => {
            text.push_str(" · ");
            text.push_str(facts.detailed_legend);
        }
    }
    // How much further to zoom, in the only unit the gesture has. "zoom in for
    // numbers" with no number is why this layer read as slow to arrive: a
    // trader could not tell a nudge from a different chart entirely.
    if level < DetailLevel::Compact && facts.candle_width > 0.0 {
        let further = facts.numbers_at_px / facts.candle_width;
        if further > 1.05 {
            text.push_str(&format!(" · numbers at {further:.1}× this width"));
        }
    }
    // The effective grouping is always spoken: the number a row stands for
    // must never change meaning silently (data honesty).
    text.push_str(&format!(" · rows {}", facts.rows));
    // Merged rows say how much taller the candles must get to split them,
    // the height twin of the width hint above.
    if let Some(taller) = facts.split_hint {
        text.push_str(&format!(" · single rows at {taller:.1}× height"));
    }
    // The imbalance floor in force, spoken like the rows are: a highlight
    // whose threshold is secret reads as arbitrary.
    if level > DetailLevel::Off && !facts.min_qty.is_zero() {
        text.push_str(&format!(" · min qty {}", fmt_qty(facts.min_qty)));
    }
    // What the cell colours mean. A six-step scale with no key is a chart
    // asking to be guessed at: bright could be "a lot" or "imbalanced", and
    // the two lead to opposite trades.
    if facts.heat_cells && level >= DetailLevel::Detailed {
        text.push_str(" · heat: cell volume vs the screen");
    }
    if facts.aggregated_any {
        text.push_str(" · coarsened bars hidden");
    }
    if facts.side_inferred {
        text.push_str(" · side inferred");
    }
    // The plate is opaque by design, so where the map used to show through it
    // no longer does — said out loud, and only where a plate is painted:
    // claiming occlusion that is not there undercuts the same guarantee.
    if facts.plate_over_map && level >= DetailLevel::Profile {
        text.push_str(" · map hidden behind the bars");
    }
    if facts.capped {
        text.push_str(" · capped");
    }
    if let Some(cap) = facts.zone_cap {
        text.push_str(&format!(" · strongest {cap} zones"));
    }
    if let Some(debug) = facts.debug {
        text.push_str(debug);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> LegendFacts<'static> {
        LegendFacts {
            requested_style: "split",
            drawn_style: "split",
            detailed_legend: "sell|buy",
            level: DetailLevel::Profile,
            candle_width: 20.0,
            numbers_at_px: 46.0,
            rows: Decimal::from(10),
            split_hint: Some(2.3),
            min_qty: Decimal::ZERO,
            heat_cells: false,
            aggregated_any: false,
            side_inferred: false,
            plate_over_map: false,
            capped: false,
            zone_cap: None,
            debug: None,
        }
    }

    #[test]
    fn the_legend_names_both_zoom_axes() {
        assert_eq!(
            legend_text(&facts()),
            "footprint · profile · numbers at 2.3× this width · rows 10 · single rows at 2.3× height"
        );
    }

    #[test]
    fn a_hand_over_and_the_filters_are_spoken() {
        let text = legend_text(&LegendFacts {
            drawn_style: "bidask",
            requested_style: "cluster",
            level: DetailLevel::Detailed,
            split_hint: None,
            min_qty: Decimal::from(20),
            plate_over_map: true,
            zone_cap: Some(24),
            ..facts()
        });
        assert_eq!(
            text,
            "footprint · cluster → bidask · sell|buy · rows 10 · min qty 20 \
             · map hidden behind the bars · strongest 24 zones"
        );
    }
}
