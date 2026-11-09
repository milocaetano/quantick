//! The Settings window and the desk, as named calls.
//!
//! A desk is the open market tabs in order, plus panels, rail, timezone and
//! window (`docs/ux/window-menu-and-workspace-tabs.md`). Export and import take
//! no path: the trader's own file dialog is what names it, so the observer
//! threat model's refusal of an arbitrary path still holds.

use quantick_control::id::PermissionId;
use quantick_control::registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy};
use quantick_control_host::authority::{
    COCKPIT_DESK_PERMISSION_ID, COCKPIT_PERMISSION_ID, DESK_MODULE_ID, SETTINGS_MODULE_ID,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::layout::transient_descriptor;
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal, snapshot};
use crate::settings_sections::SettingsSection;

pub const SETTINGS_OPEN_CAPABILITY_ID: &str = "settings.open";
pub const DESK_SAVE_CAPABILITY_ID: &str = "desk.save";
pub const DESK_EXPORT_CAPABILITY_ID: &str = "desk.export";
pub const DESK_IMPORT_CAPABILITY_ID: &str = "desk.import";
pub const DESK_FORGET_CAPABILITY_ID: &str = "desk.forget";
pub const DESK_REOPEN_CAPABILITY_ID: &str = "desk.reopen_on_launch.set";

/// A save that reached the disk.
pub const DESK_SAVED_EVENT_KIND: &str = "desk.saved";
/// A forget that reached the disk.
pub const DESK_FORGOTTEN_EVENT_KIND: &str = "desk.forgotten";
/// The trader's file dialog came up.
pub const DESK_PICKER_EVENT_KIND: &str = "desk.picker_opened";
/// A set that moved the choice.
pub const DESK_REOPEN_EVENT_KIND: &str = "desk.reopen_on_launch.changed";

/// The `details.code` of a refused `settings.open` on Agent access.
pub const AGENT_ACCESS_REFUSED_CODE: &str = "settings.agent_access_refused";

const RESULT_MAX_BYTES: usize = 512;

/// Open the Settings window.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsOpenInput {
    /// The section to show. Left out, Display. `agent_access` is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<SettingsSection>,
}

/// The section now on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct SettingsOpenResult {
    pub section: SettingsSection,
}

/// Takes nothing: the desk is what is on screen.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeskInput {}

/// Whether the file changed on disk; `false` says the log has the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct DeskWriteResult {
    pub written: bool,
}

/// Whether the trader's file dialog came up; `false` means one already is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct DeskPickResult {
    pub opened: bool,
}

/// Whether the next launch reopens the tabs open when the window closed.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeskReopenInput {
    pub reopen: bool,
}

/// The choice in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct DeskReopenResult {
    pub reopen: bool,
    /// Whether this call moved it; `false` for a repeat.
    pub changed: bool,
    /// Whether the file changed on disk.
    pub written: bool,
}

fn bounded(mut descriptor: CapabilityDescriptor) -> CapabilityDescriptor {
    descriptor.expected_cost.max_response_bytes = Some(RESULT_MAX_BYTES);
    descriptor
}

fn durable(mut descriptor: CapabilityDescriptor) -> CapabilityDescriptor {
    descriptor.persistence = EffectPersistence::Durable;
    bounded(descriptor)
}

/// Behind the desk's own consent: this call replaces or removes what the trader
/// comes back to, or raises the dialog that does.
fn desk_grant(mut descriptor: CapabilityDescriptor) -> CapabilityDescriptor {
    descriptor.required_permissions = [COCKPIT_PERMISSION_ID, COCKPIT_DESK_PERMISSION_ID]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect();
    descriptor
}

pub fn settings_open_descriptor() -> CapabilityDescriptor {
    bounded(
        transient_descriptor::<SettingsOpenInput, SettingsOpenResult>(
            SETTINGS_OPEN_CAPABILITY_ID,
            SETTINGS_MODULE_ID,
            "Open the Settings window",
            "Opens Settings on a section, as Ctrl+Comma and the status-bar clock do. Changes no setting. The Agent access section is refused: a connection cannot bring up the switch that widens its own authority.",
            "Opening a section twice is the same state, so a stale or repeated call can only show that section.",
        ),
    )
}

pub fn desk_save_descriptor() -> CapabilityDescriptor {
    desk_grant(durable(transient_descriptor::<DeskInput, DeskWriteResult>(
        DESK_SAVE_CAPABILITY_ID,
        DESK_MODULE_ID,
        "Save the desk",
        "Remembers the tabs, the charts on each, the panels, the timezone and the window as what quantick opens on, as Ctrl+Shift+S does.",
        "A repeat writes what is on screen then, which is what the caller asked to keep.",
    )))
}

