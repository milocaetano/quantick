use super::*;
use crate::layout_document::{LayoutBook, LayoutId};
use crate::session::{LayoutSession, PaneFacts};

fn with_layer(on: bool) -> WorkspaceRecipe {
    WorkspaceRecipe {
        layers: BTreeMap::from([("bubbles".to_owned(), on)]),
        ..WorkspaceRecipe::default()
    }
}

#[test]
fn a_first_pass_seeds_and_a_later_tab_opens_on_default() {
    let mut sync = TabRecipeSync::default();
    assert_eq!(sync.adoption(1, false, true), TabAdoption::Seed);
    sync.finish_first_pass();
    assert_eq!(sync.adoption(2, false, true), TabAdoption::OnDefault);
    assert_eq!(sync.adoption(1, true, false), TabAdoption::Held);
}

#[test]
fn a_used_book_never_seeds_a_tab_with_no_word_from_a_desk() {
    let mut sync = TabRecipeSync::default();
    assert_eq!(sync.adoption(1, false, false), TabAdoption::OnDefault);
}

#[test]
fn a_desk_decides_for_the_tabs_it_brought_back() {
    let mut sync = TabRecipeSync::default();
    sync.finish_first_pass();
    sync.arrive(1, Some("Scalp".to_owned()), None);
    sync.arrive(2, None, None);
    assert_eq!(
        sync.adoption(1, false, false),
        TabAdoption::Restore {
            workspace: "Scalp".to_owned(),
            copy: None,
        }
    );
    assert_eq!(sync.adoption(2, false, false), TabAdoption::Seed);
    assert!(!sync.has_arrival(1));
}

#[test]
fn the_canvas_is_laid_once_and_the_recipe_stays_until_the_panes_stand() {
    let mut sync = TabRecipeSync::default();
    sync.queue(1, WorkspaceRecipe::default());
    assert!(sync.take_canvas(1).is_some());
    assert!(sync.take_canvas(1).is_none());
    assert!(sync.is_pending(1));
    assert!(sync.finish(1).is_some());
    assert!(!sync.is_pending(1));
}

#[test]
fn a_tab_that_clamped_its_recipe_is_not_an_edit() {
    let mut sync = TabRecipeSync::default();
    let copy = WorkspaceRecipe {
        split_fraction: Some(0.99),
        ..WorkspaceRecipe::default()
    };
    let clamped = WorkspaceRecipe {
        split_fraction: Some(0.8),
        ..WorkspaceRecipe::default()
    };
    sync.settle(1, clamped.clone());
    assert_eq!(sync.observe(1, clamped, &copy), None);
}

#[test]
fn a_change_after_settling_is_an_edit_once() {
    let mut sync = TabRecipeSync::default();
    let copy = with_layer(true);
    sync.settle(1, copy.clone());
    let edited = with_layer(false);
    assert_eq!(sync.observe(1, edited.clone(), &copy), Some(edited.clone()));
    assert_eq!(sync.observe(1, edited, &copy), None);
}

#[test]
fn what_a_recipe_leaves_open_stays_open_until_it_moves() {
    let mut sync = TabRecipeSync::default();
    let copy = WorkspaceRecipe::default();
    let live = WorkspaceRecipe {
        footprint_style: Some("cluster".to_owned()),
        ..with_layer(true)
    };
    sync.settle(1, live.clone());
    let moved_elsewhere = WorkspaceRecipe {
        split_fraction: Some(0.5),
        ..live.clone()
    };
    let edit = sync.observe(1, moved_elsewhere, &copy).expect("an edit");
    assert!(edit.layers.is_empty());
    assert_eq!(edit.footprint_style, None);
    let layer_moved = WorkspaceRecipe {
        layers: with_layer(false).layers,
        ..live
    };
    let edit = sync.observe(1, layer_moved, &copy).expect("an edit");
    assert_eq!(edit.layers.get("bubbles"), Some(&false));
}

