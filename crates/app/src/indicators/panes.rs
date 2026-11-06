//! How the chart's height is shared between the candles and the indicator
//! panes stacked under them. What each pane asks for, and the floors the
//! ask is held to, are headless in `quantick_chart::indicator_views`; the
//! carve over a window rect is here.

use eframe::egui;

use quantick_chart::indicator_views::{COLLAPSED_PANE_HEIGHT_PX, PaneSizing};

/// Shortest the candles may be squeezed to by the pane band. The candles are
/// what the panes are *about*; a band that leaves them a sliver has inverted
/// the chart.
#[cfg(test)]
pub use quantick_chart::indicator_views::MIN_CHART_HEIGHT_PX;

/// One pane's band, and whether it is showing its curve or only its name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneSlot {
    pub rect: egui::Rect,
    /// `true`: too little room to draw this pane legibly, so it is a labelled
    /// strip instead. Collapsing hides the *curve*; the name and the live
    /// value are still written, because those are what the user added the
    /// indicator for.
    pub collapsed: bool,
    pub header_height: f32,
}

/// Carve the pane band off the bottom of the chart rect.
///
/// Two rules the old fraction-only split had neither of:
///
/// - **Nothing is drawn below the height at which it can be read.** A pane
///   that cannot have
///   [`MIN_PANE_HEIGHT_PX`](quantick_chart::indicator_views::MIN_PANE_HEIGHT_PX)
///   becomes a collapsed strip rather
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
#[cfg(test)]
pub fn split_panes(chart: egui::Rect, sizing: &[PaneSizing]) -> (egui::Rect, Vec<PaneSlot>) {
    split_panes_with_header(chart, sizing, 18.0)
}
pub fn split_panes_with_header(
    chart: egui::Rect,
    sizing: &[PaneSizing],
    header_height: f32,
) -> (egui::Rect, Vec<PaneSlot>) {
    let strip_height = COLLAPSED_PANE_HEIGHT_PX * (header_height / 18.0);
    let heights =
        quantick_chart::indicator_views::pane_heights(chart.height(), sizing, strip_height);
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
                header_height,
                collapsed: height <= strip_height,
            }
        })
        .collect();
    (shrunk, panes)
}
