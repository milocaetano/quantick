//! The Manage dialog's state: one row's palette, rename or armed delete at a
//! time, the row focus the arrows move, the keys that act on it, and Esc
//! closing the innermost thing first.
use super::*;

/// The mock's rows: Default protected, then seven others.
fn rows() -> Vec<(String, bool)> {
    let names = [
        "Default", "Flow", "Macro", "Night", "Opening", "Scalp", "Swing", "Tape",
    ];
    names
        .into_iter()
        .map(|name| (name.to_owned(), name == "Default"))
        .collect()
}

fn pressed(keys: &[ManageKey]) -> ManageKeys {
    let mut pressed = ManageKeys::default();
    for key in keys {
        pressed.press(*key);
    }
    pressed
}

#[test]
fn delete_asks_twice_for_the_same_name() {
    let mut dialog = ManageDialog::new(7);
    assert_eq!(dialog.click_delete("Scalp"), DeleteClick::Armed);
    assert_eq!(dialog.click_delete("Scalp"), DeleteClick::Confirmed);
    assert_eq!(dialog.armed, None);
}

#[test]
fn arming_another_row_disarms_the_first() {
    let mut dialog = ManageDialog::new(7);
    dialog.local(&ManageIntent::Arm("Scalp".to_owned()));
    dialog.local(&ManageIntent::Arm("Tape".to_owned()));
    assert_eq!(dialog.armed.as_deref(), Some("Tape"));
    dialog.local(&ManageIntent::Keep);
    assert_eq!(dialog.armed, None, "Keep disarms");
}

/// One thing open at a time: a rename closes the palette and disarms, a
/// palette ends a rename, an arm ends both.
#[test]
fn only_one_row_holds_a_palette_rename_or_armed_delete() {
    let mut dialog = ManageDialog::new(7);
    dialog.local(&ManageIntent::Chip("Scalp".to_owned()));
    assert_eq!(dialog.palette_of(), Some("Scalp"));
    dialog.local(&ManageIntent::Chip("Tape".to_owned()));
    assert_eq!(dialog.palette_of(), Some("Tape"), "only one row's palette");
    dialog.local(&ManageIntent::Chip("Tape".to_owned()));
    assert_eq!(dialog.palette_of(), None, "the same chip closes it");
    dialog.local(&ManageIntent::Chip("Scalp".to_owned()));
    dialog.local(&ManageIntent::BeginRename("Scalp".to_owned()));
    assert_eq!(dialog.palette_of(), None);
    let rename = dialog.rename.as_ref().unwrap();
    assert_eq!(
        (rename.from.as_str(), rename.name.as_str()),
        ("Scalp", "Scalp")
    );
    assert!(
        rename.wants_focus,
        "the field takes the keyboard, its name selected"
    );
    dialog.local(&ManageIntent::Arm("Tape".to_owned()));
    assert_eq!(dialog.rename, None);
    dialog.local(&ManageIntent::Chip("Swing".to_owned()));
    assert_eq!(dialog.armed, None);
}

/// Esc closes the innermost thing first: the field, the palette, the
/// armed row, then the dialog.
#[test]
fn escape_closes_the_innermost_thing_first() {
    let mut dialog = ManageDialog::new(7);
    dialog.local(&ManageIntent::BeginRename("Scalp".to_owned()));
    assert!(dialog.escape());
    assert_eq!(dialog.rename, None);
    dialog.local(&ManageIntent::Chip("Scalp".to_owned()));
    assert!(dialog.escape());
    assert_eq!(dialog.palette_of(), None);
    dialog.local(&ManageIntent::Arm("Scalp".to_owned()));
    assert!(dialog.escape());
    assert_eq!(dialog.armed, None);
    assert!(!dialog.escape(), "nothing inner: the dialog closes");
}

/// A refusal keeps the field open with the reason and the name typed.
#[test]
fn a_refused_rename_keeps_the_field_and_says_why() {
    let mut dialog = ManageDialog::new(7);
    dialog.local(&ManageIntent::BeginRename("Scalp".to_owned()));
    dialog.rename.as_mut().unwrap().name = "Tape".to_owned();
    dialog.refuse_rename("a workspace is already called Tape".to_owned());
    let rename = dialog.rename.as_ref().unwrap();
    assert_eq!(rename.name, "Tape");
    assert_eq!(
        rename.refusal.as_deref(),
        Some("a workspace is already called Tape")
    );
    dialog.local(&ManageIntent::CancelRename);
    assert_eq!(dialog.rename, None);
}

