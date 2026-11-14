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
/// frame after the first. Its panes share one strip layout, so the fuller set
/// lands on all three; that heal is no edit, so the tab reads clean.
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
    two.flow_indicators = Vec::new();
    two.context_panes[0].indicators = Vec::new();
    two.context_panes[1].indicators = vec![ema()];
    two.context_panes[1]
        .layers
        .insert(layer.id().to_owned(), !on);
    let own = app.runtime.workspace.book.tab_row(first).0;
    let book = app.runtime.workspace.book.book_mut();
    book.observe(first, &two).expect("held");
    book.save_as(first, "Two").expect("a free name");
    // The saving tab never showed `two`: off Two, it holds none of its sets.
    set_tab_workspace(&mut app, first, &own).expect("its own workspace");

    set_tab_workspace(&mut app, second, "Two").expect("a known workspace");
    frames(&mut app, 5);
    let tab = app.runtime.tabs.by_id_mut(second).expect("an open tab");
    for pane in tab.panes_mut() {
        pane.indicator_worker.flush();
        pane.apply_indicator_events();
    }
    let built = tab.pane(PaneSide::Time(1)).indicators.all().len();
    assert_eq!(built, 1, "the pane built a frame later shows its indicator");
    let live = live_recipe(&app.runtime, second).expect("an open tab");
    assert_eq!(live.context_panes.len(), 2);
    assert_eq!(live.context_panes[1].indicators, [ema()], "the second pane");
    assert_eq!(
        live.flow_indicators,
        [ema()],
        "one set for the strip layout"
    );
    assert_eq!(live.context_panes[1].layers.get(layer.id()), Some(&!on));
    assert_eq!(
        app.runtime.workspace.book.tab_row(second),
        ("Two".to_owned(), false),
        "a heal on landing is no edit"
    );
}

