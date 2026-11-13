//! The per-tab workspace at the window: one state behind chip and snapshot, an
//! edit that stays on its tab until Save to, and a first run no one can see.
use super::*;
use crate::app::tab_workspaces::{BookTabs, live_recipe};
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
    // Held edits are what these tests read: autosave off, as the trader who
    // unticks it. `autosave_on_never_marks_an_edit` covers the default.
    app.runtime.workspace.book.book_mut().set_autosave(false);
    (app, first, second)
}

/// Read one tab now, as the frame's upkeep does for the active one.
fn observe_tab(app: &mut QuantickApp, tab: u64) {
    quantick_workspace::book_upkeep::observe_tab(&mut BookTabs(app), tab);
}

fn edit_split(app: &mut QuantickApp, tab: u64, fraction: f32) {
    app.runtime
        .tabs
        .by_id_mut(tab)
        .expect("an open tab")
        .canvas
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
    let live = live_recipe(&app.runtime, tab).expect("an open tab");
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
    assert_eq!(Some(recipe), live_recipe(&app.runtime, tab).as_ref());
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
    let mut two = live_recipe(&app.runtime, first).expect("an open tab");
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
    let live = live_recipe(&app.runtime, second).expect("an open tab");
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
    let before = live_recipe(&app.runtime, other).expect("an open tab");

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
        .layer_write()
        .apply_states(&std::collections::BTreeMap::from([(layer, !on)]));
    frames(&mut app, 3);
    observe_tab(&mut app, other);

    let store = &app.runtime.workspace.book;
    assert!(store.tab_row(edited).1, "the edits mark their tab");
    assert!(!store.tab_row(other).1, "and no other");
    assert_eq!(live_recipe(&app.runtime, other).as_ref(), Some(&before));

    let saved = save_tab_to(&mut app, edited).expect("saved");
    assert_eq!(saved.tabs_changed, [other]);
    frames(&mut app, 3);
    let followed = live_recipe(&app.runtime, other).expect("an open tab");
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
    let was = app.active_tab().canvas.split_fraction;
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
        assert!(!state.workspace.starts_with("from "), "no seed prefix");
        assert_eq!(
            store.book().recipe(state.workspace),
            live_recipe(&app.runtime, tab).as_ref(),
            "the recipe is what the tab shows"
        );
        assert_eq!(store.tab_row(tab), (state.workspace.to_owned(), false));
    }
    // The upkeep's save reached the disk: the seeds are there on a reopen.
    let book = &app.runtime.workspace.book;
    let reopened = quantick_stores::workspaces::WorkspaceBookStore::open_beside(&layouts);
    for tab in [first, second] {
        let seeded = book.book().tab_state(tab).expect("seeded").workspace;
        assert!(
            reopened.book().recipe(seeded).is_some(),
            "{seeded} was saved"
        );
    }
}