#[test]
fn background_tabs_are_read_in_turn_and_never_the_active_one() {
    let mut sync = TabRecipeSync::default();
    let tabs = [1, 2, 3];
    let first = sync.next_watch(&tabs, 2).unwrap();
    let second = sync.next_watch(&tabs, 2).unwrap();
    assert_ne!(first, 2);
    assert_ne!(second, 2);
    assert_ne!(first, second);
    assert_eq!(sync.next_watch(&[2], 2), None);
}

fn seeded(session: &mut LayoutSession, pane: u64) {
    session.register(pane, None);
    session.seed(pane, None, None);
}

fn ema() -> crate::indicator_document::SavedIndicator {
    crate::indicator_document::SavedIndicator {
        kind: crate::indicator_document::SavedKind::native("native.ema"),
        hidden: false,
        mouse_vertical_line: false,
        inputs: Vec::new(),
        plot_styles: Vec::new(),
    }
}

/// Panes 1 and 2 on tab 10, pane 3 on tab 20, all on the one strip layout.
fn two_tabs_on_one_layout() -> LayoutSession {
    let mut session = LayoutSession::new(LayoutBook::starter(Vec::new()));
    for pane in [1, 2, 3] {
        seeded(&mut session, pane);
    }
    session.own_copies([(10, 1), (10, 2), (20, 3)]);
    session
}

#[test]
fn the_panes_of_one_tab_share_its_copy_and_no_other_tab_does() {
    let mut session = LayoutSession::new(LayoutBook::starter(Vec::new()));
    for pane in [1, 2, 3] {
        seeded(&mut session, pane);
    }
    let public = session.book().active_id();
    let owned = session.own_copies([(10, 1), (10, 2), (20, 3)]);
    assert!(owned.changed && owned.reshow.is_empty(), "{owned:?}");
    let (one, two, three) = (session.layout(1), session.layout(2), session.layout(3));
    assert_eq!(one, two, "one tab, one strip layout, one copy");
    assert_ne!(one, three, "another tab never shares it");
    assert_ne!(one, public);
    assert!(session.book().is_private(one) && session.book().is_private(three));
    assert_eq!(session.book().layouts().len(), 1, "the strip did not grow");
    let again = session.own_copies([(10, 1), (10, 2), (20, 3)]);
    assert!(!again.changed, "a second pass finds nothing");
    assert_eq!(session.book().strip_layout(one), public);
}

/// A pane put back on a strip layout its tab already holds a copy of lands on
/// that copy, with the copy's set; one that reached the strip layout another
/// way is moved onto the copy by the next pass and told to show its set.
#[test]
fn a_pane_back_on_the_strip_layout_rejoins_its_tabs_edited_copy() {
    let mut session = two_tabs_on_one_layout();
    let (copy, public) = (session.layout(1), session.book().active_id());
    session.indicators_mut(copy).unwrap().push(ema());
    let levels = session.create(Some("levels")).unwrap();
    let away = session.select(1, levels, PaneFacts::default()).unwrap();
    session.commit_selection(away).unwrap();
    let back = session.select(1, public, PaneFacts::default()).unwrap();
    assert_eq!(back.to(), copy);
    assert!(
        session.book().is_private(back.from()),
        "levels, as its own copy"
    );
    assert_eq!(session.book().origin_of(back.from()), levels);
    let committed = session.commit_selection(back).unwrap();
    session.finish_selection(committed).unwrap();
    assert_eq!(
        session.book().active_id(),
        public,
        "the strip lights its origin"
    );

    session.register(4, Some(public));
    session.seed(4, None, None);
    let owned = session.own_copies([(10, 1), (10, 2), (20, 3), (10, 4)]);
    assert_eq!(owned.reshow, [4]);
    assert_eq!(session.layout(4), copy);
}

