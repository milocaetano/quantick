//! How the chart's height is shared between the candles and the indicator
//! panes stacked under them.

use eframe::egui;

use super::{MAX_PANES, PANE_HEIGHT_FRAC};

/// Shortest a pane may be drawn and still be read.
///
/// Added up rather than guessed, from what a pane actually has to fit:
///
/// | Piece | Pixels |
/// | --- | --- |
/// | Title + live value row (`PANE_LABEL_FONT_PX` + its inset, top and bottom) | 28 |
/// | Two axis labels: `AXIS_LABEL_MIN_GAP_PX` between, `AXIS_LABEL_EDGE_MARGIN_PX` clear of each edge | 32 |
/// | Curve amplitude worth reading a shape from | 40 |
///
/// A hundred pixels. Below it the labels start dropping out and the trace has
/// nowhere to move, so the pane is chrome with a squiggle in it — and the
/// honest move is to stop drawing the curve and say so
/// ([`PaneSlot::collapsed`]) rather than to draw something unreadable.
///
/// The number matters: at the smallest window the app allows, three panes come
/// to about 81 px each, so a floor set below that would never bite and this
/// whole rule would be dead code.
pub const MIN_PANE_HEIGHT_PX: f32 = 100.0;

/// Height of a collapsed pane: one row, enough for its name and its live
/// value. Never zero — a pane that vanished would be an indicator the user
/// added and the chart silently dropped.
pub const COLLAPSED_PANE_HEIGHT_PX: f32 = 20.0;

/// Shortest the candles may be squeezed to by the pane band. The candles are
/// what the panes are *about*; a band that leaves them a sliver has inverted
/// the chart.
pub const MIN_CHART_HEIGHT_PX: f32 = 120.0;

/// What decides one pane's height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaneSizing {
    /// The layout decides: a share of the band, floored, collapsing when the
    /// band cannot hold it.
    Auto,
    /// The user dragged this pane's divider to a height in pixels. Still
    /// floored — a drag cannot produce a pane too short to read either.
    Manual(f32),
    /// The user collapsed it by hand. Stays collapsed however much room
    /// appears, until they expand it again.
    Collapsed,
}

impl PaneSizing {
    /// Height this sizing asks for in a band `band_height_px` tall, before the
    /// layout decides whether there is room for it.
    fn desired(self, band_height_px: f32) -> f32 {
        match self {
            Self::Auto => (band_height_px * PANE_HEIGHT_FRAC).max(MIN_PANE_HEIGHT_PX),
            Self::Manual(px) => px.max(MIN_PANE_HEIGHT_PX),
            Self::Collapsed => COLLAPSED_PANE_HEIGHT_PX,
        }
    }
}

/// One pane's band, and whether it is showing its curve or only its name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneSlot {
    pub rect: egui::Rect,
    /// `true`: too little room to draw this pane legibly, so it is a labelled
    /// strip instead. Collapsing hides the *curve*; the name and the live
    /// value are still written, because those are what the user added the
    /// indicator for.
    pub collapsed: bool,
}

/// Carve the pane band off the bottom of the chart rect.
///
/// Two rules the old fraction-only split had neither of:
///
/// - **Nothing is drawn below the height at which it can be read.** A pane
///   that cannot have [`MIN_PANE_HEIGHT_PX`] becomes a collapsed strip rather
///   than a squeezed one. Three slivers are worth less than one readable pane
///   and two labelled strips.
/// - **The candles keep [`MIN_CHART_HEIGHT_PX`].** Panes are read against the
///   bars; a band that eats the bars has inverted the chart.
///
/// Room is granted top-down, so the first pane — the one the user added first,
/// and the one the eye lands on — is the last to lose its curve. Every pane
/// always gets at least a strip: an indicator that is on must never be
/// silently absent.
///
/// Pure, so the split is unit-testable without a display.
pub fn split_panes(chart: egui::Rect, sizing: &[PaneSizing]) -> (egui::Rect, Vec<PaneSlot>) {
    let count = sizing.len().min(MAX_PANES);
    if count == 0 {
        return (chart, Vec::new());
    }
    let band_height = chart.height().max(0.0);
    // Strips are mandatory chrome; only the expansion above a strip has to be
    // negotiated for.
    let mandatory = COLLAPSED_PANE_HEIGHT_PX * count as f32;
    let mut budget = (band_height - MIN_CHART_HEIGHT_PX - mandatory).max(0.0);

    // Explicit choices are served first, in two passes over the same order:
    // a pane the user dragged or expanded by hand must get its height even
    // when the automatic ones above it would have spent the budget. Without
    // this, clicking a collapsed strip open is a click that does nothing.
    let mut heights = vec![COLLAPSED_PANE_HEIGHT_PX; count];
    for explicit in [true, false] {
        for (index, sizing) in sizing[..count].iter().enumerate() {
            if matches!(sizing, PaneSizing::Manual(_)) != explicit {
                continue;
            }
            if matches!(sizing, PaneSizing::Collapsed) {
                continue;
            }
            let desired = sizing.desired(band_height);
            let extra = desired - COLLAPSED_PANE_HEIGHT_PX;
            if extra <= budget {
                budget -= extra;
                heights[index] = desired;
            }
        }
    }

    let total: f32 = heights.iter().sum();
    let chart_bottom = (chart.bottom() - total).max(chart.top());
    let shrunk = egui::Rect::from_min_max(chart.min, egui::pos2(chart.right(), chart_bottom));
    let mut top = chart_bottom;
    let panes = heights
        .into_iter()
        .map(|height| {
            let rect = egui::Rect::from_min_max(
                egui::pos2(chart.left(), top),
                egui::pos2(chart.right(), top + height),
            );
            top += height;
            PaneSlot {
                rect,
                collapsed: height <= COLLAPSED_PANE_HEIGHT_PX,
            }
        })
        .collect();
    (shrunk, panes)
}
