//! Validated market presentation actions and bounded window inspection through one host port.
use crate::{dock::ActionDock, native_window::NativeWindowHost};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::market_windows::*;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub fn register_scope<H: NativeWindowHost + ?Sized>(
    registry: &mut impl crate::dock::ProjectionDock<H>,
) -> Result<(), quantick_control_host::projection::ProjectionRegistryError> {
    registry.register_scope(
        quantick_control::id::SnapshotScopeId::new(WINDOWS_SCOPE_ID).expect("static window scope"),
        quantick_control::id::ModuleId::new(quantick_control_schema::workspace::MODULE_ID).expect("static workspace module"),
        1, "Market windows",
        "Bounded windows, stable IDs, ordered tabs, independent selections, staged gesture state and observed native geometry. Join tab IDs with workspace.summary, chart.summary, health.summary and session.replay for runtime and processing continuity. Observations are independent of accepted requests.",
        &["observe", "observe.workspace"], project,
    )
}
fn project<H: NativeWindowHost + ?Sized>(
    host: &H,
    _: quantick_control_host::projection::CaptureContext,
) -> WindowsSnapshot {
    host.market_windows()
}

pub fn register<H: NativeWindowHost + ?Sized, A>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(descriptor::<ReorderInput>(REORDER_CAPABILITY_ID, "Reorder a market tab", "Moves the same tab within its window. Position is the insertion index after excluding that tab; ownership, feeds and workers are preserved."), reorder)?;
    registry.register_outside_replay(descriptor::<TabInput>(DETACH_CAPABILITY_ID, "Detach a market tab", "Transfers the same tab into a new native window. The last tab can detach, retiring its empty source window while sibling windows survive."), detach)?;
    registry.register_outside_replay(descriptor::<AttachInput>(ATTACH_CAPABILITY_ID, "Attach a market tab", "Transfers the same tab to the named live window at an insertion index after excluding that tab. Empty source windows retire without closing their runtime."), attach)?;
    registry.register_outside_replay(descriptor::<SelectInput>(SELECT_CAPABILITY_ID, "Select a market tab in one window", "Selects a tab owned by the named window without changing another window's selection or native keyboard focus."), select)?;
    registry.register_outside_replay(descriptor::<TabInput>(BEGIN_DRAG_CAPABILITY_ID, "Stage a market-tab drag", "Stages the same ownership token a native drag uses, without changing ownership or order. Refused while another gesture is pending. Complete through a transfer operation or cancel explicitly."), begin)?;
    registry.register_outside_replay(descriptor::<CancelInput>(CANCEL_DRAG_CAPABILITY_ID, "Cancel the staged market-tab drag", "Cancels the pending native or agent tab gesture, preserving every window's ownership and tab order. Inspection reports pending and cancelled state."), cancel)?;
    registry.register_outside_replay(descriptor::<WindowTarget>(CLOSE_WINDOW_CAPABILITY_ID, "Close a market window and preserve its tabs", "Retires a live native market window and appends its tabs to a surviving window without recreating sessions. The final market window is refused: process exit is outside agent authority."), close)?;
    registry.register_outside_replay(placement_descriptor(), place)
}
fn decode<T: DeserializeOwned>(value: &Value) -> Result<T, ControlError> {
    serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}
fn operate<H: NativeWindowHost + ?Sized>(
    app: &mut H,
    operation: TabOperation,
) -> Result<Value, ControlError> {
    serde_json::to_value(app.operate_market_tabs(operation)?)
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}
fn reorder<H: NativeWindowHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    operate(app, TabOperation::Reorder(decode(value)?))
}
fn detach<H: NativeWindowHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    operate(app, TabOperation::Detach(decode(value)?))
}
fn attach<H: NativeWindowHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    operate(app, TabOperation::Attach(decode(value)?))
}
fn select<H: NativeWindowHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    operate(app, TabOperation::Select(decode(value)?))
}
fn begin<H: NativeWindowHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    operate(app, TabOperation::BeginDrag(decode(value)?))
}
fn cancel<H: NativeWindowHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let _: CancelInput = decode(value)?;
    operate(app, TabOperation::CancelDrag)
}
fn close<H: NativeWindowHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    operate(app, TabOperation::Close(decode(value)?))
}
fn place<H: NativeWindowHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: PlacementInput = decode(value)?;
    input.placement.validate()?;
    serde_json::to_value(app.place_native_window(input)?)
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}
