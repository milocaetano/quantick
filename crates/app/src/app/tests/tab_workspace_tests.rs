//! The per-tab workspace at the window: one state behind chip and snapshot, an
//! edit that stays on its tab until Save to, and a first run no one can see.
use super::*;
use crate::app::tab_workspaces::{live_recipe, maintain, observe_tab};
use quantick_control_handlers::workspace_book::{
    WorkspaceBookHost, save_tab_to, set_tab_workspace,
};

/// A desk of two tabs opening together, seeded on the scratch book beside
/// `layouts`.
fn two_tabs(layouts: &crate::scratch::ScratchFile) -> (QuantickApp, u64, u64) {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.runtime
        .workspace
        .set_layouts_path(layouts.to_path_buf());
    app.runtime.workspace.layouts_mut().set_blocked(false);
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "OTHERUSDT".to_owned(), None);
    let (first, second) = (app.runtime.tabs.id_at(0), app.runtime.tabs.id_at(1));
    app.layout_adapter().maintain_layouts();
    maintain(&mut app);
    (app, first, second)
}

fn edit_split(app: &mut QuantickApp, tab: u64, fraction: f32) {
    app.runtime
        .tabs
        .by_id_mut(tab)
        .expect("an open tab")
        .split_fraction = fraction;
    observe_tab(app, tab);
}

#[test]
fn a_first_run_leaves_tabs_unmodified_with_the_recipe_equal_to_what_they_show() {
    let layouts = crate::scratch::ScratchFile::new("workspace-first-run", "layouts.toml");
    let (app, first, second) = two_tabs(&layouts);
    for tab in [first, second] {
        let store = &app.runtime.workspace.book;
        let state = store.book().tab_state(tab).expect("seeded");
        assert!(!state.modified, "a seeded tab reads as clean");
        assert!(state.workspace.starts_with("from "));
        assert_eq!(
            store.book().recipe(state.workspace),
            live_recipe(&app, tab).as_ref(),
            "the recipe is what the tab shows"
        );
    }
}

