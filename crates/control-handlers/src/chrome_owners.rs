//! What the window lends the Home tab, widget pane and market palette calls:
//! the owner each one acts on, borrowed for that call. The window resolves an
//! owner and decides nothing; the families in [`crate::home_tab`],
//! [`crate::widgets`] and [`crate::market_palette`] act on what it lends.
use quantick_control_schema::widget::WidgetRead;
use quantick_engine::bar_registry::BarConfiguration;
use quantick_stores::market_list::{MarketSnapshot, OpenChoice, SwitchTarget};
use quantick_stores::symbol_catalog::SymbolCatalog;
use quantick_workspace::arrangement::strip::{MarketShown, StripSurface};

/// The new tab's id, or why it did not open.
pub type TabOpening = Result<u64, String>;

/// The owners the window lends. Each default lends nothing, which every call
/// answers as unavailable.
pub trait ChromeOwners {
    /// The strip's Home surface beside the market Home would hide.
    fn home_tab(&mut self) -> Option<HomeTab<'_>> {
        None
    }
    /// The widget panes and the tab they paint for.
    fn widgets(&mut self) -> Option<Box<dyn WidgetOwner + '_>> {
        None
    }
    /// What "Open a market" lists now: the config's markets and the open tabs.
    fn listed_markets(&self) -> MarketSnapshot {
        MarketSnapshot::default()
    }
    /// The open tabs, as an open, a focus or a bar change moves them.
    fn market_tabs(&mut self) -> Option<Box<dyn MarketTabs + '_>> {
        None
    }
    /// Each feed's symbol list, which an add or a removal edits.
    fn market_catalog(&mut self) -> Option<SymbolCatalog<'_>> {
        None
    }
    /// Show the palette on `query`, switching `target`'s tab when there is one.
    fn show_market_palette(&mut self, _query: &str, _target: Option<SwitchTarget>) -> bool {
        false
    }
}

/// Home's owner for one call: the strip's surface and the market on screen,
/// which Home holds while it hides it.
pub struct HomeTab<'a> {
    surface: &'a mut StripSurface,
    shown: MarketShown<'a>,
}

impl<'a> HomeTab<'a> {
    #[must_use]
    pub fn new(surface: &'a mut StripSurface, shown: MarketShown<'a>) -> Self {
        Self { surface, shown }
    }
    /// Bring Home over the market on screen.
    pub fn show(self) {
        self.surface.show_home(self.shown);
    }
    /// Put the market Home hid back on screen.
    pub fn leave(self) {
        self.surface.show_market();
    }
}

/// The widget panes' owner, which the window lends the three capabilities.
pub trait WidgetOwner {
    /// Open or close the pane `id` names when `open` says so, then what it
    /// paints for the active tab; or why not.
    fn widget(&mut self, id: &str, open: Option<bool>) -> Result<WidgetRead, String>;
}

/// The open tabs as the palette moves them, lent for one call.
pub trait MarketTabs {
    /// Bring the tab at this zero-based place forward, off Home.
    fn focus_tab(&mut self, index: usize);
    /// Open a tab on the choice's market, on `bar` (none: the one any new
    /// tab takes), joining the choice's workspace ([`OpenChoice::arrival`]),
    /// and answer its id; a background choice keeps the tab on screen.
    fn open_tab(&mut self, choice: &OpenChoice, bar: Option<BarConfiguration>) -> TabOpening;
    /// Put tab `tab`'s flow pane on `bar`, as its bars menu does.
    fn set_flow_bar(&mut self, tab: u64, bar: BarConfiguration) -> Result<(), String>;
}

#[cfg(test)]
#[path = "chrome_owners_tests.rs"]
mod chrome_owners_tests;
