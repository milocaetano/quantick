//! The per-tab workspace at the window: one state behind chip and snapshot, an
//! edit that stays on its tab until Save to, and a first run no one can see.
use super::*;
use crate::app::tab_workspaces::live_recipe;
use quantick_control_handlers::workspace_book::{save_tab_to, set_tab_workspace};

/// A desk of two tabs opening together, seeded on the scratch book beside
/// `layouts`.
pub(super) fn two_tabs(layouts: &crate::scratch::ScratchFile) -> (QuantickApp, u64, u64) {
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
    quantick_workspace::book_upkeep::observe_tab(&mut app.workspace_calls(), tab);
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
    set_tab_workspace(&mut app.workspace_calls(), first, &own).expect("its own workspace");

    set_tab_workspace(&mut app.workspace_calls(), second, "Two").expect("a known workspace");
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
/// reaches another on the same workspace only by Save to. So do indicators on
/// a strip layout the other tab does not show; on one it shows they are the
/// workspace's: see `an_indicator_added_on_a_flow_pane_reaches_every_pane…`.
#[test]
fn footprint_style_and_layers_stay_on_their_tab_until_save_to() {
    let layouts = crate::scratch::ScratchFile::new("workspace-no-leak-all", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    for tab in [first, second] {
        set_tab_workspace(&mut app.workspace_calls(), tab, "Default").expect("always there");
    }
    frames(&mut app, 3);
    let edited = app.runtime.tabs.active_id();
    let other = if edited == first { second } else { first };
    let session = app.runtime.workspace.layouts_mut().session_mut();
    let apart = session.create(Some("Apart")).expect("a free name");
    app.layout_adapter()
        .switch_pane_layout(other, PaneSide::Flow, apart)
        .expect("a known layout");
    frames(&mut app, 2);
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

    let saved = save_tab_to(&mut app.workspace_calls(), edited).expect("saved");
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
    assert!(app.runtime.layout_state().layouts().is_private(theirs));
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

fn market(app: &QuantickApp) -> crate::app::title_bar::Market<'_> {
    (&app.runtime, &app.config)
}

/// Two tabs on two workspaces, whatever market they show: each chip carries
/// its own workspace's colour as the resolver gives it for the theme, and
/// the venue's code from config.
#[test]
fn two_workspaces_on_one_market_wear_two_colours() {
    let layouts = crate::scratch::ScratchFile::new("workspace-two-colours", "layouts.toml");
    let (mut app, first, _second) = two_tabs(&layouts);
    let book = app.runtime.workspace.book.book_mut();
    book.save_as(first, "Scalp").unwrap();
    let dark = quantick_appearance::ResolvedTheme::default();
    let chips = crate::app::title_bar::chips_of(market(&app), &[0, 1], &dark);
    assert_eq!(chips[0].workspace, "Scalp");
    assert_ne!(chips[1].workspace, "Scalp");
    let book = app.runtime.workspace.book.book();
    let resolved = |tab| book.tab_colour(tab).map(|colour| colour.rgb(true));
    assert_eq!(chips[0].symbol_ink, resolved(first));
    assert!(chips[1].symbol_ink.is_some());
    assert_ne!(chips[0].symbol_ink, chips[1].symbol_ink);
    assert_eq!(
        chips[0].venue_code,
        app.config.feed_code(&app.runtime.tabs[0].feed_id)
    );
}

/// The chip names the market the status bar names, and no workspace or mark:
/// the cell's mark is the snapshot's, with autosave off and an edit held.
#[test]
fn chip_and_snapshot_read_one_state() {
    let layouts = crate::scratch::ScratchFile::new("workspace-one-state", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    app.runtime.workspace.book.book_mut().set_autosave(false);
    edit_split(&mut app, first, 0.31);
    let theme = quantick_appearance::ResolvedTheme::default();
    let labels: Vec<String> = crate::app::title_bar::chips_of(market(&app), &[0, 1], &theme)
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
        let capsule = crate::app::chrome::capsule::snapshot(&app, &egui::Context::default());
        assert_eq!(capsule.marks().1, app.runtime.workspace.book.tab_row(tab).1);
    }
    assert!(
        app.runtime.workspace.book.tab_row(first).1
            && !app.runtime.workspace.book.tab_row(second).1
    );
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
        !crate::app::chrome::capsule::snapshot(&app, &egui::Context::default())
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
    app.workspace_calls().apply_workspace_command(read_first);
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
    set_tab_workspace(&mut app.workspace_calls(), second, &shared).expect("a known workspace");
    frames(&mut app, 1);
    let before = live_recipe(&app.runtime, second).expect("an open tab");

    edit_split(&mut app, first, 0.31);
    observe_tab(&mut app, second);
    assert!(app.runtime.workspace.book.tab_row(first).1);
    assert!(!app.runtime.workspace.book.tab_row(second).1);
    assert_eq!(live_recipe(&app.runtime, second).as_ref(), Some(&before));

    let saved = save_tab_to(&mut app.workspace_calls(), first).expect("saved");
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
    let book = app.runtime.layout_state().layouts();
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
    let strip = app.runtime.layout_state().layouts().strip_layout(focused);
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
    let strip = app
        .runtime
        .layout_state()
        .layouts()
        .active()
        .indicators
        .clone();
    set_tab_workspace(&mut app.workspace_calls(), first, "Default").expect("always there");
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
    set_tab_workspace(&mut app.workspace_calls(), b, "Pair").expect("a known workspace");
    frames(&mut app, 5);
    assert_eq!(
        app.runtime.workspace.book.tab_row(b),
        ("Pair".to_owned(), false)
    );
    let strip = |app: &QuantickApp, tab, side| app.runtime.layout_state().pane_layout(tab, side);
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
    // The mirrored add reaches tab `b`'s workers at once, but a view appears
    // only when its worker answers: wait for `b`'s workers too, not frames.
    let other_tab = app.runtime.tabs.by_id_mut(b).expect("an open tab");
    for pane in other_tab.panes_mut() {
        pane.indicator_worker.flush();
        pane.apply_indicator_events();
    }
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
    set_tab_workspace(&mut app.workspace_calls(), b, &own).expect("its own workspace");
    app.runtime.workspace.book.book_mut().set_autosave(true);

    // Edits made while autosave was off are behind us: what is due from here
    // on is the landing's alone.
    let quiet = std::time::Instant::now() + quantick_workspace::workspace_sync::AUTOSAVE_QUIET;
    app.runtime.workspace.book.sync_mut().take_due(quiet);
    set_tab_workspace(&mut app.workspace_calls(), a, "Three").expect("a known workspace");
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
        .runtime
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
    let mut book = app.runtime.layout_state().layouts().clone();
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
    let book = app.runtime.layout_state().layouts();
    assert_eq!(book.get(copies[1]).expect("held").indicators, fullest);
    assert!(book.get(copies[0]).is_none() && book.get(copies[2]).is_none());

    let saved = crate::app::arrangement_adapter::saved_tab_of(app.active_tab());
    assert_eq!(
        saved.context_layouts, [copies[1].0; 2],
        "the desk names one copy"
    );
    let text = toml::to_string(app.runtime.layout_state().layouts()).expect("a book writes");
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
    set_tab_workspace(&mut app.workspace_calls(), b, "W").expect("a known workspace");
    frames(&mut app, 5);
    let strip = |app: &QuantickApp, tab, side| app.runtime.layout_state().pane_layout(tab, side);
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
    let strip = app.runtime.layout_state().layouts().active().clone();
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "THIRDUSDT".to_owned(), None);
    // As a desk that names no layout opens it.
    app.active_tab_mut().flow_pane.request_opening_layout(None);
    app.active_tab_mut().focus = PaneSide::Flow;
    switch_by_script(&mut app, serde_json::json!({ "name": strip.name }));
    app.apply_toolbar_action(ToolbarAction::AddNative("native.ema"));

    let held = app.active_tab().flow_pane.layout_id().expect("a layout");
    let book = app.runtime.layout_state().layouts();
    assert!(book.is_private(held), "the tab's own copy");
    let added = book.get(held).expect("held").indicators.len();
    assert_eq!(added, strip.indicators.len() + 1, "the add landed there");
    let unchanged = |app: &QuantickApp| {
        let book = app.runtime.layout_state().layouts();
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
    let strip = app.runtime.layout_state().focused_pane_layout();
    let book = app.runtime.layout_state().layouts();
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
    let menu =
        |app: &mut QuantickApp, command| app.workspace_calls().apply_workspace_command(command);

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
    app.chrome.window_menu.tab_workspace.open_manage(tab);
    let manage = |intent| WorkspaceCommand::Manage(intent);
    use quantick_session::manage_workspaces::ManageIntent::{Arm, Delete};
    menu(&mut app, manage(Arm("Scalp".to_owned())));
    menu(&mut app, manage(Delete { now_ms: 0 }));
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
        let capsule = crate::app::chrome::capsule::snapshot(&app, &egui::Context::default());
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

    let menu =
        |app: &mut QuantickApp, command| app.workspace_calls().apply_workspace_command(command);
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
    save_tab_to(&mut app.workspace_calls(), tab).expect("the canvas saved first");
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
    save_tab_to(&mut app.workspace_calls(), tab).expect("saved");
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
    save_tab_to(&mut app.workspace_calls(), tab).expect("saved");
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
        let rule = |pane: &crate::pane::ChartPane| pane.series.state.spec().to_config_string();
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
    set_tab_workspace(&mut app.workspace_calls(), tab, "A").expect("a known workspace");
    frames(&mut app, 4);
    assert_eq!(shown(&app), a, "A reopens on its own timeframes");
    let state = app.runtime.workspace.book.book().tab_state(tab);
    assert!(!state.expect("adopted").modified, "and reads clean");
    set_tab_workspace(&mut app.workspace_calls(), tab, "B").expect("a known workspace");
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
        .series
        .state
        .spec()
        .to_config_string();
    let market = (
        app.active_tab().feed_id.clone(),
        app.active_tab().symbol.clone(),
    );
    let snapshot = MarketPaletteOwners::listed_markets(&app);
    let choice = snapshot.choice((&market.0, &market.1), None, None, false);
    let choice = choice.expect("the market is listed");
    assert_eq!(choice.context_bars, std::slice::from_ref(&shown));
    let _ = app.market_tabs().unwrap().open_tab(&choice, None);
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
    assert_eq!(opened.series.state.spec().to_config_string(), shown);
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

    set_tab_workspace(&mut app.workspace_calls(), second, "Folded").expect("a known workspace");
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

    set_tab_workspace(&mut app.workspace_calls(), second, "Folded max").expect("a known workspace");
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

        set_tab_workspace(&mut app.workspace_calls(), second, &name).expect("a known workspace");
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
    for _ in 0..6 {
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

/// The model's height for the menu `shown` hangs, with `workspaces` rows.
fn model_height(shown: &Shown, workspaces: usize) -> f32 {
    use quantick_session::workspace_menu::{MenuBounds, chip_menu_layout};
    let at = |r: egui::Rect| MenuBounds::new(r.left(), r.top(), r.right(), r.bottom());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, FULL_HD);
    chip_menu_layout(at(screen), at(shown.name), workspaces, workspaces).height
}

/// Designer B at the trader's nine workspaces, in Dark and in Light: 320
/// wide, no taller than the model, 4 below the capsule's name with its right
/// edge on the name's, every row in view.
#[test]
fn the_cell_menu_is_designer_b_s_size_in_dark_and_light() {
    use quantick_session::workspace_menu::{CHIP_MENU_DROP_PT, CHIP_MENU_WIDTH_PT};
    for theme in [
        quantick_appearance::Builtin::Dark,
        quantick_appearance::Builtin::Light,
    ] {
        let layouts = crate::scratch::ScratchFile::new("workspace-menu-b-size", "layouts.toml");
        let (mut app, first, _) = two_tabs(&layouts);
        app.runtime.appearance.apply_builtin(theme, false).unwrap();
        app.runtime.tabs.select(0);
        fill_book(&mut app, first, 9);
        let ctx = egui::Context::default();
        let shown = open_cell_menu(&mut app, &ctx);
        let (menu, name) = (shown.menu, shown.name);
        eprintln!("{theme:?}, 9 workspaces: {menu:?} under {name:?}");
        assert!(
            (menu.width() - CHIP_MENU_WIDTH_PT).abs() < 0.5,
            "{theme:?}: {} wide",
            menu.width()
        );
        let most = model_height(&shown, 9);
        assert_eq!(most, 482.0);
        assert!(
            (menu.height() - most).abs() < 0.5,
            "{theme:?}: {} tall, the model says {most}",
            menu.height()
        );
        assert_eq!(menu.min, menu.min.round(), "{theme:?}: on whole pixels");
        assert!((menu.right() - name.right()).abs() < 1.0, "{theme:?}");
        assert!((menu.top() - name.bottom() - CHIP_MENU_DROP_PT).abs() < 1.0);
        assert_eq!(shown.listed().len(), 9, "{theme:?}: every row in view");
    }
}

/// Thirty workspaces: the list scrolls under a fixed header and footer, the
/// filter stands over it, the tab's own row is in view on open, and the
/// scene reads the header, the rows with their summaries, the filter and
/// the footer.
#[test]
fn the_scene_reads_the_header_rows_filter_and_footer() {
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-b-scene", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 30);
    let ctx = egui::Context::default();
    let shown = open_cell_menu(&mut app, &ctx);
    let (painted, most) = (shown.menu.height(), model_height(&shown, 30));
    assert_eq!(most, 523.0);
    assert!(
        (painted - most).abs() < 0.5,
        "painted {painted}, the model {most}"
    );
    let field = shown.row("workspace_menu.filter").expect("the filter");
    for row in shown.listed() {
        assert!(row.top() >= field.bottom() + 4.0, "{row:?} under {field:?}");
        assert_eq!(row.min.y, row.min.y.round(), "{row:?} on a whole pixel");
    }
    let top_row = shown.listed()[0];
    assert_eq!(
        (field.left(), field.right()),
        (top_row.left(), top_row.right()),
        "one extent"
    );
    let scene = crate::control::scene_snapshot(&app);
    let control = |id: &str| {
        let mut controls = scene.controls.iter();
        controls.find(|control| control.control_id == id)
    };
    for part in [
        "",
        ".current",
        ".save",
        ".save_as",
        ".rename",
        ".filter",
        ".autosave",
        ".new",
        ".manage",
    ] {
        let id = format!("toolbar.workspace_menu{part}");
        let found = control(&id).unwrap_or_else(|| panic!("{id} in the scene"));
        assert!(found.bounds.is_some(), "{id} drawn");
    }
    let current = control("toolbar.workspace_menu.current").unwrap();
    assert!(
        current
            .description
            .as_deref()
            .is_some_and(|said| said.contains("tab"))
    );
    let rows: Vec<_> = (scene.controls.iter())
        .filter(|control| {
            control
                .control_id
                .starts_with("toolbar.workspace_menu.workspace.")
        })
        .collect();
    assert!(
        (9..=11).contains(&rows.len()),
        "{} rows in view",
        rows.len()
    );
    assert!(
        rows.iter().all(|row| row.description.is_some()),
        "summaries"
    );
    let (own, _) = app.runtime.workspace.book.tab_row(first);
    let lit = rows
        .iter()
        .find(|row| row.selected)
        .expect("the tab's own row in view");
    assert_eq!(lit.label, own);
    let footer = shown.row("workspace_menu.new").expect("New");
    for row in shown.listed() {
        assert!(
            row.bottom() <= footer.top(),
            "{row:?} over the footer {footer:?}"
        );
    }
}

/// Every name that fits the row paints whole over its summary, at nine
/// workspaces and at thirty.
#[test]
fn names_that_fit_paint_whole_over_their_summaries() {
    for count in [9, 30] {
        let layouts = crate::scratch::ScratchFile::new("workspace-menu-b-names", "layouts.toml");
        let (mut app, _, second) = two_tabs(&layouts);
        fill_named(&mut app, second, count);
        app.runtime.tabs.select(0);
        let ctx = egui::Context::default();
        let shown = open_cell_menu(&mut app, &ctx);
        let output = run_frame_sized(&mut app, &ctx, FULL_HD, Vec::new(), egui::Modifiers::NONE);
        let scene = crate::control::scene_snapshot(&app);
        let rows = (scene.controls.iter()).filter(|control| {
            control
                .control_id
                .starts_with("toolbar.workspace_menu.workspace.")
        });
        let mut checked = 0;
        for row in rows {
            let painted = painted_rows(&output, &row.label);
            let whole = painted
                .iter()
                .any(|(text, cut, _)| *text == row.label && !cut);
            assert!(whole, "{count}: {} whole: {painted:?}", row.label);
            // The scene ends the summary with the colour's words, which the
            // row paints as its slot, not as text.
            let said = row.description.clone().expect("a summary");
            let summary = said
                .rsplit_once(" \u{b7} ")
                .map_or(said.as_str(), |(s, _)| s);
            let under = painted_rows(&output, summary);
            assert!(!under.is_empty(), "{count}: {summary} painted");
            checked += 1;
        }
        eprintln!("{count} workspaces: {checked} rows in {:?}", shown.menu);
        assert!(checked >= 9, "{count}: {checked} rows checked");
    }
}

/// A click on a row puts the tab on that workspace and closes the menu; a
/// click on the tab's own row only closes it.
#[test]
fn a_click_on_a_row_switches_the_tab_and_closes_the_menu() {
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-b-click", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 9);
    let ctx = egui::Context::default();
    let shown = open_cell_menu(&mut app, &ctx);
    let default = shown
        .row("workspace_menu.workspace.Default")
        .expect("Default");
    click_sized(&mut app, &ctx, FULL_HD, default.center());
    run_frame_at(&mut app, &ctx, FULL_HD);
    let store = &app.runtime.workspace.book;
    assert_eq!(store.tab_row(first).0, "Default");
    let menu = &app.chrome.window_menu.tab_workspace;
    assert!(!menu.chip_menu_on(first), "the menu closed");
    let shown = open_cell_menu(&mut app, &ctx);
    let own = shown
        .row("workspace_menu.workspace.Default")
        .expect("its own");
    click_sized(&mut app, &ctx, FULL_HD, own.center());
    run_frame_at(&mut app, &ctx, FULL_HD);
    assert_eq!(app.runtime.workspace.book.tab_row(first).0, "Default");
    assert!(!app.chrome.window_menu.tab_workspace.chip_menu_on(first));
}

/// Down then Enter in the open menu switches to the row after the tab's own.
#[test]
fn down_and_enter_switch_to_the_next_workspace() {
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-b-keys", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 9);
    set_tab_workspace(&mut app.workspace_calls(), first, "Default").expect("a known workspace");
    let ctx = egui::Context::default();
    open_cell_menu(&mut app, &ctx);
    let next = app.runtime.workspace.book.book().list()[1].name.clone();
    for key in [egui::Key::ArrowDown, egui::Key::Enter] {
        let events = vec![key_press(key)];
        run_frame_sized(&mut app, &ctx, FULL_HD, events, egui::Modifiers::NONE);
    }
    run_frame_at(&mut app, &ctx, FULL_HD);
    assert_eq!(app.runtime.workspace.book.tab_row(first).0, next);
    assert!(!app.chrome.window_menu.tab_workspace.chip_menu_on(first));
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

/// Texts `output` painted that start with `prefix`, each with whether it
/// was cut short and where it was painted.
fn painted_rows(output: &egui::FullOutput, prefix: &str) -> Vec<(String, bool, egui::Rect)> {
    type Found = Vec<(String, bool, egui::Rect)>;
    fn walk(shape: &egui::Shape, prefix: &str, found: &mut Found) {
        match shape {
            egui::Shape::Text(text) if text.galley.text().starts_with(prefix) => {
                let at = text.visual_bounding_rect();
                found.push((text.galley.text().to_owned(), text.galley.elided, at));
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, prefix, found)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, prefix, &mut found);
    }
    found
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
    let id = tab.to_string();
    let entry = tabs
        .iter()
        .find(|entry| entry["tab_id"] == id.as_str())
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
/// what `a` shows, as the `analysis.indicators` scope reads both, the stored
/// recipe stays as saved, and the held edit, the workspace's, marks both.
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

    set_tab_workspace(&mut app.workspace_calls(), b, "Pair").expect("a known workspace");
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
    assert!(store.tab_row(b).1, "and the workspace's, held on b too");
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
    set_tab_workspace(&mut app.workspace_calls(), b, "Pair").expect("a known workspace");
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
    let saved = save_tab_to(&mut app.workspace_calls(), b).expect("saved");
    assert!(saved.changed);
    let recipe = app.runtime.workspace.book.book().recipe("Pair").cloned();
    let kinds: Vec<_> = (recipe.expect("saved").flow_indicators.iter())
        .map(|entry| entry.kind.stable_name().to_owned())
        .collect();
    assert_eq!(kinds, scope_kinds(&app, a)[0], "what both tabs show");
    assert!(!app.runtime.workspace.book.tab_row(a).1);
}

/// A tab on a workspace other than Default, landing on it the frame its menu
/// opens, keeps the menu open: by the toggle and by a click on the capsule's
/// name, on either tab of the strip.
#[test]
fn the_cell_menu_stays_open_on_a_tab_on_another_workspace() {
    for index in [0, 1] {
        let layouts = crate::scratch::ScratchFile::new("workspace-menu-b-stays", "layouts.toml");
        let (mut app, first, _) = two_tabs(&layouts);
        app.runtime.tabs.select(0);
        fill_book(&mut app, first, 4);
        app.runtime.tabs.select(index);
        let tab = app.runtime.tabs.active_id();
        let name = app
            .runtime
            .workspace
            .book
            .book()
            .list()
            .last()
            .unwrap()
            .name
            .clone();
        assert_ne!(name, "Default");
        set_tab_workspace(&mut app.workspace_calls(), tab, &name).expect("a known workspace");
        app.chrome.window_menu.tab_workspace.toggle_chip_menu(tab);
        let ctx = egui::Context::default();
        for frame in 0..12 {
            run_frame_at(&mut app, &ctx, FULL_HD);
            let on = app.chrome.window_menu.tab_workspace.chip_menu_on(tab);
            assert!(on, "tab {index}: closed on frame {frame}");
        }
        assert_eq!(app.runtime.workspace.book.tab_row(tab).0, name);
        app.chrome.window_menu.tab_workspace.toggle_chip_menu(tab);
        run_frame_at(&mut app, &ctx, FULL_HD);
        let name = (app.chrome.toolbar_controls.iter())
            .find(|control| control.id == "toolbar.capsule.workspace")
            .expect("the capsule's name")
            .rect;
        click_sized(&mut app, &ctx, FULL_HD, name.center());
        for frame in 0..12 {
            run_frame_at(&mut app, &ctx, FULL_HD);
            let on = app.chrome.window_menu.tab_workspace.chip_menu_on(tab);
            assert!(on, "tab {index}: clicked open, closed on frame {frame}");
        }
    }
}

/// The panes' drawings on the active tab, all told.
fn drawings(app: &QuantickApp) -> usize {
    let panes = app.active_tab().panes();
    panes.map(|(pane, _)| pane.drawings.items().len()).sum()
}

/// A click outside the open menu closes it and reaches nothing under it:
/// the window's top layer there is the menu's shield, and an armed drawing
/// tool draws nothing.
#[test]
fn a_click_outside_closes_the_menu_and_goes_no_further() {
    use crate::toolrail::Tool;
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-b-outside", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 9);
    let ctx = egui::Context::default();
    open_cell_menu(&mut app, &ctx);
    let chart = egui::pos2(400.0, 500.0);
    let top = ctx.layer_id_at(chart).expect("a layer");
    assert_eq!(top.order, egui::Order::Middle, "the shield takes the click");
    let tool = Tool::Drawing(drawing_tool("horizontal-line"));
    app.surfaces.toolrail.arm(tool);
    click_sized(&mut app, &ctx, FULL_HD, chart);
    run_frame_at(&mut app, &ctx, FULL_HD);
    assert!(
        !app.chrome.window_menu.tab_workspace.chip_menu_on(first),
        "closed"
    );
    assert_eq!(drawings(&app), 0, "the click went no further");
}

/// Only the switch and its label toggle Autosave: a click on the row's
/// state words changes nothing and leaves the menu open.
#[test]
fn only_the_switch_toggles_autosave() {
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-b-switch", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 9);
    let ctx = egui::Context::default();
    let shown = open_cell_menu(&mut app, &ctx);
    let before = app.runtime.workspace.book.book().autosave();
    let switch = shown.row("workspace_menu.autosave").expect("the toggle");
    let words = egui::pos2(shown.menu.right() - 20.0, switch.center().y);
    assert!(
        !switch.contains(words),
        "the state words lie outside the toggle"
    );
    click_sized(&mut app, &ctx, FULL_HD, words);
    run_frame_at(&mut app, &ctx, FULL_HD);
    assert_eq!(app.runtime.workspace.book.book().autosave(), before);
    assert!(
        app.chrome.window_menu.tab_workspace.chip_menu_on(first),
        "still open"
    );
    click_sized(&mut app, &ctx, FULL_HD, switch.center());
    run_frame_at(&mut app, &ctx, FULL_HD);
    assert_eq!(app.runtime.workspace.book.book().autosave(), !before);
}

/// Typed into the filter, Enter switches to the first match.
#[test]
fn enter_in_the_filter_switches_to_the_first_match() {
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-b-enter", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 14);
    let ctx = egui::Context::default();
    let shown = open_cell_menu(&mut app, &ctx);
    assert!(shown.row("workspace_menu.filter").is_some());
    let typed = vec![egui::Event::Text("desk 05".to_owned())];
    run_frame_sized(&mut app, &ctx, FULL_HD, typed, egui::Modifiers::NONE);
    assert_eq!(app.chrome.window_menu.tab_workspace.filter, "desk 05");
    let enter = vec![key_press(egui::Key::Enter)];
    run_frame_sized(&mut app, &ctx, FULL_HD, enter, egui::Modifiers::NONE);
    run_frame_at(&mut app, &ctx, FULL_HD);
    assert_eq!(app.runtime.workspace.book.tab_row(first).0, "Desk 05");
}

/// Typed into the filter and Enter in one frame: the keystroke narrows the
/// list before Enter reads it.
#[test]
fn typing_then_enter_in_one_frame_switches_to_the_match() {
    let layouts = crate::scratch::ScratchFile::new("workspace-menu-b-one-frame", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    fill_book(&mut app, first, 14);
    let ctx = egui::Context::default();
    open_cell_menu(&mut app, &ctx);
    let both = vec![
        egui::Event::Text("desk 05".to_owned()),
        key_press(egui::Key::Enter),
    ];
    run_frame_sized(&mut app, &ctx, FULL_HD, both, egui::Modifiers::NONE);
    run_frame_at(&mut app, &ctx, FULL_HD);
    assert_eq!(app.runtime.workspace.book.tab_row(first).0, "Desk 05");
}

/// The shapes `output` paints in `colour` inside `within`, one surface's
/// rectangle: rect fills (also while a menu fades in, its opacity aside),
/// rect edges and text.
fn painted_in(output: &egui::FullOutput, colour: egui::Color32, within: egui::Rect) -> [usize; 3] {
    fn walk(
        shape: &egui::Shape,
        colour: egui::Color32,
        within: egui::Rect,
        found: &mut [usize; 3],
    ) {
        // Premultiplied by the fade's alpha: each channel back over it.
        let near =
            |c: u8, want: u8, a: u8| (u32::from(c) * 255 / u32::from(a)).abs_diff(want.into()) <= 2;
        let faded = |fill: egui::Color32| {
            let a = fill.a();
            a > 0
                && near(fill.r(), colour.r(), a)
                && near(fill.g(), colour.g(), a)
                && near(fill.b(), colour.b(), a)
        };
        let inside = |at: egui::Rect| within.expand(1.0).contains_rect(at);
        match shape {
            egui::Shape::Rect(rect) if inside(rect.rect) => {
                found[0] += usize::from(faded(rect.fill));
                found[1] += usize::from(rect.stroke.color == colour);
            }
            egui::Shape::Text(text) if inside(text.visual_bounding_rect()) => {
                found[2] += usize::from(text.fallback_color == colour);
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, colour, within, found)),
            _ => {}
        }
    }
    let mut found = [0; 3];
    for clipped in &output.shapes {
        walk(&clipped.shape, colour, within, &mut found);
    }
    found
}

/// The rectangles of the open tab `tab` and of the capsule, as last drawn.
fn tab_and_capsule(app: &QuantickApp, tab: u64) -> (egui::Rect, egui::Rect) {
    let strip = &app.chrome.title_bar.strip.tab_ids;
    let open = strip
        .iter()
        .find(|(id, _)| *id == tab)
        .expect("the open tab")
        .1;
    let capsule = app.chrome.capsule;
    (open, capsule.miniature.union(capsule.name))
}

/// `colour` as the window paints it on `ctx`'s theme.
fn resolved(
    ctx: &egui::Context,
    colour: quantick_stores::workspaces::WorkspaceColour,
) -> egui::Color32 {
    let [r, g, b] = colour.rgb(crate::theme::get(ctx).resolved.dark);
    egui::Color32::from_rgb(r, g, b)
}

/// A custom colour set by `workspace.colour.set` reads alike on every
/// surface: the open tab's symbol, the capsule's border and bar, the menu's
/// row and header slots, the scene, `workspace.list` and `workspace.summary`;
/// a preset paints its resolved value for the theme on the tab, the capsule
/// and a row; the open menu shows no "Colour of" section.
#[test]
fn a_colour_set_by_call_reads_alike_on_every_surface() {
    use crate::control::ActionOrigin;
    use quantick_stores::workspaces::WorkspaceColour;
    use serde_json::json;
    let layouts = crate::scratch::ScratchFile::new("workspace-colour-surfaces", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    let call = |app: &mut QuantickApp, id: &str, input: serde_json::Value| {
        app.control_action(id, 1, ActionOrigin::Human, input)
            .unwrap_or_else(|error| panic!("{id}: {error:?}"))
    };
    call(&mut app, "workspace.save_as", json!({ "name": "Scalp" }));
    let wire = json!({ "custom": "#3A7BD5", "light": "#3A7BD5", "dark": "#3A7BD5" });
    let value = json!({ "name": "Scalp", "colour": "#3a7bd5" });
    let set = call(&mut app, "workspace.colour.set", value);
    assert_eq!(set["colour"], wire);
    let listed = call(&mut app, "workspace.list", json!({}));
    let rows = listed["workspaces"].as_array().unwrap();
    let scalp = rows.iter().find(|row| row["name"] == "Scalp").unwrap();
    assert_eq!(scalp["colour"], wire);

    let ctx = egui::Context::default();
    let shown = open_cell_menu(&mut app, &ctx);
    let output = run_frame_sized(&mut app, &ctx, FULL_HD, Vec::new(), egui::Modifiers::NONE);
    let custom = resolved(&ctx, WorkspaceColour::Custom([0x3A, 0x7B, 0xD5]));
    let (tab, capsule) = tab_and_capsule(&app, first);
    let in_tab = painted_in(&output, custom, tab);
    assert!(
        in_tab[2] >= 2,
        "the open tab's symbol, two strikes: {in_tab:?}"
    );
    let in_capsule = painted_in(&output, custom, capsule);
    assert!(in_capsule[1] >= 1, "the capsule's border: {in_capsule:?}");
    assert!(in_capsule[0] >= 1, "the capsule's bar: {in_capsule:?}");
    for (key, colour) in [
        ("workspace_menu.workspace.Scalp", custom),
        ("workspace_menu.current", custom),
        (
            "workspace_menu.workspace.Default",
            resolved(&ctx, WorkspaceColour::preset(4).unwrap()),
        ),
    ] {
        let row = shown.row(key).unwrap_or_else(|| panic!("{key}"));
        assert!(painted_in(&output, colour, row)[0] >= 1, "{key}'s slot");
    }
    assert!(
        painted_rows(&output, "COLOUR OF").is_empty(),
        "no colour section"
    );

    // A preset: the open tab and the capsule paint the theme's value.
    let sky = json!({ "name": "Scalp", "colour": "sky" });
    call(&mut app, "workspace.colour.set", sky);
    let output = run_frame_sized(&mut app, &ctx, FULL_HD, Vec::new(), egui::Modifiers::NONE);
    let sky = resolved(&ctx, WorkspaceColour::preset(2).unwrap());
    let (tab, capsule) = tab_and_capsule(&app, first);
    assert!(painted_in(&output, sky, tab)[2] >= 2, "Sky on the open tab");
    assert!(
        painted_in(&output, sky, capsule)[1] >= 1,
        "Sky on the capsule's border"
    );
    assert_eq!(
        painted_in(&output, custom, capsule),
        [0; 3],
        "the custom is gone"
    );
    let back = json!({ "name": "Scalp", "colour": "#3A7BD5" });
    call(&mut app, "workspace.colour.set", back);
    run_frame_sized(&mut app, &ctx, FULL_HD, Vec::new(), egui::Modifiers::NONE);

    let scene = crate::control::scene_snapshot(&app);
    let said = |id: &str| {
        let mut controls = scene.controls.iter();
        let control = controls.find(|control| control.control_id == id);
        control.and_then(|control| control.description.clone())
    };
    let ids = [
        "toolbar.capsule.workspace",
        "toolbar.workspace_menu.current",
    ];
    for id in ids
        .into_iter()
        .chain(["toolbar.workspace_menu.workspace.Scalp"])
    {
        let said = said(id).unwrap_or_else(|| panic!("{id}"));
        assert!(said.ends_with("Custom #3A7BD5"), "{id}: {said}");
    }

    let mut registry = crate::control::standard_registry().unwrap();
    let scope = observer_scope("workspace.summary");
    let capture = registry
        .capture(&app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    let summary = &capture.scopes[&scope].value;
    let tabs = summary["tabs"].as_array().unwrap();
    let tab = tabs
        .iter()
        .find(|tab| tab["tab_id"] == json!(first.to_string()));
    assert_eq!(tab.unwrap()["workspace_colour"], wire);
    let rows = summary["workspaces"].as_array().unwrap();
    assert!(
        rows.iter()
            .any(|row| row["name"] == "Scalp" && row["colour"] == wire)
    );
}

/// The fill painted last at `at`, clip included: what the eye sees there.
fn top_fill_at(output: &egui::FullOutput, at: egui::Pos2) -> Option<egui::Color32> {
    fn walk(
        shape: &egui::Shape,
        clip: egui::Rect,
        at: egui::Pos2,
        top: &mut Option<egui::Color32>,
    ) {
        match shape {
            egui::Shape::Rect(rect)
                if rect.fill.a() > 0 && rect.rect.contains(at) && clip.contains(at) =>
            {
                *top = Some(rect.fill);
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, clip, at, top)),
            _ => {}
        }
    }
    let mut top = None;
    for clipped in &output.shapes {
        walk(&clipped.shape, clipped.clip_rect, at, &mut top);
    }
    top
}

/// The capsule's 4 pt bar is the same at rest, under the pointer and with
/// its workspace menu open: the hover fill never paints over it.
#[test]
fn the_capsule_keeps_its_bar_at_rest_hovered_and_open() {
    use crate::control::ActionOrigin;
    use serde_json::json;
    let layouts = crate::scratch::ScratchFile::new("workspace-capsule-bar", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    for (id, input) in [
        ("workspace.save_as", json!({ "name": "Scalp" })),
        (
            "workspace.colour.set",
            json!({ "name": "Scalp", "colour": "#3A7BD5" }),
        ),
    ] {
        app.control_action(id, 1, ActionOrigin::Human, input)
            .unwrap();
    }
    let custom = egui::Color32::from_rgb(0x3A, 0x7B, 0xD5);
    let ctx = egui::Context::default();
    run_frame_at(&mut app, &ctx, FULL_HD);
    let (_, capsule) = tab_and_capsule(&app, first);
    let edge = egui::pos2(capsule.left() + 2.0, capsule.center().y);
    let hover = app.chrome.capsule.miniature.center();
    let none = egui::Modifiers::NONE;
    let output = run_frame_sized(&mut app, &ctx, FULL_HD, Vec::new(), none);
    assert_eq!(top_fill_at(&output, edge), Some(custom), "at rest");
    let moved = vec![egui::Event::PointerMoved(hover)];
    run_frame_sized(&mut app, &ctx, FULL_HD, moved, none);
    let output = run_frame_sized(&mut app, &ctx, FULL_HD, Vec::new(), none);
    assert_eq!(top_fill_at(&output, edge), Some(custom), "hovered");
    app.chrome.window_menu.tab_workspace.toggle_chip_menu(first);
    run_frame_sized(&mut app, &ctx, FULL_HD, Vec::new(), none);
    let output = run_frame_sized(&mut app, &ctx, FULL_HD, Vec::new(), none);
    assert!(app.chrome.window_menu.tab_workspace.chip_menu_on(first));
    assert_eq!(top_fill_at(&output, edge), Some(custom), "menu open");
}

/// Frames at full HD until the open menu's drawn row `id` shows; its rect.
fn drawn_row(app: &mut QuantickApp, ctx: &egui::Context, id: &str) -> egui::Rect {
    for _ in 0..6 {
        run_frame_at(app, ctx, FULL_HD);
        let rows = &app.chrome.window_menu.cell_rows;
        if let Some((_, rect)) = rows.iter().find(|(row, _)| row.scene_id() == id) {
            return *rect;
        }
    }
    panic!("{id} was never drawn");
}

/// Tab 1 on a new `Scalp` beside `Default` (Slate), its menu's
/// palette popover staged on `face`.
fn popover_on(face: &str) -> (QuantickApp, egui::Context, crate::scratch::ScratchFile) {
    let layouts = crate::scratch::ScratchFile::new("colour-popover", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    app.runtime
        .workspace
        .book
        .book_mut()
        .create(first, "Scalp")
        .unwrap();
    let staged = app.chrome.window_menu.tab_workspace.stage(first, face);
    assert!(staged, "{face}");
    (app, egui::Context::default(), layouts)
}

fn preset(index: u8) -> quantick_stores::workspaces::WorkspaceColour {
    quantick_stores::workspaces::WorkspaceColour::preset(index).unwrap()
}

/// A click on the header swatch opens the popover over the menu, and the
/// scene lists its swatches and Custom….
#[test]
fn the_header_swatch_opens_the_popover_and_the_scene_lists_it() {
    let (mut app, ctx, _layouts) = popover_on("cell");
    let swatch = drawn_row(&mut app, &ctx, "workspace_menu.colour");
    click_sized(&mut app, &ctx, FULL_HD, swatch.center());
    let menu = &app.chrome.window_menu.tab_workspace;
    assert!(
        menu.colour.is_some() && menu.chip_menu.is_some(),
        "the menu stays"
    );
    drawn_row(&mut app, &ctx, "workspace_menu.colour.slate");
    let scene = crate::control::scene_snapshot(&app);
    for key in ["cobalt", "slate", "blossom", "custom"] {
        let id = format!("toolbar.workspace_menu.colour.{key}");
        assert!(
            scene
                .controls
                .iter()
                .any(|control| control.control_id == id),
            "{id}"
        );
    }
}

/// Swap reaches the colour handler with swap: the colour moves and the
/// toast says which workspace changed.
#[test]
fn swap_reaches_the_colour_handler_and_the_toast_names_the_holder() {
    let (mut app, ctx, _layouts) = popover_on("cell:colour:ask=slate");
    let held = app.runtime.workspace.book.book().colour("Scalp").unwrap();
    let swap = drawn_row(&mut app, &ctx, "workspace_menu.colour.swap");
    click_sized(&mut app, &ctx, FULL_HD, swap.center());
    let book = app.runtime.workspace.book.book();
    assert_eq!(book.colour("Scalp"), Some(preset(4)));
    assert_eq!(book.colour("Default"), Some(held), "Scalp's old colour");
    let note = app.surfaces.toast.model_state.message();
    assert_eq!(
        note,
        Some(format!("Default is now {}", held.name()).as_str())
    );
}

/// Cancel on the question changes nothing.
#[test]
fn cancel_on_the_question_changes_nothing() {
    let (mut app, ctx, _layouts) = popover_on("cell:colour:ask=slate");
    let before = app.runtime.workspace.book.book().list();
    let cancel = drawn_row(&mut app, &ctx, "workspace_menu.colour.cancel");
    click_sized(&mut app, &ctx, FULL_HD, cancel.center());
    assert_eq!(app.runtime.workspace.book.book().list(), before);
    let popover = app.chrome.window_menu.tab_workspace.colour.as_ref();
    assert_eq!(
        popover.map(|popover| popover.asked.is_none()),
        Some(true),
        "back to the grid"
    );
}

/// Apply on a custom colour reaches the colour handler.
#[test]
fn apply_on_a_custom_colour_reaches_the_colour_handler() {
    let (mut app, ctx, _layouts) = popover_on("cell:colour:custom=#3A7BD5");
    let apply = drawn_row(&mut app, &ctx, "workspace_menu.colour.apply");
    click_sized(&mut app, &ctx, FULL_HD, apply.center());
    let custom = quantick_stores::workspaces::WorkspaceColour::Custom([0x3A, 0x7B, 0xD5]);
    assert_eq!(
        app.runtime.workspace.book.book().colour("Scalp"),
        Some(custom)
    );
    assert!(
        app.chrome.window_menu.tab_workspace.colour.is_none(),
        "it closes"
    );
}

/// The popover call reaches the window's own menu: it opens the menu and its
/// popover on the active tab, and Custom fills its field in the window's theme.
#[test]
fn the_popover_call_opens_the_window_s_menu_and_reads_its_theme() {
    use crate::control::ActionOrigin;
    use serde_json::json;
    let (mut app, _ctx, _layouts) = popover_on("cell");
    app.chrome.window_menu.tab_workspace = Default::default();
    let mut call = |input: serde_json::Value| {
        app.control_action("workspace.colour.popover", 1, ActionOrigin::Human, input)
            .unwrap_or_else(|error| panic!("{error:?}"))
    };
    let open = call(json!({ "act": "open" }));
    assert_eq!(
        (open["open"].clone(), open["face"].clone()),
        (json!(true), json!("grid"))
    );
    let custom = call(json!({ "act": "custom" }));
    let dark = crate::app::TabsPort::appearance(&app).dark;
    let scalp = app.runtime.workspace.book.book().colour("Scalp").unwrap();
    let rgb = quantick_stores::workspaces::hex(scalp.rgb(dark));
    assert_eq!(custom["hex"], json!(rgb));
    let menu = &app.chrome.window_menu.tab_workspace;
    assert!(menu.colour.as_ref().is_some_and(|popover| popover.custom));
}

/// #1057: a stacked chart folded on a saved workspace stays folded through a
/// switch away and back, in the model, the scene and the painted rail, and
/// the next capture still matches the save.
#[test]
fn a_folded_stacked_chart_survives_a_workspace_switch() {
    use quantick_control_handlers::layout::LayoutTab as _;
    let layouts = crate::scratch::ScratchFile::new("workspace-stacked-fold", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    let tab = app.runtime.tabs.by_id_mut(first).expect("an open tab");
    tab.set_layout(CanvasLayout::TimeTimeAndFlow);
    app.runtime.tabs.select(0);
    frames(&mut app, 4);
    let tab = app.runtime.tabs.by_id_mut(first).expect("an open tab");
    let pair = (tab.time_panes[0].id, tab.time_panes[1].id);
    let agent = quantick_control::wire::ActorKind::Agent;
    let folded = tab.resize_context_pair(first, pair, 1.0, agent);
    assert!(matches!(folded, Some(Ok((_, true)))), "{folded:?}");
    frames(&mut app, 2);
    let own = app.runtime.workspace.book.tab_row(first).0;
    let stacked = live_recipe(&app.runtime, first).expect("an open tab");
    assert_eq!(stacked.context_panes_collapsed, [false, true]);
    let book = app.runtime.workspace.book.book_mut();
    book.observe(first, &stacked).expect("held");
    book.save_as(first, "Stacked").expect("a free name");

    set_tab_workspace(&mut app.workspace_calls(), first, &own).expect("its own workspace");
    frames(&mut app, 3);
    set_tab_workspace(&mut app.workspace_calls(), first, "Stacked").expect("back");
    frames(&mut app, 3);
    let tab = app.runtime.tabs.by_id(first).expect("an open tab");
    let canvas = &tab.canvas;
    let slot =
        quantick_workspace::canvas::seat_slot(&canvas.state, &mut canvas.seats.clone(), Some(1))
            .expect("the lower chart");
    assert!(canvas.state.railed().contains(&slot), "the model");
    let rail = format!("pane_menu.{slot}.rail");
    assert!(
        canvas.menus.drawn.iter().any(|control| control.id == rail),
        "the scene"
    );
    let (_, bands) = canvas
        .context_stack_geometry(&tab.time_panes)
        .expect("the stack");
    let painted = bands.panes[1].height();
    assert!(
        (painted - crate::canvas_layout::COLLAPSED_PANE_WIDTH_PX).abs() < 0.5,
        "{painted}"
    );
    let live = live_recipe(&app.runtime, first).expect("an open tab");
    assert_eq!(live.context_panes_collapsed, [false, true], "the capture");
    assert_eq!(
        app.runtime.workspace.book.tab_row(first),
        ("Stacked".to_owned(), false),
        "the save still holds"
    );
}

/// The active tab's canvas as the trader sees it: the preset it draws, the
/// panes it holds, and what `canvas.layout.picker` reads.
fn canvas_read(app: &mut QuantickApp) -> (Option<&'static str>, usize, Option<String>) {
    use crate::control::ActionOrigin;
    let canvas = &app.active_tab().canvas.state;
    let (preset, panes) = (
        canvas.preset().map(|preset| preset.id),
        canvas.panes().len(),
    );
    let input = serde_json::json!({});
    let picker = app.control_action("canvas.layout.picker", 1, ActionOrigin::Human, input);
    let current = picker.expect("the picker reads")["current_id"]
        .as_str()
        .map(str::to_owned);
    (preset, panes, current)
}

/// A switch puts the other workspace's layout on the canvas, read as the
/// model, the picker and the panes: by the capsule menu's row, by the window
/// menu's Workspace submenu and by `tab.workspace.set`, each way.
#[test]
fn a_workspace_switch_paints_the_other_workspace_s_layout() {
    use crate::control::ActionOrigin;
    use serde_json::json;
    let layouts = crate::scratch::ScratchFile::new("workspace-switch-layout", "layouts.toml");
    let (mut app, first, _) = two_tabs(&layouts);
    app.runtime.tabs.select(0);
    let ctx = egui::Context::default();
    let call = |app: &mut QuantickApp, id: &str, input: serde_json::Value| {
        app.control_action(id, 1, ActionOrigin::Human, input)
            .unwrap_or_else(|error| panic!("{id}: {error:?}"))
    };
    for _ in 0..4 {
        run_frame_at(&mut app, &ctx, FULL_HD);
    }
    let default = canvas_read(&mut app);
    call(
        &mut app,
        "canvas.layout.pick",
        json!({ "preset_id": "charts-4" }),
    );
    for _ in 0..4 {
        run_frame_at(&mut app, &ctx, FULL_HD);
    }
    let quad = canvas_read(&mut app);
    assert_eq!(quad.0, Some("charts-4"));
    assert_ne!(quad, default, "the pick changed the canvas");
    call(&mut app, "workspace.save_as", json!({ "name": "Quad" }));
    run_frame_at(&mut app, &ctx, FULL_HD);
    assert_eq!(
        app.runtime.workspace.book.tab_row(first),
        ("Quad".to_owned(), false)
    );

    let shown = open_cell_menu(&mut app, &ctx);
    let row = shown
        .row("workspace_menu.workspace.Default")
        .expect("Default");
    click_sized(&mut app, &ctx, FULL_HD, row.center());
    for _ in 0..4 {
        run_frame_at(&mut app, &ctx, FULL_HD);
    }
    assert_eq!(app.runtime.workspace.book.tab_row(first).0, "Default");
    assert_eq!(
        canvas_read(&mut app),
        default,
        "the menu's row paints Default's layout"
    );

    let (_, submenu, output) = open_workspace_submenu(&mut app, &ctx);
    let row = painted_text_center(&output, "Quad").expect("Quad's row");
    assert!(submenu.contains(row), "Quad in the Workspace submenu");
    click_sized(&mut app, &ctx, TEST_WINDOW, row);
    for _ in 0..4 {
        run_frame_at(&mut app, &ctx, FULL_HD);
    }
    assert_eq!(app.runtime.workspace.book.tab_row(first).0, "Quad");
    assert_eq!(
        canvas_read(&mut app),
        quad,
        "the window menu paints Quad's layout"
    );
    call(&mut app, "tab.workspace.set", json!({ "name": "Default" }));
    for _ in 0..4 {
        run_frame_at(&mut app, &ctx, FULL_HD);
    }
    assert_eq!(canvas_read(&mut app), default, "the call paints Default's");
    call(&mut app, "tab.workspace.set", json!({ "name": "Quad" }));
    for _ in 0..4 {
        run_frame_at(&mut app, &ctx, FULL_HD);
    }
    assert_eq!(canvas_read(&mut app), quad, "the call paints Quad's layout");
}

/// The trader's book, cut down. A two-chart workspace puts its lower chart
/// on a range rule with a layer of its own. Landed on a one-chart tab, whose second chart is built
/// after the canvas, and on a tab still holding a second chart off screen,
/// each tab reads as the recipe; back on a one-chart workspace, so does the
/// tab with the second chart built. No landing reads as an edit.
#[test]
fn a_switch_lands_every_chart_s_rule_and_layers_and_reads_clean() {
    let layouts = crate::scratch::ScratchFile::new("workspace-switch-whole", "layouts.toml");
    let (mut app, first, second) = two_tabs(&layouts);
    frames(&mut app, 4);
    let (own, other) = (
        app.runtime.workspace.book.tab_row(first).0,
        app.runtime.workspace.book.tab_row(second).0,
    );
    let tab = app.runtime.tabs.by_id_mut(first).expect("an open tab");
    tab.set_layout(CanvasLayout::TimeTimeAndFlow);
    frames(&mut app, 4);
    // Read back as a save writes it: a stacked canvas whose slots a landing
    // numbers its own way, unlike the slots the pick above gave this tab.
    let mut two = live_recipe(&app.runtime, first).expect("an open tab");
    let lower = &mut two.context_panes[1];
    lower.bars = Some("renko:20".to_owned());
    let footprint = lower.layers.get("footprint").copied().expect("read");
    lower.layers.insert("footprint".to_owned(), !footprint);
    let book = app.runtime.workspace.book.book_mut();
    book.observe(first, &two).expect("held");
    book.save_as(first, "Two").expect("a free name");
    set_tab_workspace(&mut app.workspace_calls(), first, &own).expect("its own workspace");
    frames(&mut app, 4);
    let reads = |app: &QuantickApp, tab: u64, name: &str| {
        let live = live_recipe(&app.runtime, tab).expect("an open tab");
        let book = app.runtime.workspace.book.book();
        let recipe = book.recipe(name).expect("a saved workspace");
        assert_eq!(recipe.masked(&live), *recipe, "{name} on tab {tab}");
        let row = app.runtime.workspace.book.tab_row(tab);
        assert_eq!(row, (name.to_owned(), false), "no edit");
    };
    reads(&app, first, &own);

    for (tab, to) in [(second, "Two"), (first, "Two"), (second, other.as_str())] {
        set_tab_workspace(&mut app.workspace_calls(), tab, to).expect("a known workspace");
        frames(&mut app, 5);
        reads(&app, tab, to);
    }
}

/// Autosave off: a menu switch whose tab holds edits asks first, and Enter
/// (Save to) writes the workspace to disk before the tab moves.
#[test]
fn a_menu_switch_with_held_edits_asks_and_enter_saves_then_switches() {
    use quantick_session::workspace_menu::WorkspaceCommand;
    let layouts = crate::scratch::ScratchFile::new("workspace-switch-asks", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    let tab = app.runtime.tabs.active_id();
    let workspace = app.runtime.workspace.book.tab_row(tab).0;
    edit_split(&mut app, tab, 0.31);
    let set = WorkspaceCommand::Set("Default".to_owned());
    app.workspace_calls().apply_workspace_command(set);
    let row = app.runtime.workspace.book.tab_row(tab);
    assert_eq!(
        row,
        (workspace.clone(), true),
        "nothing moves before the answer"
    );
    let menu = &app.chrome.window_menu.tab_workspace;
    let prompt = quantick_control_handlers::workspace_book::menu_prompt(menu).expect("asks");
    assert_eq!(prompt.buttons[prompt.enter], format!("Save to {workspace}"));

    let ctx = egui::Context::default();
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Enter)]);
    assert_eq!(app.runtime.workspace.book.tab_row(tab).0, "Default");
    let path = quantick_stores::workspaces::path_beside(layouts.as_ref());
    let (on_disk, _) = quantick_stores::workspaces::load(&path);
    let saved = on_disk.recipe(&workspace).expect("the workspace on disk");
    assert_eq!(saved.split_fraction, Some(0.31));
}

/// The switch prompt's Esc only cancels the switch, as the close prompt's
/// does: the chart under it keeps its selected drawing.
#[test]
fn the_switch_prompt_s_esc_leaves_the_chart_s_selection_alone() {
    use quantick_session::workspace_menu::WorkspaceCommand;
    let layouts = crate::scratch::ScratchFile::new("workspace-switch-esc", "layouts.toml");
    let (mut app, _, _) = two_tabs(&layouts);
    let tab = app.runtime.tabs.active_id();
    let drawings = &mut app.active_tab_mut().flow_pane.drawings;
    drawings.place(
        drawing_tool("horizontal-line"),
        ChartPoint::at_time(1.0, 100.0, None),
    );
    drawings.select(Some(0));
    edit_split(&mut app, tab, 0.33);
    let row = app.runtime.workspace.book.tab_row(tab);
    let set = WorkspaceCommand::Set("Default".to_owned());
    app.workspace_calls().apply_workspace_command(set);
    assert!(app.chrome.window_menu.tab_workspace.asking());
    let ctx = egui::Context::default();
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
    assert!(
        !app.chrome.window_menu.tab_workspace.asking(),
        "Esc cancelled"
    );
    assert_eq!(
        app.runtime.workspace.book.tab_row(tab),
        row,
        "the tab stays"
    );
    let selected = app.active_tab().flow_pane.drawings.selected();
    assert_eq!(selected, Some(0), "the drawing stays selected");
}
