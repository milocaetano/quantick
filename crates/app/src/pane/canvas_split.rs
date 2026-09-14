//! The canvas split: which pane is which, and how the window is shared
//! between the flow chart and the context stack.
//!
//! `PaneSide` is the address every reader of the split uses; the two
//! constants and `split_time_pane` are the geometry the divider drags against.
//! Cut out of `pane.rs` as a pure move: the pane struct never reads these, the
//! tab and the layout code do.

use eframe::egui;

use super::PaneIndex;

/// Which of the canvas's panes something belongs to.
///
/// The flow pane, or one of the context panes by its slot in the stack —
/// `Time(0)` is the top context chart, `Time(1)` the one under it. A two-arm
/// enum lived here before the stack existed, and it is how the three-pane
/// canvas shipped with a dead second chart: every reader mapped `Time` to the
/// *first* context pane, so clicking the second focused the first, the BARS
/// group changed the first, and "add indicator" landed on the first. A side
/// that cannot name a pane cannot address it.
///
/// Slots are positions in [`crate::tab::Tab::time_panes`], and moving a pane
/// within the stack moves what a slot names — which is right for focus (it
/// follows the chart the trader is looking at) and irrelevant for everything
/// else, which addresses panes through [`PaneIndex`] and lives one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum PaneSide {
    #[default]
    Flow,
    Time(usize),
}

impl PaneSide {
    /// This side as a [`PaneIndex`] address: `0` is the flow pane, `1..` the
    /// context stack — the order [`crate::tab::Tab::pane_at`] uses.
    #[must_use]
    pub const fn index(self) -> PaneIndex {
        match self {
            Self::Flow => 0,
            Self::Time(slot) => slot + 1,
        }
    }

    /// The side a [`PaneIndex`] address names. The inverse of [`Self::index`].
    #[must_use]
    pub const fn from_index(index: PaneIndex) -> Self {
        match index {
            0 => Self::Flow,
            slot => Self::Time(slot - 1),
        }
    }

    /// What the chrome calls this pane: "Flow", "Timeframe", "Timeframe 2".
    ///
    /// The top context chart keeps the bare name every menu and status line
    /// showed while it was the only one; the number appears only where there
    /// is a second chart to tell it from.
    #[must_use]
    pub fn title(self) -> String {
        match self {
            Self::Flow => "Flow".to_owned(),
            Self::Time(0) => "Timeframe".to_owned(),
            Self::Time(slot) => format!("Timeframe {}", slot + 1),
        }
    }
}

/// Half-width of the divider's grab area, which reaches a little into both
/// panes so the handle is catchable without widening the rule itself.
pub const CANVAS_DIVIDER_HANDLE_PX: f32 = 5.0;

/// Where the divider sits when the split is first shown.
///
/// Roughly a third to the context pane, the rest to the flow pane. An even
/// split says the two charts matter equally, and in quantick they do not: the
/// heatmap is what the product is for, and the timeframe chart beside it is
/// context. The opening canvas should say so before the trader touches
/// anything — a default is an argument about what matters.
pub const DEFAULT_PANE_FRACTION: f32 = 0.35;

/// A time pane's area, split into the strip its selector sits in and the
/// chart below it.
pub struct TimePaneAreas {
    pub header: egui::Rect,
    pub chart: egui::Rect,
}

/// Hold a stored split inside the canvas.
///
/// A sanity clamp, not a floor. The floor is
/// [`canvas_layout::MIN_PANE_WIDTH_PX`], and it is applied where the canvas
/// width is known — inside the splitter, on every frame, for every pane.
/// Holding a *second* floor here as a share of the canvas is what made
/// collapse-by-drag unreachable: the share (a quarter) always bound before the
/// width (120 px) could, so a drag restarted at 400 px of a 1600 px canvas
/// every frame and could never travel far enough in one to dismiss the column.
/// One floor, one owner, and it is the one that knows how wide the canvas is.
#[must_use]
pub fn clamp_pane_fraction(fraction: f32) -> f32 {
    if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        DEFAULT_PANE_FRACTION
    }
}

/// Carve the time pane's header strip off the top of its area (§11); the rest
/// is the chart. The header is a strip rather than an overlay so the selector
/// is never painted across market data.
#[must_use]
pub fn split_time_pane(area: egui::Rect) -> TimePaneAreas {
    let split_y = (area.top() + crate::time_header::HEIGHT_PX).min(area.bottom());
    TimePaneAreas {
        header: egui::Rect::from_min_max(area.min, egui::pos2(area.right(), split_y)),
        chart: egui::Rect::from_min_max(egui::pos2(area.left(), split_y), area.max),
    }
}