/// Up and Down choose the row, stopping at the ends; F2, Del and C act on
/// it; Default's row takes C only.
#[test]
fn the_arrows_choose_the_row_and_f2_del_and_c_act_on_it() {
    let mut dialog = ManageDialog::new(7);
    let rows = rows();
    let key = |dialog: &mut ManageDialog, keys: &[ManageKey]| dialog.keys(pressed(keys), &rows, 5);
    assert_eq!(key(&mut dialog, &[ManageKey::Down]), None);
    assert_eq!(dialog.focus, Some(0));
    assert_eq!(
        key(&mut dialog, &[ManageKey::F2]),
        None,
        "Default renames not"
    );
    assert_eq!(key(&mut dialog, &[ManageKey::Delete]), None, "nor deletes");
    assert_eq!(
        key(&mut dialog, &[ManageKey::Colour]),
        Some(ManageIntent::Chip("Default".to_owned())),
        "its colour changes"
    );
    for _ in 0..5 {
        key(&mut dialog, &[ManageKey::Down]);
    }
    assert_eq!(dialog.focus, Some(5));
    assert_eq!(
        key(&mut dialog, &[ManageKey::F2]),
        Some(ManageIntent::BeginRename("Scalp".to_owned()))
    );
    assert_eq!(
        key(&mut dialog, &[ManageKey::Delete]),
        Some(ManageIntent::Arm("Scalp".to_owned()))
    );
    for _ in 0..20 {
        key(&mut dialog, &[ManageKey::Down]);
    }
    assert_eq!(dialog.focus, Some(7), "stops at the last row");
    for _ in 0..20 {
        key(&mut dialog, &[ManageKey::Up]);
    }
    assert_eq!(dialog.focus, Some(0), "and at the first");
}

/// Enter confirms an armed row at the time told; Esc is the innermost
/// close; Ctrl+Z undoes while an Undo row stands.
#[test]
fn enter_confirms_the_armed_row_and_ctrl_z_undoes() {
    let mut dialog = ManageDialog::new(7);
    let rows = rows();
    assert_eq!(dialog.keys(pressed(&[ManageKey::Enter]), &rows, 5), None);
    dialog.local(&ManageIntent::Arm("Scalp".to_owned()));
    assert_eq!(
        dialog.keys(pressed(&[ManageKey::Enter]), &rows, 5),
        Some(ManageIntent::Delete { now_ms: 5 })
    );
    assert_eq!(
        dialog.keys(pressed(&[ManageKey::Escape]), &rows, 5),
        Some(ManageIntent::Escape)
    );
    assert_eq!(dialog.keys(pressed(&[ManageKey::Undo]), &rows, 5), None);
    dialog.deleted(UndoRow {
        workspace: "Scalp".to_owned(),
        words: "Scalp deleted".to_owned(),
        until_ms: 10_005,
    });
    assert_eq!(dialog.armed, None, "the delete landed");
    assert_eq!(
        dialog.keys(pressed(&[ManageKey::Undo]), &rows, 6),
        Some(ManageIntent::Undo { now_ms: 6 })
    );
}

/// The Undo row stands for its ten seconds, then goes; an Undo removes it.
#[test]
fn the_undo_row_stands_until_its_time() {
    let mut dialog = ManageDialog::new(7);
    dialog.deleted(UndoRow {
        workspace: "Scalp".to_owned(),
        words: "Scalp deleted".to_owned(),
        until_ms: 10_000,
    });
    assert!(dialog.undo_row(9_999).is_some());
    assert!(dialog.undo_row(10_000).is_none(), "at exactly ten seconds");
    dialog.tick(10_000);
    assert!(dialog.undo.is_empty());
    dialog.deleted(UndoRow {
        workspace: "Tape".to_owned(),
        words: String::new(),
        until_ms: 20_000,
    });
    dialog.undone("Tape");
    assert!(dialog.undo.is_empty());
}

/// A capture stages each face of the dialog by its token.
#[test]
fn a_capture_stages_the_dialog_s_faces() {
    let mut dialog = ManageDialog::new(7);
    assert!(dialog.stage("delete=Scalp"));
    assert_eq!(dialog.armed.as_deref(), Some("Scalp"));
    assert!(dialog.stage("rename=Scalp"));
    assert_eq!(dialog.rename.as_ref().unwrap().from, "Scalp");
    assert!(dialog.stage("colour=Scalp"));
    assert_eq!(dialog.palette_of(), Some("Scalp"));
    assert!(dialog.stage("colour=Scalp:ask=violet"));
    let palette = dialog.palette.as_ref().unwrap();
    assert_eq!(palette.popover.asked.as_deref(), Some("violet"));
    assert!(dialog.stage("focus=3"));
    assert_eq!(dialog.focus, Some(3));
    assert!(!dialog.stage("drop=Scalp"));
}
