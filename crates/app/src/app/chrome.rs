//! The window chrome's own state.
//!
//! Not a surface and not a host: a declaration site. The ten fields below are
//! read by six modules and written by five, and no one of them owns enough of
//! the group to be its home — `menu_bar` names one of them once. So the group
//! lives where nothing else does, and every module reaches it by the same
//! path.

use eframe::egui;

use crate::window_scale;

/// The window chrome's transient state: where a control was last drawn,
/// which picker is open, and what the frame has not told the workspace yet.
///
/// No module owns this group, so none of them hosts it — the rectangles are
/// published by the draw that paints them and read by the hook that clicks
/// them, the pickers live exactly as long as they are open, and the window's
/// own size and surface probe are read once each. What they share is that
/// none of it is chart state and none of it outlives the frame that is
/// drawing, except to reach the workspace on the frame after.
pub(super) struct ChromeState {
    /// Where the offline chip was drawn, or `None` when it was not.
    ///
    /// Written as part of drawing it, exactly as a pane records its own chart
    /// area, and for the same two reasons. It says what is *painted* rather
    /// than what a fresh reading of the clock would have painted a
    /// millisecond later, so the scene and the screen cannot disagree across
    /// the edge of a stall budget. And it is the one control on screen with
    /// no capability behind it — opening a popup is a gesture, not a call —
    /// so its rectangle is the only way an operator reaches it at all.
    ///
    /// A `Rect` per frame, and only while the chart is not being fed. Nothing
    /// is recorded on a healthy chart, which is every frame of a normal
    /// session.
    pub(super) feed_chip_rect: Option<egui::Rect>,

    /// The tab whose chip opened the feed's recovery popup, if any.
    ///
    /// Opened by clicking the offline chip and by nothing else — the rule the
    /// trader asked for, after a card that opened itself over the chart every
    /// morning. It is the *tab's* id rather than a window-wide flag because
    /// one dead terminal stalls every MT5 tab at once: a bare flag opened on
    /// one chart and then found the next chart already offline, and drew
    /// itself there with nobody having clicked anything. The chip is window
    /// chrome speaking for the active market, and this says which market it
    /// was speaking for.
    ///
    /// Leaving that chart closes it, the way clicking elsewhere does: the
    /// frame answers for the tab it is drawing, so a switch clears the flag.
    /// A glance, not a mode — nothing waits on a chart nobody is looking at.
    pub(super) feed_popup_tab: Option<u64>,

    /// Whether the toolbar's layout popover is open.
    pub(super) layout_picker_open: bool,

    /// The layout being renamed in the strip, with the draft name.
    pub(super) layout_rename: Option<(crate::layouts::LayoutId, String)>,

    /// The layout a delete is waiting on: deleting takes its drawings with
    /// it, on disk too, so it is the one strip action behind a confirmation.
    pub(super) layout_delete_confirm: Option<crate::layouts::LayoutId>,

    /// Where the Workspace button was drawn, published by the menu bar so the
    /// hook can click it rather than guess at a coordinate.
    pub(super) workspace_menu_rect: Option<egui::Rect>,

    /// Where the toolbar's history caret is, published by the draw. `None`
    /// while the menu is unreachable — a feed that pages nothing has no menu
    /// to open, and a hook must photograph that rather than force it.
    pub(super) history_menu_rect: Option<egui::Rect>,

    /// The window's inner size as of the last frame, in points — captured here
    /// because the size a workspace records is the one the user last saw, and
    /// by exit time the viewport has already been asked to close.
    pub(super) window_size: Option<[f32; 2]>,

    /// The window this app is drawing into, kept so the health summary can
    /// report the client area the platform believes it has — see
    /// [`crate::window_scale`] for why that number is worth logging, and for
    /// the defect it was measured chasing.
    pub(super) surface: Option<window_scale::SurfaceProbe>,

    /// The popup's position changed by hand this frame and the workspace has
    /// not been told yet.
    ///
    /// The position itself is automatic until the user drags the title bar and
    /// manual from then on (only ever re-clamped), and the chart rectangle it
    /// is placed against belongs to the focused [`ChartPane`] — so a split
    /// window places against the pane the selection lives on, not the window.
    ///
    /// A flag rather than a write on the spot, for two reasons. A drag reports
    /// a new position on *every* frame the hand is moving, and writing the file
    /// sixty times a second for a window that has not landed yet is a lot of
    /// disk for one decision. And the write itself belongs beside the other
    /// workspace writes ([`Self::maintain_workspace`]), not inside the closure
    /// that is painting the window — one place that knows how a workspace
    /// reaches the disk, not two.
    ///
    /// That host runs at the top of a frame, so the file is written on the
    /// frame *after* the one the hand came off in — sixteen milliseconds, and
    /// the frame that closes the window flushes this before taking the exit
    /// save, so nothing can be dropped between the two.
    pub(super) inspector_position_dirty: bool,
}
