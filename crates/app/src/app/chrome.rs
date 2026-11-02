//! The window chrome's own state.
//!
//! Not a surface and not a host: a declaration site. The ten fields below are
//! read by six modules and written by five, and no one of them owns enough of
//! the group to be its home — `menu_bar` names one of them once. So the group
//! lives where nothing else does, and every module reaches it by the same
//! path.

use eframe::egui;

use crate::window_scale;

/// The one in-place layout rename editor and the pane footer that owns it.
pub(super) struct LayoutRename {
    pub(super) tab: u64,
    pub(super) pane: crate::pane::PaneSide,
    pub(super) layout: crate::layouts::LayoutId,
    pub(super) draft: String,
}

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
    /// Every scenario hook an agent drives this window by, captured once at
    /// launch and named. See [`crate::harness`] for what belongs here and why
    /// the trunk asks it rather than holding its flags. Exists only with the
    /// scenario harness (or under test).
    #[cfg(any(feature = "scenario-harness", test))]
    pub(super) harness: crate::harness::Harness,
    /// Ordinary startup window command, consumed on the first frame.
    #[cfg(any(feature = "scenario-harness", test))]
    pub(super) window_startup: crate::launch::window::WindowStartupState,
    /// Saved override for recording deal counters; `None` follows feed config.
    pub(super) record_deals: Option<bool>,
    /// Where the status word was drawn while it is the popup's door, or `None`.
    ///
    /// Written as part of drawing it, so the scene says what is *painted*, and it
    /// is the one control with no capability behind it, so its rectangle is the
    /// only way an operator reaches it.
    pub(super) feed_chip_rect: Option<egui::Rect>,

    /// The tab whose status word opened the feed's recovery popup, if any.
    ///
    /// Opened by clicking the word and by nothing else. It is the *tab's* id
    /// rather than a window-wide flag because one dead terminal stalls every MT5
    /// tab at once, and a bare flag would draw itself on the next chart with
    /// nobody having clicked anything. Leaving that chart closes it.
    pub(super) feed_popup_tab: Option<u64>,

    /// Whether the toolbar's layout popover is open.
    pub(super) layout_picker_open: bool,

    /// The layout being renamed, its draft, and the pane-local strip that
    /// opened the editor. The catalogue mutation remains workspace-wide; only
    /// the transient text box belongs to one footer.
    pub(super) layout_rename: Option<LayoutRename>,

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

    /// The window this app is drawing into, kept so the health summary can
    /// report the client area the platform believes it has — see
    /// [`crate::window_scale`] for why that number is worth logging, and for
    /// the defect it was measured chasing.
    pub(super) surface: Option<window_scale::SurfaceProbe>,
}
