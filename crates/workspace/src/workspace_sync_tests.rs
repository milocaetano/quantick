use super::*;
use crate::indicator_document::SavedIndicator;
use crate::layout_document::{LayoutBook, LayoutId};
use crate::session::{LayoutSession, OwnedCopies, PaneFacts};

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

/// A recipe whose panes on one strip layout named two sets lands one: what
/// the tab shows then is an edit of its copy, once. One that landed as named
/// owes nothing.
#[test]
fn a_set_healed_on_landing_is_an_edit_once() {
    let mut sync = TabRecipeSync::default();
    let mut named = WorkspaceRecipe::default();
    named.context_panes.push(Default::default());
    named.context_panes[0].indicators = vec![ema()];
    let mut shown = named.clone();
    shown.flow_indicators = vec![ema()];
    sync.settle(1, shown.clone());
    sync.owe_indicators(1, &named);
    assert_eq!(sync.observe(1, shown.clone(), &named), Some(shown.clone()));
    assert_eq!(sync.observe(1, shown.clone(), &named), None);

    sync.settle(2, named.clone());
    sync.owe_indicators(2, &named);
    assert_eq!(sync.observe(2, named.clone(), &named), None);
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

/// A recipe that gives two panes of a tab two sets parts nothing: a pane
/// switched away and back lands on the tab's one copy, with the fuller set it
/// was given, and the copy stays while the other pane holds it.
#[test]
fn a_pane_back_from_a_switch_finds_its_tabs_one_copy() {
    let mut session = two_tabs_on_one_layout();
    let (shared, public) = (session.layout(1), session.book().active_id());
    let (none, one) = (Vec::new(), vec![ema()]);
    let shown = session.shared_sets([(1, none.as_slice()), (2, one.as_slice())]);
    assert_eq!(shown, [(1, one.as_slice()), (2, one.as_slice())]);
    assert_eq!(session.layout(2), shared, "nothing parted");
    session.indicators_mut(shared).unwrap().push(ema());
    let levels = session.create(Some("levels")).unwrap();
    switch(&mut session, 2, levels);
    switch(&mut session, 2, public);
    assert_eq!(session.layout(2), shared, "the tab's one copy");
    assert_eq!(session.book().get(shared).unwrap().indicators, one);

    switch(&mut session, 2, levels);
    session.remove_pane(2);
    assert!(
        session.book().get(shared).is_some(),
        "pane 1 still holds it"
    );
}

fn script(name: &str) -> crate::indicator_document::SavedIndicator {
    crate::indicator_document::SavedIndicator {
        kind: crate::indicator_document::SavedKind::Script {
            name: name.to_owned(),
        },
        ..ema()
    }
}

fn native(id: &str) -> crate::indicator_document::SavedIndicator {
    crate::indicator_document::SavedIndicator {
        kind: crate::indicator_document::SavedKind::native(id),
        ..ema()
    }
}

/// A stored file: flow pane 1 and context panes 2 and 3 of tab 10 on three
/// copies of one strip layout, holding `sets` in that order.
fn three_twins(sets: [Vec<SavedIndicator>; 3]) -> (LayoutSession, [LayoutId; 3]) {
    let mut book = LayoutBook::starter(Vec::new());
    let origin = book.active_id();
    let copies = sets.map(|set| {
        let copy = book.make_private_copy(origin).unwrap();
        book.get_mut(copy).unwrap().indicators = set;
        copy
    });
    let mut session = LayoutSession::new(book);
    for (pane, copy) in [1, 2, 3].into_iter().zip(copies) {
        session.register(pane, Some(copy));
        session.seed(pane, None, None);
    }
    (session, copies)
}

/// The trader's WINZ26 tab, as his stored file holds it.
fn winz26() -> [Vec<SavedIndicator>; 3] {
    [
        vec![script("force_bar.pine"), native("native.cvd")],
        vec![script("force_bar.pine")],
        vec![script("force_bar.pine"), script("cvd.pine")],
    ]
}

/// Inside one tab, the panes on one strip layout show one set. A recipe giving
/// them two sets lands the fuller on both, a tie going to the first pane, and
/// never reaches another tab's; twin copies a stored file brings back merge
/// into the fullest, every pane on it, and the merge reads back from the file
/// as it was left.
#[test]
fn panes_of_one_tab_on_one_strip_layout_always_share_a_set() {
    let session = two_tabs_on_one_layout();
    let (none, one) = (Vec::new(), vec![ema()]);
    let cvd = vec![native("native.cvd")];
    let shown = session.shared_sets([(1, none.as_slice()), (2, one.as_slice())]);
    assert_eq!(
        shown,
        [(1, one.as_slice()), (2, one.as_slice())],
        "the fuller"
    );
    let tie = session.shared_sets([(1, cvd.as_slice()), (2, one.as_slice())]);
    assert_eq!(tie, [(1, cvd.as_slice()), (2, cvd.as_slice())], "the first");
    let apart = session.shared_sets([(1, none.as_slice()), (3, one.as_slice())]);
    assert_eq!(apart, [(1, none.as_slice()), (3, one.as_slice())], "tab 20");

    let (mut session, [flow, twin, other]) = three_twins(winz26());
    let owned = session.own_copies([(10, 1), (10, 2), (10, 3)]);
    assert!(owned.changed);
    assert_eq!(owned.reshow, [2, 3], "the context panes showed another set");
    assert_eq!(
        [1, 2, 3].map(|pane| session.layout(pane)),
        [flow; 3],
        "a tie"
    );
    let force = vec![script("force_bar.pine"), native("native.cvd")];
    assert_eq!(session.book().get(flow).unwrap().indicators, force);
    assert!(session.book().get(twin).is_none() && session.book().get(other).is_none());
    // An edit reaches every pane on the copy it writes to.
    let views = [1, 2, 3].map(|pane| session.register(pane, None));
    assert!(views.iter().all(|view| session.shows(view, flow)));
    let again = session.shared_sets([
        (1, force.as_slice()),
        (2, none.as_slice()),
        (3, one.as_slice()),
    ]);
    assert!(again.iter().all(|(_, set)| *set == force.as_slice()));

    let text = toml::to_string(session.book()).unwrap();
    let mut reread = LayoutSession::new(crate::layout_document::parse(&text).unwrap());
    for pane in [1, 2, 3] {
        reread.register(pane, Some(flow));
        reread.seed(pane, None, None);
    }
    assert!(!reread.prune_private([flow]), "the file holds one copy");
    let owned = reread.own_copies([(10, 1), (10, 2), (10, 3)]);
    assert_eq!(owned, OwnedCopies::default(), "nothing left to merge");
    assert_eq!([1, 2, 3].map(|pane| reread.layout(pane)), [flow; 3]);
    assert_eq!(reread.book().get(flow).unwrap().indicators, force);
}

/// Twins merge into the copy holding the most indicators, wherever its pane
/// stands: an empty flow copy never empties its tab.
#[test]
fn the_fullest_twin_survives_a_merge() {
    // The trader's Fluxo BTC tab: the context panes hold CVD, the flow pane not.
    let fluxo = vec![script("force_bar.pine"), script("cvd.pine")];
    let sets = [vec![script("force_bar.pine")], fluxo.clone(), fluxo.clone()];
    let (mut session, [_, survivor, _]) = three_twins(sets);
    let owned = session.own_copies([(10, 1), (10, 2), (10, 3)]);
    assert_eq!(owned.reshow, [1], "only the flow pane showed less");
    assert_eq!([1, 2, 3].map(|pane| session.layout(pane)), [survivor; 3]);
    assert_eq!(session.book().get(survivor).unwrap().indicators, fluxo);

    let (mut session, [flow, survivor, other]) = three_twins([Vec::new(), vec![ema()], Vec::new()]);
    let owned = session.own_copies([(10, 1), (10, 2), (10, 3)]);
    assert_eq!(owned.reshow, [1, 3]);
    assert_eq!([1, 2, 3].map(|pane| session.layout(pane)), [survivor; 3]);
    assert_eq!(session.book().get(survivor).unwrap().indicators, [ema()]);
    assert!(session.book().get(flow).is_none() && session.book().get(other).is_none());
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

/// A recipe that gives two panes of a tab two sets parts nothing: both show
/// the fuller, a tie going to the first in the order given. One set stays one.
#[test]
fn a_recipe_never_parts_a_tabs_copy() {
    let session = two_tabs_on_one_layout();
    let (none, one, cvd) = (Vec::new(), vec![ema()], vec![native("native.cvd")]);
    let same = session.shared_sets([(1, none.as_slice()), (2, none.as_slice())]);
    assert_eq!(same, [(1, none.as_slice()), (2, none.as_slice())]);
    let two = session.shared_sets([(2, none.as_slice()), (1, one.as_slice())]);
    assert_eq!(
        two,
        [(2, one.as_slice()), (1, one.as_slice())],
        "the fuller"
    );
    let tie = session.shared_sets([(2, one.as_slice()), (1, cvd.as_slice())]);
    assert_eq!(
        tie,
        [(2, one.as_slice()), (1, one.as_slice())],
        "the first given"
    );
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
        bars: None,
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
                bars: None,
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

/// A recipe saved before workspaces kept timeframes holds none: the tab's own
/// bars are no edit until the trader moves one.
#[test]
fn a_recipe_without_bars_reads_the_tab_s_own_bars_as_no_edit() {
    use crate::workspace_recipe::ContextPaneRecipe;
    let on = |flow: &str, time: &str| WorkspaceRecipe {
        flow_bars: Some(flow.to_owned()),
        context_panes: vec![ContextPaneRecipe {
            bars: Some(time.to_owned()),
            ..ContextPaneRecipe::default()
        }],
        ..WorkspaceRecipe::default()
    };
    let old = WorkspaceRecipe {
        context_panes: vec![ContextPaneRecipe::default()],
        ..WorkspaceRecipe::default()
    };
    let mut sync = TabRecipeSync::default();
    sync.settle(1, on("tick:500", "time:60000"));
    assert_eq!(sync.observe(1, on("tick:500", "time:60000"), &old), None);
    let moved = sync
        .observe(1, on("tick:2000", "time:60000"), &old)
        .expect("a flow bar change is an edit");
    assert_eq!(moved.flow_bars.as_deref(), Some("tick:2000"));
    assert_eq!(
        moved.context_panes[0].bars, None,
        "the untouched pane keeps none"
    );
}
