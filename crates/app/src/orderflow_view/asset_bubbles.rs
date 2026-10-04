//! The asset this view shows owns its bubble settings; see
//! [`quantick_stores::bubble_assets`].
use quantick_control_schema::orderflow::BubbleAssetSnapshot;
use quantick_stores::bubble_assets::{AssetBubbles, AssetTrack};

use crate::bubble_presets::{BubblePreset, BubblePresetFile};

use super::OrderflowView;

impl OrderflowView {
    /// The stored presets, which an asset's declared look names.
    pub(crate) fn bubble_presets(&self) -> &BubblePresetFile {
        &self.presets
    }

    /// The asset this view's settings belong to, once a tab has bound one.
    pub(crate) fn asset(&self) -> Option<&AssetTrack> {
        self.asset.as_ref()
    }

    pub(crate) fn asset_mut(&mut self) -> Option<&mut AssetTrack> {
        self.asset.as_mut()
    }

    /// The settings on screen, with the flow pane's `candle_aggression`.
    pub(crate) fn asset_settings(&self, candle_aggression: bool) -> AssetBubbles {
        AssetBubbles {
            candle_aggression,
            flow_ignore_opening: self.ignore_flow_opening(),
            look: BubblePreset::capture(&self.look_name, &self.config),
        }
    }

    /// Put `settings` on screen as the asset `track` names. The flow pane's
    /// candle aggression is the tab's to apply.
    pub(crate) fn bind_asset(&mut self, track: AssetTrack, settings: &AssetBubbles) {
        let before = self.config.clone();
        settings.look.apply_to(&mut self.config);
        self.look_name.clone_from(&settings.look.name);
        self.preset_name_draft.clone_from(&settings.look.name);
        self.set_ignore_flow_opening(settings.flow_ignore_opening);
        self.preset_status = Some(format!(
            "{} · '{}' ({})",
            track.key(),
            settings.look.name,
            track.source().as_str()
        ));
        self.asset = Some(track);
        self.commit_config_changes(before);
    }

    pub(crate) fn asset_snapshot(&self) -> Option<BubbleAssetSnapshot> {
        self.asset.as_ref().map(|track| BubbleAssetSnapshot {
            key: track.key().to_owned(),
            source: track.source().as_str().to_owned(),
            preset: self.look_name.clone(),
        })
    }
}
