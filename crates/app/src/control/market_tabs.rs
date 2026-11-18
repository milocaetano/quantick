//! The window's part in the feed and market calls: resolve the addressed tab
//! and lend the config, the layout's drawings and the History default.
use quantick_control_handlers::market_tab::{LentTab, MarketTabHost};

use crate::app::control_host::ControlPort;

impl MarketTabHost for dyn ControlPort {
    fn market_tab_mut(&mut self, index: usize) -> Option<LentTab<'_>> {
        self.tabs_mut().lend_tab(index)
    }
    fn drawing_gesture_on(&self, feed_id: &str, from: &str, to: &str) -> bool {
        self.workspace()
            .layout_state()
            .drawing_gesture_on(feed_id, from, to)
    }
    fn carry_market_drawings(&mut self, feed_id: &str, from: &str, to: &str) -> usize {
        self.workspace_calls()
            .layout()
            .carry_market_drawings(feed_id, from, to)
    }
}
