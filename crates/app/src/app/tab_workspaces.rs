//! The window's side of the workspace book: reading a tab into a recipe and
//! putting a recipe on a tab. The upkeep is `quantick_stores::tab_recipes`'s; what
//! a row does is `quantick_control_handlers::workspace_book`'s.
use std::collections::BTreeMap;
use std::time::Instant;

use quantick_control_handlers::workspace_book as ops;
use quantick_layers::{ChartLayer, recipe_layers};
use quantick_session::workspace_menu::WorkspaceCommand;
use quantick_stores::tab_recipes::{self, TabRecipeHost};
use quantick_stores::workspaces::{WorkspaceBookStore, WorkspaceRecipe, capture_recipe};

use super::QuantickApp;
use super::control_host::LayoutPort as _;
use crate::pane::{ChartPane, PaneSide};
use crate::tab::{CanvasCollapseRestore, CanvasLayout, LegendFold};

fn layer_states(layers: &BTreeMap<String, bool>) -> BTreeMap<ChartLayer, bool> {
    quantick_layers::recipe_states(crate::pane::registered_layers().layers(), layers)
}

/// What the frame owes the book, once its panes are built and seeded.
pub(super) fn maintain(app: &mut QuantickApp) {
    app.layout_adapter().own_indicator_copies();
    tab_recipes::maintain(&mut BookTabs(app));
}

/// Put `recipe` on a tab.
pub(super) fn show_recipe(app: &mut QuantickApp, id: u64, recipe: &WorkspaceRecipe) {
    tab_recipes::show_recipe(&mut BookTabs(app), id, recipe);
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
                && tab.time_panes.len()
                    >= crate::config::DeclaredLayout::from(tab.layout).context_panes()
        })
    }
    fn live_recipe(&self, id: u64) -> Option<WorkspaceRecipe> {
        live_recipe(self.0, id)
    }
    fn recipe_tab_symbol(&self, id: u64) -> Option<String> {
        self.0.runtime.tabs.by_id(id).map(|tab| tab.symbol.clone())
    }
    fn new_tab_recipe(&self, id: u64) -> WorkspaceRecipe {
        let app = &*self.0;
        let feed = app
            .runtime
            .tabs
            .by_id(id)
            .and_then(|tab| app.config.feed(&tab.feed_id));
        let declared = feed.and_then(|feed| feed.default_layout);
        let indicators = &app.layout_state().layouts().active().indicators;
        WorkspaceRecipe::new_tab_arrangement(declared.unwrap_or_default(), indicators)
    }
    fn recipe_signal(&self, id: u64) -> u64 {
        use std::hash::{Hash, Hasher};
        let (app, mut hash) = (&*self.0, std::hash::DefaultHasher::new());
        app.runtime.workspace.layouts().revision().hash(&mut hash);
        if let Some(tab) = app.runtime.tabs.by_id(id) {
            let canvas = (tab.layout as u8, tab.context_collapsed, tab.flow_collapsed);
            (canvas, tab.split_fraction.to_bits()).hash(&mut hash);
            tab.context_collapsed_slots().hash(&mut hash);
            for share in tab.context_height_shares() {
                share.to_bits().hash(&mut hash);
            }
            for (pane, _) in tab.panes() {
                let spec = pane.state.spec();
                let style = pane.footprint_config(&app.runtime.footprint_config).style;
                (pane.layout_id(), pane.layer_mask(&app.runtime.style)).hash(&mut hash);
                (style.id(), spec.id(), spec.parameter(), spec.choice()).hash(&mut hash);
            }
        }
        hash.finish()
    }
    fn pin_tab_footprint(&mut self, id: u64) {
        let (window, tabs) = (&self.0.runtime.footprint_config, &mut self.0.runtime.tabs);
        for pane in tabs
            .by_id_mut(id)
            .into_iter()
            .flat_map(|tab| tab.panes_mut())
        {
            pane.footprint.config.pin_current_style(window);
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
    let pane_layers = |pane: &ChartPane| recipe_layers(pane.layer_states(&app.runtime.style));
    let context = tab.panes().filter(|(_, side)| *side != PaneSide::Flow);
    let context_layers = context.map(|(pane, _)| pane_layers(pane)).collect();
    let layers = (pane_layers(&tab.flow_pane), context_layers);
    let window = &app.runtime.footprint_config;
    let style = Some(tab.flow_pane.footprint_config(window).style.id().to_owned());
    Some(capture_recipe(
        &saved,
        app.layout_state().layouts(),
        layers,
        style,
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
    for (pane, side) in tab.panes_with_sides_mut() {
        pane.apply_layer_states(&layer_states(recipe.for_pane(side.index()).1));
        if let Some(style) = style {
            pane.footprint.config.pin_style(style, &window);
        }
    }
}

fn apply_recipe_indicators(app: &mut QuantickApp, id: u64, recipe: &WorkspaceRecipe) {
    let tab = app.runtime.tabs.by_id(id);
    let sides: Vec<_> = tab
        .into_iter()
        .flat_map(|tab| tab.panes().map(|(pane, side)| (pane.id, side)))
        .collect();
    let mut layouts = app.layout_adapter();
    layouts.own_indicator_copies();
    let sets = sides
        .iter()
        .map(|(pane, side)| (*pane, recipe.for_pane(side.index()).0));
    layouts.store.session_mut().part_copies(sets);
    for (_, side) in sides {
        layouts.show_pane_indicators(id, side, recipe.for_pane(side.index()).0);
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
