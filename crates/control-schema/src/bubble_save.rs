//! "Save changes for this asset": whether the bubble settings a pane changes
//! are stored for the asset it shows.

use quantick_control::{
    registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy},
    wire::WireU64,
};
use quantick_stores::bubble_asset_store::SaveSwitch;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SAVE_CHANGES_CAPABILITY_ID: &str = "orderflow.bubbles.save_changes.set";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveChangesInput {
    pub tab_id: WireU64,
    /// The pane whose bubble settings belong to an asset: the tab's flow
    /// pane (`orderflow.bubbles` reports its `asset`).
    pub pane_id: WireU64,
    /// `true` stores every change for the asset again, starting with what
    /// this pane shows now — unless the asset's stored settings changed
    /// since this pane last showed them (another tab's change, an import):
    /// then this pane drops what it shows for the stored settings, and the
    /// result's `screen` says so. `false` keeps later changes on the pane
    /// they are made on, for this session only.
    pub save_changes: bool,
}

/// What became of what the addressed pane showed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SwitchedScreen {
    /// It stays on the pane: the switch went off, or did not move.
    Kept,
    /// Saving came on and it is now the asset's stored settings, which
    /// every tab on the asset wears.
    Stored,
    /// Saving came on, but the asset's stored settings had changed since
    /// the pane last showed them: the pane dropped what it showed and wears
    /// the stored settings; nothing of its screen was stored.
    ReplacedByStored,
}

impl From<SaveSwitch> for SwitchedScreen {
    fn from(switch: SaveSwitch) -> Self {
        match switch {
            SaveSwitch::Unmoved | SaveSwitch::Off => Self::Kept,
            SaveSwitch::ScreenStored => Self::Stored,
            SaveSwitch::StoredTaken => Self::ReplacedByStored,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SaveChangesResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// The asset whose switch this is: every tab on it shares it.
    pub asset: String,
    pub save_changes: bool,
    pub changed: bool,
    /// What became of what the addressed pane showed.
    pub screen: SwitchedScreen,
}

impl SaveChangesInput {
    pub fn result(self, asset: String, switch: SaveSwitch) -> SaveChangesResult {
        SaveChangesResult {
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            asset,
            save_changes: self.save_changes,
            changed: switch.moved(),
            screen: switch.into(),
        }
    }
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<SaveChangesInput, SaveChangesResult>(
        SAVE_CHANGES_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Save bubble changes for this asset",
        "Switches the asset's \"Save changes for this asset\", which every tab on that asset shares; it starts on. On, a bubble setting changed on a pane is stored for the asset the pane shows and every tab on it wears it. Off, a change stays on the pane it was made on for this session: not stored, not shown in other tabs on the asset, which keep the stored settings; showing another market in that tab and back, or restarting, brings the stored settings back. Switching it on again stores what the addressed pane shows now and other tabs on the asset wear it — unless the asset's stored settings changed since that pane last showed them (another tab's change, an import): then the pane drops what it shows and wears the stored settings, and nothing of its screen is stored. The result's screen says which happened: stored, replaced_by_stored, or kept when the switch went off or did not move. The switch itself is stored. No other asset and no shared presets file is touched.",
        "Stable tab and pane IDs are resolved before switching; a pane whose bubbles belong to no asset is refused. The result and orderflow.bubbles report the asset and its switch; the result also reports what became of the pane's screen.",
    );
    descriptor.persistence = EffectPersistence::Durable;
    descriptor
}

pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::snapshot(
    SAVE_CHANGES_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    crate::orderflow::BUBBLES_SCOPE_ID,
    "tabs[].panes[].bubbles.asset.save_changes",
    "the addressed pane's asset reports the requested switch",
    &[
        "save_changes_is_the_assets_permission_checked_retry_safe_and_read_back",
        crate::readback::EVERY_OPTIONAL_TEST,
    ],
)];