/// A pane that came to hold another tab's copy gets its own tab's.
#[test]
fn a_pane_on_another_tabs_copy_moves_to_a_copy_of_its_tab() {
    let session = two_tabs_on_one_layout();
    let theirs = session.layout(1);
    // Pane 4 of tab 30 is seeded from tab 10's focused pane.
    let mut fresh = LayoutSession::new(session.book().clone());
    fresh.register(1, Some(theirs));
    fresh.seed(1, None, None);
    fresh.register(4, None);
    fresh.seed(4, None, Some(1));
    assert_eq!(fresh.layout(4), theirs);
    let owned = fresh.own_copies([(10, 1), (30, 4)]);
    assert!(owned.changed && owned.reshow.is_empty());
    assert_eq!(fresh.layout(1), theirs, "the owner keeps it");
    assert_ne!(fresh.layout(4), theirs);
    assert_eq!(
        fresh.book().origin_of(fresh.layout(4)),
        fresh.book().origin_of(theirs)
    );
}

/// A tab's copy keeps its set while the tab shows another strip layout, and
/// leaves with the tab's last pane.
#[test]
fn a_tabs_copy_outlives_a_switch_away_and_leaves_with_the_tab() {
    let mut session = two_tabs_on_one_layout();
    let (shared, three) = (session.layout(1), session.layout(3));
    session.indicators_mut(three).unwrap().push(ema());
    let public = session.book().active_id();
    let levels = session.create(Some("levels")).unwrap();
    let selection = session.select(3, levels, PaneFacts::default()).unwrap();
    session.commit_selection(selection).unwrap();
    assert!(session.book().get(three).is_some(), "tab 20 still stands");
    let selection = session.select(3, public, PaneFacts::default()).unwrap();
    session.commit_selection(selection).unwrap();
    assert_eq!(session.layout(3), three, "back on its tab's copy");

    session.remove_pane(1);
    assert!(
        session.book().get(shared).is_some(),
        "pane 2 still holds it"
    );
    session.remove_pane(2);
    assert!(session.book().get(shared).is_none(), "tab 10 is gone");
    session.remove_pane(3);
    assert!(session.book().get(three).is_none(), "tab 20 is gone");
}

fn switch(session: &mut LayoutSession, pane: u64, to: LayoutId) {
    let selection = session.select(pane, to, PaneFacts::default()).unwrap();
    let committed = session.commit_selection(selection).unwrap();
    session.finish_selection(committed).unwrap();
}

/// Of two copies a recipe left one tab on one strip layout, the one a pane
/// switched away from stays that pane's: back on the strip layout it finds its
/// own set, never its twin's. It goes with the pane.
#[test]
fn a_parted_copy_stays_its_panes_through_a_switch_away_and_back() {
    let mut session = two_tabs_on_one_layout();
    let (shared, public) = (session.layout(1), session.book().active_id());
    let (none, one) = (Vec::new(), vec![ema()]);
    session.part_copies([(1, none.as_slice()), (2, one.as_slice())]);
    let parted = session.layout(2);
    session.indicators_mut(parted).unwrap().push(ema());
    let levels = session.create(Some("levels")).unwrap();
    switch(&mut session, 2, levels);
    assert!(session.book().get(parted).is_some(), "kept for pane 2");
    switch(&mut session, 2, public);
    assert_eq!(session.layout(2), parted, "its own set");
    assert_eq!(session.book().get(parted).unwrap().indicators, one);
    let again = session.select(1, public, PaneFacts::default()).unwrap();
    assert!(!again.changed(), "pane 1 stays on its own copy");
    assert_eq!(session.layout(1), shared);

    switch(&mut session, 2, levels);
    session.remove_pane(2);
    assert!(session.book().get(parted).is_none(), "gone with its pane");
    assert!(session.book().get(shared).is_some());
}

