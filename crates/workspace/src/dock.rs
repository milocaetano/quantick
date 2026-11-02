//! The right dock before anything draws it: which tab is open, how wide each is,
//! and the stable words each goes by.

use crate::arrangement_document::SavedDockTab;

/// Narrowest dock body the model allows.
pub const DOCK_MIN_WIDTH: f32 = 280.0;
/// Widest dock body the model allows.
pub const DOCK_MAX_WIDTH: f32 = 360.0;

/// One dock tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockTab {
    /// The replay session: what is playing, and the browser entry.
    Session,
    /// Paper trading: simulated position, order entry, pending orders.
    Trading,
    /// The trade ledger: closed simulated trades, this session and saved.
    Trades,
}

impl DockTab {
    /// Every tab, in strip order.
    pub const ALL: [Self; 3] = [Self::Session, Self::Trading, Self::Trades];

    /// The tab a saved name opens: an old file's L2 or Bubbles is no tab any more.
    #[must_use]
    pub const fn from_saved(tab: SavedDockTab) -> Option<Self> {
        match tab {
            SavedDockTab::L2 | SavedDockTab::Bubbles => None,
            SavedDockTab::Session => Some(Self::Session),
            SavedDockTab::Trading => Some(Self::Trading),
            SavedDockTab::Trades => Some(Self::Trades),
        }
    }

    /// The stable wire name, for the places that name a tab rather than draw
    /// it. Never the title: that is prose a reader sees, and rewording it must
    /// not move an operator's ground.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Trading => "trading",
            Self::Trades => "trades",
        }
    }

    /// Header title inside the body.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Session => "SESSION",
            Self::Trading => "PAPER TRADING · SIM",
            Self::Trades => "TRADES · SIM",
        }
    }

    /// Strip tooltip.
    #[must_use]
    pub const fn hover_text(self) -> &'static str {
        match self {
            Self::Session => "Market replay session",
            Self::Trading => "Paper trading — simulated orders, position and history",
            Self::Trades => "Closed simulated trades — this session and the saved history",
        }
    }

    /// This tab's slot in the per-tab width memory.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

impl From<DockTab> for SavedDockTab {
    fn from(tab: DockTab) -> Self {
        match tab {
            DockTab::Session => Self::Session,
            DockTab::Trading => Self::Trading,
            DockTab::Trades => Self::Trades,
        }
    }
}

/// The dock's chrome state. Hidden, collapsed to the strip, or open on one
/// tab; body width is remembered per tab.
#[derive(Debug)]
pub struct Dock {
    visible: bool,
    active: Option<DockTab>,
    widths: [f32; DockTab::ALL.len()],
}

impl Default for Dock {
    fn default() -> Self {
        Self::new()
    }
}

impl Dock {
    /// A visible dock, collapsed to its tab strip.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            visible: true,
            active: None,
            // Opening widths per tab, ordered as [`DockTab::ALL`]: the Session
            // tab carries the fewest controls.
            widths: [300.0, 320.0, 320.0],
        }
    }

    /// Whether any part of the dock (strip included) is on screen.
    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Show/hide the whole dock — the toolbar PANELS button and `Ctrl+Shift+B`.
    pub const fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    /// The open tab, or `None` while the dock is collapsed to its strip.
    /// A collapsed dock is a state in its own right, not an absence.
    #[must_use]
    pub const fn tab(&self) -> Option<DockTab> {
        self.active
    }

    /// Restore the dock exactly as a workspace recorded it: shown or hidden,
    /// with `tab` open or collapsed to the strip.
    ///
    /// Deliberately not [`Self::open_tab`] — that one *reveals* the dock, so
    /// it could never restore "hidden, with Trading last open", which is a
    /// cockpit a trader can arrange and would expect back.
    pub const fn restore(&mut self, visible: bool, tab: Option<DockTab>) {
        self.visible = visible;
        self.active = tab;
    }

    /// Open `tab`, revealing the dock if it was hidden. Never touches the
    /// layer the tab configures.
    pub const fn open_tab(&mut self, tab: DockTab) {
        self.visible = true;
        self.active = Some(tab);
    }

    /// Collapse to the strip, keeping the dock on screen.
    pub const fn collapse(&mut self) {
        self.active = None;
    }

    /// A strip click: the active tab collapses the body, any other opens it.
    pub fn select(&mut self, tab: DockTab) {
        self.active = if self.active == Some(tab) {
            None
        } else {
            Some(tab)
        };
    }

    /// The body width remembered for `tab`.
    #[must_use]
    pub const fn width(&self, tab: DockTab) -> f32 {
        self.widths[tab.index()]
    }

    /// Remember `width` for `tab`, clamped to the model's range.
    pub fn remember_width(&mut self, tab: DockTab, width: f32) {
        self.widths[tab.index()] = width.clamp(DOCK_MIN_WIDTH, DOCK_MAX_WIDTH);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_dock_shows_the_strip_with_no_tab_open() {
        let dock = Dock::new();
        assert!(dock.visible());
        assert_eq!(dock.tab(), None);
    }

    #[test]
    fn selecting_toggles_between_open_and_collapsed() {
        let mut dock = Dock::new();
        dock.select(DockTab::Trading);
        assert_eq!(dock.tab(), Some(DockTab::Trading));
        // A second click on the open tab collapses; nothing else opens.
        dock.select(DockTab::Trading);
        assert_eq!(dock.tab(), None);
        // Switching tabs is one click, not collapse-then-open.
        dock.select(DockTab::Trading);
        dock.select(DockTab::Trades);
        assert_eq!(dock.tab(), Some(DockTab::Trades));
    }

    #[test]
    fn open_tab_reveals_a_hidden_dock() {
        let mut dock = Dock::new();
        dock.toggle_visible();
        assert!(!dock.visible());
        dock.open_tab(DockTab::Session);
        assert!(dock.visible());
        assert_eq!(dock.tab(), Some(DockTab::Session));
    }

    #[test]
    fn widths_are_remembered_per_tab_within_the_model_range() {
        let mut dock = Dock::new();
        dock.remember_width(DockTab::Trading, 500.0);
        assert_eq!(dock.width(DockTab::Trading), DOCK_MAX_WIDTH);
        dock.remember_width(DockTab::Trades, 10.0);
        assert_eq!(dock.width(DockTab::Trades), DOCK_MIN_WIDTH);
        dock.remember_width(DockTab::Session, 300.0);
        assert_eq!(dock.width(DockTab::Session), 300.0);
    }

    #[test]
    fn an_old_l2_or_bubbles_tab_opens_no_dock_tab() {
        assert_eq!(DockTab::from_saved(SavedDockTab::L2), None);
        assert_eq!(DockTab::from_saved(SavedDockTab::Bubbles), None);
        for tab in DockTab::ALL {
            assert_eq!(DockTab::from_saved(tab.into()), Some(tab));
        }
    }

    #[test]
    fn every_tab_has_strip_metadata() {
        for tab in DockTab::ALL {
            assert!(!tab.id().is_empty());
            assert!(!tab.title().is_empty());
            assert!(!tab.hover_text().is_empty());
        }
    }
}