/// The chip names the market the status bar names, and no workspace or mark:
/// the cell's mark is the snapshot's, with autosave off and an edit held.
#[test]
fn chip_and_snapshot_read_one_state() {
    let layouts = crate::scratch::ScratchFile::new("workspace-one-state", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    app.runtime.workspace.book.book_mut().set_autosave(false);
    edit_split(&mut app, first, 0.31);
    let labels: Vec<String> = crate::app::title_bar::chips_of(&app.runtime.tabs, &[0, 1])
        .iter()
        .map(|chip| chip.label.to_owned())
        .collect();
    for (index, tab) in [first, second].into_iter().enumerate() {
        app.runtime.tabs.select(index);
        let status = app.status_model();
        let (symbol, venue) = quantick_workspace::arrangement::strip::chip_parts(&labels[index]);
        assert_eq!(
            (symbol, venue),
            (status.symbol.as_str(), status.venue.as_str())
        );
        let capsule = crate::app::capsule_wiring::CapsuleState::of(&app, &egui::Context::default());
        assert_eq!(capsule.marks().1, app.tab_workspace_row(tab).1);
    }
    assert!(app.tab_workspace_row(first).1 && !app.tab_workspace_row(second).1);
}

/// Autosave on, the default: an edit is written through, never held, so
/// neither the book nor the status cell ever marks it.
#[test]
fn autosave_on_never_marks_an_edit() {
    let layouts = crate::scratch::ScratchFile::new("workspace-autosave-on", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.workspace.book.book_mut().set_autosave(true);
    app.runtime.tabs.select(0);
    edit_split(&mut app, first, 0.31);
    frames(&mut app, 2);
    assert!(!app.runtime.workspace.book.tab_row(first).1);
    assert!(
        !crate::app::capsule_wiring::CapsuleState::of(&app, &egui::Context::default())
            .marks()
            .1
    );
}

/// A command reads the active tab before it acts. An edit first seen there
/// starts autosave's quiet second, as one the frame sees does, or it would
/// never be written through.
#[test]
fn an_edit_a_command_reads_first_is_due_for_write_through() {
    let layouts = crate::scratch::ScratchFile::new("workspace-command-read", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.workspace.book.book_mut().set_autosave(true);
    app.runtime.tabs.select(0);
    frames(&mut app, 2);
    app.runtime
        .tabs
        .by_id_mut(first)
        .expect("an open tab")
        .canvas
        .split_fraction = 0.27;
    let read_first = quantick_session::workspace_menu::WorkspaceCommand::BeginSaveAs;
    crate::app::tab_workspaces::apply_workspace_command(&mut app, read_first);
    let later = std::time::Instant::now() + quantick_workspace::workspace_sync::AUTOSAVE_QUIET;
    let due = app.runtime.workspace.book.sync_mut().take_due(later);
    assert_eq!(
        due,
        [first],
        "the command's reading armed the write-through"
    );
}

#[test]
fn an_edit_on_one_tab_leaves_another_on_the_same_workspace_alone_until_save_to() {
    let layouts = crate::scratch::ScratchFile::new("workspace-no-leak", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let shared = app.runtime.workspace.book.tab_row(first).0;
    set_tab_workspace(&mut app, second, &shared).expect("a known workspace");
    frames(&mut app, 1);
    let before = live_recipe(&app.runtime, second).expect("an open tab");

    edit_split(&mut app, first, 0.31);
    observe_tab(&mut app, second);
    assert!(app.runtime.workspace.book.tab_row(first).1);
    assert!(!app.runtime.workspace.book.tab_row(second).1);
    assert_eq!(live_recipe(&app.runtime, second).as_ref(), Some(&before));

    let saved = save_tab_to(&mut app, first).expect("saved");
    assert_eq!(saved.tabs_changed, [second]);
    frames(&mut app, 1);
    let store = &app.runtime.workspace.book;
    assert!(!store.tab_row(first).1);
    assert!(!store.tab_row(second).1);
    let followed = live_recipe(&app.runtime, second).expect("an open tab");
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
/// new tab has always opened with, the strip layout's set, and clean; the
/// unsaved edits of the tab it came from stay on that tab.
#[test]
fn a_new_tab_on_the_factory_default_keeps_its_charts_and_its_indicators() {
    let layouts = crate::scratch::ScratchFile::new("workspace-new-tab", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    let focused = app
        .active_tab()
        .flow_pane
        .layout_id()
        .expect("a seeded pane");
    let strip = app.layout_state().layouts().strip_layout(focused);
    let session = app.runtime.workspace.layouts_mut().session_mut();
    session
        .indicators_mut(strip)
        .expect("on the strip")
        .push(ema());
    let copy = session
        .indicators_mut(focused)
        .expect("the pane's own copy");
    copy.extend([ema(), ema()]);
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
        tab.canvas.layout,
        CanvasLayout::TimeAndFlow,
        "the feed's declaration"
    );
    assert!(!tab.time_panes.is_empty());
    let live = live_recipe(&app.runtime, third).expect("an open tab");
    assert_eq!(
        live.flow_indicators.len(),
        1,
        "the strip layout's set, never the other tab's edits"
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
    let live = live_recipe(&app.runtime, first).expect("an open tab");
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
    let live = live_recipe(&app.runtime, first).expect("an open tab");
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
    tab.time_panes[0]
        .layer_write()
        .apply_states(&std::collections::BTreeMap::from([(layer_id, !was)]));
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
    let live = live_recipe(&app.runtime, restored).expect("an open tab");
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

/// `layout.tab.switch` as a script calls it, answered as the wire answers.
fn switch_by_script(app: &mut QuantickApp, input: serde_json::Value) -> serde_json::Value {
    let mut access = crate::control::ControlAccess::new();
    let actor = crate::app::control_host::tests::fake::FakeWindow::assistant();
    crate::control::invoke_action(app, &mut access, &actor, "layout.tab.switch", input)
        .expect("a strip layout")
}

/// A script's switch and an add at once, on a pane no frame has seen: the add
/// lands on the tab's own copy, and the strip layout every tab reads is
/// untouched.
#[test]
fn a_scripted_switch_then_an_add_on_a_fresh_pane_leaves_the_strip_layout_alone() {
    let layouts = crate::scratch::ScratchFile::new("workspace-fresh-pane-add", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    let strip = app.layout_state().layouts().active().clone();
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "THIRDUSDT".to_owned(), None);
    // As a desk that names no layout opens it.
    app.active_tab_mut().flow_pane.request_opening_layout(None);
    app.active_tab_mut().focus = PaneSide::Flow;
    switch_by_script(&mut app, serde_json::json!({ "name": strip.name }));
    app.apply_toolbar_action(ToolbarAction::AddNative("native.ema"));

    let held = app.active_tab().flow_pane.layout_id().expect("a layout");
    let book = app.layout_state().layouts();
    assert!(book.is_private(held), "the tab's own copy");
    let added = book.get(held).expect("held").indicators.len();
    assert_eq!(added, strip.indicators.len() + 1, "the add landed there");
    let unchanged = |app: &QuantickApp| {
        let book = app.layout_state().layouts();
        book.get(strip.id).expect("on the strip").indicators == strip.indicators
    };
    assert!(unchanged(&app), "the strip layout is untouched");
    frames(&mut app, 2);
    assert!(unchanged(&app), "and stays so");
}

/// The strip's snapshot counts what the focused pane shows, its tab's copy, so
/// the chart and the snapshot agree.
#[test]
fn the_layout_snapshot_counts_what_the_focused_pane_shows() {
    let layouts = crate::scratch::ScratchFile::new("workspace-snapshot-count", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    let tab = app.runtime.tabs.active_id();
    app.active_tab_mut().focus = PaneSide::Flow;
    app.layout_adapter()
        .show_pane_indicators(tab, PaneSide::Flow, &[ema()]);
    let strip = app.layout_state().focused_pane_layout();
    let book = app.layout_state().layouts();
    let held = app.active_tab().flow_pane.layout_id().expect("a layout");
    let shown = book.get(held).expect("held").indicators.len();
    assert_eq!(shown, 1, "the chart shows the tab's copy");
    assert!(
        book.get(strip).expect("on the strip").indicators.is_empty(),
        "the edit is the tab's alone"
    );

    let answer = switch_by_script(&mut app, serde_json::json!({}));
    let rows: Vec<quantick_control_schema::layout::LayoutTabSnapshot> =
        serde_json::from_value(answer["layouts"].clone()).expect("the strip rows");
    let row = rows
        .iter()
        .find(|row| row.layout_id.get() == strip.0)
        .expect("its row");
    assert!(row.active);
    assert_eq!(row.indicator_count.get(), shown as u64);
}

/// The switch and delete rows in the window's journal, as kind and payload.
fn workspace_journal(app: &QuantickApp) -> Vec<(String, serde_json::Value)> {
    use quantick_control_schema::workspace_book as kinds;
    let ours = [
        kinds::TAB_WORKSPACE_SET_EVENT_KIND,
        kinds::WORKSPACE_DELETED_EVENT_KIND,
    ];
    let access = app.control.control_access.as_ref().expect("control access");
    let read = access.journal().read(1, 256, 1 << 20);
    read.events
        .iter()
        .filter(|event| ours.contains(&event.kind.as_str()))
        .map(|event| (event.kind.as_str().to_owned(), event.payload.clone()))
        .collect()
}

/// The `⋮` rows take the calls' path: a switch from the menu and a delete from
/// the menu each leave the journal row the control call leaves.
#[test]
fn a_menu_switch_and_delete_journal_what_the_control_calls_do() {
    use crate::control::ActionOrigin;
    use quantick_session::workspace_menu::WorkspaceCommand;
    use serde_json::json;
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-journal", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    let tab = app.runtime.tabs.active_id();
    let call = |app: &mut QuantickApp, id: &str, input: serde_json::Value| {
        app.control_action(id, 1, ActionOrigin::Human, input)
            .unwrap_or_else(|error| panic!("{id}: {error:?}"));
    };
    let menu = crate::app::tab_workspaces::apply_workspace_command;

    call(&mut app, "workspace.save_as", json!({ "name": "Scalp" }));
    call(&mut app, "tab.workspace.set", json!({ "name": "Default" }));
    call(&mut app, "tab.workspace.set", json!({ "name": "Scalp" }));
    menu(&mut app, WorkspaceCommand::Set("Default".to_owned()));
    let rows = workspace_journal(&app);
    assert_eq!(rows.len(), 3, "one row per switch: {rows:?}");
    assert_eq!(rows[2], rows[0], "the menu's switch is the call's");
    assert_eq!(rows[0].1["tab_id"], json!(tab.to_string()));

    call(&mut app, "tab.workspace.set", json!({ "name": "Scalp" }));
    call(&mut app, "workspace.delete", json!({ "name": "Scalp" }));
    call(&mut app, "workspace.save_as", json!({ "name": "Scalp" }));
    menu(&mut app, WorkspaceCommand::Delete("Scalp".to_owned()));
    let rows = workspace_journal(&app);
    let deletes: Vec<_> = rows
        .iter()
        .filter(|row| row.0.ends_with("deleted"))
        .collect();
    assert_eq!(deletes.len(), 2, "one row per delete: {rows:?}");
    assert_eq!(deletes[1], deletes[0], "the menu's delete is the call's");
    assert_eq!(app.runtime.workspace.book.tab_row(tab).0, "Default");
}

/// The capsule reads the book's row: the same name and mark, on a clean tab
/// and on an edited one, and never a mark with autosave on.
#[test]
fn chip_and_status_cell_read_one_state() {
    let layouts = crate::scratch::ScratchFile::new("workspace-cell-state", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    app.runtime.workspace.book.set_autosave(true);
    edit_split(&mut app, first, 0.31);
    assert!(
        !app.runtime.workspace.book.tab_row(first).1,
        "autosave on never marks"
    );
    app.runtime.workspace.book.book_mut().set_autosave(false);
    edit_split(&mut app, first, 0.42);
    for (index, tab) in [first, second].into_iter().enumerate() {
        app.runtime.tabs.select(index);
        assert_eq!(app.runtime.tabs.active_id(), tab);
        let capsule = crate::app::capsule_wiring::CapsuleState::of(&app, &egui::Context::default());
        let (workspace, modified) = app.runtime.workspace.book.tab_row(tab);
        let (name, unsaved, _) = capsule.marks();
        assert_eq!((name.to_owned(), unsaved), (workspace, modified));
    }
    assert!(app.runtime.workspace.book.tab_row(first).1);
}

/// A torn-off window's cell opens the menu on its own tab, the main window's
/// tab never sees it, and a row chosen there changes only that tab.
#[test]
fn the_cell_menu_in_a_torn_off_window_acts_on_that_windows_tab() {
    use quantick_session::workspace_menu::WorkspaceCommand;
    use quantick_workspace::arrangement::{TabId, windows::TransferTarget};
    let layouts = crate::scratch::ScratchFile::new("workspace-cell-window", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let first_before = app.runtime.workspace.book.tab_row(first);
    let tabs = &mut app.runtime.tabs;
    let child = tabs
        .transfer_tab(TabId(second), TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    tabs.focus_window(child).unwrap();
    tabs.present_window(child).unwrap();
    assert_eq!(app.runtime.tabs.active_id(), second);
    let cell = egui::Rect::from_min_size(egui::pos2(10.0, 700.0), egui::vec2(90.0, 20.0));
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        crate::app::menu_bar::workspace_cell(&mut app, ctx, Some(cell), true);
    });
    let menu = &app.chrome.window_menu.tab_workspace;
    assert!(menu.chip_menu_on(second));
    assert!(!menu.chip_menu_on(first), "the main window's tab");

    let menu = crate::app::tab_workspaces::apply_workspace_command;
    menu(&mut app, WorkspaceCommand::BeginNew);
    let dialog = app.chrome.window_menu.tab_workspace.dialog.as_mut();
    dialog.expect("the name comes first").name = "Blank".to_owned();
    menu(&mut app, WorkspaceCommand::SubmitDialog);
    let book = &app.runtime.workspace.book;
    assert_eq!(book.tab_row(second), ("Blank".to_owned(), false));
    assert_eq!(book.tab_row(first), first_before);
}

/// A new workspace reads clean once the tab's panes stand on it again: the
/// empty recipe keeps the sizes the tab reads back.
#[test]
fn a_new_workspace_reads_clean_after_real_frames() {
    let layouts = crate::scratch::ScratchFile::new("workspace-new-clean", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    edit_split(&mut app, first, 0.37);
    let input = serde_json::json!({ "name": "Blank" });
    app.control_action(
        "workspace.new",
        1,
        crate::control::ActionOrigin::Human,
        input,
    )
    .expect("workspace.new");
    frames(&mut app, 4);
    let store = &app.runtime.workspace.book;
    assert_eq!(store.tab_row(first), ("Blank".to_owned(), false));
    let recipe = store.book().recipe("Blank").expect("stored");
    assert!(recipe.flow_indicators.is_empty());
    assert!(
        recipe
            .context_panes
            .iter()
            .all(|pane| pane.indicators.is_empty())
    );
}

/// Autosave off: a level drawn on the chart marks the tab and waits in memory;
/// Save writes the market file with the recipe and the mark clears.
#[test]
fn autosave_off_holds_a_drawing_until_save_writes_it() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let layouts = crate::scratch::ScratchFile::new("workspace-held-drawing", "layouts.toml");
    app.runtime
        .workspace
        .set_layouts_path(layouts.to_path_buf());
    app.runtime.workspace.layouts_mut().set_blocked(false);
    (app.runtime.workspace.layouts_mut().market_mut()).set_blocked(false);
    frames(&mut app, 2);
    app.runtime.workspace.book.book_mut().set_autosave(false);
    let tab = app.runtime.tabs.active_id();
    save_tab_to(&mut app, tab).expect("the canvas saved first");
    frames(&mut app, 1);
    let state = app.runtime.workspace.book.book().tab_state(tab);
    assert!(!state.expect("adopted").modified, "nothing held yet");
    place_level(&mut app, PaneSide::Flow, 100.0);
    frames(&mut app, 2);
    let market = app
        .runtime
        .workspace
        .layouts()
        .market()
        .path()
        .to_path_buf();
    let book = app.runtime.workspace.book.book();
    assert!(book.tab_state(tab).expect("adopted").modified, "the mark");
    let held = book.unsaved_edits(tab).expect("closing asks").changes;
    assert_eq!(held, ["1 drawing"]);
    assert!(!market.exists(), "held: nothing reached the disk");
    save_tab_to(&mut app, tab).expect("saved");
    let state = app.runtime.workspace.book.book().tab_state(tab);
    assert!(!state.expect("adopted").modified, "Save clears the mark");
    let written = std::fs::read_to_string(&market).expect("Save wrote the market file");
    assert!(written.contains("horizontal-line"), "{written}");
}

/// A flow bar-size change re-cuts the series, so a saved level's bar offset
/// moves with it. That is the chart re-deriving its anchor from market time,
/// not the trader editing the drawing: nothing marks and nothing is held.
#[test]
fn a_flow_bar_size_change_never_counts_as_a_drawing_edit() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let layouts = crate::scratch::ScratchFile::new("workspace-recut-drawing", "layouts.toml");
    app.runtime
        .workspace
        .set_layouts_path(layouts.to_path_buf());
    app.runtime.workspace.layouts_mut().set_blocked(false);
    (app.runtime.workspace.layouts_mut().market_mut()).set_blocked(false);
    frames(&mut app, 2);
    app.runtime.workspace.book.book_mut().set_autosave(false);
    let tab = app.runtime.tabs.active_id();
    let slot = app.active_tab().pane(PaneSide::Flow).slots() / 2;
    let flow = app.active_tab().pane(PaneSide::Flow);
    let time = flow.slot_open_time(slot).expect("bars to anchor on");
    let tool = crate::drawings::DrawingTool::by_id("horizontal-line").expect("the tool");
    #[allow(clippy::cast_precision_loss)]
    let point = crate::drawings::ChartPoint {
        bar: slot as f32,
        price: 100.0,
        time_ms: Some(time),
    };
    let flow = app.active_tab_mut().pane_mut(PaneSide::Flow);
    assert!(flow.drawings.place(tool, point), "a level places");
    frames(&mut app, 2);
    save_tab_to(&mut app, tab).expect("saved");
    frames(&mut app, 1);
    let held = |app: &QuantickApp| app.runtime.workspace.book.book().unsaved_edits(tab);
    assert_eq!(held(&app), None, "Save left nothing held");
    let wider = quantick_engine::bar_registry::BUILTIN_BARS.parse("tick:3");
    let tab_mut = app.active_tab_mut();
    assert!(
        tab_mut
            .set_pane_bar_spec(0, wider.expect("a tick bar"))
            .unwrap()
    );
    frames(&mut app, 3);
    let bars = |app: &QuantickApp| {
        let marks = app.active_tab().pane(PaneSide::Flow).drawings.items();
        marks
            .iter()
            .map(|mark| mark.points[0].bar)
            .collect::<Vec<_>>()
    };
    #[allow(clippy::cast_precision_loss)]
    let moved = bars(&app) != [slot as f32];
    assert!(moved, "the re-cut moved the bar offset: {:?}", bars(&app));
    let changes = held(&app)
        .expect("the bar change is a workspace edit")
        .changes;
    assert_eq!(
        changes,
        ["timeframe"],
        "the re-cut level is no drawing edit"
    );
    // The next real edit writes the whole pane back, the re-cut level with
    // it: only the new level is the trader's.
    place_level(&mut app, PaneSide::Flow, 101.0);
    frames(&mut app, 2);
    let changes = held(&app).expect("the new level is held").changes;
    assert_eq!(
        changes,
        ["timeframe", "1 drawing"],
        "the re-cut level is not counted"
    );
}

/// Golden: a workspace remembers its timeframes. A saved on 5m and Tick 2000,
/// B on 1m and Tick 500; switching back to A reopens 5m and Tick 2000.
#[test]
fn a_workspace_switch_restores_the_timeframes_it_saved() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 50);
    let tab = app.runtime.tabs.active_id();
    app.runtime.workspace.book.book_mut().set_autosave(false);
    let put = |app: &mut QuantickApp, flow: &str, time: &str| {
        let rules = &quantick_engine::bar_registry::BUILTIN_BARS;
        let tab = app.active_tab_mut();
        tab.set_pane_bar_spec(0, rules.parse(flow).expect("a flow rule"))
            .expect("taken");
        tab.set_pane_bar_spec(1, rules.parse(time).expect("a time rule"))
            .expect("taken");
        frames(app, 3);
    };
    let shown = |app: &QuantickApp| {
        let tab = app.active_tab();
        let rule = |pane: &crate::pane::ChartPane| pane.state.spec().to_config_string();
        (rule(&tab.flow_pane), rule(&tab.time_panes[0]))
    };
    put(&mut app, "tick:2000", "time:300000");
    let a = shown(&app);
    let book = app.runtime.workspace.book.book_mut();
    book.save_as(tab, "A").expect("a free name");
    put(&mut app, "tick:500", "time:60000");
    let state = app.runtime.workspace.book.book().tab_state(tab);
    assert!(
        state.expect("adopted").modified,
        "a timeframe change marks A"
    );
    let b = shown(&app);
    let book = app.runtime.workspace.book.book_mut();
    book.save_as(tab, "B").expect("a free name");
    assert_ne!(a, b);
    set_tab_workspace(&mut app, tab, "A").expect("a known workspace");
    frames(&mut app, 4);
    assert_eq!(shown(&app), a, "A reopens on its own timeframes");
    let state = app.runtime.workspace.book.book().tab_state(tab);
    assert!(!state.expect("adopted").modified, "and reads clean");
    set_tab_workspace(&mut app, tab, "B").expect("a known workspace");
    frames(&mut app, 4);
    assert_eq!(shown(&app), b, "and B on its");
}

/// A new tab on a market opens its context chart on the interval that market
/// remembers, as it opens its flow chart on the market's bar.
#[test]
fn a_new_tab_on_a_market_opens_on_the_timeframe_it_remembers() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 50);
    let hour = quantick_engine::bar_registry::BUILTIN_BARS.parse("time:3600000");
    app.active_tab_mut()
        .set_pane_bar_spec(1, hour.expect("a time bar"))
        .expect("the time pane takes it");
    frames(&mut app, 2);
    let shown = app.active_tab().time_panes[0]
        .state
        .spec()
        .to_config_string();
    let market = (
        app.active_tab().feed_id.clone(),
        app.active_tab().symbol.clone(),
    );
    let snapshot = MarketPaletteHost::market_snapshot(&app);
    let choice = snapshot.choice((&market.0, &market.1), None, None, false);
    let choice = choice.expect("the market is listed");
    assert_eq!(choice.context_bars, std::slice::from_ref(&shown));
    let _ = MarketPaletteHost::open_tab(&mut app, &choice, None);
    frames(&mut app, 2);
    // The new tab opens on its feed's canvas; its first context chart, when
    // the trader adds one, stands on the market's timeframe.
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    frames(&mut app, 3);
    let opened = app
        .active_tab()
        .time_panes
        .first()
        .expect("a context chart");
    assert_eq!(opened.state.spec().to_config_string(), shown);
}

/// Autosave off: closing a chip whose tab holds edits asks first, and Enter
/// (Save) writes the workspace to disk before the tab goes.
#[test]
fn a_strip_close_with_held_edits_asks_and_enter_saves_then_closes() {
    let layouts = crate::scratch::ScratchFile::new("workspace-close-asks", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let workspace = app.runtime.workspace.book.tab_row(first).0;
    edit_split(&mut app, first, 0.31);
    let index = app.runtime.tabs.position(first).expect("an open tab");
    app.apply_tab_action(crate::tabstrip::TabAction::Close(index));
    assert_eq!(
        app.runtime.tabs.len(),
        2,
        "nothing closes before the answer"
    );
    let queue = &app.chrome.window_menu.tab_workspace.close;
    let prompt = quantick_control_handlers::workspace_book::close_prompt(queue).expect("asks");
    let symbol = app.runtime.tabs.by_id(first).expect("open").symbol.clone();
    assert_eq!(prompt.title, format!("Close {symbol}"));
    assert_eq!(prompt.buttons[prompt.enter], format!("Save to {workspace}"));

    let ctx = egui::Context::default();
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Enter)]);
    assert!(app.runtime.tabs.by_id(first).is_none(), "the tab closed");
    assert!(app.runtime.tabs.by_id(second).is_some());
    let path = quantick_stores::workspaces::path_beside(layouts.as_ref());
    let (on_disk, _) = quantick_stores::workspaces::load(&path);
    let saved = on_disk.recipe(&workspace).expect("the workspace on disk");
    assert_eq!(saved.split_fraction, Some(0.31));
}

/// Autosave off: the close prompt's Esc only cancels the close. The chart
/// under it keeps its selected drawing, as the Cancel button does.
#[test]
fn the_close_prompt_s_esc_leaves_the_chart_s_selection_alone() {
    let layouts = crate::scratch::ScratchFile::new("workspace-close-esc", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    let tab = app.runtime.tabs.active_id();
    let drawings = &mut app.active_tab_mut().flow_pane.drawings;
    drawings.place(
        drawing_tool("horizontal-line"),
        ChartPoint::at_time(1.0, 100.0, None),
    );
    drawings.select(Some(0));
    edit_split(&mut app, tab, 0.33);
    let index = app.runtime.tabs.active_index();
    app.apply_tab_action(crate::tabstrip::TabAction::Close(index));
    let ctx = egui::Context::default();
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
    let queue = &app.chrome.window_menu.tab_workspace.close;
    assert!(queue.question().is_none(), "Esc cancelled the close");
    assert!(app.runtime.tabs.by_id(tab).is_some(), "the tab stays");
    let selected = app.active_tab().flow_pane.drawings.selected();
    assert_eq!(selected, Some(0), "the drawing stays selected");
}

/// Autosave off: the window's close asks before the app exits, and Esc
/// (Cancel) keeps every tab and the app.
#[test]
fn an_exit_with_held_edits_asks_and_esc_keeps_the_app() {
    let layouts = crate::scratch::ScratchFile::new("workspace-exit-asks", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    edit_split(&mut app, first, 0.29);
    let ctx = egui::Context::default();
    close_requested_frame(&mut app, &ctx);
    assert!(
        app.chrome
            .window_menu
            .tab_workspace
            .close
            .question()
            .is_some()
    );
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
    let queue = &app.chrome.window_menu.tab_workspace.close;
    assert!(queue.question().is_none(), "Cancel ends the exit");
    assert_eq!(app.runtime.tabs.len(), 2);
    assert!(
        app.runtime.workspace.book.tab_row(first).1,
        "the edit stays held"
    );
}

/// The window size the trader's desk runs at.
const FULL_HD: egui::Vec2 = egui::vec2(1920.0, 1000.0);

/// The cell's menu as the scene publishes it, after `count` frames open at
/// full HD: the menu's rect, the capsule's name rect and every row id.
fn open_cell_menu(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    count: usize,
) -> (egui::Rect, egui::Rect, Vec<String>) {
    let tab = app.runtime.tabs.active_id();
    app.chrome.window_menu.tab_workspace.toggle_chip_menu(tab);
    for _ in 0..count {
        run_frame_at(app, ctx, FULL_HD);
    }
    assert!(app.chrome.window_menu.tab_workspace.chip_menu_on(tab));
    let controls = &app.chrome.toolbar_controls;
    let find = |id: &str| controls.iter().find(|control| control.id == id);
    let menu = find("toolbar.workspace_menu").expect("the open menu is in the scene");
    assert!(menu.selected);
    let name = find("toolbar.capsule.workspace").expect("the capsule's name");
    let rows = (controls.iter())
        .filter(|control| control.id.starts_with("toolbar.workspace_menu."))
        .map(|control| control.id.clone())
        .collect();
    (menu.rect, name.rect, rows)
}

/// The trader's complaint: the menu off the workspace name stays inside the
/// window, scrolls its list alone, and is as wide as what it shows this time,
/// not the widest thing it ever showed.
#[test]
fn the_cell_menu_stays_inside_the_window_and_narrows_when_its_rows_do() {
    use quantick_session::workspace_menu::CHIP_MENU_MAX_WIDTH_PT;
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-bounded", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    let long = "W".repeat(quantick_workspace::workspace_book::MAX_WORKSPACE_NAME);
    let names: Vec<String> = (1..40).map(|n| format!("Desk {n:02}")).collect();
    let book = app.runtime.workspace.book.book_mut();
    for name in names.iter().chain([&long]) {
        book.create(first, name).expect("a fresh name");
    }
    let ctx = egui::Context::default();
    let (menu, name, rows) = open_cell_menu(&mut app, &ctx, 3);
    assert!(
        menu.width() <= CHIP_MENU_MAX_WIDTH_PT,
        "{} wide",
        menu.width()
    );
    assert!(menu.bottom() <= FULL_HD.y, "bottom {}", menu.bottom());
    assert!(menu.top() >= name.bottom(), "hangs below the capsule");
    for fixed in ["autosave", "save", "save_as", "rename", "new", "manage"] {
        let id = format!("toolbar.workspace_menu.{fixed}");
        assert!(rows.contains(&id), "{id} never scrolls away: {rows:?}");
    }
    let stored = app.runtime.workspace.book.book().list().len();
    let listed = rows.iter().filter(|id| id.contains(".workspace.")).count();
    assert!(
        (1..stored).contains(&listed),
        "{listed} rows in view of {stored}"
    );
    let wide = menu.width();

    app.chrome.window_menu.tab_workspace.close_chip_menu();
    run_frame_at(&mut app, &ctx, FULL_HD);
    let book = app.runtime.workspace.book.book_mut();
    for name in names.iter().skip(1).chain([&long]) {
        book.delete(name).expect("a stored workspace");
    }
    let (menu, _, rows) = open_cell_menu(&mut app, &ctx, 3);
    assert!(menu.width() < wide, "{} after {wide}", menu.width());
    let stored = app.runtime.workspace.book.book().list().len();
    let listed = rows.iter().filter(|id| id.contains(".workspace.")).count();
    assert_eq!(listed, stored, "every row in view: {rows:?}");
}

/// The trader's nine workspaces fit whole below the capsule, and the capsule
/// names the call that reads what the menu lists.
#[test]
fn nine_workspaces_fit_below_the_capsule_and_the_name_names_its_call() {
    use quantick_session::workspace_menu::CHIP_MENU_MAX_WIDTH_PT;
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-nine", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    let book = app.runtime.workspace.book.book_mut();
    for n in 1.. {
        if book.list().len() >= 9 {
            break;
        }
        book.create(first, &format!("Scalp {n}"))
            .expect("a fresh name");
    }
    assert_eq!(book.list().len(), 9, "the fixture seeds fewer than nine");
    let ctx = egui::Context::default();
    let (menu, name, rows) = open_cell_menu(&mut app, &ctx, 3);
    assert!(menu.width() <= CHIP_MENU_MAX_WIDTH_PT);
    assert!(menu.height() <= FULL_HD.y - name.bottom());
    let listed = rows.iter().filter(|id| id.contains(".workspace.")).count();
    assert_eq!(listed, 9, "{rows:?}");
    let capsule = (app.chrome.toolbar_controls.iter())
        .find(|control| control.id == "toolbar.capsule.workspace")
        .expect("the capsule's name");
    let list = quantick_control_schema::workspace_book::WORKSPACE_LIST_CAPABILITY_ID;
    assert_eq!(capsule.capability, Some(list));
}
