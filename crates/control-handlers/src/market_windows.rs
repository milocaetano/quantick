//! Validated market presentation actions and bounded window inspection through one host port.
use crate::{dock::ActionDock, native_window::NativeWindowHost};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::market_windows::*;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub fn register_scope<H: NativeWindowHost + ?Sized>(
    registry: &mut impl crate::dock::ProjectionDock<H>,
) -> Result<(), quantick_control_host::projection::ProjectionRegistryError> {
    scope_spec().register(registry, project)
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
    registry.register_outside_replay(reorder_capability_descriptor(), reorder)?;
    registry.register_outside_replay(detach_capability_descriptor(), detach)?;
    registry.register_outside_replay(attach_capability_descriptor(), attach)?;
    registry.register_outside_replay(select_capability_descriptor(), select)?;
    registry.register_outside_replay(begin_capability_descriptor(), begin)?;
    registry.register_outside_replay(cancel_capability_descriptor(), cancel)?;
    registry.register_outside_replay(close_capability_descriptor(), close)?;
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

fn reorder_capability_descriptor() -> quantick_control::registry::CapabilityDescriptor {
    descriptor::<ReorderInput>(
        REORDER_CAPABILITY_ID,
        "Reorder a market tab",
        "Moves the same tab within its window. Position is the insertion index after excluding that tab; ownership, feeds and workers are preserved.",
    )
}

fn detach_capability_descriptor() -> quantick_control::registry::CapabilityDescriptor {
    descriptor::<TabInput>(
        DETACH_CAPABILITY_ID,
        "Detach a market tab",
        "Transfers the same tab into a new native window. The last tab can detach, retiring its empty source window while sibling windows survive.",
    )
}

fn attach_capability_descriptor() -> quantick_control::registry::CapabilityDescriptor {
    descriptor::<AttachInput>(
        ATTACH_CAPABILITY_ID,
        "Attach a market tab",
        "Transfers the same tab to the named live window at an insertion index after excluding that tab. Empty source windows retire without closing their runtime.",
    )
}

fn select_capability_descriptor() -> quantick_control::registry::CapabilityDescriptor {
    descriptor::<SelectInput>(
        SELECT_CAPABILITY_ID,
        "Select a market tab in one window",
        "Selects a tab owned by the named window without changing another window's selection or native keyboard focus.",
    )
}

fn begin_capability_descriptor() -> quantick_control::registry::CapabilityDescriptor {
    descriptor::<TabInput>(
        BEGIN_DRAG_CAPABILITY_ID,
        "Stage a market-tab drag",
        "Stages the same ownership token a native drag uses, without changing ownership or order. Refused while another gesture is pending. Complete through a transfer operation or cancel explicitly.",
    )
}

fn cancel_capability_descriptor() -> quantick_control::registry::CapabilityDescriptor {
    descriptor::<CancelInput>(
        CANCEL_DRAG_CAPABILITY_ID,
        "Cancel the staged market-tab drag",
        "Cancels the pending native or agent tab gesture, preserving every window's ownership and tab order. Inspection reports pending and cancelled state.",
    )
}

fn close_capability_descriptor() -> quantick_control::registry::CapabilityDescriptor {
    descriptor::<WindowTarget>(
        CLOSE_WINDOW_CAPABILITY_ID,
        "Close a market window and preserve its tabs",
        "Retires a live native market window and appends its tabs to a surviving window without recreating sessions. The final market window is refused: process exit is outside agent authority.",
    )
}

pub(crate) fn action_descriptors() -> Vec<quantick_control::registry::CapabilityDescriptor> {
    vec![
        reorder_capability_descriptor(),
        detach_capability_descriptor(),
        attach_capability_descriptor(),
        select_capability_descriptor(),
        begin_capability_descriptor(),
        cancel_capability_descriptor(),
        close_capability_descriptor(),
        placement_descriptor(),
    ]
}

pub(crate) fn scope_spec() -> crate::dock::ScopeSpec<WindowsSnapshot> {
    crate::dock::ScopeSpec::new(
        WINDOWS_SCOPE_ID,
        quantick_control_schema::workspace::MODULE_ID,
        1,
        "Market windows",
        "Bounded windows, stable IDs, ordered tabs, independent selections, staged gesture state and observed native geometry. Join tab IDs with workspace.summary, chart.summary, health.summary and session.replay for runtime and processing continuity. Observations are independent of accepted requests.",
        &["observe", "observe.workspace"],
    )
}

pub(crate) fn scope_descriptors() -> Vec<quantick_control_host::catalogue::SnapshotScopeDescriptor>
{
    vec![scope_spec().descriptor()]
}