/// The restored-desk fallback: tab 30's pane seeded from tab 10's edited copy
/// gets a copy of the strip layout, never tab 10's unsaved set.
#[test]
fn a_pane_seeded_from_another_tabs_edited_copy_never_shows_its_edits() {
    let session = two_tabs_on_one_layout();
    let theirs = session.layout(1);
    let mut fresh = LayoutSession::new(session.book().clone());
    fresh.indicators_mut(theirs).unwrap().push(ema());
    fresh.register(1, Some(theirs));
    fresh.seed(1, None, None);
    fresh.register(4, None);
    fresh.seed(4, None, Some(1));
    assert_eq!(fresh.layout(4), theirs);
    let owned = fresh.own_copies([(10, 1), (30, 4)]);
    assert_eq!(owned.reshow, [4], "it showed tab 10's set");
    let mine = fresh.layout(4);
    assert!(fresh.book().get(mine).unwrap().indicators.is_empty());
    assert_eq!(fresh.book().get(theirs).unwrap().indicators, [ema()]);
}

/// A pane registered with its tab lands on a copy at its first commit, even a
/// commit that changes nothing, so no edit can reach the strip layout.
#[test]
fn a_fresh_pane_of_a_known_tab_never_commits_on_a_strip_layout() {
    let mut session = LayoutSession::new(LayoutBook::starter(Vec::new()));
    let public = session.book().active_id();
    session.register_in(10, 1, None);
    let selection = session.select(1, public, PaneFacts::default()).unwrap();
    assert!(!selection.changed());
    session.commit_selection(selection).unwrap();
    let held = session.layout(1);
    assert!(session.book().is_private(held));
    assert_eq!(session.book().origin_of(held), public);
}

/// Deleting a strip layout moves the panes on its copies to the neighbour and
/// takes the copies with it.
#[test]
fn deleting_a_strip_layout_moves_the_panes_on_its_copies() {
    let mut session = two_tabs_on_one_layout();
    let public = session.book().active_id();
    let levels = session.create(Some("levels")).unwrap();
    switch(&mut session, 3, levels);
    let copy = session.layout(3);
    assert_eq!(session.book().origin_of(copy), levels);
    let facts = [1, 2, 3].map(|pane| (pane, PaneFacts::default()));
    let plan = session.plan_delete(levels, facts.into_iter()).unwrap();
    assert_eq!(plan.iter().map(|s| s.pane()).collect::<Vec<_>>(), [3]);
    for selection in plan {
        let committed = session.commit_selection(selection).unwrap();
        session.finish_selection(committed).unwrap();
    }
    session.finish_delete(levels).unwrap();
    assert_eq!(session.book().strip_layout(session.layout(3)), public);
    assert!(session.book().get(copy).is_none(), "its copy went too");
}

/// The strip and the control plane name strip layouts; another tab's own copy
/// is not one, and an edit through it would reach that tab.
#[test]
fn another_tabs_private_copy_is_refused_by_the_strip_and_the_switch() {
    let session = two_tabs_on_one_layout();
    let (one, three) = (session.layout(1), session.layout(3));
    assert!(session.book().on_strip(three).is_none());
    assert_eq!(
        session.select(1, three, PaneFacts::default()).unwrap_err(),
        crate::layout_document::LayoutError::Unknown
    );
    assert!(
        session.select(1, one, PaneFacts::default()).is_ok(),
        "its own"
    );
    let mut book = session.book().clone();
    assert_eq!(
        book.switch(three),
        Err(crate::layout_document::LayoutError::Unknown)
    );
}

/// A recipe that gives two panes of a tab two sets parts their copy; one that
/// gives them one set keeps it shared.
#[test]
fn a_recipe_parts_a_shared_copy_only_where_its_sets_differ() {
    let mut session = two_tabs_on_one_layout();
    let (shared, other_tab) = (session.layout(1), session.layout(3));
    let (none, one) = (Vec::new(), vec![ema()]);
    assert!(!session.part_copies([(1, none.as_slice()), (2, none.as_slice())]));
    assert_eq!(session.layout(2), shared);
    assert!(session.part_copies([(1, none.as_slice()), (2, one.as_slice())]));
    assert_eq!(session.layout(1), shared);
    let parted = session.layout(2);
    assert_ne!(parted, shared);
    assert!(session.book().is_private(parted));
    assert_eq!(
        session.book().origin_of(parted),
        session.book().origin_of(shared)
    );
    assert!(!session.part_copies([(1, none.as_slice()), (2, one.as_slice())]));
    assert_eq!(session.layout(3), other_tab, "tab 20 untouched");
}

