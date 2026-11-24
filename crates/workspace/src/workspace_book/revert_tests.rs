use super::*;
use crate::arrangement_document::SavedTab;
use crate::indicator_document::{SavedIndicator, SavedKind};
use crate::layout_document::LayoutBook;

fn ema() -> SavedIndicator {
    SavedIndicator {
        kind: SavedKind::native("native.ema"),
        hidden: false,
        mouse_vertical_line: false,
        inputs: Vec::new(),
        plot_styles: Vec::new(),
    }
}

fn on_bars(flow: &str, time: &str) -> WorkspaceRecipe {
    let tab: SavedTab = toml::from_str(&format!(
        "feed = \"mt5\"\nsymbol = \"WINV26\"\nlayout = \"time+flow\"\n\
         flow_bars = \"{flow}\"\ncontext_bars = [\"{time}\"]\n"
    ))
    .unwrap();
    capture_recipe(
        &tab,
        &LayoutBook::starter(Vec::new()),
        (BTreeMap::new(), Vec::new()),
        None,
    )
}

/// Tab 1 on `Scalp`, saved on Tick 500 and 1m, autosave off.
fn on_scalp() -> (WorkspaceBook, WorkspaceRecipe) {
    let mut book = WorkspaceBook::default();
    book.set_autosave(false);
    book.open_tab(1);
    let saved = on_bars("tick:500", "time:60000");
    book.observe(1, &saved).unwrap();
    book.save_as(1, "Scalp").unwrap();
    (book, saved)
}

fn edited(saved: &WorkspaceRecipe) -> WorkspaceRecipe {
    let mut edit = on_bars("tick:2000", "time:120000");
    edit.flow_indicators = vec![ema()];
    edit.split_fraction = Some(0.7);
    edit.footprint_style = Some("candle".to_owned());
    assert_ne!(&edit, saved);
    edit
}

#[test]
fn a_revert_puts_the_saved_workspace_back_and_its_undo_the_edits() {
    let (mut book, saved) = on_scalp();
    let edit = edited(&saved);
    book.observe(1, &edit).unwrap();
    assert!(!book.can_undo_revert(1), "nothing reverted yet");
    let reverted = book.revert(1).unwrap();
    assert_eq!(
        (reverted.workspace.as_str(), reverted.changed),
        ("Scalp", true)
    );
    assert_eq!(book.tab_copy(1), Some(&saved));
    assert!(!book.differs(1));
    assert!(book.can_undo_revert(1));
    assert!(book.undo_revert(1).unwrap().changed);
    assert_eq!(book.tab_copy(1), Some(&edit));
    assert!(book.differs(1), "the edits are unsaved again");
    assert!(!book.can_undo_revert(1), "an undo is taken once");
    assert!(
        !book.undo_revert(1).unwrap().changed,
        "a repeat changes nothing"
    );
}

#[test]
fn a_clean_tab_has_nothing_to_revert() {
    let (mut book, saved) = on_scalp();
    assert!(!book.revert(1).unwrap().changed);
    assert!(!book.can_undo_revert(1));
    assert_eq!(book.tab_copy(1), Some(&saved));
    assert_eq!(book.revert(9), Err(WorkspaceRefusal::UnknownTab(9)));
    assert_eq!(book.undo_revert(9), Err(WorkspaceRefusal::UnknownTab(9)));
}

#[test]
fn a_save_or_a_switch_ends_the_undo() {
    let (mut book, saved) = on_scalp();
    book.observe(1, &edited(&saved)).unwrap();
    book.revert(1).unwrap();
    book.observe(1, &on_bars("tick:500", "time:300000"))
        .unwrap();
    book.save_to(1).unwrap();
    assert!(!book.can_undo_revert(1), "the workspace was saved since");

    let (mut book, saved) = on_scalp();
    book.open_tab(2);
    book.observe(2, &on_bars("tick:50", "time:60000")).unwrap();
    book.save_as(2, "Other").unwrap();
    book.observe(1, &edited(&saved)).unwrap();
    book.revert(1).unwrap();
    book.set(1, "Other").unwrap();
    book.set(1, "Scalp").unwrap();
    assert!(!book.can_undo_revert(1), "a switch ends it, even back");
}

