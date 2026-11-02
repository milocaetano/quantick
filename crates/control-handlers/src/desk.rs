//! `settings.open` and the `desk.*` calls, through a headless host port.
//!
//! Every act is the one the window's own menu row makes. A call that wrote
//! nothing says so and journals nothing: a caller reading `written: false` must
//! not find an event claiming otherwise.
//!
//! Rate class: a human or an agent acting on the window. Never per trade.

use quantick_control::{
    error::{ControlError, codes},
    id::ErrorCode,
    registry::RegistryError,
    wire::ActorContext,
};
use quantick_control_host::authority::DESK_MODULE_ID;
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::desk::{
    AGENT_ACCESS_REFUSED_CODE, DESK_EXPORT_CAPABILITY_ID, DESK_FORGOTTEN_EVENT_KIND,
    DESK_IMPORT_CAPABILITY_ID, DESK_PICKER_EVENT_KIND, DESK_REOPEN_EVENT_KIND,
    DESK_SAVED_EVENT_KIND, DeskInput, DeskPickResult, DeskReopenInput, DeskReopenResult,
    DeskWriteResult, SettingsOpenInput, SettingsOpenResult, desk_export_descriptor,
    desk_forget_descriptor, desk_import_descriptor, desk_reopen_descriptor, desk_save_descriptor,
    settings_open_descriptor,
};
use quantick_control_schema::settings_sections::SettingsSection;
use serde::Serialize;
use serde_json::{Value, json};

use crate::dock::ActionDock;
use crate::notify::NotifyAccess;

fn unavailable() -> ControlError {
    ControlError::invalid_request("the desk is unavailable")
}

/// One thing a call asks of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeskAct {
    /// Bring Settings up on this section.
    OpenSettings(SettingsSection),
    /// Write the desk as what the app opens on.
    Save,
    /// Forget the saved desk.
    Forget,
    /// Raise the export dialog.
    PickExport,
    /// Raise the import dialog.
    PickImport,
    /// Read whether the next launch reopens the last tabs.
    ReadReopen,
    /// Change that choice.
    SetReopen(bool),
}

/// The window's Settings and desk acts. The answer is the act's one fact: a
/// write reached the disk, a dialog came up (not one already up), or the choice
/// read. The default answers for a host without them.
pub trait DeskHost {
    fn desk(&mut self, _act: DeskAct) -> Result<bool, ControlError> {
        Err(unavailable())
    }
}

/// Outside the replay trace: none of these changes a market, and a recorded
/// call must not rewrite the trader's desk when a session re-runs.
pub fn register<H, A, D>(registry: &mut D) -> Result<(), RegistryError>
where
    H: DeskHost + ?Sized,
    A: NotifyAccess,
    D: ActionDock<H, A>,
{
    registry.register_outside_replay(settings_open_descriptor(), open_settings)?;
    registry.register_outside_replay(desk_save_descriptor(), save)?;
    registry.register_outside_replay(desk_export_descriptor(), export)?;
    registry.register_outside_replay(desk_import_descriptor(), import)?;
    registry.register_outside_replay(desk_forget_descriptor(), forget)?;
    registry.register_outside_replay(desk_reopen_descriptor(), reopen)
}

fn invalid(error: &serde_json::Error) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

fn input<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, ControlError> {
    serde_json::from_value(value.clone()).map_err(|error| invalid(&error))
}

fn answer<T: Serialize>(result: &T) -> Result<Value, ControlError> {
    serde_json::to_value(result).map_err(|error| invalid(&error))
}

fn event(kind: &'static str, actor: &ActorContext, payload: Value) -> NewEvent {
    NewEvent::by(DESK_MODULE_ID, kind, actor, payload)
}

/// The refusal a caller branches on: this section holds the switch that widens
/// a connection's own authority, which `control.access.open` is excluded for.
fn agent_access_refused() -> ControlError {
    let mut refusal = ControlError::new(
        ErrorCode::new(codes::PERMISSION_DENIED).expect("static error code is valid"),
        "the Agent access section is the trader's: a connection cannot open it",
        false,
    );
    refusal.context.details = Some(json!({ "code": AGENT_ACCESS_REFUSED_CODE }));
    refusal
}

fn open_settings<H: DeskHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let SettingsOpenInput { section } = input(value)?;
    let section = section.unwrap_or_default();
    if section == SettingsSection::AgentAccess {
        return Err(agent_access_refused());
    }
    app.desk(DeskAct::OpenSettings(section))?;
    answer(&SettingsOpenResult { section })
}

fn wrote<A: NotifyAccess>(
    kind: &'static str,
    written: bool,
    access: &mut A,
    actor: &ActorContext,
) -> Result<Value, ControlError> {
    if written {
        access.record_event(event(kind, actor, json!({ "written": true })));
    }
    answer(&DeskWriteResult { written })
}

fn save<H: DeskHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let DeskInput {} = input(value)?;
    wrote(
        DESK_SAVED_EVENT_KIND,
        app.desk(DeskAct::Save)?,
        access,
        actor,
    )
}

fn forget<H: DeskHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let DeskInput {} = input(value)?;
    wrote(
        DESK_FORGOTTEN_EVENT_KIND,
        app.desk(DeskAct::Forget)?,
        access,
        actor,
    )
}

fn picked<A: NotifyAccess>(
    capability: &'static str,
    opened: bool,
    access: &mut A,
    actor: &ActorContext,
) -> Result<Value, ControlError> {
    if opened {
        let payload = json!({ "capability": capability });
        access.record_event(event(DESK_PICKER_EVENT_KIND, actor, payload));
    }
    answer(&DeskPickResult { opened })
}

fn export<H: DeskHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let DeskInput {} = input(value)?;
    let opened = app.desk(DeskAct::PickExport)?;
    picked(DESK_EXPORT_CAPABILITY_ID, opened, access, actor)
}

fn import<H: DeskHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let DeskInput {} = input(value)?;
    let opened = app.desk(DeskAct::PickImport)?;
    picked(DESK_IMPORT_CAPABILITY_ID, opened, access, actor)
}

fn reopen<H: DeskHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let DeskReopenInput { reopen } = input(value)?;
    if app.desk(DeskAct::ReadReopen)? == reopen {
        return answer(&DeskReopenResult {
            reopen,
            changed: false,
            written: false,
        });
    }
    let written = app.desk(DeskAct::SetReopen(reopen))?;
    if written {
        let payload = json!({ "reopen": reopen, "written": true });
        access.record_event(event(DESK_REOPEN_EVENT_KIND, actor, payload));
    }
    // A failed write leaves the old choice in force, so a retry writes again.
    answer(&DeskReopenResult {
        reopen: if written { reopen } else { !reopen },
        changed: written,
        written,
    })
}

#[cfg(test)]
#[path = "desk_tests.rs"]
mod desk_tests;