/// A desk read from disk names the copies its tabs use; the rest are dropped.
#[test]
fn prune_keeps_held_and_named_copies_and_drops_the_rest() {
    let mut session = LayoutSession::new(LayoutBook::starter(Vec::new()));
    for pane in [1, 2, 3] {
        seeded(&mut session, pane);
    }
    session.own_copies([(10, 1), (20, 2), (30, 3)]);
    let (one, two, three) = (session.layout(1), session.layout(2), session.layout(3));
    let mut book = session.book().clone();
    let text = toml::to_string(&book).unwrap();
    book = crate::layout_document::parse(&text).unwrap();
    let mut reread = LayoutSession::new(book);
    reread.register(3, Some(three));
    assert!(reread.prune_private([one]));
    assert!(reread.book().get(one).is_some(), "a restored tab names it");
    assert!(reread.book().get(three).is_some(), "a pane holds it");
    assert!(reread.book().get(two).is_none(), "nobody refers to it");
    assert!(!reread.prune_private([one]), "a second pass finds nothing");
}

#[test]
fn a_tab_on_the_factory_default_is_clean_until_it_moves_and_then_holds_what_it_shows() {
    let mut sync = TabRecipeSync::default();
    let factory = WorkspaceRecipe::default();
    let opened = WorkspaceRecipe::new_tab_arrangement(
        crate::workspace_recipe::DeclaredLayout::TimeAndFlow,
        &[],
    );
    assert_eq!(sync.observe(1, opened.clone(), &factory), None);
    assert_eq!(sync.observe(1, opened.clone(), &factory), None);
    let moved = WorkspaceRecipe {
        split_fraction: Some(0.4),
        ..opened
    };
    assert_eq!(sync.observe(1, moved.clone(), &factory), Some(moved));
}

#[test]
fn a_layer_moved_on_a_context_pane_is_an_edit_of_that_pane_only() {
    use crate::workspace_recipe::ContextPaneRecipe;
    let pane = |on: bool| ContextPaneRecipe {
        indicators: Vec::new(),
        layers: BTreeMap::from([("bubbles".to_owned(), on)]),
    };
    let settled = WorkspaceRecipe {
        context_panes: vec![pane(true), pane(true)],
        ..WorkspaceRecipe::default()
    };
    let copy = WorkspaceRecipe {
        context_panes: vec![
            ContextPaneRecipe {
                indicators: Vec::new(),
                layers: BTreeMap::new(),
            };
            2
        ],
        ..WorkspaceRecipe::default()
    };
    let mut sync = TabRecipeSync::default();
    sync.settle(1, settled);
    let edit = sync
        .observe(
            1,
            WorkspaceRecipe {
                context_panes: vec![pane(true), pane(false)],
                ..WorkspaceRecipe::default()
            },
            &copy,
        )
        .expect("the second pane moved");
    assert!(edit.context_panes[0].layers.is_empty());
    assert_eq!(edit.context_panes[1].layers.get("bubbles"), Some(&false));
}

/// A tab opened onto a workspace restores onto it when adopted; one left to
/// Default arrives as nothing, so it joins the way any new tab does.
#[test]
fn a_new_tab_joins_its_chosen_workspace_and_none_leaves_it_alone() {
    let mut sync = TabRecipeSync::default();
    sync.join(4, None);
    assert!(!sync.has_arrival(4));
    sync.join(5, Some("Swing".to_owned()));
    assert!(matches!(
        sync.adoption(5, false, false),
        TabAdoption::Restore { workspace, copy: None } if workspace == "Swing"
    ));
}
