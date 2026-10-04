//! The asset this view shows owns its bubble settings; see
//! [`quantick_stores::bubble_asset_store`].
use quantick_control_schema::orderflow::BubbleAssetSnapshot;
use quantick_stores::bubble_asset_store::AssetBinding;
use quantick_stores::bubble_assets::AssetBubbles;

use crate::bubble_presets::{BubblePreset, BubblePresetFile};

use super::OrderflowView;

impl OrderflowView {
    /// The stored presets, which an asset's declared look names.
    pub(crate) fn bubble_presets(&self) -> &BubblePresetFile {
        &self.presets
    }

    /// The asset this view's settings belong to, once a tab has bound one.
    pub(crate) fn asset(&self) -> Option<&AssetBinding> {
        self.asset.as_ref()
    }

    /// The settings on screen, with the flow pane's `candle_aggression`.
    pub(crate) fn asset_settings(&self, candle_aggression: bool) -> AssetBubbles {
        AssetBubbles {
            candle_aggression,
            flow_ignore_opening: self.ignore_flow_opening(),
            look: BubblePreset::capture(&self.look_name, &self.config),
        }
    }

    /// Put `settings` on screen as the asset `binding` names. The flow
    /// pane's candle aggression is the tab's to apply.
    pub(crate) fn bind_asset(&mut self, binding: AssetBinding, settings: &AssetBubbles) {
        self.asset = Some(binding);
        self.wear_asset(settings, false);
    }

    /// Wear what another view filed for this asset, else file what this one
    /// changed ([`AssetBinding::adoption`], [`AssetBinding::file`]). Returns
    /// the candle aggression to put on the flow pane when it adopted.
    pub(crate) fn sync_asset(&mut self, candle_aggression: bool) -> Option<bool> {
        let binding = self.asset.as_mut()?;
        if let Some((settings, lane_moved)) = binding.adoption() {
            self.wear_asset(&settings, !lane_moved);
            return Some(settings.candle_aggression);
        }
        if binding.edited() || binding.filed().candle_aggression != candle_aggression {
            let current = self.asset_settings(candle_aggression);
            self.asset.as_mut()?.file(current);
        }
        None
    }

    /// A setting outside the config changed: the look's name, the candles'
    /// opening scale.
    pub(crate) fn note_asset_change(&mut self) {
        if let Some(binding) = &mut self.asset {
            binding.note_change();
        }
    }

    /// Re-read the asset's declared look from the presets: an asset nobody
    /// tuned wears the new one, one with settings of its own keeps them.
    pub(super) fn follow_declared_look(&mut self) {
        let Some(binding) = &mut self.asset else {
            return;
        };
        self.preset_status = Some(match binding.refresh_declared(&self.presets) {
            Some(settings) => {
                self.wear_asset(&settings, false);
                format!("presets reloaded · '{}' applied", self.look_name)
            }
            None => format!(
                "presets reloaded · {} keeps its own settings",
                binding.key()
            ),
        });
    }

    /// Put `settings` on screen without counting it as an edit;
    /// `keep_navigation` leaves the lane's width and window where this
    /// view's own gestures put them.
    fn wear_asset(&mut self, settings: &AssetBubbles, keep_navigation: bool) {
        let before = self.config.clone();
        settings.look.apply_to(&mut self.config);
        if keep_navigation {
            self.config.live_lane.window = before.live_lane.window;
            self.config.live_lane.width_share = before.live_lane.width_share;
        }
        self.look_name.clone_from(&settings.look.name);
        self.preset_name_draft.clone_from(&settings.look.name);
        self.flow_execution
            .set_ignore_opening(settings.flow_ignore_opening);
        self.apply_config(before);
    }

    pub(crate) fn asset_snapshot(&self) -> Option<BubbleAssetSnapshot> {
        self.asset.as_ref().map(|binding| {
            let unsaved = binding.unsaved();
            BubbleAssetSnapshot {
                key: binding.key().to_owned(),
                source: binding.source().as_str().to_owned(),
                preset: self.look_name.clone(),
                saved: unsaved.is_none(),
                save_error: unsaved,
            }
        })
    }
}
