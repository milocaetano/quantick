//! The application adapter for the home tab.

use crate::app::QuantickApp;
use quantick_control_handlers::home_tab::{HomeTab, HomeTabHost};

impl HomeTabHost for QuantickApp {
    fn home_tab(&mut self) -> Option<HomeTab<'_>> {
        let roots = crate::app::control_host::control_roots_mut(self);
        let shown = crate::app::home_wiring::market_shown(&roots.runtime.tabs);
        Some(HomeTab::new(&mut roots.chrome.title_bar.surface, shown))
    }
}
