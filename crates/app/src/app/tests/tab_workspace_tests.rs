//! The per-tab workspace at the window: one state behind chip and snapshot, an
//! edit that stays on its tab until Save to, and a first run no one can see.
use super::*;
use crate::app::tab_workspaces::{live_recipe, observe_tab};
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
    run_frame(&mut app, &egui::Context::default());
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

/// Real frames: `frame::process` carries the book's upkeep.
fn frames(app: &mut QuantickApp, count: usize) {
    let ctx = egui::Context::default();
    for _ in 0..count {
        run_frame(app, &ctx);
    }
}

fn ema() -> quantick_workspace::indicator_document::SavedIndicator {
    quantick_workspace::indicator_document::SavedIndicator {
        kind: quantick_workspace::indicator_document::SavedKind::native("native.ema"),
        hidden: false,
        mouse_vertical_line: false,
        inputs: Vec::new(),
        plot_styles: Vec::new(),
    }
}

/// A layer the pane at `index` reads, and the state it reads in.
fn some_layer(app: &QuantickApp, tab: u64, index: usize) -> (ChartLayer, bool) {
    let live = live_recipe(app, tab).expect("an open tab");
    let (layer, on) = live
        .for_pane(index)
        .1
        .iter()
        .next()
        .map(|(layer, on)| (layer.clone(), *on))
        .expect("the pane reads its layers");
    let layer = crate::pane::registered_layers()
        .layers()
        .iter()
        .copied()
        .find(|known| known.id() == layer)
        .expect("a registered layer");
    (layer, on)
}

/// The panes of a time+time+flow canvas arrive one per frame; the first run
/// waits for all of them, so the seeded recipe is what the tab ends up showing.
#[test]
fn a_first_run_on_a_two_context_desk_seeds_after_every_pane_stands() {
    let layouts = crate::scratch::ScratchFile::new("workspace-two-context-seed", "layouts.toml");
    let (mut app, _evt, _cmd, _book) = test_app();
    app.runtime
        .workspace
        .set_layouts_path(layouts.to_path_buf());
    app.runtime.workspace.layouts_mut().set_blocked(false);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    frames(&mut app, 4);
    let tab = app.runtime.tabs.active_id();
    assert_eq!(app.active_tab().time_panes.len(), 2);
    let store = &app.runtime.workspace.book;
    let state = store.book().tab_state(tab).expect("seeded");
    assert!(!state.modified, "a seeded tab reads as clean");
    let recipe = store.book().recipe(state.workspace).expect("its recipe");
    assert_eq!(recipe.context_panes.len(), 2, "both context panes");
    assert_eq!(Some(recipe), live_recipe(&app, tab).as_ref());
}

