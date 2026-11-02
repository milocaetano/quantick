//! The window's side of the workspace book: reading a tab into a recipe, putting
//! a recipe on a tab, and carrying out what the workspace rows ask. Every decision
//! is `quantick_workspace::workspace_sync`'s or the book's.
use std::collections::BTreeMap;
use std::time::Instant;

use quantick_control_handlers::workspace_book as ops;
use quantick_control_schema::window_menu as text;
use quantick_layers::{ChartLayer, LayerSource};
use quantick_session::workspace_menu::{NameDialogMode, WorkspaceCommand};
use quantick_stores::workspaces::{SeedTab, TabAdoption, WorkspaceRecipe, capture_recipe};

use super::QuantickApp;
use crate::pane::PaneSide;
use crate::tab::{CanvasCollapseRestore, CanvasLayout, LegendFold};

/// A recipe never speaks for the grid, which is the window's, or the drawings'
/// switch, which is the market's.
fn belongs_to_recipe(layer: ChartLayer) -> bool {
    !matches!(layer.0.source, LayerSource::Grid | LayerSource::Drawings)
}

fn layer_states(recipe: &WorkspaceRecipe) -> BTreeMap<ChartLayer, bool> {
    crate::pane::registered_layers()
        .layers()
        .iter()
        .copied()
        .filter(|layer| belongs_to_recipe(*layer))
        .filter_map(|layer| recipe.layers.get(layer.id()).map(|on| (layer, *on)))
        .collect()
}

/// What the frame owes the book, once its panes are built and seeded.
pub(super) fn maintain(app: &mut QuantickApp) {
    app.layout_adapter().own_indicator_copies();
    adopt_tabs(app);
    apply_pending_recipes(app);
    let active = app.runtime.tabs.active_id();
    observe_tab(app, active);
    let tabs: Vec<u64> = app.runtime.tabs.iter_with_ids().map(|(id, _)| id).collect();
    let other = app
        .runtime
        .workspace
        .book
        .sync_mut()
        .next_watch(&tabs, active);
    if let Some(other) = other {
        observe_tab(app, other);
    }
}

/// The tab's panes stand: seeded, and the context charts it asks for built.
fn tab_settled(app: &QuantickApp, id: u64) -> bool {
    app.runtime.tabs.by_id(id).is_some_and(|tab| {
        tab.panes().all(|(pane, _)| pane.layout_seeded())
            && (!tab.layout.shows_time() || !tab.time_panes.is_empty())
    })
}

