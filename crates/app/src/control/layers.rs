//! Layer discovery and visibility actions over the same catalog used by pane menus.
use super::{
    actions::ActionRegistry,
    gateway::ControlAccess,
    journal::{EventActor, NewEvent},
    layout::{self, TabTarget},
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
};
use crate::app::{ChromePort, LayersPort, TabsPort};
use crate::pane::ChartPane;
use quantick_control::{
    error::{ControlError, codes},
    id::{ErrorCode, EventKind, ModuleId, SnapshotScopeId},
    registry::{ModuleDescriptor, RegistryError},
    schema::generated_schema,
    wire::{ActorContext, WireU64},
};
use quantick_layers::{ChartLayer, LayerScope, Persistence};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub(crate) const SCOPE_ID: &str = "layers.visibility";
pub(crate) const SET_VISIBILITY_CAPABILITY_ID: &str = "layers.visibility.set";
pub(crate) const EVENT_KIND: &str = "layers.visibility.set";
const MODULE_ID: &str = "layers";
/// A bounded snapshot; omitted panes are explicit and can still be targeted by ID.
const MAX_SNAPSHOT_PANES: usize = 64;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetVisibilityInput {
    pub tab_id: WireU64,
    /// Stable identity from chart.summary, never a position that a pane move can change.
    pub pane_id: WireU64,
    #[schemars(length(min = 1, max = quantick_layers::MAX_LAYER_ID_BYTES))]
    pub layer_id: String,
    pub visible: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) struct LayerSnapshot {
    pub id: String,
    pub label: String,
    pub scope: String,
    pub persistence: String,
    pub requested: bool,
    /// Visibility eligible under current layer policy; no pixel geometry is claimed.
    pub effective: bool,
    pub blocked_reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) struct PaneLayersSnapshot {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    #[schemars(length(max = quantick_layers::MAX_LAYERS))]
    pub layers: Vec<LayerSnapshot>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) struct LayersSnapshot {
    #[schemars(length(max = MAX_SNAPSHOT_PANES))]
    pub panes: Vec<PaneLayersSnapshot>,
    pub omitted_panes: WireU64,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(crate) struct VisibilityResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub layer: LayerSnapshot,
    pub changed: bool,
}
fn read_layer<P: TabsPort + ChromePort + ?Sized>(
    app: &P,
    tab: &crate::tab::Tab,
    pane: &ChartPane,
    layer: ChartLayer,
) -> LayerSnapshot {
    let blocked = pane.layer_blocked(layer, tab.capabilities(app.tab_reads().config()));
    LayerSnapshot {
        id: layer.id().to_owned(),
        label: layer.label().to_owned(),
        scope: match layer.0.scope {
            LayerScope::Window => "window",
            LayerScope::Pane => "pane",
            LayerScope::FlowPane => "flow_pane",
        }
        .to_owned(),
        persistence: match layer.0.persistence {
            Persistence::Layers => "chart_layers",
            Persistence::OrderflowPreset => "orderflow_preset",
        }
        .to_owned(),
        requested: pane.layer_switched_on(layer, app.chrome_reads().style()),
        effective: pane.layer_effective(
            layer,
            pane.layer_switched_on(layer, app.chrome_reads().style()),
            tab.capabilities(app.tab_reads().config()),
        ),
        blocked_reason: blocked.map(|block| block.code.to_owned()),
    }
}
pub(crate) fn snapshot<P: TabsPort + ChromePort + ?Sized>(app: &P) -> LayersSnapshot {
    let mut panes = Vec::new();
    let mut omitted = 0;
    for (tab_id, tab) in app.tab_reads().tabs().iter_with_ids() {
        for (pane, _) in tab.panes() {
            if panes.len() == MAX_SNAPSHOT_PANES {
                omitted += 1;
                continue;
            }
            panes.push(PaneLayersSnapshot {
                tab_id: WireU64::new(tab_id),
                pane_id: WireU64::new(pane.id),
                layers: pane
                    .layers
                    .registry()
                    .layers()
                    .iter()
                    .copied()
                    .map(|layer| read_layer(app, tab, pane, layer))
                    .collect(),
            });
        }
    }
    LayersSnapshot {
        panes,
        omitted_panes: WireU64::new(omitted),
    }
}
fn project<P: TabsPort + ChromePort + ?Sized>(app: &P, _: CaptureContext) -> LayersSnapshot {
    snapshot(app)
}

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module = ModuleId::new(MODULE_ID).expect("static module ID");
    registry.register_module(
        ModuleDescriptor {
            id: module.clone(),
            title: "Chart layers".to_owned(),
            description: "Registered chart visibility and availability, including hidden panes."
                .to_owned(),
        },
        snapshot,
    )?;
    registry.register_scope(SnapshotScopeId::new(SCOPE_ID).expect("static scope ID"), module, 1,
        "Layer visibility", "Bounded registered layer discovery and requested/effective visibility per stable pane ID; grid is window-wide.",
        &["observe", "observe.workspace", "observe.market"], project)
}
pub(crate) fn register_action(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    let mut descriptor = layout::descriptor(
        SET_VISIBILITY_CAPABILITY_ID,
        "Set chart layer visibility",
        "Sets an existing registered display switch on a stable pane ID through the same operation as the menu; grid affects the entire window.",
        generated_schema::<SetVisibilityInput>(),
    );
    descriptor.module = ModuleId::new(MODULE_ID).expect("static module ID");
    descriptor.output_schema = generated_schema::<VisibilityResult>();
    descriptor.stale_input_safety = Some("Stable tab and pane IDs are resolved before mutation; a stale requested visibility affects display only, and the result names its actual scope.".to_owned());
    registry.register(descriptor, set_visibility)
}
fn set_visibility<P: TabsPort + ChromePort + LayersPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: SetVisibilityInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = layout::tab_index(
        app,
        TabTarget {
            tab_id: Some(input.tab_id),
        },
    )?;
    let tab = app
        .tab_reads()
        .tab_at(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed"))?;
    let (pane, side) = tab
        .panes()
        .find(|(pane, _)| pane.id == input.pane_id.get())
        .ok_or_else(|| {
            ControlError::invalid_request(
                "layer visibility names an unknown pane on the requested tab",
            )
        })?;
    let layer = pane
        .layers
        .registry()
        .resolve(&input.layer_id)
        .ok_or_else(|| {
            ControlError::invalid_request("layer visibility names an unknown registered layer")
        })?;
    if let Some(blocked) = pane.layer_blocked(layer, tab.capabilities(app.tab_reads().config())) {
        let mut error = ControlError::new(
            ErrorCode::new(codes::CAPABILITY_UNAVAILABLE).expect("static error code"),
            blocked.explanation,
            false,
        );
        error.context.details = Some(serde_json::json!({ "reason": blocked.code }));
        return Err(error);
    }
    let before = pane.layer_switched_on(layer, app.chrome_reads().style());
    app.layer_wiring()
        .set_visible(index, side, layer, input.visible);
    let tab = app
        .tab_reads()
        .tab_at(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed"))?;
    let pane = tab.pane(side);
    let result = serde_json::to_value(VisibilityResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        layer: read_layer(app, tab, pane, layer),
        changed: before != pane.layer_switched_on(layer, app.chrome_reads().style()),
    })
    .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    // Every admitted application, including a no-op, has a bounded readback.
    // The idempotency host replays keyed answers without reaching this point.
    access.journal_mut().record(
        NewEvent {
            module_id: ModuleId::new(MODULE_ID).expect("static module ID"),
            kind: EventKind::new(EVENT_KIND).expect("static event kind"),
            actor: Some(EventActor {
                kind: actor.actor_kind,
                client_name: actor.client_name.clone(),
            }),
            payload: serde_json::json!({ "connection_id": actor.connection_id, "request_id": actor.request_id, "result": result }),
        },
        crate::metrics::wall_clock_ms(),
    );
    Ok(result)
}
