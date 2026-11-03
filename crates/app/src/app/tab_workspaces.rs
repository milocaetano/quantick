//! The window's side of the workspace book: reading a tab into a recipe and
//! putting a recipe on a tab. The upkeep is `quantick_stores::tab_recipes`'s; what
//! a row does is `quantick_control_handlers::workspace_book`'s.
use std::collections::BTreeMap;
use std::time::Instant;

use quantick_control_handlers::workspace_book as ops;
use quantick_layers::{ChartLayer, LayerSource};
use quantick_session::workspace_menu::WorkspaceCommand;
use quantick_stores::tab_recipes::{self, TabRecipeHost};
use quantick_stores::workspaces::{WorkspaceBookStore, WorkspaceRecipe, capture_recipe};

use super::QuantickApp;
use super::control_host::LayoutPort as _;
use crate::pane::PaneSide;
use crate::tab::{CanvasCollapseRestore, CanvasLayout, LegendFold};

/// A recipe never speaks for the grid, which is the window's, or the drawings'
/// switch, which is the market's.
fn belongs_to_recipe(layer: ChartLayer) -> bool {
    !matches!(layer.0.source, LayerSource::Grid | LayerSource::Drawings)
}

fn layer_states(layers: &BTreeMap<String, bool>) -> BTreeMap<ChartLayer, bool> {
    crate::pane::registered_layers()
        .layers()
        .iter()
        .copied()
        .filter(|layer| belongs_to_recipe(*layer))
        .filter_map(|layer| layers.get(layer.id()).map(|on| (layer, *on)))
        .collect()
}

/// What a pane's switches read as in a recipe.
fn pane_layers(pane: &crate::pane::ChartPane, app: &QuantickApp) -> BTreeMap<String, bool> {
    pane.layer_states(&app.runtime.style)
        .into_iter()
        .filter(|(layer, _)| belongs_to_recipe(*layer))
        .map(|(layer, on)| (layer.id().to_owned(), on))
        .collect()
}

/// What the frame owes the book, once its panes are built and seeded.
pub(super) fn maintain(app: &mut QuantickApp) {
    app.layout_adapter().own_indicator_copies();
    tab_recipes::maintain(&mut BookTabs(app));
}

/// Put `recipe` on a tab. The factory `Default` amounts to what a new tab on
/// this feed opens with: never a bare canvas.
pub(super) fn show_recipe(app: &mut QuantickApp, id: u64, recipe: &WorkspaceRecipe) {
    let shown = if recipe.is_factory() {
        let declared = app
            .runtime
            .tabs
            .by_id(id)
            .and_then(|tab| app.config.feed(&tab.feed_id))
            .and_then(|feed| feed.default_layout)
            .unwrap_or(crate::config::DeclaredLayout::Flow);
        let layouts = app.layout_state().layouts();
        WorkspaceRecipe::new_tab_arrangement(declared, &layouts.active().indicators)
    } else {
        recipe.clone()
    };
    tab_recipes::show_recipe(&mut BookTabs(app), id, shown);
}

/// The window as [`TabRecipeHost`]: the protected root gains no impl.
struct BookTabs<'a>(&'a mut QuantickApp);

impl TabRecipeHost for BookTabs<'_> {
    fn recipe_store(&self) -> &WorkspaceBookStore {
        &self.0.runtime.workspace.book
    }
    fn recipe_store_mut(&mut self) -> &mut WorkspaceBookStore {
        &mut self.0.runtime.workspace.book
    }
    fn recipe_tabs(&self) -> Vec<u64> {
        self.0
            .runtime
            .tabs
            .iter_with_ids()
            .map(|(id, _)| id)
            .collect()
    }
    fn active_recipe_tab(&self) -> u64 {
        self.0.runtime.tabs.active_id()
    }
    fn tab_settled(&self, id: u64) -> bool {
        self.0.runtime.tabs.by_id(id).is_some_and(|tab| {
            tab.panes().all(|(pane, _)| pane.layout_seeded())
                && (!tab.layout.shows_time() || !tab.time_panes.is_empty())
        })
    }
    fn live_recipe(&self, id: u64) -> Option<WorkspaceRecipe> {
        live_recipe(self.0, id)
    }
    fn recipe_tab_symbol(&self, id: u64) -> Option<String> {
        self.0.runtime.tabs.by_id(id).map(|tab| tab.symbol.clone())
    }
    fn pin_tab_footprint(&mut self, id: u64) {
        let window = self.0.runtime.footprint_config.clone();
        if let Some(tab) = self.0.runtime.tabs.by_id_mut(id) {
            for pane in tab.panes_mut() {
                pane.set_footprint_style(window.style, &window);
            }
        }
    }
    fn apply_recipe_canvas(&mut self, id: u64, recipe: &WorkspaceRecipe) {
        apply_recipe_canvas(self.0, id, recipe);
    }
    fn apply_recipe_switches(&mut self, id: u64, recipe: &WorkspaceRecipe) {
        apply_recipe_switches(self.0, id, recipe);
    }
    fn apply_recipe_indicators(&mut self, id: u64, recipe: &WorkspaceRecipe) {
        apply_recipe_indicators(self.0, id, recipe);
    }
}

