//! Layer discovery over the same catalog the pane menus use, and
//! `layers.visibility.set`: one pane's layer switched the way its menu
//! switches it.
//!
//! The window lends each pane through [`ChartPort`]; the pane reads its own
//! layers ([`crate::chart::ChartPaneSource::layers`]) because only it knows
//! which are switched on. What the scope bounds, which address a call names
//! and the refusal when it names nothing, what is refused, what changed and
//! what the journal keeps are decided here.
use quantick_control::{
    error::{ControlError, codes},
    id::{ErrorCode, ModuleId, SnapshotScopeId},
    registry::{ModuleDescriptor, RegistryError},
    wire::{ActorContext, WireU64},
};
use quantick_control_host::{
    journal::NewEvent,
    projection::{CaptureContext, ProjectionRegistryError},
};
use quantick_control_schema::layers::{
    EVENT_KIND, LayersSnapshot, MAX_SNAPSHOT_PANES, MODULE_ID, PaneLayersSnapshot, SCOPE_ID,
    SetVisibilityInput, VisibilityResult, set_visibility_descriptor,
};
use serde_json::{Value, json};

use crate::JournalAccess;
use crate::chart::{ChartPort, MissingPane};
use crate::chart_layer::LayerRead;
use crate::dock::{ActionDock, ProjectionDock};
use crate::invalid;

/// Dock `layers.visibility.set`.
///
/// # Errors
/// A capability already docked.
pub fn register<D: ActionDock<H, A>, H: ChartPort + ?Sized, A: JournalAccess>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(set_visibility_descriptor(), set_visibility)
}

/// Dock the layers module and its `layers.visibility` scope.
///
/// # Errors
/// A module or scope already docked.
pub fn register_scope<D, H>(registry: &mut D) -> Result<(), ProjectionRegistryError>
where
    D: ProjectionDock<H>,
    H: ChartPort + ?Sized + 'static,
{
    let module = ModuleId::new(MODULE_ID).expect("static module ID");
    registry.register_module(
        ModuleDescriptor {
            id: module.clone(),
            title: "Chart layers".to_owned(),
            description: "Registered chart visibility and availability, including hidden panes."
                .to_owned(),
        },
        snapshot::<H>,
    )?;
    registry.register_scope(SnapshotScopeId::new(SCOPE_ID).expect("static scope ID"), module, 1,
        "Layer visibility", "Bounded registered layer discovery and requested/effective visibility per stable pane ID; grid belongs to each pane.",
        &["observe", "observe.workspace", "observe.market"], project::<H>)
}

/// Every pane's registered layers, tab by tab, hidden panes included. Past
/// [`MAX_SNAPSHOT_PANES`] a pane is counted rather than listed: it can still
/// be addressed by id.
pub fn snapshot<H: ChartPort + ?Sized>(host: &H) -> LayersSnapshot {
    let mut panes = Vec::new();
    let mut omitted = 0;
    let style = host.chart_style();
    host.visit_chart_panes(&mut |entry, _| {
        if panes.len() == MAX_SNAPSHOT_PANES {
            omitted += 1;
            return;
        }
        panes.push(PaneLayersSnapshot {
            tab_id: WireU64::new(entry.tab_id),
            pane_id: WireU64::new(entry.pane.pane_id()),
            layers: entry.pane.layers(style),
        });
    });
    LayersSnapshot {
        panes,
        omitted_panes: WireU64::new(omitted),
    }
}

fn project<H: ChartPort + ?Sized>(host: &H, _: CaptureContext) -> LayersSnapshot {
    snapshot(host)
}

/// The layer a call names, as the pane's menu reads it, or the refusal
/// saying which part of the address named nothing.
///
/// # Errors
/// An unknown tab, pane on that tab, or registered layer on that pane.
pub fn chart_layer<H: ChartPort + ?Sized>(
    host: &H,
    tab_id: WireU64,
    pane_id: WireU64,
    layer_id: &str,
) -> Result<LayerRead, ControlError> {
    let (mut read, style) = (None, host.chart_style());
    host.visit_chart_pane(tab_id.get(), pane_id.get(), &mut |entry| {
        read = entry.pane.layer(layer_id, style);
    })
    .map_err(|missing| match missing {
        MissingPane::Tab => crate::tabs::no_tab(tab_id.get()),
        MissingPane::Pane => ControlError::invalid_request(
            "layer visibility names an unknown pane on the requested tab",
        ),
    })?;
    read.ok_or_else(|| {
        ControlError::invalid_request("layer visibility names an unknown registered layer")
    })
}

fn set_visibility<H: ChartPort + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: SetVisibilityInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let (tab_id, pane_id) = (input.tab_id, input.pane_id);
    let before = chart_layer(host, tab_id, pane_id, &input.layer_id)?;
    if let Some(blocked) = before.blocked {
        let code = ErrorCode::new(codes::CAPABILITY_UNAVAILABLE).expect("static error code");
        let mut error = ControlError::new(code, blocked.explanation, false);
        error.context.details = Some(json!({ "reason": blocked.code }));
        return Err(error);
    }
    host.switch_chart_layer(tab_id.get(), pane_id.get(), &input.layer_id, input.visible);
    let after = chart_layer(host, tab_id, pane_id, &input.layer_id)?;
    let result = serde_json::to_value(VisibilityResult {
        tab_id,
        pane_id,
        changed: before.layer.requested != after.layer.requested,
        layer: after.layer,
    })
    .map_err(invalid)?;
    // Every admitted application, including a no-op, has a bounded readback.
    // The idempotency host replays keyed answers without reaching this point.
    let payload = json!({
        "connection_id": actor.connection_id,
        "request_id": actor.request_id,
        "result": result,
    });
    access.record_event(NewEvent::by(MODULE_ID, EVENT_KIND, actor, payload));
    Ok(result)
}

#[cfg(test)]
#[path = "layers_tests.rs"]
mod tests;
