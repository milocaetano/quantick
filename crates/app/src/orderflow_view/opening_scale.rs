//! Explicit opening-burst display preference, shared with the control plane.

use super::OrderflowView;

impl OrderflowView {
    pub(crate) fn set_ignore_opening_burst_in_scale(&mut self, enabled: bool) -> bool {
        if self.config.volume_dots.ignore_opening_burst_in_scale == enabled {
            return false;
        }
        let before = self.config.clone();
        self.config.volume_dots.ignore_opening_burst_in_scale = enabled;
        self.commit_config_changes(before);
        true
    }
}