/// What the tab shows now, as a recipe.
pub(super) fn live_recipe(app: &QuantickApp, id: u64) -> Option<WorkspaceRecipe> {
    let tab = app.runtime.tabs.by_id(id)?;
    let saved = super::arrangement_adapter::saved_tab_of(tab);
    let context_layers = tab
        .panes()
        .filter(|(_, side)| *side != PaneSide::Flow)
        .map(|(pane, _)| pane_layers(pane, app))
        .collect();
    let layers = (pane_layers(&tab.flow_pane, app), context_layers);
    let footprint = tab
        .flow_pane
        .footprint_config(&app.runtime.footprint_config);
    Some(capture_recipe(
        &saved,
        app.layout_state().layouts(),
        layers,
        Some(footprint.style.id().to_owned()),
    ))
}

/// Read one tab; a change the book has not seen becomes its copy, and only its.
pub(super) fn observe_tab(app: &mut QuantickApp, id: u64) {
    tab_recipes::observe_tab(&mut BookTabs(app), id);
}

pub(super) fn symbols_of(app: &QuantickApp, tabs: &[u64]) -> Vec<String> {
    tabs.iter()
        .filter_map(|id| app.runtime.tabs.by_id(*id))
        .map(|tab| tab.symbol.clone())
        .collect()
}

fn apply_recipe_canvas(app: &mut QuantickApp, id: u64, recipe: &WorkspaceRecipe) {
    let intervals = super::saved_context_intervals(&recipe.context_bars, None);
    let Some(tab) = app.runtime.tabs.by_id_mut(id) else {
        return;
    };
    let legends = LegendFold {
        flow: tab.flow_pane.legend_collapsed,
        time: tab.time_pane().is_some_and(|pane| pane.legend_collapsed),
    };
    tab.restore_canvas(
        CanvasLayout::from(recipe.layout),
        recipe.split_fraction,
        CanvasCollapseRestore {
            context: recipe.context_collapsed,
            flow: recipe.flow_collapsed,
            heights: &recipe.context_heights,
            collapsed_slots: &recipe.context_panes_collapsed,
        },
        None,
        &intervals,
        legends,
    );
    for (slot, bars) in recipe.context_bars.iter().enumerate() {
        let index = slot + 1;
        let moved = tab
            .pane_at(index)
            .is_some_and(|pane| pane.state.spec().to_config_string() != *bars);
        if let (true, Ok(spec)) = (
            moved,
            quantick_engine::bar_registry::BUILTIN_BARS.parse(bars),
        ) {
            tab.recut_pane_with(
                index,
                quantick_strategy::DisarmReason::BarSpecChanged,
                |pane| pane.set_spec(spec),
            );
        }
    }
    apply_recipe_switches(app, id, recipe);
}

fn apply_recipe_switches(app: &mut QuantickApp, id: u64, recipe: &WorkspaceRecipe) {
    let window = app.runtime.footprint_config.clone();
    let style = recipe
        .footprint_style
        .as_deref()
        .and_then(crate::footprint_config::FootprintStyle::from_id);
    let Some(tab) = app.runtime.tabs.by_id_mut(id) else {
        return;
    };
    let mut context = recipe.context_panes.iter();
    for (pane, side) in tab.panes_with_sides_mut() {
        if side == PaneSide::Flow {
            pane.apply_layer_states(&layer_states(&recipe.layers));
        } else if let Some(saved) = context.next() {
            pane.apply_layer_states(&layer_states(&saved.layers));
        }
        if let Some(style) = style {
            pane.set_footprint_style(style, &window);
        }
    }
}

fn apply_recipe_indicators(app: &mut QuantickApp, id: u64, recipe: &WorkspaceRecipe) {
    let Some(tab) = app.runtime.tabs.by_id(id) else {
        return;
    };
    let mut context = recipe.context_panes.iter();
    let targets: Vec<_> = tab
        .panes()
        .map(|(_, side)| {
            let set = if side == PaneSide::Flow {
                recipe.flow_indicators.clone()
            } else {
                context
                    .next()
                    .map(|pane| pane.indicators.clone())
                    .unwrap_or_default()
            };
            (side, set)
        })
        .collect();
    let mut layouts = app.layout_adapter();
    layouts.own_indicator_copies();
    for (side, set) in targets {
        layouts.show_pane_indicators(id, side, &set);
    }
}

/// The workspace the active tab holds edits against, while it holds any.
pub(super) fn edits_of_active(app: &QuantickApp) -> Option<String> {
    let tab = app.runtime.tabs.active_id();
    let (workspace, modified) = app.runtime.workspace.book.tab_row(tab);
    modified.then_some(workspace)
}

/// The toolbar's Save to.
pub(super) fn save_active(app: &mut QuantickApp) {
    apply_workspace_command(app, WorkspaceCommand::SaveTo);
}

/// The toolbar's Revert.
pub(super) fn revert_active(app: &mut QuantickApp) {
    apply_workspace_command(app, WorkspaceCommand::Revert);
}

/// Carry out what a workspace row asked for, on what the active tab shows now.
pub(super) fn apply_workspace_command(app: &mut QuantickApp, command: WorkspaceCommand) {
    let tab = app.runtime.tabs.active_id();
    observe_tab(app, tab);
    let symbol = app.active_tab().symbol.clone();
    let mut menu = std::mem::take(&mut app.chrome.window_menu.tab_workspace);
    let note = ops::apply_row_command(app, &mut menu, tab, &symbol, command);
    app.chrome.window_menu.tab_workspace = menu;
    if let Some(message) = note {
        app.surfaces.toast.note(message, Instant::now());
    }
}
