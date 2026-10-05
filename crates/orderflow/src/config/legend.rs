//! The canvas key's content: which layers can draw right now, one glyph and
//! one label each. The window flows it into its corner and paints the glyphs.

use super::theme::OrderflowRenderStyle;

/// What a legend entry's swatch depicts; the window draws each one.
#[derive(Debug, Clone, Copy)]
pub enum LegendGlyph {
    Heat,
    Buy,
    Sell,
    Aligned,
    DepthOnly,
    Gap,
}

/// The legend keys for this style, one per layer that can actually draw.
///
/// A layer draws when its family is active (L2 capture for the depth family,
/// the bubbles switch for aggression) *and* its own display switch is on.
/// Announcing anything else would describe a chart the viewer is not looking
/// at — the legend is a key for what is on screen, not a feature list.
///
/// "On screen" means either pane. The canvas holds two of them and the layers
/// are switched apart, so a key withheld because the candles are clear would
/// deny a mark the tape is drawing right now — the legend has one canvas to
/// describe, not one pane of it.
#[must_use]
pub fn legend_entries(
    style: &OrderflowRenderStyle,
    liquidity_label: String,
) -> Vec<(LegendGlyph, String)> {
    let depth = style.depth_layer || style.lane_depth_layer;
    let aggression = style.aggression_layer || style.lane_aggression_layer;
    let mut entries = Vec::new();
    if depth && style.show_liquidity {
        entries.push((LegendGlyph::Heat, liquidity_label));
    }
    if aggression && style.show_buy {
        entries.push((LegendGlyph::Buy, "buy aggression".to_owned()));
    }
    if aggression && style.show_sell {
        entries.push((LegendGlyph::Sell, "sell aggression".to_owned()));
    }
    if depth && style.show_aligned {
        entries.push((
            LegendGlyph::Aligned,
            "aggression-aligned depletion".to_owned(),
        ));
    }
    if depth && style.show_unattributed {
        entries.push((
            LegendGlyph::DepthOnly,
            "L2 reduction (unattributed)".to_owned(),
        ));
    }
    if depth && style.show_gaps {
        entries.push((LegendGlyph::Gap, "L2 gap".to_owned()));
    }
    entries
}

#[cfg(test)]
#[path = "legend_tests.rs"]
mod tests;
