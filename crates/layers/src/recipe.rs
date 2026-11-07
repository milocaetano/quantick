//! The layer switches a tab's workspace keeps, by stable layer id. A recipe
//! never speaks for the grid, which is the window's, or the drawings' switch,
//! which is the market's.

use crate::{ChartLayer, LayerSource};
use std::collections::BTreeMap;

/// Whether a workspace recipe speaks for `layer`.
#[must_use]
pub fn in_recipe(layer: ChartLayer) -> bool {
    !matches!(
        layer.0.source,
        LayerSource::Grid | LayerSource::TimeGrid | LayerSource::Drawings
    )
}

/// A pane's switches as a recipe keeps them.
#[must_use]
pub fn recipe_layers(states: BTreeMap<ChartLayer, bool>) -> BTreeMap<String, bool> {
    states
        .into_iter()
        .filter(|(layer, _)| in_recipe(*layer))
        .map(|(layer, on)| (layer.id().to_owned(), on))
        .collect()
}

/// A recipe's switches as the `registered` layers they name. A layer the
/// recipe leaves out keeps what the pane decided.
#[must_use]
pub fn recipe_states(
    registered: &[ChartLayer],
    layers: &BTreeMap<String, bool>,
) -> BTreeMap<ChartLayer, bool> {
    registered
        .iter()
        .copied()
        .filter(|layer| in_recipe(*layer))
        .filter_map(|layer| layers.get(layer.id()).map(|on| (layer, *on)))
        .collect()
}