/// What the tab shows now, as a recipe.
pub(super) fn live_recipe(app: &QuantickApp, id: u64) -> Option<WorkspaceRecipe> {
    let tab = app.runtime.tabs.by_id(id)?;
    let saved = super::arrangement_adapter::saved_tab_of(tab);
    let layers = tab
        .flow_pane
        .layer_states(&app.runtime.style)
        .into_iter()
        .filter(|(layer, _)| belongs_to_recipe(*layer))
        .map(|(layer, on)| (layer.id().to_owned(), on))
        .collect();
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

fn adopt_tabs(app: &mut QuantickApp) {
    let tabs: Vec<u64> = app.runtime.tabs.iter_with_ids().map(|(id, _)| id).collect();
    let mut seeds = Vec::new();
    let mut all_settled = true;
    for id in tabs {
        let store = &app.runtime.workspace.book;
        let held = store.book().tab_state(id).is_some();
        if held && !store.sync().has_arrival(id) {
            continue;
        }
        if !tab_settled(app, id) {
            all_settled = false;
            continue;
        }
        let store = &mut app.runtime.workspace.book;
        let unused = store.book().is_unused();
        match store.sync_mut().adoption(id, held, unused) {
            TabAdoption::Held => {}
            TabAdoption::Restore { workspace, copy } => {
                let book = store.book_mut();
                book.restore_tab(id, &workspace, copy);
                if let Some(recipe) = book.tab_copy(id).cloned() {
                    store.sync_mut().queue(id, recipe);
                }
            }
            TabAdoption::OnDefault => {
                let recipe = store.book_mut().open_tab(id).clone();
                store.sync_mut().queue(id, recipe);
            }
            TabAdoption::Seed => {
                pin_tab_footprint(app, id);
                let live = live_recipe(app, id);
                let symbol = app.runtime.tabs.by_id(id).map(|tab| tab.symbol.clone());
                if let (Some(live), Some(symbol)) = (live, symbol) {
                    seeds.push(SeedTab {
                        tab: id,
                        symbol,
                        live,
                    });
                }
            }
        }
    }
    if all_settled {
        app.runtime.workspace.book.sync_mut().finish_first_pass();
    }
    if seeds.is_empty() {
        return;
    }
    let store = &mut app.runtime.workspace.book;
    for seed in &seeds {
        store.sync_mut().settle(seed.tab, seed.live.clone());
    }
    store.book_mut().seed_first_run(seeds);
    if let Err(reason) = store.persist() {
        tracing::warn!(target: "quantick::app", schema_version = 1_u8,
            event_code = "WORKSPACES_SEED_NOT_SAVED", reason = %reason,
            action = "seeded_for_this_session_only",
            "the workspaces seeded from the open tabs were not saved");
    }
}

/// A window-wide footprint edit must not move a tab on its own recipe, so a
/// seeded tab holds the setup it shows. No pixel changes.
fn pin_tab_footprint(app: &mut QuantickApp, id: u64) {
    let window = app.runtime.footprint_config.clone();
    if let Some(tab) = app.runtime.tabs.by_id_mut(id) {
        for pane in tab.panes_mut() {
            pane.set_footprint_style(window.style, &window);
        }
    }
}

/// Carry each recipe on its way as far as the tab's panes allow.
pub(super) fn apply_pending_recipes(app: &mut QuantickApp) {
    for id in app.runtime.workspace.book.sync().pending_tabs() {
        let sync = app.runtime.workspace.book.sync();
        let arrived =
            tab_settled(app, id) && live_recipe(app, id).as_ref() == sync.pending_recipe(id);
        if !arrived && let Some(recipe) = app.runtime.workspace.book.sync_mut().take_canvas(id) {
            apply_recipe_canvas(app, id, &recipe);
        }
        if !tab_settled(app, id) {
            continue;
        }
        let Some(recipe) = app.runtime.workspace.book.sync_mut().finish(id) else {
            continue;
        };
        if !arrived {
            apply_recipe_switches(app, id, &recipe);
            apply_recipe_indicators(app, id, &recipe);
        }
        if let Some(live) = live_recipe(app, id) {
            app.runtime.workspace.book.sync_mut().settle(id, live);
        }
    }
}

/// The canvas and the context intervals. Drawings, anchored studies, the flow
/// bar size and the paper position are not touched: they are the market's.
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

/// Layers and the footprint style, on every pane the tab has now.
fn apply_recipe_switches(app: &mut QuantickApp, id: u64, recipe: &WorkspaceRecipe) {
    let states = layer_states(recipe);
    let window = app.runtime.footprint_config.clone();
    let style = recipe
        .footprint_style
        .as_deref()
        .and_then(crate::footprint_config::FootprintStyle::from_id);
    let Some(tab) = app.runtime.tabs.by_id_mut(id) else {
        return;
    };
    for pane in tab.panes_mut() {
        pane.apply_layer_states(&states);
        if let Some(style) = style {
            pane.set_footprint_style(style, &window);
        }
    }
}

/// Each pane by role takes the recipe's set, onto a layout entry only it holds.
fn apply_recipe_indicators(app: &mut QuantickApp, id: u64, recipe: &WorkspaceRecipe) {
    let Some(tab) = app.runtime.tabs.by_id(id) else {
        return;
    };
    let mut context = recipe.context_indicators.iter();
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

/// Read one tab; a change the book has not seen becomes its copy, and only
/// its. Public to this module's callers so a row reads the tab before acting.
pub(super) fn observe_tab(app: &mut QuantickApp, id: u64) {
    let store = &app.runtime.workspace.book;
    if store.sync().is_pending(id) || store.book().tab_state(id).is_none() {
        return;
    }
    if !tab_settled(app, id) {
        return;
    }
    let Some(live) = live_recipe(app, id) else {
        return;
    };
    let (book, sync) = app.runtime.workspace.book.parts_mut();
    let Some(copy) = book.tab_copy(id) else {
        return;
    };
    if let Some(edit) = sync.observe(id, live, copy) {
        // The tab was just found in the book, so this cannot refuse.
        let _ = book.observe(id, &edit);
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

fn symbols_of(app: &QuantickApp, tabs: &[u64]) -> Vec<String> {
    tabs.iter()
        .filter_map(|id| app.runtime.tabs.by_id(*id))
        .map(|tab| tab.symbol.clone())
        .collect()
}

/// Carry out what a workspace row asked for, on the same calls the control
/// plane makes, and say what happened by naming the tabs.
pub(super) fn apply_workspace_command(app: &mut QuantickApp, command: WorkspaceCommand) {
    let tab = app.runtime.tabs.active_id();
    observe_tab(app, tab);
    let symbol = app.active_tab().symbol.clone();
    let note = match command {
        WorkspaceCommand::BeginSaveAs => {
            app.chrome.window_menu.tab_workspace.open_save_as();
            return;
        }
        WorkspaceCommand::BeginRename(from) => {
            app.chrome.window_menu.tab_workspace.open_rename(&from);
            return;
        }
        WorkspaceCommand::SubmitDialog => {
            submit_workspace_dialog(app, tab, &symbol);
            return;
        }
        WorkspaceCommand::Set(name) => ops::set_tab_workspace(app, tab, &name).map(|set| {
            set.changed
                .then(|| text::switched_toast(&symbol, &name, set.dropped_edits))
        }),
        WorkspaceCommand::SaveTo => ops::save_tab_to(app, tab).map(|saved| {
            saved
                .changed
                .then(|| text::saved_toast(&saved.workspace, &symbols_of(app, &saved.tabs_changed)))
        }),
        WorkspaceCommand::Revert => {
            ops::revert_tab(app, tab).map(|changed| changed.then(|| text::reverted_toast(&symbol)))
        }
        WorkspaceCommand::Delete(name) => ops::delete_workspace(app, &name).map(|gone| {
            gone.changed.then(|| {
                text::deleted_toast(
                    &name,
                    &symbols_of(app, &gone.adopted),
                    &symbols_of(app, &gone.kept_edits),
                )
            })
        }),
    };
    let message = match note {
        Ok(Some(message)) => message,
        Ok(None) => return,
        Err(error) => error.message,
    };
    app.surfaces.toast.note(message, Instant::now());
}

/// Enter on the name dialog: done and closed, or refused with the reason
/// under the field and the name still there.
fn submit_workspace_dialog(app: &mut QuantickApp, tab: u64, symbol: &str) {
    let Some(dialog) = app.chrome.window_menu.tab_workspace.dialog.clone() else {
        return;
    };
    let outcome = match &dialog.mode {
        NameDialogMode::SaveAsNew => ops::save_tab_as(app, tab, &dialog.name)
            .map(|saved| text::saved_as_toast(symbol, &saved.workspace)),
        NameDialogMode::Rename { from } => ops::rename_workspace(app, from, &dialog.name)
            .map(|_| text::renamed_toast(from, &dialog.name)),
    };
    match outcome {
        Ok(message) => {
            app.chrome.window_menu.tab_workspace.close_dialog();
            app.surfaces.toast.note(message, Instant::now());
        }
        Err(error) => app.chrome.window_menu.tab_workspace.refuse(error.message),
    }
}
