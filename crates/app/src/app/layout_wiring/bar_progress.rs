//! The forming bar's progress on a market's price tag: read and switched in
//! the market settings document, for the market a tab shows.
use super::{LayoutAdapter, LayoutRead};

impl LayoutRead<'_> {
    /// Whether tab `tab`'s market prints the forming bar's progress on its
    /// last-price tag; on for a tab that is gone, the setting's default.
    pub(crate) fn bar_progress_on(&self, tab: u64) -> bool {
        let tab = self.tabs.by_id(tab).map(LayoutAdapter::market_of);
        tab.is_none_or(|(venue, symbol)| self.market.bar_progress_on(venue, symbol))
    }
}

impl crate::app::chart_layers_wiring::LayerWiring<'_> {
    /// Switch the progress line on tab `tab`'s market's price tag, saved with
    /// the market settings; whether it changed, `None` for a tab that is gone.
    /// The one writer, for the named call and the axis menu's drained wish.
    pub(crate) fn set_bar_progress(&mut self, tab: u64, on: bool) -> Option<bool> {
        let (venue, symbol) = LayoutAdapter::market_of(self.tabs.by_id(tab)?);
        let now = std::time::Instant::now();
        let market = self.workspace.layouts_mut().market_mut();
        Some(market.set_bar_progress(venue, symbol, on, now))
    }
}