/// The footprint style and the layers are the tab's: an edit on one tab
/// reaches another on the same workspace only by Save to. The indicators are
/// the workspace's: see `two_tabs_on_one_workspace_show_one_set_per_layout`.
#[test]
fn footprint_style_and_layers_stay_on_their_tab_until_save_to() {
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

/// The panes of every tab on one workspace and one strip layout share one
/// copy: an indicator added on one tab's flow pane reaches its context pane
/// and both panes of the other tab, and both tabs read the one held edit.
#[test]
fn an_indicator_added_on_a_flow_pane_reaches_every_pane_of_its_layout_in_both_tabs() {
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
    assert_eq!(after, other.map(|count| count + 1), "and the other tab's");
    observe_tab(&mut app, b);
    let store = &app.runtime.workspace.book;
    assert!(store.tab_row(a).1, "the edit marks its tab");
    assert!(store.tab_row(b).1, "and the other, which shows it");
}

const THREE_PANES: [PaneSide; 3] = [PaneSide::Flow, PaneSide::Time(0), PaneSide::Time(1)];

/// How many indicators each of the active tab's three panes shows.
fn three_counts(app: &mut QuantickApp) -> [usize; 3] {
    settle_indicators(app);
    THREE_PANES.map(|side| app.active_tab().pane(side).indicators.all().len())
}

/// The copy every one of the active tab's three panes holds: one, or a panic.
fn one_copy(app: &QuantickApp) -> quantick_workspace::layout_document::LayoutId {
    let held = THREE_PANES.map(|side| app.active_tab().pane(side).layout_id().expect("seeded"));
    assert!(held.iter().all(|id| *id == held[0]), "one copy: {held:?}");
    held[0]
}

/// The trader's Force report: inside one tab, the panes on one strip layout
/// show one set. A recipe giving three panes three sets lands the fullest on
/// all of them; the tab reads clean and autosave owes the stored recipe
/// nothing. An add or a removal on a context pane then reaches the flow pane
/// and the other context pane.
#[test]
fn a_recipe_of_three_sets_lands_one_and_an_edit_reaches_every_pane_of_the_tab() {
    let layouts = crate::scratch::ScratchFile::new("workspace-one-set-a-tab", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let a = app.runtime.tabs.active_id();
    let b = if a == first { second } else { first };
    let tab = app.runtime.tabs.by_id_mut(b).expect("an open tab");
    tab.set_layout(CanvasLayout::TimeTimeAndFlow);
    frames(&mut app, 4);
    let mut three = live_recipe(&app.runtime, b).expect("an open tab");
    assert_eq!(three.context_panes.len(), 2);
    three.flow_indicators = vec![ema()];
    three.context_panes[0].indicators = Vec::new();
    let cvd = quantick_workspace::indicator_document::SavedIndicator {
        kind: quantick_workspace::indicator_document::SavedKind::native("native.cvd"),
        ..ema()
    };
    three.context_panes[1].indicators = vec![ema(), cvd.clone()];
    let own = app.runtime.workspace.book.tab_row(b).0;
    let book = app.runtime.workspace.book.book_mut();
    book.observe(b, &three).expect("held");
    book.save_as(b, "Three").expect("a free name");
    let stored = book.recipe("Three").cloned();
    // The saving tab never showed `three`: off Three, it holds none of its sets.
    set_tab_workspace(&mut app, b, &own).expect("its own workspace");
    app.runtime.workspace.book.book_mut().set_autosave(true);

    // Edits made while autosave was off are behind us: what is due from here
    // on is the landing's alone.
    let quiet = std::time::Instant::now() + quantick_workspace::workspace_sync::AUTOSAVE_QUIET;
    app.runtime.workspace.book.sync_mut().take_due(quiet);
    set_tab_workspace(&mut app, a, "Three").expect("a known workspace");
    frames(&mut app, 5);
    let store = &mut app.runtime.workspace.book;
    assert_eq!(store.tab_row(a), ("Three".to_owned(), false), "no edit");
    let later = std::time::Instant::now() + quantick_workspace::workspace_sync::AUTOSAVE_QUIET;
    let due = store.sync_mut().take_due(later);
    assert!(due.is_empty(), "nothing of the landing to write: {due:?}");
    assert_eq!(store.book().recipe("Three").cloned(), stored, "as saved");
    let copy = one_copy(&app);
    assert_eq!(three_counts(&mut app), [2; 3], "the fullest set");
    let set = &app
        .layout_state()
        .layouts()
        .get(copy)
        .expect("held")
        .indicators;
    assert_eq!(set, &[ema(), cvd]);

    app.active_tab_mut().focus = PaneSide::Time(1);
    app.apply_toolbar_action(ToolbarAction::AddNative("native.cvd"));
    frames(&mut app, 2);
    assert_eq!(three_counts(&mut app), [3; 3], "an add reaches every pane");
    let slot = app.active_tab().pane(PaneSide::Time(0)).indicators.all()[0].slot;
    app.active_tab_mut().focus = PaneSide::Time(0);
    app.apply_toolbar_action(ToolbarAction::RemoveIndicator(slot.0));
    frames(&mut app, 2);
    assert_eq!(three_counts(&mut app), [2; 3], "and so does a removal");
    assert_eq!(one_copy(&app), copy);
}

/// Bring `saved` back as the whole desk, as a launch reads it: on a market no
/// open tab shows, so its panes open fresh on the layouts it names.
fn restore_desk(
    app: &mut QuantickApp,
    mut saved: quantick_workspace::arrangement_document::SavedTab,
    symbol: &str,
) {
    symbol.clone_into(&mut saved.symbol);
    let config = app.config.clone();
    let desk = ui_state::Workspace::new(true, None, 0, vec![saved], None);
    app.arrangement_adapter()
        .restore_workspace(desk.restore(&config));
    frames(app, 4);
}

/// A stored file like the trader's: the flow pane and two context panes of one
/// tab on three copies of one strip layout, each with another set. Brought
/// back, all three show the fullest set, a context pane's, on one copy, and
/// the file saved from that reads back the same.
#[test]
fn a_desk_on_twin_copies_comes_back_on_the_fullest_set() {
    let layouts = crate::scratch::ScratchFile::new("workspace-twin-copies", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    frames(&mut app, 4);
    let flow = app.active_tab().flow_pane.layout_id().expect("seeded");
    let mut book = app.layout_state().layouts().clone();
    let native = |id: &str| quantick_workspace::indicator_document::SavedIndicator {
        kind: quantick_workspace::indicator_document::SavedKind::native(id),
        ..ema()
    };
    let fullest = vec![ema(), native("native.cvd"), native("native.volume")];
    let sets = [
        vec![ema()],
        fullest.clone(),
        vec![ema(), native("native.cvd")],
    ];
    let copies = sets.map(|set| {
        let copy = book.make_private_copy(flow).expect("a known layout");
        book.get_mut(copy).expect("just made").indicators = set;
        copy
    });
    let mut saved = crate::app::arrangement_adapter::saved_tab_of(app.active_tab());
    saved.flow_layout = Some(copies[0].0);
    saved.context_layouts = vec![copies[1].0, copies[2].0];
    app.runtime.workspace.layouts_mut().set_book(book);

    restore_desk(&mut app, saved, "ETHUSDT");
    assert_eq!(one_copy(&app), copies[1], "the fullest copy");
    assert_eq!(three_counts(&mut app), [3; 3]);
    let book = app.layout_state().layouts();
    assert_eq!(book.get(copies[1]).expect("held").indicators, fullest);
    assert!(book.get(copies[0]).is_none() && book.get(copies[2]).is_none());

    let saved = crate::app::arrangement_adapter::saved_tab_of(app.active_tab());
    assert_eq!(
        saved.context_layouts, [copies[1].0; 2],
        "the desk names one copy"
    );
    let text = toml::to_string(app.layout_state().layouts()).expect("a book writes");
    let reread = quantick_workspace::layout_document::parse(&text).expect("and reads back");
    let (mut app, _evt, _cmd, _book) = test_app();
    let relaunch = crate::scratch::ScratchFile::new("workspace-twin-relaunch", "layouts.toml");
    app.runtime
        .workspace
        .set_layouts_path(relaunch.to_path_buf());
    app.runtime.workspace.layouts_mut().set_book(reread);
    restore_desk(&mut app, saved, "ETHUSDT");
    assert_eq!(one_copy(&app), copies[1], "after a relaunch too");
    assert_eq!(three_counts(&mut app), [3; 3]);
}

/// The review's case: workspace W saved from tab A, whose flow pane is on the
/// strip layout tab B shows everywhere and whose context pane is on Levels on
/// purpose. B lands W healed, and autosave on rewrites nothing: W stays as
/// saved and A's Levels pane keeps its own set.
#[test]
fn a_heal_on_one_tab_never_reaches_another_through_autosave() {
    let layouts = crate::scratch::ScratchFile::new("workspace-heal-stays", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let a = app.runtime.tabs.active_id();
    let b = if a == first { second } else { first };
    for tab in [a, b] {
        let tab = app.runtime.tabs.by_id_mut(tab).expect("an open tab");
        tab.set_layout(CanvasLayout::TimeAndFlow);
    }
    frames(&mut app, 3);
    let session = app.runtime.workspace.layouts_mut().session_mut();
    let levels = session.create(Some("Levels")).expect("a free name");
    app.layout_adapter()
        .switch_pane_layout(a, PaneSide::Time(0), levels)
        .expect("a known layout");
    app.active_tab_mut().focus = PaneSide::Flow;
    app.apply_toolbar_action(ToolbarAction::AddNative("native.ema"));
    app.apply_toolbar_action(ToolbarAction::AddNative("native.cvd"));
    settle_indicators(&mut app);
    frames(&mut app, 2);
    observe_tab(&mut app, a);
    let book = app.runtime.workspace.book.book_mut();
    book.save_as(a, "W").expect("a free name");
    book.set_autosave(true);
    let w = book.recipe("W").cloned().expect("saved");
    assert_eq!(w.flow_indicators.len(), 2);
    assert!(
        w.context_panes[0].indicators.is_empty(),
        "Levels holds none"
    );

    // Edits made while autosave was off are behind us: what is due from here
    // on is the landing's alone.
    let quiet = std::time::Instant::now() + quantick_workspace::workspace_sync::AUTOSAVE_QUIET;
    app.runtime.workspace.book.sync_mut().take_due(quiet);
    set_tab_workspace(&mut app, b, "W").expect("a known workspace");
    frames(&mut app, 5);
    let strip = |app: &QuantickApp, tab, side| app.layout_state().pane_layout(tab, side);
    assert_eq!(
        strip(&app, b, PaneSide::Flow),
        strip(&app, b, PaneSide::Time(0))
    );
    assert_eq!(strip(&app, a, PaneSide::Time(0)), levels);
    let shows = |app: &mut QuantickApp, tab: u64, side| {
        let tab = app.runtime.tabs.by_id_mut(tab).expect("an open tab");
        for pane in tab.panes_mut() {
            pane.indicator_worker.flush();
            pane.apply_indicator_events();
        }
        tab.pane(side).indicators.all().len()
    };
    assert_eq!(shows(&mut app, b, PaneSide::Time(0)), 2, "B healed");
    let store = &mut app.runtime.workspace.book;
    assert_eq!(store.tab_row(b), ("W".to_owned(), false), "no edit");
    let later = std::time::Instant::now() + quantick_workspace::workspace_sync::AUTOSAVE_QUIET;
    let due = store.sync_mut().take_due(later);
    assert!(due.is_empty(), "nothing of the landing to write: {due:?}");
    frames(&mut app, 2);
    assert_eq!(app.runtime.workspace.book.book().recipe("W"), Some(&w));
    assert_eq!(shows(&mut app, a, PaneSide::Time(0)), 0, "A's Levels pane");
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

/// A workspace saved before rails carried its column fold only as a flag, no
/// canvas: applied to a tab it lands as one rail and the tab reads clean.
#[test]
fn an_old_folded_recipe_lands_as_one_rail_and_reads_clean() {
    let layouts = crate::scratch::ScratchFile::new("workspace-old-fold", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let tab = app.runtime.tabs.by_id_mut(first).expect("an open tab");
    tab.set_layout(CanvasLayout::TimeAndFlow);
    frames(&mut app, 4);
    let mut old = live_recipe(&app.runtime, first).expect("an open tab");
    (old.canvas, old.context_collapsed, old.flow_collapsed) = (None, true, false);
    let book = app.runtime.workspace.book.book_mut();
    book.observe(first, &old).expect("held");
    book.save_as(first, "Folded").expect("a free name");

    set_tab_workspace(&mut app, second, "Folded").expect("a known workspace");
    frames(&mut app, 5);
    let tab = app.runtime.tabs.by_id(second).expect("an open tab");
    assert_eq!(tab.canvas.folds(), (true, false), "the column is folded");
    assert!(
        tab.canvas.column_painted(),
        "on the column painter's own rail"
    );
    assert_eq!(
        app.runtime.workspace.book.tab_row(second),
        ("Folded".to_owned(), false),
        "and the tab reads clean"
    );
}

/// A workspace saved before rails with a canvas *and* the column fold as a
/// flag: the fold flags were cleared only by a layout pick or a divider drag,
/// so a maximized chart kept the canvas with no rail beside
/// `context_collapsed`. Applied to a tab it lands as one rail and the tab
/// reads clean.
#[test]
fn an_old_folded_recipe_with_a_canvas_lands_as_one_rail_and_reads_clean() {
    let layouts = crate::scratch::ScratchFile::new("workspace-old-fold-canvas", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let tab = app.runtime.tabs.by_id_mut(first).expect("an open tab");
    tab.set_layout(CanvasLayout::TimeAndFlow);
    frames(&mut app, 4);
    let mut old = live_recipe(&app.runtime, first).expect("an open tab");
    let preset = crate::canvas_layout::preset("time+flow").expect("registered");
    // As the base saved it: the tree's shares mirror the column split.
    let mut canvas =
        quantick_workspace::canvas::CanvasState::from_preset_split(preset, old.split_fraction);
    assert_eq!(canvas.maximize(1), Ok(true), "the flow chart maximized");
    (old.canvas, old.context_collapsed, old.flow_collapsed) = (Some(canvas), true, false);
    // Read back as the base wrote it.
    let old: quantick_workspace::workspace_recipe::WorkspaceRecipe =
        toml::from_str(&toml::to_string(&old).expect("a recipe writes")).expect("and reads");
    let book = app.runtime.workspace.book.book_mut();
    book.observe(first, &old).expect("held");
    book.save_as(first, "Folded max").expect("a free name");

    set_tab_workspace(&mut app, second, "Folded max").expect("a known workspace");
    frames(&mut app, 5);
    let tab = app.runtime.tabs.by_id(second).expect("an open tab");
    assert_eq!(tab.canvas.folds(), (true, false), "the column is folded");
    assert_eq!(tab.canvas.state.railed().len(), 1, "on one rail");
    assert_eq!(
        tab.canvas.state.maximized(),
        Some(1),
        "under the maximized flow"
    );
    assert_eq!(
        app.runtime.workspace.book.tab_row(second),
        ("Folded max".to_owned(), false),
        "and the tab reads clean"
    );
    for _ in 0..2 {
        frames(&mut app, 3);
        assert!(!app.runtime.workspace.book.tab_row(second).1, "still clean");
    }
}

/// The same old save with the flow chart floated or minimized instead: no
/// rail can stand beside it, so the fold flag reads as no fold, the tab
/// lands clean and stays clean, and the save reads back as itself.
#[test]
fn an_old_fold_flag_with_nowhere_to_rail_lands_clean() {
    use quantick_workspace::canvas::{CanvasRect, CanvasState};
    use quantick_workspace::workspace_recipe::WorkspaceRecipe;
    for (case, float) in [("floated", true), ("minimized", false)] {
        let file = format!("workspace-old-fold-{case}");
        let layouts = crate::scratch::ScratchFile::new(&file, "layouts.toml");
        let (mut app, first, second) = two_tabs(&layouts);
        let tab = app.runtime.tabs.by_id_mut(first).expect("an open tab");
        tab.set_layout(CanvasLayout::TimeAndFlow);
        frames(&mut app, 4);
        let mut old = live_recipe(&app.runtime, first).expect("an open tab");
        let preset = crate::canvas_layout::preset("time+flow").expect("registered");
        let mut canvas = CanvasState::from_preset_split(preset, old.split_fraction);
        let moved = if float {
            canvas.float(1, CanvasRect::new(100.0, 100.0, 400.0, 300.0))
        } else {
            canvas.minimize(1)
        };
        assert_eq!(moved, Ok(true), "{case}");
        (old.canvas, old.context_collapsed, old.flow_collapsed) = (Some(canvas), true, false);
        let read = |recipe: &WorkspaceRecipe| -> WorkspaceRecipe {
            toml::from_str(&toml::to_string(recipe).expect("a recipe writes")).expect("and reads")
        };
        let old = read(&old);
        assert!(!old.context_collapsed, "{case}: no rail, no fold");
        assert_eq!(read(&old), old, "{case}: a second read is the first");
        let name = format!("Folded {case}");
        let book = app.runtime.workspace.book.book_mut();
        book.observe(first, &old).expect("held");
        book.save_as(first, &name).expect("a free name");

        set_tab_workspace(&mut app, second, &name).expect("a known workspace");
        frames(&mut app, 5);
        let tab = app.runtime.tabs.by_id(second).expect("an open tab");
        assert_eq!(tab.canvas.folds(), (false, false), "{case}");
        let row = app.runtime.workspace.book.tab_row(second);
        assert_eq!(row, (name.clone(), false), "{case}: the tab reads clean");
        for _ in 0..2 {
            frames(&mut app, 3);
            assert!(
                !app.runtime.workspace.book.tab_row(second).1,
                "{case}: still clean"
            );
        }
    }
}

/// The window size the trader's desk runs at.
const FULL_HD: egui::Vec2 = egui::vec2(1920.0, 1000.0);

/// The cell's menu on the first frame it is shown.
struct Shown {
    /// The menu's rect and the capsule name's.
    menu: egui::Rect,
    name: egui::Rect,
    /// Each row in view by scene id, with the rect the scene publishes.
    rows: Vec<(String, egui::Rect)>,
    /// The frames before it was shown.
    unseen: usize,
}

impl Shown {
    fn row(&self, id: &str) -> Option<egui::Rect> {
        let mut rows = self.rows.iter();
        rows.find(|(row, _)| row == id).map(|(_, rect)| *rect)
    }

    fn listed(&self) -> Vec<egui::Rect> {
        let rows = self
            .rows
            .iter()
            .filter(|(id, _)| id.contains(".workspace."));
        rows.map(|(_, rect)| *rect).collect()
    }
}

/// Frames at full HD until the cell's menu is shown, as the scene reads it.
fn until_shown(app: &mut QuantickApp, ctx: &egui::Context) -> Shown {
    let tab = app.runtime.tabs.active_id();
    for unseen in 0..6 {
        run_frame_at(app, ctx, FULL_HD);
        assert!(app.chrome.window_menu.tab_workspace.chip_menu_on(tab));
        let drawn = &app.chrome.window_menu.cell_rows;
        let Some(((menu_row, menu), rows)) = drawn.split_first() else {
            continue;
        };
        assert_eq!(menu_row.scene_id(), "workspace_menu");
        let rows = rows.iter().map(|(row, rect)| (row.scene_id(), *rect));
        let name = (app.chrome.toolbar_controls.iter())
            .find(|control| control.id == "toolbar.capsule.workspace")
            .expect("the capsule's name")
            .rect;
        let rows = rows.collect();
        return Shown {
            menu: *menu,
            name,
            rows,
            unseen,
        };
    }
    panic!("the menu was never shown");
}

/// Open the cell's menu and run it to its first shown frame.
fn open_cell_menu(app: &mut QuantickApp, ctx: &egui::Context) -> Shown {
    let tab = app.runtime.tabs.active_id();
    app.chrome.window_menu.tab_workspace.toggle_chip_menu(tab);
    until_shown(app, ctx)
}

/// Workspaces `Desk 01`.. on the first tab until the book holds `count`.
fn fill_book(app: &mut QuantickApp, tab: u64, count: usize) {
    let book = app.runtime.workspace.book.book_mut();
    for n in 1.. {
        if book.list().len() >= count {
            break;
        }
        book.create(tab, &format!("Desk {n:02}"))
            .expect("a fresh name");
    }
}

/// The trader's complaint: the menu off the workspace name stays inside the
/// window, scrolls its list alone, opens where it hangs, and is as wide as
/// what it shows this time, not the widest thing it ever showed.
#[test]
fn the_cell_menu_stays_inside_the_window_and_narrows_when_its_rows_do() {
    use quantick_session::workspace_menu::{CHIP_MENU_MAX_LIST_ROWS, CHIP_MENU_MAX_WIDTH_PT};
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-bounded", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 39);
    let long = "W".repeat(quantick_workspace::workspace_book::MAX_WORKSPACE_NAME);
    let book = app.runtime.workspace.book.book_mut();
    book.create(first, &long).expect("a fresh name");
    let ctx = egui::Context::default();
    let shown = open_cell_menu(&mut app, &ctx);
    let menu = shown.menu;
    eprintln!("40 workspaces: {} x {} pt", menu.width(), menu.height());
    assert!(
        menu.width() <= CHIP_MENU_MAX_WIDTH_PT,
        "{} wide",
        menu.width()
    );
    assert!(
        (menu.right() - shown.name.right()).abs() < 1.0,
        "the first frame shown hangs from the capsule: {menu:?} under {:?}",
        shown.name
    );
    assert!(menu.bottom() <= FULL_HD.y, "bottom {}", menu.bottom());
    assert!(menu.top() >= shown.name.bottom(), "hangs below the capsule");
    for fixed in ["autosave", "save", "save_as", "rename", "new", "manage"] {
        let id = format!("workspace_menu.{fixed}");
        assert!(shown.row(&id).is_some(), "{id} never scrolls away");
    }
    let listed = shown.listed();
    assert!(
        listed.len() <= CHIP_MENU_MAX_LIST_ROWS + 1,
        "{}",
        listed.len()
    );
    let new = shown.row("workspace_menu.new").expect("New");
    for row in &listed {
        assert!(row.bottom() <= new.top(), "{row:?} over the footer {new:?}");
    }
    let own = format!("workspace_menu.workspace.{long}");
    assert!(
        shown.row(&own).is_some(),
        "the tab's own row is scrolled into view"
    );
    let wide = menu.width();

    app.chrome.window_menu.tab_workspace.close_chip_menu();
    run_frame_at(&mut app, &ctx, FULL_HD);
    let book = app.runtime.workspace.book.book_mut();
    let names: Vec<String> = book.list().into_iter().map(|row| row.name).collect();
    for name in names
        .iter()
        .filter(|name| name.starts_with("Desk") || **name == long)
    {
        book.delete(name).expect("a stored workspace");
    }
    let shown = open_cell_menu(&mut app, &ctx);
    assert!(
        shown.menu.width() < wide,
        "{} after {wide}",
        shown.menu.width()
    );
    let stored = app.runtime.workspace.book.book().list().len();
    assert_eq!(shown.listed().len(), stored, "every row in view");
}

/// A workspace made while the menu is open, with a name wider than any row,
/// is measured again: the menu widens to the cap and the row is listed.
#[test]
fn rows_that_change_while_the_menu_is_open_are_measured_again() {
    use quantick_session::workspace_menu::CHIP_MENU_MAX_WIDTH_PT;
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-remeasure", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    let ctx = egui::Context::default();
    let narrow = open_cell_menu(&mut app, &ctx).menu.width();
    let long = "W".repeat(quantick_workspace::workspace_book::MAX_WORKSPACE_NAME);
    let book = app.runtime.workspace.book.book_mut();
    book.create(first, &long).expect("a fresh name");
    let shown = until_shown(&mut app, &ctx);
    assert!(shown.unseen > 0, "the new words were measured unseen first");
    assert!(
        shown.menu.width() > narrow,
        "{} after {narrow}",
        shown.menu.width()
    );
    assert!(shown.menu.width() <= CHIP_MENU_MAX_WIDTH_PT);
    let own = format!("workspace_menu.workspace.{long}");
    assert!(shown.row(&own).is_some());
}

/// How many area states egui keeps in `ctx`'s memory.
fn stored_areas(ctx: &egui::Context) -> usize {
    let areas = ctx.memory(|memory| format!("{:?}", memory.areas()));
    areas.matches("AreaState").count()
}

/// Opening the menu again and again leaves egui no area state per opening.
#[test]
fn reopening_the_cell_menu_keeps_one_area_state_per_tab() {
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-reopen", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    let ctx = egui::Context::default();
    let cycle = |app: &mut QuantickApp, count: usize| {
        for _ in 0..count {
            open_cell_menu(app, &ctx);
            app.chrome.window_menu.tab_workspace.close_chip_menu();
            run_frame_at(app, &ctx, FULL_HD);
        }
    };
    cycle(&mut app, 2);
    let settled = stored_areas(&ctx);
    cycle(&mut app, 30);
    assert_eq!(stored_areas(&ctx), settled, "30 more openings");
}

/// The trader's nine workspaces fit whole below the capsule, no row clipped,
/// and the capsule names the call that reads what the menu lists.
#[test]
fn nine_workspaces_fit_below_the_capsule_and_the_name_names_its_call() {
    use quantick_session::workspace_menu::CHIP_MENU_MAX_WIDTH_PT;
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-nine", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 9);
    assert_eq!(app.runtime.workspace.book.book().list().len(), 9);
    let ctx = egui::Context::default();
    let shown = open_cell_menu(&mut app, &ctx);
    eprintln!(
        "9 workspaces: {} x {} pt",
        shown.menu.width(),
        shown.menu.height()
    );
    assert!(shown.menu.width() <= CHIP_MENU_MAX_WIDTH_PT);
    assert!(shown.menu.height() <= FULL_HD.y - shown.name.bottom());
    let listed = shown.listed();
    assert_eq!(listed.len(), 9);
    for row in &listed {
        assert_eq!(
            row.height(),
            listed[0].height(),
            "no row clipped: {listed:?}"
        );
    }
    let capsule = (app.chrome.toolbar_controls.iter())
        .find(|control| control.id == "toolbar.capsule.workspace")
        .expect("the capsule's name");
    let list = quantick_control_schema::workspace_book::WORKSPACE_LIST_CAPABILITY_ID;
    assert_eq!(capsule.capability, Some(list));
}

/// Thirty workspaces show twelve rows and scroll the rest.
#[test]
fn thirty_workspaces_show_twelve_rows_and_scroll() {
    use quantick_session::workspace_menu::CHIP_MENU_MAX_LIST_ROWS;
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-thirty", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 30);
    let ctx = egui::Context::default();
    let shown = open_cell_menu(&mut app, &ctx);
    eprintln!(
        "30 workspaces: {} x {} pt",
        shown.menu.width(),
        shown.menu.height()
    );
    let listed = shown.listed().len();
    assert!(
        (CHIP_MENU_MAX_LIST_ROWS - 1..=CHIP_MENU_MAX_LIST_ROWS + 1).contains(&listed),
        "{listed} rows in view"
    );
}

/// The scene lists the greyed Save unavailable, with the reason a client can
/// branch on, and the rows a press reaches as available.
#[test]
fn the_scene_lists_a_greyed_row_unavailable_with_its_reason() {
    use quantick_control_handlers::workspace_book::cell_menu::NO_EDITS_REASON;
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-scene", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    let ctx = egui::Context::default();
    open_cell_menu(&mut app, &ctx);
    let scene = crate::control::scene_snapshot(&app);
    let control = |id: &str| {
        let mut controls = scene.controls.iter();
        controls.find(|control| control.control_id == id).expect(id)
    };
    let save = control("toolbar.workspace_menu.save");
    assert!(!save.availability.available);
    assert_eq!(save.availability.reason.as_deref(), Some(NO_EDITS_REASON));
    assert!(save.bounds.is_some());
    let menu = control("toolbar.workspace_menu");
    assert!(menu.selected && menu.availability.available);
    assert!(control("toolbar.workspace_menu.new").availability.available);
}

/// Names painted whole or cut short, among the texts `output` painted that
/// start with `prefix`: (whole, cut).
fn painted_names(output: &egui::FullOutput, prefix: &str) -> (usize, usize) {
    fn walk(shape: &egui::Shape, prefix: &str, found: &mut (usize, usize)) {
        match shape {
            egui::Shape::Text(text) if text.galley.text().starts_with(prefix) => {
                if text.galley.elided {
                    found.1 += 1;
                } else {
                    found.0 += 1;
                }
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, prefix, found)),
            _ => {}
        }
    }
    let mut found = (0, 0);
    for clipped in &output.shapes {
        walk(&clipped.shape, prefix, &mut found);
    }
    found
}

/// Workspaces named `Flow Mini Index Morning Desk NN` until the book holds `count`.
fn fill_named(app: &mut QuantickApp, tab: u64, count: usize) {
    let book = app.runtime.workspace.book.book_mut();
    for n in 1.. {
        if book.list().len() >= count {
            break;
        }
        book.create(tab, &format!("Flow Mini Index Morning Desk {n:02}"))
            .expect("a fresh name");
    }
}

/// The scroll bar's gutter is counted in the width the menu wants, so names
/// that fit with nine workspaces still fit with thirty.
#[test]
fn names_that_fit_with_nine_workspaces_fit_with_thirty() {
    let mut widths = Vec::new();
    for count in [9, 30] {
        let layouts = crate::scratch::ScratchFile::new("workspace-menu-gutter", "layouts.toml");
        let (mut app, _, second) = two_tabs(&layouts);
        // Made on the other tab, so the list's rows, not Save, set the width.
        fill_named(&mut app, second, count);
        app.runtime.tabs.select(0);
        let ctx = egui::Context::default();
        let shown = open_cell_menu(&mut app, &ctx);
        let output = run_frame_sized(&mut app, &ctx, FULL_HD, Vec::new(), egui::Modifiers::NONE);
        let (whole, cut) = painted_names(&output, "Flow Mini Index");
        eprintln!(
            "{count} workspaces: {} x {} pt",
            shown.menu.width(),
            shown.menu.height()
        );
        assert!(whole > 0, "{count}: no name painted");
        assert_eq!(cut, 0, "{count}: {cut} names cut, {whole} whole");
        widths.push(shown.menu.width());
    }
    assert!(
        widths[0] > 215.0,
        "nine names wider than the floor: {widths:?}"
    );
    assert!(widths[1] > widths[0], "the gutter is counted: {widths:?}");
}

/// Open the `?` menu and its Workspace submenu at the test window's size:
/// the menu's rect, the submenu's and the last frame painted.
fn open_workspace_submenu(
    app: &mut QuantickApp,
    ctx: &egui::Context,
) -> (egui::Rect, egui::Rect, egui::FullOutput) {
    use quantick_control_schema::window_menu::KEBAB_CONTROL;
    run_frame(app, ctx);
    let kebab = app.chrome.title_bar.control_rect(KEBAB_CONTROL).unwrap();
    click_sized(app, ctx, TEST_WINDOW, kebab.center());
    let output = run_frame(app, ctx);
    let row = painted_text_center(&output, "Workspace").expect("the Workspace row");
    click_sized(app, ctx, TEST_WINDOW, row);
    run_frame(app, ctx);
    let output = run_frame(app, ctx);
    let pane = |level: usize| {
        let id = egui::Id::new(("window_menu_pane", level));
        ctx.memory(|memory| memory.area_rect(id)).expect("a pane")
    };
    (pane(0), pane(1), output)
}

/// The `?` menu's Workspace submenu keeps to the window with thirty
/// workspaces: twelve rows scroll, the rows after them stay in view.
#[test]
fn the_window_menu_workspace_submenu_stays_inside_the_window() {
    let layouts = crate::scratch::ScratchFile::new("workspace-submenu-bounded", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 30);
    let ctx = egui::Context::default();
    let (_, submenu, output) = open_workspace_submenu(&mut app, &ctx);
    eprintln!("window submenu, 30 workspaces: {submenu:?}");
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW);
    assert!(window.contains_rect(submenu), "{submenu:?}");
    for row in [
        "Manage workspaces\u{2026}",
        "Layouts",
        "Save as new\u{2026}",
    ] {
        let at = painted_text_center(&output, row).unwrap_or_else(|| panic!("{row}"));
        assert!(submenu.contains(at), "{row} at {at:?} out of {submenu:?}");
    }
    let listed = painted_text(&output)
        .iter()
        .filter(|text| text.starts_with("Desk "))
        .count();
    assert!(listed <= 13, "{listed} workspace rows painted");
}

/// Where the Workspace submenu meets the `?` menu, the menu is on top, as
/// it was before the rows were shared, and nine workspaces list whole.
#[test]
fn the_window_menu_covers_its_workspace_submenu_and_lists_nine_whole() {
    let layouts = crate::scratch::ScratchFile::new("workspace-submenu-order", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 9);
    let ctx = egui::Context::default();
    let (menu, submenu, output) = open_workspace_submenu(&mut app, &ctx);
    let overlap = menu.intersect(submenu);
    eprintln!("menu {menu:?}, submenu {submenu:?}, overlap {overlap:?}");
    if overlap.is_positive() {
        let top = ctx.layer_id_at(overlap.center()).expect("a layer");
        let menu_layer = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(("window_menu_pane", 0)),
        );
        assert_eq!(top, menu_layer, "the submenu draws over the menu");
    }
    let names = app.runtime.workspace.book.book().list();
    for row in names {
        assert!(
            painted_text_center(&output, &row.name).is_some(),
            "{}",
            row.name
        );
    }
}

/// The kinds each pane of `tab` shows, flow first, as the observer's
/// `analysis.indicators` scope reads them: how the rule is checked without a
/// mouse.
fn scope_kinds(app: &QuantickApp, tab: u64) -> Vec<Vec<String>> {
    let mut registry = crate::control::standard_registry().unwrap();
    let scope = observer_scope("analysis.indicators");
    let capture = registry
        .capture(app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    let tabs = capture.scopes[&scope].value["tabs"].clone();
    let tabs = tabs.as_array().expect("tabs");
    let entry = tabs
        .iter()
        .find(|entry| entry["tab_id"] == tab.to_string())
        .expect("the tab");
    let panes = entry["panes"].as_array().expect("panes").iter();
    panes
        .map(|pane| {
            let views = pane["indicators"].as_array().expect("indicators").iter();
            views
                .map(|view| view["kind"].as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .collect()
}

/// Frames until `done` holds, at most `limit` of them: the indicator workers
/// answer on threads of their own. Answers whether it held.
fn frames_until(
    app: &mut QuantickApp,
    limit: usize,
    mut done: impl FnMut(&QuantickApp) -> bool,
) -> bool {
    for _ in 0..limit {
        if done(app) {
            return true;
        }
        frames(app, 1);
        for pane in app.runtime.tabs.iter_mut().flat_map(|tab| tab.panes_mut()) {
            pane.indicator_worker.flush();
            pane.apply_indicator_events();
        }
    }
    done(app)
}

fn native(id: &str) -> quantick_workspace::indicator_document::SavedIndicator {
    quantick_workspace::indicator_document::SavedIndicator {
        kind: quantick_workspace::indicator_document::SavedKind::native(id),
        ..ema()
    }
}

/// Tab `a` holds an unsaved CVD on its layout; tab `b`, on a workspace of its
/// own with two kinds there, is put on `a`'s. Through real frames `b` shows
/// what `a` shows, as the `analysis.indicators` scope reads both, and the
/// stored recipe stays as saved: a landing is no edit.
#[test]
fn a_tab_put_on_a_held_workspace_shows_what_its_tab_shows() {
    let layouts = crate::scratch::ScratchFile::new("workspace-land-on-held", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let a = app.runtime.tabs.active_id();
    let b = if a == first { second } else { first };
    frames(&mut app, 2);
    observe_tab(&mut app, a);
    let book = app.runtime.workspace.book.book_mut();
    book.save_as(a, "Pair").expect("a free name");
    let stored = book.recipe("Pair").cloned();
    app.active_tab_mut().focus = PaneSide::Flow;
    app.apply_toolbar_action(ToolbarAction::AddNative("native.cvd"));
    let theirs = [ema(), native("native.volume")];
    app.layout_adapter()
        .show_pane_indicators(b, PaneSide::Flow, &theirs);
    let cvd = "native.cvd".to_owned();
    let shown = frames_until(&mut app, 50, |app| {
        scope_kinds(app, a)[0].contains(&cvd) && scope_kinds(app, b)[0].len() == 2
    });
    assert!(
        shown,
        "a {:?}, b {:?}",
        scope_kinds(&app, a),
        scope_kinds(&app, b)
    );
    assert_ne!(scope_kinds(&app, a)[0], scope_kinds(&app, b)[0]);

    set_tab_workspace(&mut app, b, "Pair").expect("a known workspace");
    let same = frames_until(&mut app, 50, |app| {
        scope_kinds(app, a)[0] == scope_kinds(app, b)[0]
    });
    assert!(
        same,
        "a {:?}, b {:?}",
        scope_kinds(&app, a),
        scope_kinds(&app, b)
    );
    let store = &app.runtime.workspace.book;
    assert_eq!(store.book().recipe("Pair").cloned(), stored, "as saved");
    assert!(store.tab_row(a).1, "the held edit is a's");
    assert!(!store.tab_row(b).1, "the landing is no edit");
}

/// The defect the report's reproduction found: a save from the second tab of
/// a workspace wrote its own set over the first tab's. Both tabs show the
/// first tab's add, so the second tab's save writes it.
#[test]
fn a_save_from_the_second_tab_keeps_the_first_tabs_add() {
    let layouts = crate::scratch::ScratchFile::new("workspace-second-save", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    let a = app.runtime.tabs.active_id();
    let b = if a == first { second } else { first };
    frames(&mut app, 2);
    observe_tab(&mut app, a);
    app.runtime
        .workspace
        .book
        .book_mut()
        .save_as(a, "Pair")
        .expect("a free name");
    set_tab_workspace(&mut app, b, "Pair").expect("a known workspace");
    frames(&mut app, 3);
    app.active_tab_mut().focus = PaneSide::Flow;
    app.apply_toolbar_action(ToolbarAction::AddNative("native.ema"));
    let ema = "native.ema".to_owned();
    let same = frames_until(&mut app, 50, |app| {
        let shows = |tab| scope_kinds(app, tab)[0].contains(&ema);
        shows(a) && shows(b)
    });
    assert!(
        same,
        "a {:?}, b {:?}",
        scope_kinds(&app, a),
        scope_kinds(&app, b)
    );
    let saved = save_tab_to(&mut app, b).expect("saved");
    assert!(saved.changed);
    let recipe = app.runtime.workspace.book.book().recipe("Pair").cloned();
    let kinds: Vec<_> = (recipe.expect("saved").flow_indicators.iter())
        .map(|entry| entry.kind.stable_name().to_owned())
        .collect();
    assert_eq!(kinds, scope_kinds(&app, a)[0], "what both tabs show");
    assert!(!app.runtime.workspace.book.tab_row(a).1);
}
