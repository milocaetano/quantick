//! What one indicator pane asks the layout for, and the floors every ask is
//! held to. The layout that shares the chart's height between the candles
//! and the panes lives with the window; the heights it is shared in live
//! here.

/// Fraction of the chart's height each indicator pane takes (plan §4.3:
/// fixed fraction v1, draggable dividers later).
pub const PANE_HEIGHT_FRAC: f32 = 0.20;

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
/// honest move is to stop drawing the curve and say so (the app's
/// `PaneSlot::collapsed`) rather than to draw something unreadable.
///
/// The number matters: at the smallest window the app allows, three panes come
/// to about 81 px each, so a floor set below that would never bite and this
/// whole rule would be dead code.
pub const MIN_PANE_HEIGHT_PX: f32 = 100.0;

/// Height of a collapsed pane: one row, enough for its name and its live
/// value. Never zero — a pane that vanished would be an indicator the user
/// added and the chart silently dropped.
pub const COLLAPSED_PANE_HEIGHT_PX: f32 = 20.0;

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
    pub fn desired(self, band_height_px: f32) -> f32 {
        match self {
            Self::Auto => (band_height_px * PANE_HEIGHT_FRAC).max(MIN_PANE_HEIGHT_PX),
            Self::Manual(px) => px.max(MIN_PANE_HEIGHT_PX),
            Self::Collapsed => COLLAPSED_PANE_HEIGHT_PX,
        }
    }
}
