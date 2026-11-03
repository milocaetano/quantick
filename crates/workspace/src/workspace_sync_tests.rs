use super::*;
use crate::layout_document::LayoutBook;
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

#[test]
fn every_pane_ends_on_a_copy_no_other_pane_holds() {
    let mut session = LayoutSession::new(LayoutBook::starter(Vec::new()));
    seeded(&mut session, 1);
    seeded(&mut session, 2);
    let public = session.book().active_id();
    assert!(session.own_copies([1, 2]));
    let (one, two) = (session.layout(1), session.layout(2));
    assert_ne!(one, two);
    assert_ne!(one, public);
    assert!(session.book().is_private(one) && session.book().is_private(two));
    assert_eq!(session.book().layouts().len(), 1, "the strip did not grow");
    assert!(!session.own_copies([1, 2]), "a second pass finds nothing");
    assert_eq!(session.book().strip_layout(one), public);
}

#[test]
fn a_private_copy_leaves_with_its_pane_or_when_its_pane_switches_away() {
    let mut session = LayoutSession::new(LayoutBook::starter(Vec::new()));
    seeded(&mut session, 1);
    seeded(&mut session, 2);
    session.own_copies([1, 2]);
    let (one, two) = (session.layout(1), session.layout(2));
    session.remove_pane(1);
    assert!(session.book().get(one).is_none());
    let public = session.book().active_id();
    let selection = session.select(2, public, PaneFacts::default()).unwrap();
    session.commit_selection(selection).unwrap();
    assert!(session.book().get(two).is_none());
}

/// The strip and the control plane name strip layouts; another pane's own copy
/// is not one, and an edit through it would reach that pane.
#[test]
fn another_panes_private_copy_is_refused_by_the_strip_and_the_switch() {
    let mut session = LayoutSession::new(LayoutBook::starter(Vec::new()));
    seeded(&mut session, 1);
    seeded(&mut session, 2);
    session.own_copies([1, 2]);
    let (one, two) = (session.layout(1), session.layout(2));
    assert!(session.book().on_strip(two).is_none());
    assert_eq!(
        session.select(1, two, PaneFacts::default()).unwrap_err(),
        crate::layout_document::LayoutError::Unknown
    );
    assert!(
        session.select(1, one, PaneFacts::default()).is_ok(),
        "its own"
    );
    let mut book = session.book().clone();
    assert_eq!(
        book.switch(two),
        Err(crate::layout_document::LayoutError::Unknown)
    );
}

/// A desk read from disk names the copies its tabs use; the rest are dropped.
#[test]
fn prune_keeps_held_and_named_copies_and_drops_the_rest() {
    let mut session = LayoutSession::new(LayoutBook::starter(Vec::new()));
    seeded(&mut session, 1);
    seeded(&mut session, 2);
    seeded(&mut session, 3);
    session.own_copies([1, 2, 3]);
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
