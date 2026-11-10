//! The order-flow panel by name: open it, on a section if one is asked for, close
//! it, and fold or open a section for the asset on screen. The gear, its menu, the
//! key and the headers reach the same state.

use quantick_control::{
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use quantick_stores::ui_state::{
    AssetFolds, LIGHTS_DESCRIPTION, LIGHTS_NAME, OrderFlowPanel, PanelControl, PanelSection,
};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};
use crate::scene::SceneRoleDto;

pub const OPEN_CAPABILITY_ID: &str = "orderflow.settings.open";
pub const CLOSE_CAPABILITY_ID: &str = "orderflow.settings.close";
pub const SECTION_CAPABILITY_ID: &str = "orderflow.settings.section.set";
pub const LIGHTS_CAPABILITY_ID: &str = "orderflow.settings.lights.set";
/// The panel's state, read off the tab: no paint needed, unlike the scene's controls.
pub const SETTINGS_SCOPE_ID: &str = "orderflow.settings";

/// A section of the panel, as a call names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSection {
    L2,
    Bubbles,
}

impl From<SettingsSection> for PanelSection {
    fn from(section: SettingsSection) -> Self {
        match section {
            SettingsSection::L2 => Self::L2,
            SettingsSection::Bubbles => Self::Bubbles,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsOpenInput {
    pub tab_id: WireU64,
    /// Expand this section and scroll it into view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<SettingsSection>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsCloseInput {
    pub tab_id: WireU64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsSectionInput {
    pub tab_id: WireU64,
    pub section: SettingsSection,
    pub open: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsLightsInput {
    pub tab_id: WireU64,
    /// On paints the flow pane on the theme's lit surface; off, its dark island.
    pub on: bool,
}

/// The panel after a call, whichever one it was.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsResult {
    pub tab_id: WireU64,
    /// The asset whose sections these are.
    pub asset: String,
    pub open: bool,
    pub l2_open: bool,
    pub bubbles_open: bool,
    /// The flow pane's lights: the bulb beside the gear.
    #[serde(default)]
    pub lights_on: bool,
    pub changed: bool,
}

impl SettingsResult {
    /// What `panel` and `folds` say now about `asset`.
    #[must_use]
    pub fn read(
        tab_id: WireU64,
        asset: &str,
        panel: &OrderFlowPanel,
        folds: &AssetFolds,
        changed: bool,
    ) -> Self {
        Self {
            tab_id,
            asset: asset.to_owned(),
            open: panel.is_open(),
            l2_open: folds.section_open(asset, PanelSection::L2),
            bubbles_open: folds.section_open(asset, PanelSection::Bubbles),
            lights_on: panel.lights_on(),
            changed,
        }
    }
}

/// Every tab's panel, as it would save.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsSnapshot {
    pub tabs: Vec<TabSettingsSnapshot>,
}

/// One tab's panel and its asset's folds, whether or not anything painted them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabSettingsSnapshot {
    pub tab_id: WireU64,
    /// The asset whose sections these are.
    pub asset: String,
    pub open: bool,
    pub l2_open: bool,
    pub bubbles_open: bool,
    /// The flow pane's lights, kept with the tab whatever the theme; only a
    /// theme that authors a lit surface (Light) paints them.
    #[serde(default)]
    pub lights_on: bool,
}

impl TabSettingsSnapshot {
    /// What `panel` and `folds` say now about `asset`.
    #[must_use]
    pub fn read(tab_id: WireU64, asset: &str, panel: &OrderFlowPanel, folds: &AssetFolds) -> Self {
        Self {
            tab_id,
            asset: asset.to_owned(),
            open: panel.is_open(),
            l2_open: folds.section_open(asset, PanelSection::L2),
            bubbles_open: folds.section_open(asset, PanelSection::Bubbles),
            lights_on: panel.lights_on(),
        }
    }
}

/// One panel control in the scene: its label, role, whether it is selected, why it
/// cannot be operated, and the call that operates it.
#[must_use]
pub fn scene_row(
    control: PanelControl,
    panel: &OrderFlowPanel,
    folds: &AssetFolds,
    asset: &str,
) -> (
    &'static str,
    SceneRoleDto,
    bool,
    Option<&'static str>,
    &'static str,
) {
    match control {
        PanelControl::Gear => (
            "Order flow settings",
            SceneRoleDto::Toggle,
            panel.is_open(),
            None,
            OPEN_CAPABILITY_ID,
        ),
        PanelControl::Section(section) => (
            section.title(),
            SceneRoleDto::Toggle,
            folds.section_open(asset, section),
            None,
            SECTION_CAPABILITY_ID,
        ),
        PanelControl::Close => (
            "Close order flow settings",
            SceneRoleDto::Action,
            false,
            None,
            CLOSE_CAPABILITY_ID,
        ),
        PanelControl::Lights => (
            LIGHTS_NAME,
            SceneRoleDto::Toggle,
            panel.lights_on(),
            None,
            LIGHTS_CAPABILITY_ID,
        ),
    }
}

/// A call on the panel: a tab saves its panel and the window its asset's folds.
fn durable_descriptor<Input: JsonSchema>(
    id: &str,
    title: &str,
    description: &str,
    stale_input_safety: &str,
) -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<Input, SettingsResult>(
        id,
        crate::orderflow::MODULE_ID,
        title,
        description,
        stale_input_safety,
    );
    descriptor.persistence = quantick_control::registry::EffectPersistence::Durable;
    descriptor
}

pub fn open_descriptor() -> CapabilityDescriptor {
    durable_descriptor::<SettingsOpenInput>(
        OPEN_CAPABILITY_ID,
        "Open the order-flow settings panel",
        "Opens the panel attached to the tab's flow pane, holding the L2 and Bubbles settings. With a section (l2 or bubbles) it also expands that section and scrolls to it. Refused on a tab not showing its flow pane. Presentation only: looking is not enabling, so no layer switches.",
        "A stable tab ID selects the panel; repeating a call has no further effect, and the result and the orderflow.settings scope report the state.",
    )
}

pub fn close_descriptor() -> CapabilityDescriptor {
    durable_descriptor::<SettingsCloseInput>(
        CLOSE_CAPABILITY_ID,
        "Close the order-flow settings panel",
        "Closes the panel attached to the tab's flow pane, and the chart takes back its width. Presentation only.",
        "A stable tab ID selects the panel; repeating a call has no further effect, and the result and the orderflow.settings scope report the state.",
    )
}

pub fn section_descriptor() -> CapabilityDescriptor {
    durable_descriptor::<SettingsSectionInput>(
        SECTION_CAPABILITY_ID,
        "Fold or open a section of the order-flow settings panel",
        "Folds or opens the L2 or the Bubbles section for the asset the tab shows; every tab on that asset reads the same. Whether or not the panel is open. Presentation only.",
        "A stable tab ID selects the asset; repeating a value has no further effect, and the result and the orderflow.settings scope report it, the panel open or not.",
    )
}

pub fn lights_descriptor() -> CapabilityDescriptor {
    durable_descriptor::<SettingsLightsInput>(
        LIGHTS_CAPABILITY_ID,
        "Switch the order-flow pane's lights",
        "Sets the bulb beside the order-flow band's gear. On, the flow pane (Trail and Tape) paints on the theme's lit surface: a white plot, liquidity in grey with only the wall in plum, flat rimmed bubbles, and drawings, VWAP and plot inks in the theme's own paper colours. Off, it keeps its dark island. Refused while the theme authors no lit surface (Dark). Kept with the tab and restored on restart. Presentation only: executions, the book and the layers are untouched.",
        "A stable tab ID selects the tab; repeating a value has no further effect, and the result and the orderflow.settings scope report it.",
    )
}

const fn readback(
    capability: &'static str,
    field: &'static str,
    applied_when: &'static str,
) -> Readback {
    snapshot(
        capability,
        IdempotencyPolicy::Optional,
        SETTINGS_SCOPE_ID,
        field,
        applied_when,
        &[EVERY_OPTIONAL_TEST],
    )
}

pub const READBACKS: &[Readback] = &[
    readback(
        OPEN_CAPABILITY_ID,
        "tabs[].open",
        "the tab's row reads open, and the section asked for reads open too",
    ),
    readback(
        CLOSE_CAPABILITY_ID,
        "tabs[].open",
        "the tab's row no longer reads open",
    ),
    readback(
        LIGHTS_CAPABILITY_ID,
        "tabs[].lights_on",
        "the tab's row reads the value asked for",
    ),
    readback(
        SECTION_CAPABILITY_ID,
        "tabs[].l2_open",
        "the tab's row reads the value asked for in l2_open or bubbles_open",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_is_optional_validated_and_nothing_else_is_taken() {
        let bare: SettingsOpenInput = serde_json::from_str(r#"{"tab_id":"1"}"#).unwrap();
        assert_eq!(bare.section, None);
        let named: SettingsOpenInput =
            serde_json::from_str(r#"{"tab_id":"1","section":"bubbles"}"#).unwrap();
        assert_eq!(named.section, Some(SettingsSection::Bubbles));
        for bad in [
            r#"{"tab_id":"1","section":"session"}"#,
            r#"{"tab_id":"1","section":2}"#,
            r#"{"tab_id":"1","width":300}"#,
            r#"{"section":"l2"}"#,
        ] {
            assert!(
                serde_json::from_str::<SettingsOpenInput>(bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_fold_names_its_section_and_its_value() {
        let input: SettingsSectionInput =
            serde_json::from_str(r#"{"tab_id":"1","section":"l2","open":false}"#).unwrap();
        assert_eq!(PanelSection::from(input.section), PanelSection::L2);
        for bad in [
            r#"{"tab_id":"1","section":"l2"}"#,
            r#"{"tab_id":"1","open":true}"#,
            r#"{"tab_id":"1","section":"tape","open":true}"#,
        ] {
            assert!(
                serde_json::from_str::<SettingsSectionInput>(bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_lights_name_their_tab_and_a_value_and_nothing_else() {
        let input: SettingsLightsInput =
            serde_json::from_str(r#"{"tab_id":"1","on":true}"#).unwrap();
        assert!(input.on);
        for bad in [
            r#"{"tab_id":"1"}"#,
            r#"{"on":true}"#,
            r#"{"tab_id":"1","on":"yes"}"#,
            r#"{"tab_id":"1","on":true,"theme":"light"}"#,
        ] {
            assert!(
                serde_json::from_str::<SettingsLightsInput>(bad).is_err(),
                "{bad}"
            );
        }
        let mut panel = OrderFlowPanel::default();
        panel.set_lights(true);
        let folds = AssetFolds::default();
        let row = TabSettingsSnapshot::read(WireU64::new(1), "BTCUSDT", &panel, &folds);
        assert!(row.lights_on);
        let scene = scene_row(PanelControl::Lights, &panel, &folds, "BTCUSDT");
        assert_eq!(scene.0, LIGHTS_NAME);
        assert!(scene.2);
        assert_eq!(scene.4, LIGHTS_CAPABILITY_ID);
    }

    #[test]
    fn the_result_reads_the_panel_and_the_assets_folds() {
        let mut folds = AssetFolds::default();
        folds.set("BTCUSDT", PanelSection::L2, false);
        let mut panel = OrderFlowPanel::default();
        panel.open_on(None);
        let tab = WireU64::new(1);
        let read = SettingsResult::read(tab, "BTCUSDT", &panel, &folds, true);
        assert!(read.open && !read.l2_open && read.bubbles_open && read.changed);
        let other = SettingsResult::read(tab, "ETHUSDT", &panel, &folds, false);
        assert!(other.l2_open, "a fold belongs to its asset");
    }
}
