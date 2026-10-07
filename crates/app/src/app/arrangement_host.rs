//! The app's tab runtime docked into the workspace arrangement.
use crate::tab::Tab;
use quantick_workspace::arrangement::{Arrangement, TabRuntime};

impl TabRuntime for Tab {
    fn feed(&self) -> &str {
        &self.feed_id
    }
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn close(&mut self) {
        Tab::close(self);
    }
}

/// Sole physical tab order and runtime effects for the window's tabs.
pub(crate) type ArrangementHost = Arrangement<Tab>;