#[test]
fn new_edits_after_a_revert_hold_the_undo_back_and_the_next_revert_replaces_it() {
    let (mut book, saved) = on_scalp();
    let first = edited(&saved);
    book.observe(1, &first).unwrap();
    book.revert(1).unwrap();
    let second = on_bars("tick:500", "time:300000");
    book.observe(1, &second).unwrap();
    assert!(!book.can_undo_revert(1), "edits of its own since");
    assert!(!book.undo_revert(1).unwrap().changed, "never written over");
    assert_eq!(book.tab_copy(1), Some(&second));
    book.revert(1).unwrap();
    book.undo_revert(1).unwrap();
    assert_eq!(book.tab_copy(1), Some(&second), "the latest revert's edits");
}

/// A workspace naming no bar rule: a revert puts back the timeframes the tab
/// opened with, not whatever it shows.
#[test]
fn a_revert_on_a_workspace_naming_no_bars_restores_the_opened_timeframes() {
    let mut book = WorkspaceBook::default();
    book.set_autosave(false);
    book.open_tab(1);
    book.observe(1, &on_bars("tick:500", "time:60000").without_bars())
        .unwrap();
    book.save_as(1, "Legacy").unwrap();
    let opened = on_bars("tick:500", "time:60000");
    book.settle_tab(1, &opened);
    book.observe(1, &on_bars("tick:2000", "time:300000"))
        .unwrap();
    assert!(book.revert(1).unwrap().changed);
    let copy = book.tab_copy(1).unwrap();
    assert_eq!(copy.flow_bars.as_deref(), Some("tick:500"));
    assert_eq!(copy.context_panes[0].bars.as_deref(), Some("time:60000"));
    assert!(!book.differs(1));
    assert_eq!(
        book.recipe("Legacy").unwrap().flow_bars,
        None,
        "nothing written"
    );
}

#[test]
fn closing_the_tab_forgets_its_revert() {
    let (mut book, saved) = on_scalp();
    book.observe(1, &edited(&saved)).unwrap();
    book.revert(1).unwrap();
    book.close_tab(1);
    assert!(book.reverted.is_empty());
}

/// Tab 1 on `Legacy`, a workspace naming no bar rule, settled on Tick 500
/// and 1m.
fn on_legacy() -> WorkspaceBook {
    let mut book = WorkspaceBook::default();
    book.set_autosave(false);
    book.open_tab(1);
    book.observe(1, &on_bars("tick:500", "time:60000").without_bars())
        .unwrap();
    book.save_as(1, "Legacy").unwrap();
    book.settle_tab(1, &on_bars("tick:500", "time:60000"));
    book
}

/// The window re-settles a tab after an undo redraws it: the timeframes it
/// brought back stay an edit.
#[test]
fn an_undone_revert_s_timeframes_stay_an_edit_once_the_tab_re_settles() {
    let mut book = on_legacy();
    let edit = on_bars("tick:2000", "time:300000");
    book.observe(1, &edit).unwrap();
    book.revert(1).unwrap();
    book.settle_tab(1, &on_bars("tick:500", "time:60000"));
    assert!(book.undo_revert(1).unwrap().changed);
    book.settle_tab(1, &edit);
    assert!(book.differs(1), "the floppy stays lit");
    assert_eq!(
        book.saved_fields(1).1.and_then(|copy| copy.flow_bars),
        Some("tick:2000".to_owned()),
        "the desk keeps the edit"
    );
}

/// A relaunch brings a held copy back: its timeframes stay an edit after
/// the panes settle on them.
#[test]
fn a_held_timeframe_edit_survives_a_relaunch_on_a_workspace_naming_none() {
    let mut book = on_legacy();
    let edit = on_bars("tick:2000", "time:300000");
    book.observe(1, &edit).unwrap();
    relaunch(&mut book, &edit);
    assert!(book.differs(1));
    assert!(book.layout_changes(1).contains(&"timeframe".to_owned()));
    assert!(book.revert(1).unwrap().changed);
    let copy = book.tab_copy(1).unwrap();
    assert_eq!(copy.flow_bars.as_deref(), Some("tick:500"), "as it opened");
}