pub fn desk_export_descriptor() -> CapabilityDescriptor {
    desk_grant(bounded(transient_descriptor::<DeskInput, DeskPickResult>(
        DESK_EXPORT_CAPABILITY_ID,
        DESK_MODULE_ID,
        "Export the desk to a file",
        "Brings up the trader's save dialog. It takes no path: nothing is written until the trader picks one there.",
        "While one dialog is up a repeat raises no second one and answers opened false.",
    )))
}

pub fn desk_import_descriptor() -> CapabilityDescriptor {
    desk_grant(bounded(transient_descriptor::<DeskInput, DeskPickResult>(
        DESK_IMPORT_CAPABILITY_ID,
        DESK_MODULE_ID,
        "Import a desk from a file",
        "Brings up the trader's open dialog. It takes no path: the cockpit on screen stays until the trader picks a file there.",
        "While one dialog is up a repeat raises no second one and answers opened false.",
    )))
}

pub fn desk_forget_descriptor() -> CapabilityDescriptor {
    desk_grant(durable(transient_descriptor::<DeskInput, DeskWriteResult>(
        DESK_FORGET_CAPABILITY_ID,
        DESK_MODULE_ID,
        "Forget the saved desk",
        "Forgets the saved startup desk; it does not come back. The charts on screen are left alone. The next launch opens on the configured default only while reopen on launch is off: with it on, closing the window saves the screen again.",
        "Forgetting twice leaves nothing saved either way, and the second answers written false.",
    )))
}

pub fn desk_reopen_descriptor() -> CapabilityDescriptor {
    durable(transient_descriptor::<DeskReopenInput, DeskReopenResult>(
        DESK_REOPEN_CAPABILITY_ID,
        DESK_MODULE_ID,
        "Reopen the last tabs on launch",
        "Says whether the arrangement is kept automatically when the window closes. Off, only desk.save changes what quantick opens on.",
        "The call names the choice it wants, so a repeat leaves the same choice and writes nothing.",
    ))
}

/// How a client reconciles an interrupted call.
pub const READBACKS: &[Readback] = &[
    snapshot(
        SETTINGS_OPEN_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        crate::scene::CONTROLS_SCOPE_ID,
        "controls[].selected",
        "the asked section's settings.section control is listed and selected",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        DESK_SAVE_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        DESK_SAVED_EVENT_KIND,
        "payload.written",
        "an event after the pre-call cursor says written",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        DESK_EXPORT_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        DESK_PICKER_EVENT_KIND,
        "payload.capability",
        "an event after the pre-call cursor names desk.export",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        DESK_IMPORT_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        DESK_PICKER_EVENT_KIND,
        "payload.capability",
        "an event after the pre-call cursor names desk.import",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        DESK_FORGET_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        DESK_FORGOTTEN_EVENT_KIND,
        "payload.written",
        "an event after the pre-call cursor says written",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        DESK_REOPEN_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        DESK_REOPEN_EVENT_KIND,
        "payload.reopen",
        "an event after the pre-call cursor carries the requested choice",
        &[EVERY_OPTIONAL_TEST],
    ),
];

/// Published schemas of the desk and the settings opener.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        (
            "settings-open-input-v1.schema.json",
            schema::<SettingsOpenInput>(),
        ),
        (
            "settings-open-result-v1.schema.json",
            schema::<SettingsOpenResult>(),
        ),
        ("desk-input-v1.schema.json", schema::<DeskInput>()),
        (
            "desk-write-result-v1.schema.json",
            schema::<DeskWriteResult>(),
        ),
        (
            "desk-pick-result-v1.schema.json",
            schema::<DeskPickResult>(),
        ),
        (
            "desk-reopen-input-v1.schema.json",
            schema::<DeskReopenInput>(),
        ),
        (
            "desk-reopen-result-v1.schema.json",
            schema::<DeskReopenResult>(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn export_and_import_take_no_path() {
        let path = json!({"path": "C:/elsewhere.toml"});
        assert!(serde_json::from_value::<DeskInput>(path).is_err());
        assert!(serde_json::from_value::<DeskInput>(json!({})).is_ok());
    }

    #[test]
    fn a_section_is_optional_and_a_stranger_is_refused() {
        let input: SettingsOpenInput = serde_json::from_value(json!({})).unwrap();
        assert_eq!(input.section, None);
        let input: SettingsOpenInput = serde_json::from_value(json!({"section": "data"})).unwrap();
        assert_eq!(input.section, Some(SettingsSection::Data));
        assert!(serde_json::from_value::<SettingsOpenInput>(json!({"section": "x"})).is_err());
    }
}
