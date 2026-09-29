//! Temporary source-declared tape looks return to the trader's prior appearance.
use crate::bubble_presets::BubblePreset;

use super::OrderflowView;

impl OrderflowView {
    /// Ordinary declarations keep their historical sticky behavior. A tape-only
    /// declaration instead borrows the panel until an undeclared market arrives.
    /// Manual preset selection never creates this source-owned restoration point.
    pub(crate) fn apply_source_preset(&mut self, name: Option<&str>) -> bool {
        let preset = match name {
            Some(name) => {
                let Some(preset) = self.presets.get(name).cloned() else {
                    return false;
                };
                if preset.live_lane.tape_only {
                    if self.source_preset_restore.is_none() {
                        self.source_preset_restore =
                            Some(BubblePreset::capture(&self.presets.active, &self.config));
                    }
                } else {
                    self.source_preset_restore = None;
                }
                preset
            }
            None => {
                let Some(previous) = self.source_preset_restore.take() else {
                    return true;
                };
                previous
            }
        };
        let before = self.config.clone();
        preset.apply_to(&mut self.config);
        self.presets.active = preset.name.clone();
        self.preset_name_draft = preset.name.clone();
        let action = if name.is_some() {
            "applied"
        } else {
            "restored"
        };
        self.preset_status = Some(format!("'{}' {action}", preset.name));
        self.commit_config_changes(before);
        true
    }
}