#[test]
fn chip_and_snapshot_read_one_state() {
    let layouts = crate::scratch::ScratchFile::new("workspace-one-state", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    edit_split(&mut app, first, 0.31);
    let indices = [0, 1];
    let chips =
        crate::app::title_bar::chips_of(&app.runtime.tabs, &app.runtime.workspace.book, &indices);
    for (tab, chip) in [first, second].into_iter().zip(&chips) {
        let (workspace, modified) = app.tab_workspace_row(tab);
        assert_eq!(chip.workspace, workspace);
        assert_eq!(chip.modified, modified);
    }
    assert!(chips[0].modified);
    assert!(!chips[1].modified);
}

#[test]
fn an_edit_on_one_tab_leaves_another_on_the_same_workspace_alone_until_save_to() {
    let layouts = crate::scratch::ScratchFile::new("workspace-no-leak", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let shared = app.runtime.workspace.book.tab_row(first).0;
    set_tab_workspace(&mut app, second, &shared).expect("a known workspace");
    app.layout_adapter().maintain_layouts();
    maintain(&mut app);
    let before = live_recipe(&app, second).expect("an open tab");

    edit_split(&mut app, first, 0.31);
    observe_tab(&mut app, second);
    assert!(app.runtime.workspace.book.tab_row(first).1);
    assert!(!app.runtime.workspace.book.tab_row(second).1);
    assert_eq!(live_recipe(&app, second).as_ref(), Some(&before));

    let saved = save_tab_to(&mut app, first).expect("saved");
    assert_eq!(saved.tabs_changed, [second]);
    app.layout_adapter().maintain_layouts();
    maintain(&mut app);
    let store = &app.runtime.workspace.book;
    assert!(!store.tab_row(first).1);
    assert!(!store.tab_row(second).1);
    let followed = live_recipe(&app, second).expect("an open tab");
    assert_eq!(followed.split_fraction, Some(0.31));
}

#[test]
fn panes_on_one_strip_layout_end_on_copies_of_their_own() {
    let layouts = crate::scratch::ScratchFile::new("workspace-own-copies", "layouts.toml");
    let (app, first, second) = two_tabs(&layouts);
    let own = |tab: u64| {
        app.runtime
            .tabs
            .by_id(tab)
            .and_then(|tab| tab.flow_pane.layout_id())
            .expect("a seeded pane")
    };
    assert_ne!(own(first), own(second));
    let book = app.layout_state().layouts();
    assert!(book.is_private(own(first)) && book.is_private(own(second)));
    assert_eq!(book.layouts().len(), 1, "the strip did not grow");
}

/// The trader's report: a new tab opened empty on Default. It opens with what a
/// new tab has always opened with, and clean.
#[test]
fn a_new_tab_on_the_factory_default_keeps_its_charts_and_its_indicators() {
    let layouts = crate::scratch::ScratchFile::new("workspace-new-tab", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    let ema = quantick_workspace::indicator_document::SavedIndicator {
        kind: quantick_workspace::indicator_document::SavedKind::native("native.ema"),
        hidden: false,
        mouse_vertical_line: false,
        inputs: Vec::new(),
        plot_styles: Vec::new(),
    };
    let focused = app
        .active_tab()
        .flow_pane
        .layout_id()
        .expect("a seeded pane");
    app.runtime
        .workspace
        .layouts_mut()
        .session_mut()
        .indicators_mut(focused)
        .expect("the pane's own copy")
        .push(ema);
    app.config.feeds[0].default_layout = Some(crate::config::DeclaredLayout::TimeAndFlow);
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "THIRDUSDT".to_owned(), None);
    let third = app.runtime.tabs.active_id();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    assert_eq!(
        app.runtime.workspace.book.tab_row(third),
        ("Default".to_owned(), false)
    );
    let tab = app.runtime.tabs.by_id(third).expect("the new tab");
    assert_eq!(
        tab.layout,
        CanvasLayout::TimeAndFlow,
        "the feed's declaration"
    );
    assert!(!tab.time_panes.is_empty());
    let live = live_recipe(&app, third).expect("an open tab");
    assert_eq!(
        live.flow_indicators.len(),
        1,
        "what the tab it came from shows"
    );
}

/// Switching a tab onto the factory Default lays out a new tab, not a blank.
#[test]
fn switching_to_the_factory_default_is_never_a_bare_canvas() {
    let layouts = crate::scratch::ScratchFile::new("workspace-to-default", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    let strip = app.layout_state().layouts().active().indicators.clone();
    set_tab_workspace(&mut app, first, "Default").expect("always there");
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.runtime.workspace.book.tab_row(first),
        ("Default".to_owned(), false),
        "the arrangement it landed on is no edit"
    );
    let live = live_recipe(&app, first).expect("an open tab");
    assert_eq!(live.flow_indicators, strip);
}

/// A layer switched on a context pane belongs to that pane's role in the recipe.
#[test]
fn a_context_pane_layer_edit_lands_on_that_pane_in_the_copy() {
    let layouts = crate::scratch::ScratchFile::new("workspace-pane-layers", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    let tab = app.runtime.tabs.by_id_mut(first).expect("an open tab");
    tab.set_layout(CanvasLayout::TimeAndFlow);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let live = live_recipe(&app, first).expect("an open tab");
    let (layer, was) = live.context_panes[0]
        .layers
        .iter()
        .next()
        .map(|(layer, on)| (layer.clone(), *on))
        .expect("a context pane reads its layers");
    let layer_id = crate::pane::registered_layers()
        .layers()
        .iter()
        .copied()
        .find(|known| known.id() == layer)
        .expect("a registered layer");
    let tab = app.runtime.tabs.by_id_mut(first).expect("an open tab");
    tab.time_panes[0].apply_layer_states(&std::collections::BTreeMap::from([(layer_id, !was)]));
    observe_tab(&mut app, first);
    let store = &app.runtime.workspace.book;
    assert!(store.tab_row(first).1, "the edit marks the tab");
    let copy = store.book().tab_copy(first).expect("held");
    assert_eq!(copy.context_panes[0].layers.get(&layer), Some(&!was));
    assert_eq!(
        copy.layers.get(&layer),
        live.layers.get(&layer),
        "the flow pane did not move, so its switch reads as it did"
    );
}
