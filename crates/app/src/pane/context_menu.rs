//! What the right-click that opened the layer menu resolved.
//!
//! A context menu stays open across frames while the chart under it keeps
//! moving, so every question the menu asks is answered once — at press time —
//! and held until the menu closes. That is the whole reason these five exist
//! rather than being re-derived per menu frame: a price re-read from the
//! pointer would follow the cursor mid-reach, and an index re-hit-tested would
//! go stale under a menu the trader is still reading.
//!
//! The rename buffer is here for the same reason from the other direction: it
//! is seeded on the press and edited across the frames the menu is open.

use crate::drawings::{self, ChartPoint};

/// The last right-click, as the press resolved it. See the module docs.
#[derive(Default)]
pub struct ContextMenu {
    /// Whether the right-click that opened the menu landed on the tape rather
    /// than on the candles. The two panes are configured apart, so the menu has
    /// to know which one was asked.
    pub(super) on_tape: bool,
    /// Price under the right-click that opened the layer menu — the trade
    /// section's anchor. Refreshed by every secondary click on the canvas.
    pub(super) price: Option<f64>,
    /// The placing entries of the last right-click: each registry tool that
    /// declares a `context_menu_label`, with the chart point *its own*
    /// `anchor_snap` resolved for that click — so the menu never re-derives
    /// a projection and a new tool's snap rule needs no edit here.
    pub(super) places: Vec<(drawings::DrawingTool, ChartPoint)>,
    /// The drawing under the last right-click, resolved at press time like
    /// the price and the tape flag. Held as an id, not an index: the menu
    /// stays open across frames, and an index can go stale under it.
    /// `pub(crate)` so the menu tests can stage the click's outcome.
    pub(crate) drawing: Option<drawings::DrawingId>,
    /// Rename buffer for the layer menu's drawing section, seeded from the
    /// clicked object's current name on the press that opened the menu.
    pub(super) rename: String,
}