/// A recipe of two context panes reaches the second pane too, which is built a
/// frame after the first.
#[test]
fn a_switch_to_a_two_pane_recipe_reaches_both_context_panes() {
    let layouts = crate::scratch::ScratchFile::new("workspace-two-context-switch", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let tab = app.runtime.tabs.by_id_mut(first).expect("an open tab");
    tab.set_layout(CanvasLayout::TimeTimeAndFlow);
    frames(&mut app, 4);
    let (layer, on) = some_layer(&app, first, 2);
    let mut two = live_recipe(&app, first).expect("an open tab");
    assert_eq!(two.context_panes.len(), 2);
    two.context_panes[1].indicators = vec![ema()];
    two.context_panes[1]
        .layers
        .insert(layer.id().to_owned(), !on);
    let book = app.runtime.workspace.book.book_mut();
    book.observe(first, &two).expect("held");
    book.save_as(first, "Two").expect("a free name");

    set_tab_workspace(&mut app, second, "Two").expect("a known workspace");
    frames(&mut app, 5);
    let live = live_recipe(&app, second).expect("an open tab");
    assert_eq!(live.context_panes.len(), 2);
    assert_eq!(live.context_panes[1].indicators, [ema()], "the second pane");
    assert_eq!(live.context_panes[1].layers.get(layer.id()), Some(&!on));
    assert_eq!(
        app.runtime.workspace.book.tab_row(second),
        ("Two".to_owned(), false)
    );
}

/// The footprint style, the indicators and the layers are the tab's: an edit on
/// one tab reaches another on the same workspace only by Save to.
#[test]
fn footprint_style_indicators_and_layers_stay_on_their_tab_until_save_to() {
    let layouts = crate::scratch::ScratchFile::new("workspace-no-leak-all", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    for tab in [first, second] {
        set_tab_workspace(&mut app, tab, "Default").expect("always there");
    }
    frames(&mut app, 3);
    let edited = app.runtime.tabs.active_id();
    let other = if edited == first { second } else { first };
    let before = live_recipe(&app, other).expect("an open tab");

    let window = app.runtime.footprint_config.clone();
    let style = crate::footprint_config::FootprintStyle::ALL
        .into_iter()
        .find(|style| *style != window.style)
        .expect("more than one style");
    let change = crate::footprint_config::FootprintConfig { style, ..window };
    app.layer_wiring()
        .apply_footprint_change(crate::surfaces::FootprintChange::Applied(Box::new(change)));
    app.layout_adapter()
        .show_pane_indicators(edited, PaneSide::Flow, &[ema()]);
    let (layer, on) = some_layer(&app, edited, 0);
    let tab = app.runtime.tabs.by_id_mut(edited).expect("an open tab");
    tab.flow_pane
        .apply_layer_states(&std::collections::BTreeMap::from([(layer, !on)]));
    frames(&mut app, 3);
    observe_tab(&mut app, other);

    let store = &app.runtime.workspace.book;
    assert!(store.tab_row(edited).1, "the edits mark their tab");
    assert!(!store.tab_row(other).1, "and no other");
    assert_eq!(live_recipe(&app, other).as_ref(), Some(&before));

    let saved = save_tab_to(&mut app, edited).expect("saved");
    assert_eq!(saved.tabs_changed, [other]);
    frames(&mut app, 3);
    let followed = live_recipe(&app, other).expect("an open tab");
    assert_eq!(followed.footprint_style.as_deref(), Some(style.id()));
    assert_eq!(followed.flow_indicators, [ema()]);
    assert_eq!(followed.layers.get(layer.id()), Some(&!on));
    assert!(!app.runtime.workspace.book.tab_row(other).1);
}

/// On the factory Default an edit undone by hand clears the mark, as anywhere.
#[test]
fn an_edit_undone_by_hand_on_the_factory_default_reads_clean() {
    let layouts = crate::scratch::ScratchFile::new("workspace-default-undo", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "THIRDUSDT".to_owned(), None);
    let third = app.runtime.tabs.active_id();
    frames(&mut app, 3);
    let was = app.active_tab().split_fraction;
    edit_split(&mut app, third, 0.27);
    assert_eq!(
        app.runtime.workspace.book.tab_row(third),
        ("Default".to_owned(), true)
    );
    edit_split(&mut app, third, was);
    assert_eq!(
        app.runtime.workspace.book.tab_row(third),
        ("Default".to_owned(), false)
    );
}

/// A pane's own copy is its own: no other pane switches onto it.
#[test]
fn a_pane_cannot_switch_onto_another_tabs_private_copy() {
    let layouts = crate::scratch::ScratchFile::new("workspace-private-refused", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let theirs = app
        .runtime
        .tabs
        .by_id(second)
        .and_then(|tab| tab.flow_pane.layout_id())
        .expect("a seeded pane");
    assert!(app.layout_state().layouts().is_private(theirs));
    assert!(
        app.layout_adapter()
            .switch_pane_layout(first, PaneSide::Flow, theirs)
            .is_err()
    );
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
    frames(&mut app, 1);
    let before = live_recipe(&app, second).expect("an open tab");

    edit_split(&mut app, first, 0.31);
    observe_tab(&mut app, second);
    assert!(app.runtime.workspace.book.tab_row(first).1);
    assert!(!app.runtime.workspace.book.tab_row(second).1);
    assert_eq!(live_recipe(&app, second).as_ref(), Some(&before));

    let saved = save_tab_to(&mut app, first).expect("saved");
    assert_eq!(saved.tabs_changed, [second]);
    frames(&mut app, 1);
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

/// A desk brought back joins the book through the real frame alone, on the
/// workspace it named, and that workspace's recipe lands on it.
#[test]
fn the_real_frame_adopts_a_restored_tab_on_its_saved_workspace() {
    let layouts = crate::scratch::ScratchFile::new("workspace-frame-adopts", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    edit_split(&mut app, first, 0.31);
    let book = app.runtime.workspace.book.book_mut();
    book.save_as(first, "Scalp").expect("a free name");
    let tab = app.runtime.tabs.by_id(first).expect("an open tab");
    let mut saved = crate::app::arrangement_adapter::saved_tab_of(tab);
    saved.symbol = "ETHUSDT".to_owned();
    saved.split_fraction = Some(0.6);
    saved.workspace = Some("Scalp".to_owned());
    saved.workspace_copy = None;
    let config = app.config.clone();
    let desk = ui_state::Workspace::new(true, None, 0, vec![saved], None);
    app.arrangement_adapter()
        .restore_workspace(desk.restore(&config));
    let restored = app.runtime.tabs.active_id();
    assert_eq!(app.active_tab().symbol, "ETHUSDT");
    assert!(
        app.runtime
            .workspace
            .book
            .book()
            .tab_state(restored)
            .is_none(),
        "nothing has adopted it yet"
    );

    frames(&mut app, 4);
    assert_eq!(
        app.runtime.workspace.book.tab_row(restored),
        ("Scalp".to_owned(), false)
    );
    let live = live_recipe(&app, restored).expect("an open tab");
    assert_eq!(
        live.split_fraction,
        Some(0.31),
        "the workspace's, not the desk's"
    );
}

/// The panes of one tab on one strip layout share the tab's copy: an indicator
/// added on the flow pane reaches the tab's context pane, and never a pane of
/// another tab on the same workspace and the same strip layout.
#[test]
fn an_indicator_added_on_a_flow_pane_reaches_its_tabs_context_pane_and_no_other_tab() {
    let layouts = crate::scratch::ScratchFile::new("workspace-mirror-in-tab", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let a = app.runtime.tabs.active_id();
    let b = if a == first { second } else { first };
    for tab in [a, b] {
        let tab = app.runtime.tabs.by_id_mut(tab).expect("an open tab");
        tab.set_layout(CanvasLayout::TimeAndFlow);
    }
    frames(&mut app, 3);
    observe_tab(&mut app, a);
    let book = app.runtime.workspace.book.book_mut();
    book.save_as(a, "Pair").expect("a free name");
    set_tab_workspace(&mut app, b, "Pair").expect("a known workspace");
    frames(&mut app, 5);
    assert_eq!(
        app.runtime.workspace.book.tab_row(b),
        ("Pair".to_owned(), false)
    );
    let strip = |app: &QuantickApp, tab, side| app.layout_state().pane_layout(tab, side);
    let flow = strip(&app, a, PaneSide::Flow);
    assert_eq!(flow, strip(&app, a, PaneSide::Time(0)), "one strip layout");
    assert_eq!(flow, strip(&app, b, PaneSide::Flow), "on both tabs");
    let count = |app: &QuantickApp, tab: u64, side| {
        let tab = app.runtime.tabs.by_id(tab).expect("an open tab");
        tab.pane_at(PaneSide::index(side))
            .expect("a built pane")
            .indicators
            .all()
            .len()
    };
    let context = count(&app, a, PaneSide::Time(0));
    let other = [PaneSide::Flow, PaneSide::Time(0)].map(|side| count(&app, b, side));

    app.active_tab_mut().focus = PaneSide::Flow;
    app.apply_toolbar_action(ToolbarAction::AddNative("native.ema"));
    settle_indicators(&mut app);
    frames(&mut app, 2);
    assert_eq!(
        count(&app, a, PaneSide::Time(0)),
        context + 1,
        "its own context pane"
    );
    let after = [PaneSide::Flow, PaneSide::Time(0)].map(|side| count(&app, b, side));
    assert_eq!(after, other, "never another tab");
    let store = &app.runtime.workspace.book;
    assert!(store.tab_row(a).1, "the edit marks its tab");
    assert!(!store.tab_row(b).1, "and no other");
}
