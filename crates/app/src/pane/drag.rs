//! The pause rule of a strategy region: one rule, read by the gate and the
//! badge. The pointer's grip on a drawing moved to `quantick_drawings::gesture`.

use crate::drawings;

/// Why an armed instance's region cannot honestly be tested right now, or
/// `None` when it can.
///
/// One rule, two readers: [`ChartPane::strategy_region`] shuts the gate on it
/// and [`ChartPane::badge_text_for`] prints it. Two copies would let the chart
/// paint a running bot over a region every bar is refused against — the
/// divergence a trader only discovers by watching a setup go by, which is
/// exactly how this was found.
///
/// The order is the order the trader can act on: another market needs the
/// region redrawn, a lost series needs the drawing re-anchored, a hidden one
/// needs a click.
pub fn region_pause(drawing: &drawings::Drawing, all_hidden: bool) -> Option<&'static str> {
    if drawing.foreign_market {
        return Some("region on another market — paused");
    }
    if drawing.off_series {
        return Some("region off its series — paused");
    }
    if drawing.hidden || all_hidden {
        return Some("region hidden — paused");
    }
    None
}