/// Close the desk on tab 1 and bring it back, its panes settling on `shown`.
fn relaunch(book: &mut WorkspaceBook, shown: &WorkspaceRecipe) {
    let mut saved: SavedTab = toml::from_str(concat!(
        "feed = \"mt5\"\nsymbol = \"WINV26\"\nlayout = \"time+flow\"\n",
        "flow_bars = \"tick:500\"\ncontext_bars = [\"time:60000\"]\n",
    ))
    .unwrap();
    book.stamp_saved_tab(1, &mut saved);
    let text = toml::to_string(&saved).unwrap();
    let saved: SavedTab = toml::from_str(&text).unwrap();
    book.close_tab(1);
    let workspace = book.restore_saved_workspace(1, &saved).unwrap();
    book.restore_tab(1, &workspace, saved.workspace_copy.clone());
    book.settle_tab(1, shown);
}

/// A relaunch with a held edit that is not a bar rule shows no timeframe
/// edit, and a revert reads clean.
#[test]
fn a_relaunch_with_a_split_edit_reads_no_timeframe_edit() {
    let mut book = on_legacy();
    let mut edit = on_bars("tick:500", "time:60000");
    edit.split_fraction = Some(0.7);
    book.observe(1, &edit).unwrap();
    relaunch(&mut book, &edit);
    assert!(book.differs(1));
    assert!(!book.layout_changes(1).contains(&"timeframe".to_owned()));
    assert!(book.revert(1).unwrap().changed);
    book.settle_tab(1, &on_bars("tick:500", "time:60000"));
    assert!(!book.differs(1));
}

/// A revert on the factory `Default` keeps the timeframes the tab shows.
#[test]
fn a_revert_on_the_factory_default_keeps_the_shown_timeframes() {
    let mut book = WorkspaceBook::default();
    book.set_autosave(false);
    book.open_tab(1);
    let shown = on_bars("tick:2000", "time:60000");
    book.settle_tab(1, &shown);
    let mut edit = shown.clone();
    edit.split_fraction = Some(0.7);
    book.observe(1, &edit).unwrap();
    assert!(book.revert(1).unwrap().changed);
    assert_eq!(book.tab_copy(1), Some(&shown));
}

/// The factory `Default` names no bar rule and refuses Save to: a modified
/// tab settling there takes no reading its timeframes would be told from.
#[test]
fn a_modified_tab_on_the_factory_default_takes_no_settled_reading() {
    let mut book = WorkspaceBook::default();
    book.set_autosave(false);
    book.open_tab(1);
    let mut edit = on_bars("tick:500", "time:60000");
    edit.split_fraction = Some(0.7);
    book.observe(1, &edit).unwrap();
    book.settle_tab(1, &edit);
    assert!(book.differs(1));
    assert_eq!(book.tabs[&1].settled, None);
}

/// A tab differing only by a failed save's held capture has nothing to lay
/// back: the revert changes nothing and keeps the failed save's marker.
#[test]
fn a_revert_with_only_a_failed_save_s_capture_keeps_its_marker() {
    let mut book = on_legacy();
    book.unsynced.insert(1, Unsynced::Failed);
    assert!(book.differs(1));
    assert!(!book.revert(1).unwrap().changed);
    assert!(book.unsynced.contains_key(&1));
}

/// A rename carries the undo; a delete drops it with the workspace.
#[test]
fn a_rename_carries_the_undo_and_a_delete_drops_it() {
    let (mut book, saved) = on_scalp();
    book.observe(1, &edited(&saved)).unwrap();
    book.revert(1).unwrap();
    book.rename("Scalp", "Scalp2").unwrap();
    assert!(book.can_undo_revert(1));
    book.delete("Scalp2").unwrap();
    assert!(book.reverted.is_empty());
}
