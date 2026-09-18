//! Filesystem and window effects around the headless layer session.
use super::{load, save};
use crate::{pane::ChartPane, style::ChartStyle, tab::Tab, workspace_store::WorkspaceStore};
use quantick_layers::LayerSource;

pub(crate) fn maintain(
    workspace: &mut WorkspaceStore,
    tab: u64,
    pane: &ChartPane,
    style: &ChartStyle,
) {
    let mask = pane.layer_mask(style);
    let Some(flipped) = workspace.layers_mut().observe(tab, mask) else {
        return;
    };
    for (slot, layer) in pane
        .layers
        .registry()
        .layers()
        .iter()
        .enumerate()
        .filter(|(slot, _)| flipped & (1 << slot) != 0)
    {
        tracing::info!(target: "quantick::app", schema_version = 1_u8,
            event_code = "CHART_LAYER_SWITCHED", layer = layer.id(),
            on = mask & (1 << slot) != 0, action = "persist_switch",
            "a chart layer switch moved; recording it as the trader's choice");
    }
    save(workspace.chart_layers_path(), &pane.layer_states(style));
    // Preserve the existing best-effort save contract; retries are a separate policy change.
    workspace.layers_mut().record(mask);
}

pub(crate) fn restore(
    workspace: &mut WorkspaceStore,
    tabs: &mut crate::app::arrangement_host::ArrangementHost,
    active: usize,
    style: &mut ChartStyle,
) {
    let defaults = load(workspace.chart_layers_path());
    if defaults.is_empty() {
        tracing::error!(target: "quantick::app", schema_version = 1_u8,
            event_code = "CHART_LAYERS_UNAVAILABLE", path = %workspace.chart_layers_path().display(),
            action = "keep_code_defaults", "no layer visibility to apply; the shipped config did not parse");
    } else {
        for (&layer, &visible) in &defaults {
            if layer.0.source == LayerSource::Grid {
                style.canvas.grid_enabled = visible;
            }
        }
        for pane in tabs.iter_mut().flat_map(Tab::panes_mut) {
            pane.apply_layer_states(&defaults);
        }
        tracing::info!(target: "quantick::app", schema_version = 1_u8,
            event_code = "CHART_LAYERS_RESTORED", path = %workspace.chart_layers_path().display(),
            off = defaults.values().filter(|visible| !**visible).count(), layers = defaults.len(),
            "chart layer visibility restored");
    }
    workspace
        .layers_mut()
        .rebaseline(tabs.id_at(active), tabs[active].flow_pane.layer_mask(style));
}
