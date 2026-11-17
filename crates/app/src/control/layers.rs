//! Layer discovery over the same catalog the pane menus use, and the pane
//! and layer `layers.visibility.set` names, read and switched as those menus
//! do; the call itself is `quantick_control_handlers::layers`.

pub(crate) use quantick_control_schema::layers::*;

use super::{
    layout::{self, TabTarget},
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
};

use crate::app::{ChromePort, LayersPort, TabsPort};
use crate::pane::{ChartPane, PaneSide};

use quantick_control::{
    error::ControlError,
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::WireU64,
};
use quantick_control_handlers::chart_layer::{LayerBlocked, LayerRead};

use quantick_layers::ChartLayer;

fn read_layer<P: TabsPort + ChromePort + ?Sized>(
    app: &P,
    tab: &crate::tab::Tab,
    pane: &ChartPane,
    layer: ChartLayer,
) -> LayerSnapshot {
    LayerSnapshot::from_policy(
        layer,
        pane.layer_switched_on(layer, app.chrome_reads().style()),
        pane.layer_facts(Some(tab.capabilities(app.tab_reads().config()))),
    )
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
        "Layer visibility", "Bounded registered layer discovery and requested/effective visibility per stable pane ID; grid belongs to each pane.",
        &["observe", "observe.workspace", "observe.market"], project)
}

/// Which pane and which registered layer a layer call names: the tab's
/// position, the pane's side and the layer, refused in the call's words.
fn resolve<P: TabsPort + ?Sized>(
    app: &P,
    tab_id: WireU64,
    pane_id: WireU64,
    layer_id: &str,
) -> Result<(usize, PaneSide, ChartLayer), ControlError> {
    let tab_id = Some(tab_id);
    let index = layout::tab_index(app, TabTarget { tab_id })?;
    let tab = app
        .tab_reads()
        .tab_at(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed"))?;
    let (pane, side) = tab
        .panes()
        .find(|(pane, _)| pane.id == pane_id.get())
        .ok_or_else(|| {
            ControlError::invalid_request(
                "layer visibility names an unknown pane on the requested tab",
            )
        })?;
    let layer = pane.layers.registry().resolve(layer_id).ok_or_else(|| {
        ControlError::invalid_request("layer visibility names an unknown registered layer")
    })?;
    Ok((index, side, layer))
}

/// The layer a call names, as the pane's menu reads it.
pub(super) fn chart_layer<P: TabsPort + ChromePort + ?Sized>(
    app: &P,
    tab_id: WireU64,
    pane_id: WireU64,
    layer_id: &str,
) -> Result<LayerRead, ControlError> {
    let (index, side, layer) = resolve(app, tab_id, pane_id, layer_id)?;
    let reads = app.tab_reads();
    let tab = reads
        .tab_at(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed"))?;
    let pane = tab.pane(side);
    let blocked = pane.layer_blocked(layer, tab.capabilities(reads.config()));
    Ok(LayerRead {
        layer: read_layer(app, tab, pane, layer),
        blocked: blocked.map(|blocked| LayerBlocked {
            code: blocked.code,
            explanation: blocked.explanation,
        }),
    })
}

/// Switch the layer a call names the way the pane's menu does.
pub(super) fn switch_chart_layer<P: TabsPort + LayersPort + ?Sized>(
    app: &mut P,
    tab_id: WireU64,
    pane_id: WireU64,
    layer_id: &str,
    visible: bool,
) -> Result<(), ControlError> {
    let (index, side, layer) = resolve(app, tab_id, pane_id, layer_id)?;
    app.layer_wiring().set_visible(index, side, layer, visible);
    Ok(())
}
