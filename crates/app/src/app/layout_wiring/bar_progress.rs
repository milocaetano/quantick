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

impl LayoutAdapter<'_> {
    /// Switch the progress line on tab `tab`'s market's price tag, saved with
    /// the market settings; whether it changed, `None` for a tab that is gone.
    pub(crate) fn set_bar_progress(&mut self, tab: u64, on: bool) -> Option<bool> {
        let (venue, symbol) = Self::market_of(self.tabs.by_id(tab)?);
        let now = std::time::Instant::now();
        Some((self.store.market_mut()).set_bar_progress(venue, symbol, on, now))
    }
}
